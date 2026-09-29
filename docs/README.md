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
| [`process/conventions.md`](process/conventions.md) | 开发规约：分支 / 提交 / CI 门禁 / DoD | 门禁唯一入口 `bash tools/ci/check.sh`（其 ⑤ 文档门对仓内每个已跟踪 md 兜底：不得为空、相对链接不得断，本索引也在其覆盖内）；**步序与条数只登记在其 §2**，本索引不抄数字 |
| [`process/naming.md`](process/naming.md) | 命名唯一权威（`rr3-` 前缀矩阵、模块目录四件套的四类结构角色） | `rr3-mkmod` 生成自检 + `rr-registry` 契约测试强制，不靠人记 |
| [`process/adr/`](process/adr/) | 决策记录 ADR-0001 ~ ADR-0007（平台选型 / 模块模型 / IPC 选型 / Airymax 接入 / 命名规约 / AiryIPC 线格式 / 控制面分层） | 架构文档每节顶部回链对应 ADR |
| [`process/dev/0.0.1版本洛克三号开发方案.md`](process/dev/0.0.1版本洛克三号开发方案.md) | 当前版本改造方案与 Definition of Done | 收口评审按此逐条对账 |
| [`status.md`](status.md) | 唯一实况快照：能力 × 状态 × 证据路径、里程碑进度（M0–M6 当前实况）、本机无法收口的三项、在制品 | 收口时整篇重写，不做增量修补；**测试条数一律不抄进来**（见其开篇规矩） |

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
