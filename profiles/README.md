# profiles —— 装配单（能力格绑定真值）

本目录是"这台机器启用哪些能力、绑定到哪些件"的**纯数据**真值：零逻辑、零代码路径。
装载期由 rrd `--profile` 读取并逐格校验（缺格点名拒启动，fail-closed）；形状与
在册性由 `base/runtime/rr-registry/tests/contract/profile_*.rs` 钉死——新增/删除
一份装配单必须同批改那条 pin，否则契约测试红。

## 名册

| 文件 | 形态 | 说明 |
|---|---|---|
| `default.toml` | RK3566 参考机 | 16 条绑定（body 12 + cognition 4）+ faces 6 模块，全绑定基线 |
| `distro.toml` | 发行版快路 | 只绑 body 12；cognition 4 格缺位恰等于 brain provides 全集 |
| `pi4b-host.toml` | Pi 宿主 | 本机回放 + 显示 + WiFi + 认知；无 capture/vad/servo |
| `pi4b-solo.toml` | **Pi 4B 单板全链** | 本机麦、本机声卡、本机 pan 舵机、本机 OLED；独立整机 |
| `pi4b-board.toml` | Pi 当板端点 | rtos 双轴档，经桥与宿主配合（双板台架形态） |
| `pi4b-host-esp32-classic.toml` | 双板台架 | Pi 宿主 + classic 末梢，测试台架，不是目标形态 |
| `pi4b-host-esp32s3.toml` | 双板台架 | 同上，S3 末梢 |
| `esp32-mini.toml` | 桥接微型 | 三格（capture/vad/servo），链路与认知都在宿主 |
| `esp32s3-mini.toml` | S3 末梢/自治档 | 板载双麦与双轴舵机；单板全链档的基座 |

## 规则

- `schema_version = 5`，`[runtime]` 段必填（缺即红）；字段字典唯一真源是
  [`../base/runtime/rr-registry/src/module.schema.toml`](../base/runtime/rr-registry/src/module.schema.toml)。
- 能力格坐标唯一真源是 [`../contracts/slots.toml`](../contracts/slots.toml)；装配单只能
  引用在册格位，不能自造名字。
- 每个芯片平台是**独立开发板**，没有主/副板：单板形态各自一份装配单，双板档只作台架。
- 部署档选择（哪份装配单装到哪台机器）见
  [`../docs/design/模块设计索引.md`](../docs/design/模块设计索引.md) 与 `docs/process/dev` 对应批次。
