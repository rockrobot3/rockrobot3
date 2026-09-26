#include "wifi.h"
#include "wifi_config.h"
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_wifi.h"
#include "esp_event.h"
#include "esp_netif.h"
#include "esp_err.h"
#include "nvs.h"

static volatile bool s_got_ip;
static volatile int  s_ap_clients;    /* 面板热点在线设备数（>0 = 配网模式） */
static char s_ssid[33] = WIFI_SSID;   /* 当前使用的账号（NVS 优先，默认兜底） */
static TaskHandle_t s_recon_task;

/* 重连专用任务：断开后 3 秒重试；热点上有设备时暂停（防信道跳变踢掉面板用户） */
static void recon_task(void *arg)
{
    for (;;) {
        ulTaskNotifyTake(pdTRUE, portMAX_DELAY);
        vTaskDelay(pdMS_TO_TICKS(3000));
        while (!s_got_ip) {
            if (s_ap_clients == 0)
                esp_wifi_connect();
            vTaskDelay(pdMS_TO_TICKS(3000));
        }
    }
}

static void wifi_event_handler(void *arg, esp_event_base_t base,
                               int32_t id, void *data)
{
    if (base == WIFI_EVENT && id == WIFI_EVENT_STA_DISCONNECTED) {
        wifi_event_sta_disconnected_t *ev = (wifi_event_sta_disconnected_t *)data;
        s_got_ip = false;
        printf("WiFi 断开（原因码 %d），3 秒后重试\n", ev->reason);
        xTaskNotifyGive(s_recon_task);
    } else if (base == WIFI_EVENT && id == WIFI_EVENT_AP_STACONNECTED) {
        s_ap_clients++;
        printf("设备已连上面板热点（配网模式：暂停连路由，防信道冲突）\n");
    } else if (base == WIFI_EVENT && id == WIFI_EVENT_AP_STADISCONNECTED) {
        if (s_ap_clients > 0) s_ap_clients--;
        if (!s_got_ip) xTaskNotifyGive(s_recon_task);   /* 面板用户走了，恢复连路由 */
    } else if (base == IP_EVENT && id == IP_EVENT_STA_GOT_IP) {
        ip_event_got_ip_t *ev = (ip_event_got_ip_t *)data;
        printf("★ WiFi 已连接！本机 IP: " IPSTR "（面板：手机连热点 %s 开 192.168.4.1）\n",
               IP2STR(&ev->ip_info.ip), WIFI_AP_SSID);
        s_got_ip = true;
    }
}

/* 从 NVS 读凭据；没有就落回编译默认值。返回是否用到了 NVS */
static bool load_saved_creds(char *ssid, size_t n_ssid, char *pass, size_t n_pass)
{
    nvs_handle_t h;
    if (nvs_open("wifi", NVS_READONLY, &h) != ESP_OK) return false;
    size_t l1 = n_ssid, l2 = n_pass;
    esp_err_t e1 = nvs_get_str(h, "ssid", ssid, &l1);
    esp_err_t e2 = nvs_get_str(h, "pass", pass, &l2);
    nvs_close(h);
    if (e1 != ESP_OK || e2 != ESP_OK || ssid[0] == 0) return false;
    return true;
}

void wifi_start(void)
{
    esp_netif_init();
    esp_event_loop_create_default();
    esp_netif_create_default_wifi_sta();
    esp_netif_create_default_wifi_ap();     /* 面板热点常开 */
    wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_wifi_init(&cfg));
    esp_event_handler_register(WIFI_EVENT, ESP_EVENT_ANY_ID, &wifi_event_handler, NULL);
    esp_event_handler_register(IP_EVENT, IP_EVENT_STA_GOT_IP, &wifi_event_handler, NULL);
    xTaskCreate(recon_task, "wifi_recon", 3072, NULL, 5, &s_recon_task);

    char ssid[33] = WIFI_SSID, pass[65] = WIFI_PASS;
    if (load_saved_creds(ssid, sizeof(ssid), pass, sizeof(pass)))
        printf("使用面板保存的 WiFi：%s\n", ssid);
    else
        printf("使用内置默认 WiFi：%s\n", ssid);
    strlcpy(s_ssid, ssid, sizeof(s_ssid));

    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_APSTA));
    wifi_config_t wc = { .sta = { .threshold = { .authmode = WIFI_AUTH_WPA_PSK } } };
    strlcpy((char *)wc.sta.ssid, ssid, sizeof(wc.sta.ssid));
    strlcpy((char *)wc.sta.password, pass, sizeof(wc.sta.password));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_STA, &wc));

    wifi_config_t ap = {
        .ap = { .ssid = WIFI_AP_SSID, .password = WIFI_AP_PASS,
                .max_connection = 4, .authmode = WIFI_AUTH_WPA_WPA2_PSK },
    };
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_AP, &ap));

    ESP_ERROR_CHECK(esp_wifi_start());
    esp_wifi_set_ps(WIFI_PS_NONE);   /* 关 WiFi 省电模式，降低网络延迟 */
    esp_wifi_connect();
}

