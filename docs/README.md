# docs —— 文档索引（四档）

docs/ 按**变更频率与读者动作**分四档：`architecture/` 半年一变，读者要的是"为什么这样切"；
`reference/` 随代码同批变，读者要的是"照着写、照着调"；`process/` 是协作契约，读者要的是
"提交前对什么"；`status.md` 是快照，每次里程碑收口整篇重写。

**每类内容只有一个权威落点**，其余文档引用它、不复述它——复述是漂移的源头（架构文档 §5
曾因把设计期草图当既成事实书写而整段重写）。

## 四档落点

| 档 / 路径 | 内容 | 谁在它上面兜底 |
|---|---|---|
| [`architecture/architecture.md`](architecture/architecture.md) | 唯一架构图：定位、分层、目录树、模块模型、HAL、IPC、数据流、迁移映射、风险登记（§10 只留一行指向 `status.md`，本档不写进度） | 目录树变更必须同步其 §3（`process/conventions.md` §5 文档纪律） |
| [`reference/rrd-cli.md`](reference/rrd-cli.md) · [`reference/rrd-rpc.md`](reference/rrd-rpc.md) · [`reference/rrd-registry.md`](reference/rrd-registry.md) | 运行期接口的可照抄事实：启动参数与默认值、RPC 方法与错误映射、在册表阶段与退避上限 | 与代码同批变更；引用须与代码逐字一致 |
| [`process/conventions.md`](process/conventions.md) | 开发规约：分支 / 提交 / CI 门禁 / DoD | 门禁唯一入口 `bash tools/ci/check.sh`（其 ⑤ 文档门对仓内每个已跟踪 md 兜底：不得为空、相对链接不得断；仓内每份 README 还必须出现在下文库地图里，漏链即红。本索引自身也在覆盖内）；**步序与条数只登记在其 §2**，本索引不抄数字 |
| [`process/naming.md`](process/naming.md) | 命名唯一权威（`rr3-` 前缀矩阵、模块目录四件套的四类结构角色） | `rr3-mkmod` 生成自检 + `rr-registry` 契约测试强制，不靠人记 |
| [`process/adr/`](process/adr/) | 决策记录 ADR-0001 ~ ADR-0010（平台选型 / 模块模型 / IPC 选型 / Airymax 接入 / 命名规约 / AiryIPC 线格式 / 控制面分层 / 传输层 trait / 依赖仲裁 / 控制通道与查询面） | 架构文档每节顶部回链对应 ADR |
| [`process/dev/0.0.1版本洛克三号开发方案.md`](process/dev/0.0.1版本洛克三号开发方案.md) | 当前版本改造方案与 Definition of Done | 收口评审按此逐条对账 |
| [`status.md`](status.md) | 唯一实况快照：能力 × 状态 × 证据路径、里程碑进度（M0–M6 当前实况）、本机无法收口的三项、在制品 | 收口时整篇重写，不做增量修补；**测试条数一律不抄进来**（见其开篇规矩） |

## 库地图（按目录）

**本索引是文档的入口，不是目录树的事实源**：文件是否存在以
[`architecture/architecture.md`](architecture/architecture.md) §3 为准，模块名册以
[`../modules/README.md`](../modules/README.md) 为准，crate 职责以
[`../base/runtime/README.md`](../base/runtime/README.md) 为准。下表只回答"这个目录下有哪份
README、什么时候该点开它"；小节名以文档自身为准，本表只作导航。`legacy/` 是冻结区，不进本表，
⑤ 门也不要求它出现在导航里（下文动作索引留了一条对照入口，那是有意的）。新增 README 不写进
本表即判红。

### 仓根 · docs/

| 路径 | 什么时候读它 |
|---|---|
| [`../README.md`](../README.md) · [`../README_zh.md`](../README_zh.md) | 双语门面，各自外挂一份文档索引表 |
| 本文件 | 找文档从这里开始；四档落点、事实归属、动作索引都在这里 |
| [`architecture/architecture.md`](architecture/architecture.md) | §1 定位与目标 · §2 总体分层 · **§3 仓库目录树**（文件级，全仓唯一目录树事实源）· §4 模块模型 · §5 HAL 分层 · §6 IPC 与事件总线 · §7 关键数据流 · §8 legacy 迁移映射 · §9 Airymax 接入路径 · §11 风险登记 |
| [`status.md`](status.md) | 能力实况 · 里程碑进度 · 本机无法收口的三项 · 在制品 |
| [`process/conventions.md`](process/conventions.md) | §1 分支与提交 · **§2 机器可检查项**（门禁步序与条数的唯一落点）· §3 结构纪律 · §4 DoD · §5 文档纪律 |
| [`process/naming.md`](process/naming.md) | 命名矩阵与模块目录四件套的结构角色——对不上名字时 |
| [`reference/rrd-cli.md`](reference/rrd-cli.md) | §1 用法串 · §2 参数表 · §3 fail-closed 门禁文案 · §4 两条路径绝对化的分工 · §5 启动序列、输出流与退出码 |
| [`reference/rrd-rpc.md`](reference/rrd-rpc.md) | §1 帧层前提 · §2 JSON-RPC 信封约束 · §3 方法白名单 · §4 握手额外门禁 · **§5 错误码映射表** · §6 心跳方向 · §7 发布面 |
| [`reference/rrd-registry.md`](reference/rrd-registry.md) | §1 类型与共享方式 · §2 常量表 · §3 阶段与活边 · §4 时钟约定 · **§5 `rr3_reg_add` 门序** · §6 心跳与到期 · §7 判死与重启计划 · §8 发布归属四道门 · §9 fanout · §10 其余入口 |
| [`process/adr/`](process/adr/) | 决策原文 ADR-0001 ~ ADR-0010；动架构或线格式前先读对应那篇 |
| [`process/dev/0.0.1版本洛克三号开发方案.md`](process/dev/0.0.1版本洛克三号开发方案.md) | 当前版本交付什么、怎么算完成 |

