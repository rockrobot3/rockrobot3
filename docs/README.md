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
| [`reference/rrd-cli.md`](reference/rrd-cli.md) · [`reference/rrd-rpc.md`](reference/rrd-rpc.md) · [`reference/rrd-registry.md`](reference/rrd-registry.md) | 运行期接口的可照抄事实：启动参数与默认值、RPC 方法与错误映射、在册表阶段与退避上限 | 与代码同批变更；引用须与代码逐字一致；代码定位一律用**符号名锚定**（如 `registry.rs 的 rr3_reg_beat`），不写行号（行号随代码漂移） |
| [`process/conventions.md`](process/conventions.md) | 开发规约：分支 / 提交 / CI 门禁 / DoD | 门禁唯一入口 `bash tools/ci/check.sh`（其 ⑤ 文档门对仓内每个已跟踪 md 兜底：不得为空、相对链接不得断；仓内每份 README 还必须出现在下文库地图里，漏链即红。本索引自身也在覆盖内）；**步序与条数只登记在其 §2**，本索引不抄数字 |
| [`process/naming.md`](process/naming.md) | 命名唯一权威（`rr3-` 前缀矩阵、模块目录四件套的四类结构角色） | `rr3-mkmod` 生成自检 + `rr-registry` 契约测试强制，不靠人记 |
| [`process/adr/`](process/adr/) | 决策记录 ADR-0001 ~ ADR-0017（平台选型 / 模块模型 / IPC 选型 / Airymax 接入 / 命名规约 / AiryIPC 线格式 / 控制面分层 / 传输层 trait / 依赖仲裁 / 控制通道与查询面 / shm 传输 / 能力格坐标系与装配单 / OS 双轨 / Python 模块接入与 M4 对话闭环 / 板卡策略 / brain 迁移路线 / 模块内部分层） | 架构文档每节顶部回链对应 ADR |
| [`process/dev/0.0.2版本洛克三号开发方案.md`](process/dev/0.0.2版本洛克三号开发方案.md) | 历史存档（M5 主战场，已收口；同目录 v0.0.1 是 M0 骨架存档） | 收口对账已由 status.md 锚点承接 |
| [`process/dev/0.0.3版本洛克三号收口方案.md`](process/dev/0.0.3版本洛克三号收口方案.md) | 历史存档（M5 最后一跳 + M6 OTA 写侧，已收口） | 收口对账已由 status.md 锚点承接 |
| [`process/dev/0.0.4版本洛克三号消化方案.md`](process/dev/0.0.4版本洛克三号消化方案.md) | 历史存档（legacy 待融合三项 + 退役批，已收口） | 收口对账已由 status.md 锚点承接 |
| [`process/dev/0.0.5版本洛克三号对齐与裁定方案.md`](process/dev/0.0.5版本洛克三号对齐与裁定方案.md) | 执行中（2026-10-08 增补轮四定位升格：裁定批 → 裁定 + 板载开发前技术落地批；重裁轮 §2.1 R1–R6、增补轮 §2.2 R7–R12、增补轮二 §2.3 R13–R16、增补轮三 §2.4 R17–R22、增补轮四 §2.5 R23–R29 已裁定） | 收口对账已由 status.md 锚点承接 |
| [`process/dev/0.0.5增补轮四——板载开发前技术落地全案.md`](process/dev/0.0.5增补轮四——板载开发前技术落地全案.md) | 增补轮四全案（两板开工前技术点落地：CI 回执 / counters 节流 / 真件装配与标定手册 / espidf std 冒烟 / 桥设计 / e2e 判据 / S3 预案；批W0–W7 WBS 与深度基准四件套） | 按批W 序依序执行，裁定记录回填 0.0.5 方案 §2.5 |
| [`process/dev/0.0.6版本洛克三号树莓派验证方案.md`](process/dev/0.0.6版本洛克三号树莓派验证方案.md) | 当前版本方案与 Definition of Done（修订版 6：两阶段 + 批F5a/F5b 重排 + 真件矩阵八行 + §5.4 BOM 定案 + §5.5 板上工程执行手册；0.0.7 ESP32 / 0.0.8 brain 专项排期已钉） | 收口评审按此逐条对账 |
| [`process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md`](process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md) | 已裁定执行中（修订版 4：Q1–Q6 已随 R20 裁定回填；层③④升必达（R19），批D1/D2 + §9 四层真板执行手册；批A 落档 + 批A' 工具链绿 + 批B 层① 绿（2026-10-07——xtensa 编译期 golden 对拍第④路 PASS），批C 解禁） | 按修订版 4 WBS 依序执行，执行细则见 §9 |
| [`process/dev/0.0.8版本洛克三号模块内部分层实施方案.md`](process/dev/0.0.8版本洛克三号模块内部分层实施方案.md) | 已立项待实施（0.0.5 §2.4 R17"各模块拆分清单"的细化：rr3mod 签名级规格 §2 + 五模块目标目录树与迁移映射 §4 + 批0–批6 实施序列 §5 与每批验收四件套 §6；与 brain 迁移同窗，与 0.0.6/0.0.7 真件批不混排） | 实施批开工按 §5 依序执行、按 §6 四件套留痕 |
| [`process/dev/真件装配与标定手册.md`](process/dev/真件装配与标定手册.md) | 真件到货桌面级验收、装配与标定 runbook（0.0.5 增补轮四 批W5：USB 麦 UAC / SG90+UBEC / MAX7219 / TP4056+保护板 / INA219 两点标定 / ESP32 UART2，每件四件套；型号权威=0.0.6 §5.4 BOM 定案，接线权威=§5.5.2） | 器件到货即照单执行；型号差异先改手册与 BOM 表再装配 |
| [`process/legacy-digest.md`](process/legacy-digest.md) | legacy/ 逐组件消化盘点（已归档）：三分类台账、证据路径 | 查"某 legacy 语义在新架构哪"（`legacy/` 整树已随 0.0.4 退役批删除） |
| [`status.md`](status.md) | 唯一实况快照：能力 × 状态 × 证据路径、里程碑进度（M0–M6 当前实况）、本机无法收口的项、在制品 | 收口时整篇重写，不做增量修补；**测试条数一律不抄进来**（见其开篇规矩） |

