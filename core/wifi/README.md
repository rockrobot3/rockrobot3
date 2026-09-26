# core/wifi —— 独立联网模块（ESP-IDF 组件）

WiFi STA 连接 + 断线自动重连，供任何 ESP32 工程复用。

## API（include/wifi.h）

| 函数 | 说明 |
|------|------|
| `wifi_start` | 初始化并连接（已关省电模式降延迟，断线 3 秒自动重试） |
| `wifi_connected` | 是否已拿到 IP |

## 配置（wifi_config.h）

`WIFI_SSID` / `WIFI_PASS`。注意：名字**区分大小写**；ESP32 只支持 2.4GHz。

## 复用方法

工程 `CMakeLists.txt`（`project()` 前）加 `list(APPEND EXTRA_COMPONENT_DIRS "路径/core/wifi")`，使用方组件 `REQUIRES` 加 `wifi`，代码 `#include "wifi.h"`；NVS 初始化（`nvs_flash_init`）由使用方负责。
