# core/relay —— 独立继电器模块（ESP-IDF 组件）

板载继电器开关，供任何 ESP32 工程复用（语音"开灯/关灯"、定时通断等场景）。

## API（include/relay.h）

| 函数 | 说明 |
|------|------|
| `relay_init` | 初始化引脚（默认断开） |
| `relay_set(bool on)` | true=吸合(通电)，false=断开 |

## 配置（src/relay_config.h）

`RELAY_GPIO` 继电器 GPIO（默认 GPIO21 板载 REL 丝印针）。

## 复用方法

工程 `CMakeLists.txt`（`project()` 前）加 `list(APPEND EXTRA_COMPONENT_DIRS "路径/core/relay")`，使用方组件 `REQUIRES` 加 `relay`，代码 `#include "relay.h"`。