## 库地图（按目录）

**本索引是文档的入口，不是目录树的事实源**：文件是否存在以
[`architecture/architecture.md`](architecture/architecture.md) §3 为准，模块名册以
[`../modules/README.md`](../modules/README.md) 为准，crate 职责以
[`../base/runtime/README.md`](../base/runtime/README.md) 为准。下表只回答"这个目录下有哪份
README、什么时候该点开它"；小节名以文档自身为准，本表只作导航。`legacy/` 已退役
（0.0.5 §2.4 R21——git 历史 + tag `v0-pre-legacy` 可回溯），不进本表，⑤ 门同样
不要求它出现在导航里（下文动作索引留了一条对照入口，那是有意的）。新增 README 不写进
本表即判红。

### 仓根 · docs/

| 路径 | 什么时候读它 |
|---|---|
| [`../README.md`](../README.md) · [`../README_zh.md`](../README_zh.md) | 双语门面，各自外挂一份文档索引表 |
| 本文件 | 找文档从这里开始；四档落点、事实归属、动作索引都在这里 |
| [`architecture/architecture.md`](architecture/architecture.md) | §1 定位与目标（底座 payload、三层模型、快速部署三步）· §2 总体分层 · **§3 仓库目录树**（文件级，全仓唯一目录树事实源，含规划落点注记）· §4 模块模型（§4.3 能力格坐标系与装配单）· §5 HAL 分层 · §6 IPC 与事件总线 · §7 关键数据流 · §8 legacy 迁移映射 · §9 Airymax 接入路径 · §11 风险登记 |
| [`architecture/板卡设计规格书.md`](architecture/板卡设计规格书.md) | ADR-0015 过桥阶段实施档：核心板选型核对单 · 电源树与工作待机 <1W 预算 · 引脚/格位预算 · 连接器与设计规则 · 首版验收计划 · 开放项表 |
| [`status.md`](status.md) | 能力实况 · 里程碑进度 · 本机无法收口的项 · 在制品 |
| [`process/conventions.md`](process/conventions.md) | §1 分支与提交 · **§2 机器可检查项**（门禁步序与条数的唯一落点）· §3 结构纪律 · §4 DoD · §5 文档纪律 |
| [`process/naming.md`](process/naming.md) | 命名矩阵与模块目录四件套的结构角色——对不上名字时 |
| [`reference/rrd-cli.md`](reference/rrd-cli.md) | §1 用法串 · §2 参数表 · §3 fail-closed 门禁文案 · §4 两条路径绝对化的分工 · §5 启动序列、输出流与退出码 |
| [`reference/rrd-rpc.md`](reference/rrd-rpc.md) | §1 帧层前提 · §2 JSON-RPC 信封约束 · §3 方法白名单 · §4 握手额外门禁 · **§5 错误码映射表** · §6 心跳方向 · §7 发布面 |
| [`reference/rrd-registry.md`](reference/rrd-registry.md) | §1 类型与共享方式 · §2 常量表 · §3 阶段与活边 · §4 时钟约定 · **§5 `rr3_reg_add` 门序** · §6 心跳与到期 · §7 判死与重启计划 · §8 发布归属四道门 · §9 fanout · §10 其余入口 |
| [`process/adr/`](process/adr/) | 决策原文 ADR-0001 ~ ADR-0017；动架构或线格式前先读对应那篇 |
| [`process/dev/0.0.2版本洛克三号开发方案.md`](process/dev/0.0.2版本洛克三号开发方案.md) | 历史存档：上一版本（M5 主战场）交付了什么 |
| [`process/dev/0.0.3版本洛克三号收口方案.md`](process/dev/0.0.3版本洛克三号收口方案.md) | 历史存档：上一版本（M5 最后一跳 + M6 OTA 写侧）交付了什么 |
| [`process/dev/0.0.4版本洛克三号消化方案.md`](process/dev/0.0.4版本洛克三号消化方案.md) | 历史存档：上一版本（legacy 消化 + 退役批）交付了什么 |
| [`process/dev/0.0.5版本洛克三号对齐与裁定方案.md`](process/dev/0.0.5版本洛克三号对齐与裁定方案.md) | 执行中：上一版本（rrctl 运维面对齐 + 两大演进裁定 + 四轮增补 R1–R29）——增补轮四起兼"板载开发前技术落地批" |
| [`process/dev/0.0.5增补轮四——板载开发前技术落地全案.md`](process/dev/0.0.5增补轮四——板载开发前技术落地全案.md) | 增补轮四全案：两板开工前全部技术点（批W0–W7）落地到哪了 |
| [`process/dev/0.0.6版本洛克三号树莓派验证方案.md`](process/dev/0.0.6版本洛克三号树莓派验证方案.md) | 当前版本交付什么、怎么算完成（修订版 6：树莓派两阶段 + 真件矩阵八行 + BOM 定案 + 板上工程执行手册 §5.5） |
| [`process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md`](process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md) | 下一版本方案：ESP32 系统级验证四层验证梯子（修订版 4：Q1–Q6 已裁，层③④必达，执行手册 §9） |
| [`process/dev/0.0.8版本洛克三号模块内部分层实施方案.md`](process/dev/0.0.8版本洛克三号模块内部分层实施方案.md) | 已立项待实施方案：rr3mod 公共骨架与五模块拆分施工档（签名级规格 / 迁移映射 / 批序列 / 验收四件套） |
| [`process/dev/真件装配与标定手册.md`](process/dev/真件装配与标定手册.md) | 真件到货做验收、装配与标定时照单执行（批W5 每件四件套 + 到货差异登记表） |
| [`process/legacy-digest.md`](process/legacy-digest.md) | legacy（ESP32 冻结区）逐组件消化到哪了（已归档：0.0.4 退役批整树删除，待融合清零） |

