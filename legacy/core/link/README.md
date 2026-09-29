# core/link —— 通信链路模块组

所有"对外连接"能力统一放在 link 路径下管理：无线（WiFi/蓝牙）、有线（以太网）、
以及为下一代硬件预留的星闪（NearLink）。**新增链路模块放这里，照 core 统一模板。**

## 模块清单

| 模块 | 链路 | 状态 | 依赖 |
|------|------|------|------|
| `wifi` | WiFi STA + AP 热点 | ✅ 已实现 | esp_wifi / nvs_flash |
| `btctl` | 经典蓝牙 SPP 串口控制台 | ✅ 已实现 | IDF bt 组件（Bluedroid） |
| `nearlink` | 星闪 NearLink | ⏳ 接口预留（本硬件无射频） | 无 |
| `eth` | 以太网 | ⏳ 接口预留（未接 PHY） | 无 |

## 设计约定

1. **一条链路一个模块**：互相独立、互不依赖，都只对外暴露 `<模块名>.h`
2. **统一生命周期**：每条链路都有 `xxx_start()` + `xxx_up()`（是否在线）两个基本 API，
   上层（main/panel）按同一套姿势调用，不关心链路细节
3. **组件名不能撞 IDF 内置组件**：蓝牙实现叫 `btctl`（`bt` 是 IDF 内置 Bluedroid 组件名，
   日志实现叫 `applog`（`log` 是 IDF 内置日志组件名）——同名会把内置组件顶掉导致全工程编译崩
4. **平台迁移边界**：换到瑞芯微主板时，链路层整体换实现（Linux NetworkManager/BlueZ/NearLink SDK），
   `include/*.h` 的接口形态不变，上层业务代码可平移

## 复用方法

```cmake
list(APPEND EXTRA_COMPONENT_DIRS .../core/link/wifi
                                 .../core/link/btctl
                                 .../core/link/nearlink
                                 .../core/link/eth)
```