### contracts/ · base/ · modules/ · tools/ · .github/

| 路径 | 什么时候读它 |
|---|---|
| [`../contracts/README.md`](../contracts/README.md) | 字节级契约入口：内容 · topics.toml 裁定 · 演进规则 · 标准歧义登记 |
| [`../contracts/events/README.md`](../contracts/events/README.md) | 给某个事件负载登记 JSON Schema 时 |
| [`../contracts/golden/README.md`](../contracts/golden/README.md) | 对同机字节基准向量做编解码校验时 |
| [`../base/README.md`](../base/README.md) | 基座层为什么承载横切关注点、模块为什么不碰它 |
| [`../base/os/README.md`](../base/os/README.md) | Buildroot 层：内核、A/B 分区与 maskrom 自烧录 |
| [`../base/os/patches/README.md`](../base/os/patches/README.md) | 建镜像前内核/U-Boot 的 dts 与 defconfig 核对清单、补丁回移流程、交叉产物注入方式 |
| [`../base/ota/README.md`](../base/ota/README.md) | OTA 布局文档与槽位 CLI（不写实际槽位写入） |
| [`../base/runtime/README.md`](../base/runtime/README.md) | 基座 Rust workspace 的 crate 职责表与依赖预算 |
| [`../base/runtime/rr-ipc/README.md`](../base/runtime/rr-ipc/README.md) | 协议层 + 传输层：帧读写、契约版本再导出 |
| [`../base/runtime/rr-bus/README.md`](../base/runtime/rr-bus/README.md) | 总线常量层，及订阅/QoS/回压的落点位置 |
| [`../base/runtime/rr-registry/README.md`](../base/runtime/rr-registry/README.md) | manifest 字段字典与主题注册表校验器 |
| [`../base/runtime/rr-hal/README.md`](../base/runtime/rr-hal/README.md) | HAL 的 trait 面、数据类型与后端规划 |
| [`../base/runtime/rr-ota/README.md`](../base/runtime/rr-ota/README.md) | 升级"双轨"的类型形状 |
| [`../base/runtime/rrd/README.md`](../base/runtime/rrd/README.md) | 守护进程装配层：supervisor / health / config / 在册表宿主 |
| [`../modules/README.md`](../modules/README.md) | 模块名册与固定五件（加模块第一站） |
| [`../modules/rr3-sense/README.md`](../modules/rr3-sense/README.md) | 感知：麦克风阵列、环境传感器、IMU |
| [`../modules/rr3-act/README.md`](../modules/rr3-act/README.md) | 执行：舵机、LED、反射环宿主 |
| [`../modules/rr3-interact/README.md`](../modules/rr3-interact/README.md) | 交互：表情屏、TTS 播放、Web 面板 |
| [`../modules/rr3-power/README.md`](../modules/rr3-power/README.md) | 电源：电池、充电、低压关断兜底 |
| [`../modules/rr3-connect/README.md`](../modules/rr3-connect/README.md) | 连接：WiFi 链路、云端会话、蓝牙配网 |
| [`../modules/rr3-brain/README.md`](../modules/rr3-brain/README.md) | 认知（Python）：ASR/LLM/TTS 编排与对话策略 |
| [`../tools/README.md`](../tools/README.md) | 主机侧工具名册：目录 × bin × 用途 |
| [`../tools/ci/README.md`](../tools/ci/README.md) | `check.sh` 每步跑什么、为什么这么跑 |
| [`../tools/mkmodule/README.md`](../tools/mkmodule/README.md) | 用 `rr3-mkmod` 生成模块骨架时 |
| [`../tools/sim/README.md`](../tools/sim/README.md) | 没有板子要跑全链路时（`rr3-sim`） |
| [`../tools/rrctl/README.md`](../tools/rrctl/README.md) | 总线侧运维 CLI 的命令面（`rr3-ctl`） |
| [`../.github/README.md`](../.github/README.md) | GitHub 侧工作流声明与它的实际地位 |

