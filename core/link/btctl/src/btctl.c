/* bt 模块实现：经典蓝牙 SPP 串口控制台
 * 收到命令入队，独立工作线程执行（不在蓝牙栈回调里阻塞）；
 * 日志镜像走 log_set_mirror，连上手机就实时转发。 */
#include "btctl.h"
#include "bt_config.h"
#include <stdio.h>
#include <string.h>
#include <stdbool.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/queue.h"
#include "esp_bt.h"
#include "esp_bt_main.h"
#include "esp_bt_device.h"
#include "esp_gap_bt_api.h"
#include "esp_spp_api.h"
#include "esp_timer.h"
#include "esp_system.h"
#include "esp_app_desc.h"
#include "esp_log.h"
#include "wifi.h"
#include "audio.h"
#include "relay.h"
#include "applog.h"
#include "data.h"

static const char *TAG = "bt";

static uint32_t s_handle;                 /* SPP 连接句柄，0=未连接 */
static QueueHandle_t s_cmdq;              /* 收到的命令行队列 */
static char s_rxbuf[256];
static int  s_rxlen;

/* ---------------- 发送（未连接则丢弃） ---------------- */
static void spp_send(const char *s)
{
    if (!s_handle) return;
    int len = strlen(s), off = 0;
    uint8_t chunk[400];
    while (off < len) {                   /* 按 400 字节分片，避开 MTU 限制 */
        int m = (len - off < 400) ? len - off : 400;
        memcpy(chunk, s + off, m);        /* esp_spp_write 不接受 const，先拷贝 */
        esp_spp_write(s_handle, m, chunk);
        off += m;
    }
}

static void bt_mirror(const char *line)
{
    if (!s_handle) return;
    spp_send(line);
    spp_send("\r\n");
}

/* ---------------- 命令处理（工作线程） ---------------- */
static void cmd_status(void)
{
    char ssid[33] = "", ip[16] = "";
    int8_t rssi = 0;
    bool conn = wifi_get_status(ssid, sizeof(ssid), ip, sizeof(ip), &rssi);
    char buf[224];
    snprintf(buf, sizeof(buf),
             "WiFi:%s(%s,%ddBm) IP:%s\r\n继电器:%s 麦克风:%s\r\n内存:%uKB 运行:%lld秒\r\n",
             conn ? ssid : "未连", conn ? "" : "", conn ? rssi : 0, ip,
             relay_get() ? "开" : "关",
             audio_mic_enabled() ? "开" : "静音",
             (unsigned)(esp_get_free_heap_size() / 1024),
             (long long)(esp_timer_get_time() / 1000000));
    spp_send(buf);
}

static void cmd_selftest(void)
{
    int32_t raw[AUDIO_CHUNK];
    int16_t tmp[AUDIO_CHUNK];
    int peak = 0;
    bool read_ok = false;
    for (int i = 0; i < 40; i++) {        /* 0.64 秒取峰值 */
        size_t br = 0;
        if (audio_read(raw, sizeof(raw), &br, 100) == ESP_OK && br > 0)
            read_ok = true;
        for (int k = 0; k < (int)(br / 4); k++) {
            int p = audio_mic_process(raw[k], &tmp[k]);
            if (p > peak) peak = p;
        }
    }
    char buf[128];
    snprintf(buf, sizeof(buf), "自检:麦克风%s 峰值%d(>100正常)\r\n",
             read_ok ? "能读到数据" : "读不到!", peak);
    spp_send(buf);
}

