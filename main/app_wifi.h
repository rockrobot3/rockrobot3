/* WiFi 联网模块：启动 STA 并自动重连 */
#ifndef APP_WIFI_H
#define APP_WIFI_H

#include <stdbool.h>

void app_wifi_start(void);        /* 初始化并连接 WiFi（断线自动重连） */
bool app_wifi_connected(void);    /* 是否已拿到 IP */

#endif
