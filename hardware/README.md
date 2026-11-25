# hardware —— 接线真值（wiring）

本目录是"这台机器的电源与信号线怎么接"的**纯数据**真值，每个机型一个目录。装载期
按装配单推导定位，与装配单同时校验，对不上即拒启动。字段语义与九项判据见
[`default/wiring.toml`](default/wiring.toml) 头注——那份是格式的唯一说明处。

## 名册

| 目录 | 机型 |
|---|---|
| `default/` | 参考机（格式说明与基线） |
| `distro/` | 发行版快路机型 |
| `pi4b-host/` | Pi 宿主 |
| `pi4b-solo/` | **Pi 4B 单板全链**（含 `axis/pan.toml` 行程真值） |
| `pi4b-board/` | Pi 当板端点 |
| `pi4b-host-esp32-classic/` · `pi4b-host-esp32s3/` | 双板台架 |
| `esp32-mini/` | ESP32 末梢（含 `axis/pan.toml`、`axis/tilt.toml`） |

## 规则

- 新增机型 = 新建目录 + `wiring.toml`，**不改代码**；没有对应装配单的接线档不生效。
- 电源与地一律显式登记（`[[wire]]`）；不可控电源线组（USB 枚举设备、SPI 模块）
  不立假线，在档头说明。
- 舵机行程是独立真值（`axis/<轴>.toml`），门 ⑨-e 保证"旧行程键名全仓零命中"，
  代码侧常量由 `rr3-codegen` 从该真值生成，重跑 diff 必须为空（门 ⑨-d）。
- 引脚级真值以装配单 `[pins]` 为准；本目录只描述**线与供电**，不重复引脚分配。
