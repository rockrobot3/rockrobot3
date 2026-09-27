# eth —— 以太网链路（接口预留）

## 为什么是空的

经典 ESP32 芯片本身支持以太网（内部 EMAC + 外接 PHY，如 LAN8720/TLK110/RTL8201），
但当前学习板没接 PHY 芯片，所以只做**接口占位**，保持 link 体系完整。

## API

| 函数 | 行为 |
|------|------|
| `eth_start()` | 打一条"未接 PHY"日志，幂等 |
| `eth_up()` | 恒返回 false |

## 接上 PHY 后怎么补实现

1. 接线：PHY 通过 RMII 接 ESP32（GPIO0/19/21/22/25/26/27 + MDC/MDIO），晶振 50MHz
2. 替换 `src/eth.c` 为真实驱动：`esp_eth_driver_install` → `esp_netif_attach`
3. 对外头文件 `eth.h` 不变，上层（main/panel/btctl）零改动

## 复用方法

1. 工程 CMakeLists：`list(APPEND EXTRA_COMPONENT_DIRS .../core/link/eth)`
2. `#include "eth.h"`
