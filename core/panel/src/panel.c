/* panel —— 独立 Web 操作面板模块（ESP-IDF 组件 core/panel）
 * 手机连 RockRobot3 热点访问 http://192.168.4.1，或连同一 WiFi 访问设备 IP
 * 功能：状态与版本 / WiFi 配网 / 麦克风开关 / 硬件自检(单项) /
 *       实时交互流(日志) / 运行数据 / 重启与恢复出厂 */
#include "panel.h"
#include "panel_config.h"
#include "panel_html.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_http_server.h"
#include "esp_timer.h"
#include "esp_system.h"
#include "esp_wifi.h"
#include "esp_app_desc.h"
#include "wifi.h"
#include "audio.h"
#include "relay.h"
#include "applog.h"
#include "data.h"

/* ---------------- 小工具 ---------------- */

static esp_err_t send_json(httpd_req_t *req, const char *json)
{
    httpd_resp_set_type(req, "application/json");
    return httpd_resp_send(req, json, HTTPD_RESP_USE_STRLEN);
}

/* 极简 JSON 取字段（面板请求很小，不引 cJSON） */
static bool json_str(const char *body, const char *key, char *out, size_t n)
{
    char pat[32];
    snprintf(pat, sizeof(pat), "\"%s\"", key);
    const char *p = strstr(body, pat);
    if (!p) return false;
    p = strchr(p + strlen(pat), ':');
    if (!p) return false;
    while (*p == ' ' || *p == ':') p++;
    if (*p != '"') return false;
    p++;
    size_t i = 0;
    while (*p && *p != '"' && i + 1 < n) out[i++] = *p++;
    out[i] = 0;
    return true;
}

static bool json_int(const char *body, const char *key, int *out)
{
    char pat[32];
    snprintf(pat, sizeof(pat), "\"%s\"", key);
    const char *p = strstr(body, pat);
    if (!p) return false;
    *out = atoi(strchr(p + strlen(pat), ':') + 1);
    return true;
}

static void restart_ms(int ms)
{
    vTaskDelay(pdMS_TO_TICKS(ms));
    esp_restart();
}

/* ---------------- 接口处理 ---------------- */

/* GET / 面板页面 */
static esp_err_t h_index(httpd_req_t *req)
{
    httpd_resp_set_type(req, "text/html; charset=utf-8");
    return httpd_resp_send(req, PANEL_HTML, HTTPD_RESP_USE_STRLEN);
}

/* GET /api/status 运行状态（含版本号、麦克风状态） */
static esp_err_t h_status(httpd_req_t *req)
{
    char ssid[33] = "", ip[16] = "";
    int8_t rssi = 0;
    bool conn = wifi_get_status(ssid, sizeof(ssid), ip, sizeof(ip), &rssi);
    wifi_sta_list_t sl = {0};
    esp_wifi_ap_get_sta_list(&sl);
    char buf[512];
    snprintf(buf, sizeof(buf),
             "{\"wifi\":%s,\"ssid\":\"%s\",\"ip\":\"%s\",\"rssi\":%d,"
             "\"uptime\":%lld,\"heap\":%u,\"ap_clients\":%d,\"relay\":%s,"
             "\"mic\":%s,\"ver\":\"%s\"}",
             conn ? "true" : "false", ssid, ip, rssi,
             (long long)(esp_timer_get_time() / 1000000),
             (unsigned)esp_get_free_heap_size(), sl.num,
             relay_get() ? "true" : "false",
             audio_mic_enabled() ? "true" : "false",
             esp_app_get_description()->version);
    return send_json(req, buf);
}

/* GET /api/wifi/scan 扫描周围 WiFi */
static esp_err_t h_scan(httpd_req_t *req)
{
    wifi_scan_item_t items[20];
    int n = wifi_scan(items, 20);
    if (n < 0) {
        httpd_resp_send_err(req, HTTPD_500_INTERNAL_SERVER_ERROR, "scan failed");
        return ESP_OK;
    }
    char buf[2048];
    int off = 0;
    off += snprintf(buf + off, sizeof(buf) - off, "[");
    for (int i = 0; i < n; i++)
        off += snprintf(buf + off, sizeof(buf) - off,
                        "%s{\"ssid\":\"%s\",\"rssi\":%d,\"auth\":%s}",
                        i ? "," : "", items[i].ssid, items[i].rssi,
                        items[i].auth ? "true" : "false");
    snprintf(buf + off, sizeof(buf) - off, "]");
    return send_json(req, buf);
}

