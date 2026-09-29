# nearlink —— 星闪 NearLink 链路（接口预留）

## 为什么是空的

经典 ESP32 没有星闪（NearLink/SparkLink）射频硬件，无法实现真实链路。
本模块的作用是**占位**：让 link 体系（WiFi/蓝牙/星闪/网线）的接口形态保持完整，
迁移到瑞芯微 RK-S1 等带 NearLink 的板卡时，实现同名 API 即可无缝接入。

## API

| 函数 | 行为 |
|------|------|
| `nearlink_start()` | 打一条"硬件不支持"日志，幂等 |
| `nearlink_up()` | 恒返回 false |

## 复用方法

1. 工程 CMakeLists：`list(APPEND EXTRA_COMPONENT_DIRS .../core/link/nearlink)`
2. `#include "nearlink.h"`
3. 换平台时：把 `src/nearlink.c` 换成真实实现（对外头文件不变）