## 我该读哪篇（按动作）

| 我要…… | 读这里 |
|---|---|
| 建立整体图（新人） | [`architecture/architecture.md`](architecture/architecture.md) §1–§3 → 下文"阅读顺序" |
| 加一个模块 | [`../modules/README.md`](../modules/README.md)（五件）+ [`../tools/mkmodule/README.md`](../tools/mkmodule/README.md) + [`process/naming.md`](process/naming.md) |
| 登记或改一个主题 | [`../contracts/README.md`](../contracts/README.md) 的 topics.toml 裁定，主题表本身在 [`../contracts/topics.toml`](../contracts/topics.toml) |
| 定义一个事件负载 | [`../contracts/events/README.md`](../contracts/events/README.md) |
| 查 128B 头 / payload 枚举的字节布局 | [`../contracts/envelope.schema.json`](../contracts/envelope.schema.json) + [`../contracts/golden/README.md`](../contracts/golden/README.md) |
| 起 rrd、查参数与默认值 | [`reference/rrd-cli.md`](reference/rrd-cli.md) §2、§5 |
| 调上行 RPC、对错误码 | [`reference/rrd-rpc.md`](reference/rrd-rpc.md) §5（帧层前提看 §1） |
| 查在册表的阶段、门序与退避 | [`reference/rrd-registry.md`](reference/rrd-registry.md) §3、§5、§7 |
| 提交前对门禁 | [`process/conventions.md`](process/conventions.md) §2 + [`../tools/ci/README.md`](../tools/ci/README.md) |
| 看进度实况与在制品 | [`status.md`](status.md) |
| 名字对不上（目录 / crate / bin / trait） | [`process/naming.md`](process/naming.md) |
| 改 Buildroot 镜像与分区 | [`../base/os/README.md`](../base/os/README.md) + [`../base/os/board/radxa-zero3/flash-maskrom.md`](../base/os/board/radxa-zero3/flash-maskrom.md) |
| 动 OTA 槽位与双轨 | [`../base/ota/README.md`](../base/ota/README.md) + [`../base/runtime/rr-ota/README.md`](../base/runtime/rr-ota/README.md) |
| 无硬件跑通整条链路 | [`../tools/sim/README.md`](../tools/sim/README.md) |
| 对照 legacy ESP32 固件 | [`../legacy/README.md`](../legacy/README.md) + [`architecture/architecture.md`](architecture/architecture.md) §8 |

## 事实归属（改文档前先对这张表）

| 事实类型 | 唯一落点 | 其他文档的写法 |
|---|---|---|
| 门禁步序、测试条数 | [`process/conventions.md`](process/conventions.md) §2 | "步序与条数见 conventions §2"——`tools/ci/check.sh` 头部注释已立此规 |
| 已实装 / 未实装、里程碑进度 | [`status.md`](status.md) | 不写。`architecture/` 只写设计语义，不写"当前 HEAD 有没有" |
| 主题清单与 kind | [`../contracts/topics.toml`](../contracts/topics.toml) | 只写"如何登记、如何校验、前缀归属"，不抄表 |
| manifest 字段字典 | [`../base/runtime/rr-registry/src/module.schema.toml`](../base/runtime/rr-registry/src/module.schema.toml) | 只写字段语义与一份合规示例 |
| 字节布局（128B 头 / payload 枚举） | [`../contracts/envelope.schema.json`](../contracts/envelope.schema.json) + [`../contracts/golden/`](../contracts/golden/) | 只写语义与歧义裁定 |
| 运行期接口字面量（rrd 参数与默认值、RPC 方法与错误码、在册表常量与门序） | [`reference/`](reference/) 三件 | 架构文档只留判据与设计语义，指针指向 reference/；断言口径分档（`Display`/错误码逐字 `assert_eq`，横幅只 `contains`，`RRD_UNSUPPORTED` 无用例断言——见各文件顶部），**改代码须同批改 reference/** |

## 阅读顺序

新人：`architecture/architecture.md` §1–§3 → `process/conventions.md`（提交前必读）→
`process/adr/ADR-0002` 与 `ADR-0006`（模块模型与线格式两根支柱）。

改契约/字节格式：先读 [`../contracts/README.md`](../contracts/README.md)（歧义裁定与演进规则），
再动 `contracts/` 本体。

## 实况与规划的边界

**实况与规划分家**：架构文档只写目标架构与设计语义，不承载"某能力今天有没有"这类实况——
后者唯一落在 [`status.md`](status.md)（见上文"事实归属"表）。里程碑的**内容、验收标准与每条的
当前进度都在 `status.md` 的"里程碑进度"一节**，架构文档 §10 只留一行指向它：同一张表抄两份
必然漂移，所以只留一份。

写文档时不要把设计期草图当既成事实——这是本项目已发生过的最大一次订正。