/* POST /api/wifi/connect 配网：存 NVS 并重连 */
static esp_err_t h_connect(httpd_req_t *req)
{
    char body[256] = {0};
    int len = req->content_len;
    if (len <= 0 || len >= (int)sizeof(body)) len = sizeof(body) - 1;
    httpd_req_recv(req, body, len);

    char ssid[33], pass[65];
    if (!json_str(body, "ssid", ssid, sizeof(ssid)) ||
        !json_str(body, "pass", pass, sizeof(pass))) {
        send_json(req, "{\"ok\":false,\"err\":\"参数缺失\"}");
        return ESP_OK;
    }
    int err = wifi_set_credentials(ssid, pass);
    if (err == -2) {
        send_json(req, "{\"ok\":false,\"err\":\"密码至少 8 位\"}");
        return ESP_OK;
    }
    if (err != 0) {
        send_json(req, "{\"ok\":false,\"err\":\"参数无效\"}");
        return ESP_OK;
    }

    /* 最多等 12 秒看是否连上，给页面真实结果 */
    for (int w = 0; w < 24 && !wifi_connected(); w++)
        vTaskDelay(pdMS_TO_TICKS(500));
    if (wifi_connected()) audio_play_tone(1320, 200);   /* 连上了：提示音 */
    char buf[96];
    snprintf(buf, sizeof(buf), "{\"ok\":true,\"connected\":%s}",
             wifi_connected() ? "true" : "false");
    return send_json(req, buf);
}

/* POST /api/relay 继电器开关 */
static esp_err_t h_relay(httpd_req_t *req)
{
    char body[64] = {0};
    int len = req->content_len;
    if (len <= 0 || len >= (int)sizeof(body)) len = sizeof(body) - 1;
    httpd_req_recv(req, body, len);
    int on = 0;
    json_int(body, "on", &on);
    relay_set(on != 0);
    data_inc(on ? DATA_RELAY_ON : DATA_RELAY_OFF);
    log_writef("面板：继电器%s", on ? "吸合（开灯）" : "断开（关灯）");
    return send_json(req, "{\"ok\":true}");
}

/* POST /api/beep 喇叭测试音 */
static esp_err_t h_beep(httpd_req_t *req)
{
    audio_play_tone(1000, 300);
    return send_json(req, "{\"ok\":true}");
}

/* POST /api/mic 麦克风开关 {"on":true|false} */
static esp_err_t h_mic(httpd_req_t *req)
{
    char body[64] = {0};
    int len = req->content_len;
    if (len <= 0 || len >= (int)sizeof(body)) len = sizeof(body) - 1;
    httpd_req_recv(req, body, len);
    int on = audio_mic_enabled();
    json_int(body, "on", &on);
    audio_mic_set_enable(on != 0);
    log_writef("面板：麦克风%s", on ? "开启" : "静音");
    char buf[64];
    snprintf(buf, sizeof(buf), "{\"ok\":true,\"mic\":%s}",
             audio_mic_enabled() ? "true" : "false");
    return send_json(req, buf);
}

/* 麦克风单项自检：采样 0.6 秒取峰值 */
static int selftest_mic(int *peak_out)
{
    int32_t raw[AUDIO_CHUNK];
    int16_t tmp[AUDIO_CHUNK];
    int peak = 0;
    bool read_ok = false;
    for (int i = 0; i < 40; i++) {                 /* 40 次 ≈ 0.64 秒 */
        size_t br = 0;
        if (audio_read(raw, sizeof(raw), &br, 100) == ESP_OK && br > 0)
            read_ok = true;
        for (int k = 0; k < (int)(br / 4); k++) {
            int p = audio_mic_process(raw[k], &tmp[k]);
            if (p > peak) peak = p;
        }
    }
    *peak_out = peak;
    return read_ok ? 0 : -1;
}