static void cmd_exec(char *line)
{
    char reply[96];
    if (!strcmp(line, "help")) {
        spp_send("命令: status ver logs relay on|off mic on|off "
                 "selftest reboot\r\n");
    } else if (!strcmp(line, "status")) {
        cmd_status();
    } else if (!strcmp(line, "ver")) {
        snprintf(reply, sizeof(reply), "版本: %s\r\n", esp_app_get_description()->version);
        spp_send(reply);
    } else if (!strcmp(line, "logs")) {
        static char dump[4096];
        log_dump_json(dump, sizeof(dump), 20);
        spp_send("最近日志(JSON,新的在前):\r\n");
        spp_send(dump);
        spp_send("\r\n");
    } else if (!strcmp(line, "relay on")) {
        relay_set(true);
        data_inc(DATA_RELAY_ON);
        spp_send("继电器吸合（开灯）\r\n");
    } else if (!strcmp(line, "relay off")) {
        relay_set(false);
        data_inc(DATA_RELAY_OFF);
        spp_send("继电器断开（关灯）\r\n");
    } else if (!strcmp(line, "mic on")) {
        audio_mic_set_enable(true);
        spp_send("麦克风已开\r\n");
    } else if (!strcmp(line, "mic off")) {
        audio_mic_set_enable(false);
        spp_send("麦克风已静音\r\n");
    } else if (!strcmp(line, "selftest")) {
        cmd_selftest();
    } else if (!strcmp(line, "reboot")) {
        spp_send("重启...\r\n");
        vTaskDelay(pdMS_TO_TICKS(300));
        esp_restart();
    } else {
        snprintf(reply, sizeof(reply), "未知命令: %s（help 查看命令）\r\n", line);
        spp_send(reply);
    }
}

static void bt_worker(void *arg)
{
    char line[192];
    while (1) {
        if (xQueueReceive(s_cmdq, line, portMAX_DELAY) != pdTRUE) continue;
        cmd_exec(line);
    }
}

/* ---------------- SPP / GAP 回调 ---------------- */
static void spp_process_rx(const uint8_t *data, int len)
{
    for (int i = 0; i < len; i++) {
        char c = data[i];
        if (c == '\n' || c == '\r') {
            if (s_rxlen > 0) {
                s_rxbuf[s_rxlen] = 0;
                if (s_cmdq && xQueueSend(s_cmdq, s_rxbuf, 0) != pdTRUE)
                    ESP_LOGW(TAG, "命令队列满，丢弃: %s", s_rxbuf);
                s_rxlen = 0;
            }
        } else if (s_rxlen < (int)sizeof(s_rxbuf) - 1) {
            s_rxbuf[s_rxlen++] = c;
        }
    }
}

static void spp_cb(esp_spp_cb_event_t event, esp_spp_cb_param_t *param)
{
    switch (event) {
    case ESP_SPP_INIT_EVT:
        esp_spp_start_srv(ESP_SPP_SEC_NONE, ESP_SPP_ROLE_SLAVE, 0, BT_DEVICE_NAME);
        break;
    case ESP_SPP_START_EVT:
        esp_bt_gap_set_device_name(BT_DEVICE_NAME);
        esp_bt_gap_set_scan_mode(ESP_BT_CONNECTABLE, ESP_BT_GENERAL_DISCOVERABLE);
        ESP_LOGI(TAG, "蓝牙串口就绪，设备名 %s", BT_DEVICE_NAME);
        log_writef("蓝牙串口就绪（%s）", BT_DEVICE_NAME);
        break;
    case ESP_SPP_SRV_OPEN_EVT:
        s_handle = param->srv_open.handle;
        s_rxlen = 0;
        spp_send("RockRobot3 蓝牙控制台已连接\r\n输入 help 查看命令\r\n");
        log_writef("蓝牙客户端已连接");
        break;
    case ESP_SPP_CLOSE_EVT:
        s_handle = 0;
        log_writef("蓝牙客户端已断开");
        break;
    case ESP_SPP_DATA_IND_EVT:
        spp_process_rx(param->data_ind.data, param->data_ind.len);
        break;
    default:
        break;
    }
}

void bt_start(void)
{
    esp_bt_controller_config_t bt_cfg = BT_CONTROLLER_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_bt_controller_init(&bt_cfg));
    ESP_ERROR_CHECK(esp_bt_controller_enable(ESP_BT_MODE_CLASSIC_BT));
    ESP_ERROR_CHECK(esp_bluedroid_init());
    ESP_ERROR_CHECK(esp_bluedroid_enable());
    ESP_ERROR_CHECK(esp_spp_register_callback(spp_cb));
    const esp_spp_cfg_t spp_cfg = {
        .mode = ESP_SPP_MODE_CB,
        .enable_l2cap_ertm = true,
        .tx_buffer_size = 0,
    };
    ESP_ERROR_CHECK(esp_spp_enhanced_init(&spp_cfg));

    s_cmdq = xQueueCreate(4, sizeof(s_rxbuf));
    xTaskCreate(bt_worker, "bt_cmd", 8192, NULL, 4, NULL);
    log_set_mirror(bt_mirror);
}

bool bt_connected(void)
{
    return s_handle != 0;
}
