/* wifi —— 独立联网模块（ESP-IDF 组件 core/wifi）
 * STA 联网（NVS 凭据优先，编译默认兜底）+ 断线自动重连
 * AP 热点常开（RockRobot3），供手机直连操作面板（192.168.4.1） */
#ifndef WIFI_H
#define WIFI_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* 扫描结果条目 */
typedef struct {
    char   ssid[33];
    int8_t rssi;
    bool   auth;          /* 是否加密 */
} wifi_scan_item_t;

void wifi_start(void);        /* 初始化：AP 常开 + STA 连接（断线自动重连） */
bool wifi_connected(void);    /* STA 是否已拿到 IP */

/* 修改 WiFi 凭据：存入 NVS 并立即重连（面板用）。返回 0 成功 */
int  wifi_set_credentials(const char *ssid, const char *pass);

/* 清除 NVS 里的凭据（恢复出厂后回到编译默认账号），一般配 esp_restart 用 */
void wifi_clear_credentials(void);

/* 查询状态：ssid/ip/rssi 任一可为 NULL。返回 true=已连上网 */
bool wifi_get_status(char *ssid, size_t ssid_len, char *ip, size_t ip_len, int8_t *rssi);

/* 扫描周围 WiFi（阻塞约 2~3 秒），返回条目数，失败返回 -1 */
int  wifi_scan(wifi_scan_item_t *items, int max);

#endif