/* POST /api/selftest 单项自检 {"item":"speaker"|"relay"|"mic"}，缺省全测 */
static esp_err_t h_selftest(httpd_req_t *req)
{
    char body[96] = {0};
    int len = req->content_len;
    if (len > 0 && len < (int)sizeof(body)) httpd_req_recv(req, body, len);
    char item[16] = "all";
    json_str(body, "item", item, sizeof(item));

    char buf[256];
    if (!strcmp(item, "speaker")) {
        audio_play_tone(1000, 250);
        snprintf(buf, sizeof(buf), "{\"item\":\"speaker\",\"tone\":true}");
    } else if (!strcmp(item, "relay")) {
        relay_set(true);
        vTaskDelay(pdMS_TO_TICKS(300));
        relay_set(false);
        vTaskDelay(pdMS_TO_TICKS(300));
        snprintf(buf, sizeof(buf), "{\"item\":\"relay\",\"relay\":true}");
    } else if (!strcmp(item, "mic")) {
        int peak = 0;
        int err = selftest_mic(&peak);
        snprintf(buf, sizeof(buf),
                 "{\"item\":\"mic\",\"mic_read\":%s,\"mic_peak\":%d}",
                 err == 0 ? "true" : "false", peak);
    } else {                                       /* 全测（兼容旧版） */
        audio_play_tone(1000, 250);
        relay_set(true);
        vTaskDelay(pdMS_TO_TICKS(300));
        relay_set(false);
        vTaskDelay(pdMS_TO_TICKS(300));
        int peak = 0;
        int err = selftest_mic(&peak);
        snprintf(buf, sizeof(buf),
                 "{\"tone\":true,\"relay\":true,\"mic_read\":%s,\"mic_peak\":%d,"
                 "\"mic\":\"%s\"}",
                 err == 0 ? "true" : "false", peak,
                 (err == 0 && peak > 100) ? "ok" : "check");
    }
    log_writef("面板：自检 %s 完成", item);
    return send_json(req, buf);
}

/* GET /api/logs?n=20 实时交互流（最近日志，新的在前） */
static esp_err_t h_logs(httpd_req_t *req)
{
    int n = 20;
    char q[32], val[8];
    if (httpd_req_get_url_query_str(req, q, sizeof(q)) == ESP_OK &&
        httpd_query_key_value(q, "n", val, sizeof(val)) == ESP_OK)
        n = atoi(val);
    if (n < 1) n = 1;
    if (n > 40) n = 40;
    static char buf[8192];                        /* 单线程服务，静态省栈 */
    log_dump_json(buf, sizeof(buf), n);
    return send_json(req, buf);
}

/* GET /api/data 运行数据统计 */
static esp_err_t h_data(httpd_req_t *req)
{
    static char buf[256];
    data_get_json(buf, sizeof(buf));
    return send_json(req, buf);
}

/* POST /api/reboot 重启 */
static esp_err_t h_reboot(httpd_req_t *req)
{
    send_json(req, "{\"ok\":true}");
    log_writef("面板：收到重启请求");
    restart_ms(300);
    return ESP_OK;
}

/* POST /api/factory_reset 清除保存的 WiFi 并重启 */
static esp_err_t h_factory(httpd_req_t *req)
{
    wifi_clear_credentials();
    send_json(req, "{\"ok\":true}");
    log_writef("面板：恢复出厂 WiFi，重启");
    restart_ms(300);
    return ESP_OK;
}

/* ---------------- 启动 ---------------- */

void panel_start(void)
{
    httpd_config_t cfg = HTTPD_DEFAULT_CONFIG();
    cfg.stack_size = 8192;
    cfg.max_uri_handlers = 16;   /* 默认 8 不够（本面板 12 个接口） */

    httpd_handle_t server = NULL;
    if (httpd_start(&server, &cfg) != ESP_OK) {
        log_writef("✘ 面板服务启动失败");
        return;
    }

    static const httpd_uri_t uris[] = {
        { .uri = "/",              .method = HTTP_GET,  .handler = h_index },
        { .uri = "/api/status",    .method = HTTP_GET,  .handler = h_status },
        { .uri = "/api/wifi/scan", .method = HTTP_GET,  .handler = h_scan },
        { .uri = "/api/wifi/connect", .method = HTTP_POST, .handler = h_connect },
        { .uri = "/api/relay",     .method = HTTP_POST, .handler = h_relay },
        { .uri = "/api/beep",      .method = HTTP_POST, .handler = h_beep },
        { .uri = "/api/mic",       .method = HTTP_POST, .handler = h_mic },
        { .uri = "/api/selftest",  .method = HTTP_POST, .handler = h_selftest },
        { .uri = "/api/logs",      .method = HTTP_GET,  .handler = h_logs },
        { .uri = "/api/data",      .method = HTTP_GET,  .handler = h_data },
        { .uri = "/api/reboot",    .method = HTTP_POST, .handler = h_reboot },
        { .uri = "/api/factory_reset", .method = HTTP_POST, .handler = h_factory },
    };
    for (size_t i = 0; i < sizeof(uris) / sizeof(uris[0]); i++)
        httpd_register_uri_handler(server, &uris[i]);

    log_writef("★ 操作面板已启动：热点 %s → http://192.168.4.1", PANEL_AP_SSID);
}