bool wifi_connected(void)
{
    return s_got_ip;
}

int wifi_set_credentials(const char *ssid, const char *pass)
{
    if (!ssid || !ssid[0] || !pass) return -1;
    if (strlen(pass) > 0 && strlen(pass) < 8) return -2;   /* WPA 密码至少 8 位 */

    nvs_handle_t h;
    if (nvs_open("wifi", NVS_READWRITE, &h) == ESP_OK) {
        nvs_set_str(h, "ssid", ssid);
        nvs_set_str(h, "pass", pass);
        nvs_commit(h);
        nvs_close(h);
    }

    wifi_config_t wc = {0};
    strlcpy((char *)wc.sta.ssid, ssid, sizeof(wc.sta.ssid));
    strlcpy((char *)wc.sta.password, pass, sizeof(wc.sta.password));
    strlcpy(s_ssid, ssid, sizeof(s_ssid));
    s_got_ip = false;
    esp_wifi_disconnect();
    esp_wifi_set_config(WIFI_IF_STA, &wc);
    esp_wifi_connect();
    return 0;
}

void wifi_clear_credentials(void)
{
    nvs_handle_t h;
    if (nvs_open("wifi", NVS_READWRITE, &h) == ESP_OK) {
        nvs_erase_key(h, "ssid");
        nvs_erase_key(h, "pass");
        nvs_commit(h);
        nvs_close(h);
    }
    strlcpy(s_ssid, WIFI_SSID, sizeof(s_ssid));
}

bool wifi_get_status(char *ssid, size_t ssid_len, char *ip, size_t ip_len, int8_t *rssi)
{
    if (ssid) strlcpy(ssid, s_ssid, ssid_len);
    if (ip) {
        ip[0] = 0;
        if (s_got_ip) {
            esp_netif_t *nf = esp_netif_get_handle_from_ifkey("WIFI_STA_DEF");
            esp_netif_ip_info_t info;
            if (nf && esp_netif_get_ip_info(nf, &info) == ESP_OK)
                snprintf(ip, ip_len, IPSTR, IP2STR(&info.ip));
        }
    }
    if (rssi) {
        *rssi = 0;
        if (s_got_ip) {
            wifi_ap_record_t rec;
            if (esp_wifi_sta_get_ap_info(&rec) == ESP_OK) *rssi = rec.rssi;
        }
    }
    return s_got_ip;
}

int wifi_scan(wifi_scan_item_t *items, int max)
{
    if (!items || max <= 0) return -1;
    wifi_scan_config_t sc = { .show_hidden = false };
    if (esp_wifi_scan_start(&sc, true) != ESP_OK) return -1;
    uint16_t n = 0;
    esp_wifi_scan_get_ap_num(&n);
    if (n > (uint16_t)max) n = (uint16_t)max;
    wifi_ap_record_t *rec = calloc(n ? n : 1, sizeof(*rec));
    if (!rec) return -1;
    if (esp_wifi_scan_get_ap_records(&n, rec) != ESP_OK) { free(rec); return -1; }
    for (int i = 0; i < (int)n; i++) {
        strlcpy(items[i].ssid, (const char *)rec[i].ssid, sizeof(items[i].ssid));
        items[i].rssi = rec[i].rssi;
        items[i].auth = rec[i].authmode != WIFI_AUTH_OPEN;
    }
    free(rec);
    return (int)n;
}
