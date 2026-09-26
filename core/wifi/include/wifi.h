/* wifi —— 独立联网模块（ESP-IDF 组件 core/wifi）
 * STA 模式连接 + 断线自动重连，供任何 ESP32 工程复用 */
#ifndef WIFI_H
#define WIFI_H

#include <stdbool.h>

void wifi_start(void);        /* 初始化并连接 WiFi（断线自动重连） */
bool wifi_connected(void);    /* 是否已拿到 IP */

#endif