### contracts/ · base/ · modules/ · tools/ · .github/

| 路径 | 什么时候读它 |
|---|---|
| [`../contracts/README.md`](../contracts/README.md) | 字节级契约入口：内容 · topics.toml 裁定 · 演进规则 · 标准歧义登记 |
| [`../contracts/events/README.md`](../contracts/events/README.md) | 给某个事件负载登记 JSON Schema 时 |
| [`../contracts/golden/README.md`](../contracts/golden/README.md) | 对同机字节基准向量做编解码校验时 |
| [`../base/README.md`](../base/README.md) | 基座层为什么承载横切关注点、模块为什么不碰它 |
| [`../base/os/README.md`](../base/os/README.md) | OS 双轨层（ADR-0013）：buildroot 保底轨与 distro 快路轨的分工 |
| [`../base/os/buildroot/patches/README.md`](../base/os/buildroot/patches/README.md) | 建镜像前内核/U-Boot 的 dts 与 defconfig 核对清单、补丁回移流程、交叉产物注入方式 |
| [`../base/os/distro/README.md`](../base/os/distro/README.md) | 发行版快路：payload 打包、systemd 单元、已验证板卡笔记 |
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
| [`../tools/espidf-std-hello/README.md`](../tools/espidf-std-hello/README.md) | espidf std 构建链冒烟件（批W1）：版本钉死登记对表（ESP-IDF/esp-idf-sys/esp-clang/ldproxy）与三坑复现必读 |
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
| 改 Buildroot 镜像与分区 | [`../base/os/README.md`](../base/os/README.md) + [`../base/os/buildroot/board/radxa-zero3/flash-maskrom.md`](../base/os/buildroot/board/radxa-zero3/flash-maskrom.md) |
| 选核心板 / 画底板 / 整机硬件询价与验收 | [`architecture/板卡设计规格书.md`](architecture/板卡设计规格书.md)（决策依据在 [`process/adr/ADR-0015`](process/adr/ADR-0015-板卡策略-rk3566核心板过桥与量产自定制.md)） |
| 动 OTA 槽位与双轨 | [`../base/ota/README.md`](../base/ota/README.md) + [`../base/runtime/rr-ota/README.md`](../base/runtime/rr-ota/README.md) |
| 无硬件跑通整条链路 | [`../tools/sim/README.md`](../tools/sim/README.md) |
| 器件到货做验收 / 装配 / 标定 | [`process/dev/真件装配与标定手册.md`](process/dev/真件装配与标定手册.md)（接线对照 [`process/dev/0.0.6版本洛克三号树莓派验证方案.md`](process/dev/0.0.6版本洛克三号树莓派验证方案.md) §5.5.2） |
| 对照 legacy ESP32 固件 | [`process/legacy-digest.md`](process/legacy-digest.md)（归档台账）+ [`architecture/architecture.md`](architecture/architecture.md) §8（`legacy/` 整树已随 0.0.4 退役批删除，源码在 git 历史） |

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
