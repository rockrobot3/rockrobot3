# status —— 唯一实况快照

本文件是仓内**唯一**允许回答"某能力在当前 HEAD 有没有实现"的地方。其余文档只写设计语义，
不写实况；设计文档里出现"已落地/未落地"的字样一律视为待订正（架构文档 §5 就因此整段重写过）。

- **实况锚点**：本快照按提交 `2323b84`（分支 `dev/hubs-01`）逐条读代码写成，不是按规划推断。
- **状态只取三值**：`已实装` / `骨架` / `未设计`。限定条件（如"仅 Unix 参与编译""仅单元层"）
  写在证据列，不塞进状态列——否则三值就退化成自由文本，也就失去了可对比性。
  这套词表里**没有"未实现"这个值**，所以"设计已定但一行没写"的项只能落进 `未设计`（无接口形状）
  或 `骨架`（类型/计划已在、动作缺席）；不软化的保证不在状态列，而在证据列那句实话——
  例如"退避算得出、但无人 spawn"与"依赖仲裁连类型都未定义"就各自写清了缺的是哪一手。
- **本文件不写测试条数**。两平台条数本就不等、且每加一条就变；唯一的登记处是
  [`process/conventions.md`](process/conventions.md) §2（含差额的逐条可枚举解释）。
- **本文件不抄主题表，也不抄 manifest 字段**：主题与 kind 的真相是
  [`../contracts/topics.toml`](../contracts/topics.toml)，manifest 字段字典的真相是
  [`../base/runtime/rr-registry/src/module.schema.toml`](../base/runtime/rr-registry/src/module.schema.toml)。
- 收口时整篇重写，不做增量修补——增量修补会让"上次快照的结论"和"这次的实测"混在一段里。

---

## 能力实况

| 能力 | 状态 | 证据（读代码取得，非规划） |
|---|---|---|
| 128B 定长消息头编解码（Layout C） | 已实装 | [`header.rs`](../base/runtime/rr-ipc/src/protocol/header.rs)：`RR3_IPC_HDR_SIZE = 128`、`Rr3Header::from_bytes` / `to_bytes`（reserved 自动全 0），偏移逐字段对齐契约 |
| 帧读写（半帧 / 坏 magic / 长度不符即拒） | 已实装 | [`frame.rs`](../base/runtime/rr-ipc/src/transport/frame.rs)：`rr3_frame_read` / `rr3_frame_write` / `rr3_frame_to_bytes`；`Rr3FrameError` 区分"头只读了一半就 EOF"与"头本身非法"；校验失败时不得有半帧落进流里 |
| payload 类型前缀（前 2 字节小端枚举） | 已实装 | [`body.rs`](../base/runtime/rr-ipc/src/protocol/body.rs)：`rr3_body_encode` / `rr3_body_split`；枚举 `Rr3PayloadType` 定义在 [`header.rs`](../base/runtime/rr-ipc/src/protocol/header.rs) |
| `trait Transport` 与 uds / shm / tcp 分层 | 未设计 | [`transport/mod.rs`](../base/runtime/rr-ipc/src/transport/mod.rs) 头注明确把签名推给 M2 一并定，理由是"以免现在造一个 ADR-0007 §裁定 3 要推翻的接口"；该目录只有 `frame.rs` 与两个路径常量 `RR3_UDS_PATH_DEFAULT` / `RR3_BUS_PATH_ENV`，**无任何 trait 定义** |
| manifest 校验（字段 + 跨字段规则） | 已实装 | [`schema.rs`](../base/runtime/rr-registry/src/schema.rs) + [`validate.rs`](../base/runtime/rr-registry/src/validate.rs)（入口 `rr3_registry_validate_manifest_text`）；契约面在 [`tests/contract.rs`](../base/runtime/rr-registry/tests/contract.rs) |
| 主题注册表与前缀归属 | 已实装 | [`topics.toml`](../contracts/topics.toml)（内容本身不在此复述）+ [`topics.rs`](../base/runtime/rr-registry/src/topics.rs) 的 `Rr3TopicSet` / `rr3_topic_set_owner`；装载侧规范化与叠加见 [`topics_load.rs`](../base/runtime/rrd/src/topics_load.rs) |
| rrd 在册表：注册 / 心跳 / 死亡记账 / 扇出 / 快照 | 已实装 | **且被连接层实际调用**：[`registry.rs`](../base/runtime/rrd/src/registry.rs) 的 `rr3_reg_add` / `rr3_reg_beat` / `rr3_reg_remove` / `rr3_reg_death` / `rr3_reg_fanout` / `rr3_reg_snapshot`，六个都有 `rr3_lock(&ctx.shared)` 的调用点在 [`conn.rs`](../base/runtime/rrd/src/conn.rs)。（同文件的 `rr3_reg_name_of` 今天**没有**跨文件调用点，它只是"发布分支将来该用哪种取名方式"的约定，见 `registry.rs` 里那段"`sender_name` 绝不得取自帧体"的注释） |
| 在册表阶段机（`Rr3Phase`） | 已实装 | [`registry.rs`](../base/runtime/rrd/src/registry.rs) 的 `enum Rr3Phase { Registered, Running, Degraded, Failed }`。**限定：四个阶段里只有三个有置位路径**——`Degraded` 的 setter 与持久化按其文档注释随 rrd 实现计划的 Task 9/11 落地，今天无从发生；`as_str()` 仍会回显 `"degraded"`，所以词表是完整的 |
| 模块重启的退避计算 | 已实装 | **只算不执行**（派生动作单列为下文的"模块 spawn / 重启执行"一行）：`rr3_reg_death` 返回的重启计划带 `attempt` / `delay_ms` / `entry`，见 [`registry.rs`](../base/runtime/rrd/src/registry.rs)；[`conn.rs`](../base/runtime/rrd/src/conn.rs) 的 `rr3_conn_finish` 把这份计划**写进日志**，并注明 spawn 见 rrd 实现计划 Task 11 |
| 发布归属判定（越权前缀 / 冒充内建命名空间） | 已实装 | **限定：仅到单元层，连接层尚无调用点**。[`registry.rs`](../base/runtime/rrd/src/registry.rs) 的 `RrdRegistry::rr3_reg_publish_check`（返回 `Rr3PublishDeny`）逻辑完整、被同文件内的单元用例覆盖；但 `conn.rs` 只分发 register / heartbeat / unregister，没有发布分支，该函数今天唯一的调用处就是它的单元用例。"已实装并被连接层调用"这个说法在本 HEAD **不成立** |
| rrd 上行 JSON-RPC 三方法 | 已实装 | [`rpc.rs`](../base/runtime/rrd/src/rpc.rs)：`Rr3RpcMethod` = `rr3.register` / `rr3.heartbeat` / `rr3.unregister`，含 `rr3_rpc_parse_request` 的版本与 id 校验、`rr3_rpc_build_result` / `rr3_rpc_build_error` 错误映射 |
| rrd UDS 监听与注册握手 | 已实装 | **限定：仅 Unix**——`conn` / `daemon` 两模块整体挂在 `#[cfg(unix)]` 下。[`daemon.rs`](../base/runtime/rrd/src/daemon.rs) 的 `rr3_daemon_run`：清残留套接字 → `UnixListener::bind` → accept 循环，每连接一条线程；[`conn.rs`](../base/runtime/rrd/src/conn.rs) 的 `rr3_conn_serve` / `rr3_handshake`：首帧必须是发给 `RR3_TASK_RRD` 的 `rr3.register`，`src_task` 由 rrd 分配、伪造者那一帧被拒但连接不断，注册前设读超时收掉静默连接。线级用例：[`tests/register_publish.rs`](../base/runtime/rrd/tests/register_publish.rs) 与 [`tests/socket_banner.rs`](../base/runtime/rrd/tests/socket_banner.rs)，两文件首行都是 `#![cfg(unix)]`，故 Windows 侧它们**不参与编译**（那是 cfg 边界的正确结果，不等于通过）。cfg 边界的自述见 [`lib.rs`](../base/runtime/rrd/src/lib.rs) |
| 模块 spawn / 重启执行 | 未设计 | **一行实现都没有，也没有接口形状**：`base/runtime/rrd/src/` 内 `thread::spawn` 只有两处——[`daemon.rs`](../base/runtime/rrd/src/daemon.rs) 的每连接一线程、[`conn.rs`](../base/runtime/rrd/src/conn.rs) 的那条 writer 线程——都是线程，不是进程派生；全目录搜 `Command::new` / `std::process::Command` 命中为零。[`daemon.rs`](../base/runtime/rrd/src/daemon.rs) 头注逐字："Task 10 在这里加唯一的 watchdog 线程，Task 11 加 spawn/重启。本任务只做：清残留套接字 → bind → accept 循环"——这里的 Task 号属 **rrd 实现计划**，与本文档所属的文档重构计划无关 |
| 到期判活扫描（`rr3_reg_expire`） | 已实装 | **限定：仅单元层，没有线程调用它**。[`registry.rs`](../base/runtime/rrd/src/registry.rs) 的 `rr3_reg_expire(now_ms) -> Vec<Rr3Expiry>` 逻辑完整（`saturating_sub` 防时钟倒退、已 `Failed` 的条目跳过不重复回报、死连接句柄当场丢弃），它的文档注释自己就写着"（唯一的监视线程调用）"——**那个线程还不存在**，本 HEAD 里该函数的调用处全在它自己文件的单元用例内 |
| watchdog 监视线程 | 未设计 | 判活逻辑已就绪（见上一行），但**没有 tick 循环、没有线程、没有接口形状**去驱动它（缺的就是上面那行 `daemon.rs` 头注里的 Task 10）。后果：模块静默僵死时，今天只有它自己断线或 rrd 重启才会被发现。manifest 侧的 `health.watchdog_timeout_ms` 能被解析与跨字段校验（[`manifest.rs`](../base/runtime/rrd/src/manifest.rs) 的 `watchdog_ms`，并要求 `heartbeat_ms < watchdog_ms`），但那只证明参数进得来，不证明有人按它判活 |
| 依赖仲裁 | 未设计 | **连类型都未定义**：全仓 rrd 内没有 resolve / arbitrate 相关的任何实现（命中的 `rr3_contract_resolved` 在 [`topics_load.rs`](../base/runtime/rrd/src/topics_load.rs)，那是契约路径规范化，与依赖仲裁无关） |
| CLI 参数与默认值注入 | 已实装 | [`cli.rs`](../base/runtime/rrd/src/cli.rs) + [`main.rs`](../base/runtime/rrd/src/main.rs)（套接字路径在打印横幅**之前**绝对化，横幅与 bind 取同一个值，由 [`tests/socket_banner.rs`](../base/runtime/rrd/tests/socket_banner.rs) 钉住）；参数与默认值的字面量归 `reference/rrd-cli.md`（**本档**计划的 Task 7 落位），此处不复述 |
| HAL trait 形状与占位后端 | 骨架 | [`lib.rs`](../base/runtime/rr-hal/src/lib.rs) 定义了 `Rr3GpioOut` / `Rr3PwmServo` / `Rr3AudioSource` / `Rr3Power` / `Rr3Link` 与 `Rr3UnavailableSource`（所有能力一律回 `Rr3HalError::Unsupported`）；具名后端 `linux` / `sim` **只有注释没有实现**——[`backend.rs`](../base/runtime/rr-hal/src/backend.rs) 通体只有 `RR3_BACKEND_LINUX` / `RR3_BACKEND_SIM` / `RR3_BACKEND_PLACEHOLDER` 三个常量 |
| A/B OTA 切换 | 骨架 | [`rr-ota/src/lib.rs`](../base/runtime/rr-ota/src/lib.rs) 只冻结类型形状（`Rr3Slot`）；[`rr3-ota-ctl/src/main.rs`](../base/ota/rr3-ota-ctl/src/main.rs) 的 `rr3_ota_ctl_refuse_switch` 对任意目标恒拒，注释写明"随 M6 落地，当前拒绝执行" |
| Buildroot OS 层 | 骨架 | [`base/os/`](../base/os/) 目录下只有一个 README，无 recipe、无 defconfig、无 genimage 配置 |
| `rr3-act` / `rr3-connect` / `rr3-interact` / `rr3-power` / `rr3-sense` | 骨架 | [`modules/*/src/main.rs`](../modules/) 五个 Rust 入口，实测 11–17 行，只打印自述；各目录的 `module.toml` 由 [`tests/contract.rs`](../base/runtime/rr-registry/tests/contract.rs) 逐目录读回判合规（`rr3-brain` 那份也在内） |
| `rr3-brain` | 骨架 | **不是 Rust 模块，没有 `src/main.rs`**：入口是 [`rr3_brain/__main__.py`](../modules/rr3-brain/rr3_brain/__main__.py)，`dialog.py` 与 `cloud/{asr,llm,tts}.py` 均为十几行的自述占位，注释写明"不做任何网络调用：握手/注册随 M2 与 M4 接入"；配置见 [`config.toml`](../modules/rr3-brain/config.toml) |
| `rr3-ctl` | 骨架 | [`tools/rrctl/src/commands.rs`](../tools/rrctl/src/commands.rs) 的 `COMMANDS = ["status", "health", "logs", "ota"]`，各命令均只回显计划文案（"实现随 M2"）；参数解析与套接字优先级在 [`main.rs`](../tools/rrctl/src/main.rs) 已实装 |
| `rr3-sim` | 骨架 | [`tools/sim/src/devices.rs`](../tools/sim/src/devices.rs) 的 `rr3_sim_silence` 只出静音帧以钉住尺寸与时长换算，注释写明"波形注入（正弦/噪声）随 M2" |
| `contracts/events/*.schema.json` | 未设计 | [`contracts/events/`](../contracts/events/) 目录下只有一个 README，无任何 schema 文件 |

---

## 里程碑进度

"内容"与"验收标准"两栏按架构文档 §10 原文逐字抄录（该表在 §10 仍有一份，待下一次架构文档收口
时删；迁入与删原分两步，是为了让删除那次改动单独可审）。**当前实况**一栏是本快照读代码实测的
结论，不是对验收标准的复述或期望。

| 里程碑 | 内容（§10 原文） | 验收标准（§10 原文） | 当前实况（`2323b84` 实测） |
|---|---|---|---|
| M0 | 仓库骨架 + ADR-0001~0005 + legacy 冻结 | 目录树落地，rr3-mkmod 能生成合规模块 | 目录树已落地，ADR 已写到 0007（超出原验收范围），`rr3-mkmod` 的产物被回灌进契约用例 —— **已收口** |
| M1 | base/os：Buildroot 在 Radxa ZERO 3 跑通 | 串口出 shell；maskrom 自烧录镜像成功 | [`base/os/`](../base/os/) 只有一个 README，无 recipe / defconfig —— **未开工** |
| M2 | rrd MVP：A-IPC 协议层 + uds transport + registry + sim 双假模块 | 注册/心跳/崩溃自动重启；IPC-T-001/002/003 通过 | **部分交付，且交付面与原规划错位**。已到位：协议层（128B 头 + 帧读写 + payload 类型前缀）、在册表的注册/心跳/死亡记账/扇出/快照（被连接层调用）、上行三方法、UDS 监听与注册握手（仅 Unix）——注意原规划把 "uds" 记为仍缺，这一条**已不成立**。仍缺三项：① **崩溃自动重启**——退避计划算得出、到期扫描 `rr3_reg_expire` 也写好了，但既无人 spawn 进程也无 tick 线程去调它（缺的是 rrd 实现计划 Task 10 / 11）；② `trait Transport` 与 sim 双假模块（`transport/mod.rs` 只有注释与两个路径常量）；③ IPC-T-003 跨架构字节布局等值——需交叉编译两端对拍，当前 HEAD 无此能力，见 [`contracts/README.md`](../contracts/README.md) 与 [`envelope.schema.json`](../contracts/envelope.schema.json) 的 `x-rock3-v0.0.1-uncovered` |
| M3 | rr3-sense + rr3-interact：ALSA 采集与播放 | 板上录音回放，VAD 事件上总线，音频走 STREAM | HAL 只有 trait 形状与恒回 `Unsupported` 的占位后端，`linux` / `sim` 无实现 —— **未开工** |
| M4 | rr3-brain：移植 asr/llm/tts | 全语音对话闭环（legacy 功能等价） | `rr3-brain` 各 py 文件均为自述占位，注释写明不做任何网络调用 —— **未开工** |
| M5 | rr3-act / rr3-connect / rr3-power | 语音控继电器 + WiFi 配网 + 电量上报；shm 控制面初版 | 五个 Rust 模块入口均为 11–17 行自述；shm 只在 `transport/mod.rs` 注释里挂号 —— **未开工** |
| M6 | A/B OTA + 恢复出厂 | 断电升级不 brick，坏 slot 自动回滚；genimage 预留模块独立可写分区 | `rr-ota` 只有类型形状 `Rr3Slot`，`rr3-ota-ctl switch` 恒拒 —— **未开工** |

---

## 本机无法收口的三项

这三项**不是待办清单里的模糊项，是本机没有能力证明的项**。任何"应该是好的"的推断都不构成
收口证据，所以下面如实写明缺的是哪一手证据，不软化。

| # | 无法收口的项 | 缺的证据 | 现状与后果 |
|---|---|---|---|
| 1 | GitHub Actions 从未在本 remote 触发过 | 一次真实的 workflow 运行记录 | 仓里的 workflow 面向 GitHub，remote 是 atomgit，它**一次都没有跑过**。门禁实际只靠本地 `bash tools/ci/check.sh`，也就是说：门禁的强度等于"提代码的人记不记得跑它"。步序见 [`process/conventions.md`](process/conventions.md) §2，脚本入口 [`tools/ci/check.sh`](../tools/ci/check.sh) |
| 2 | 设备侧 `idf.py build` 的密钥门未验证 | 在装有 ESP-IDF 的机器上的一次成功构建 + 一次无密钥失败的对照 | 本机没有 ESP-IDF，也没有目标设备；`legacy/` 是冻结区，不在本轮改动范围内，但"固件侧改一处配置会不会撞上密钥门"这件事至今无人拿过证据 |
| 3 | 泄漏 API key 的提供方侧轮换未确认 | 提供方控制台里那把 key 已失效/已替换的凭据 | 仓内曾经出现过的 key 从 git 历史里抹不掉；**从仓库角度能做的都做完了**（移到 `legacy/` 冻结、文档不再复述），但"这把 key 现在还能不能用"只有提供方能回答，这一条不闭环，第 3 项就一直是敞口 |

---

## 在制品

**本快照记的实况就是 `2323b84` 的树，工作树此刻没有未提交的实现改动。**

以下增量**不写进任何文档**，因为它们还没进树；写进去就等于把"某人在做"记成"仓里有"：

- 并行协作者的在途改动（本轮盘点期间 `git status` 短暂出现过
  [`../base/runtime/rr-ipc/src/protocol/body.rs`](../base/runtime/rr-ipc/src/protocol/body.rs)
  与 [`../base/runtime/rrd/src/cli.rs`](../base/runtime/rrd/src/cli.rs) 两处 `M`，提交前已消失）。
  按本仓规矩：**实况只认已进树的提交**，未进树的一律记为在制品、不进能力表。
- 曾经挂在"在制品"名下的 socket 横幅绝对路径与 `--socket` 注入参数**已经进树**了
  （`4204656` 横幅回显绝对路径、`13dc1d5` 默认值改注入参数、`1cfde17` 契约路径装载即规范化）。
  本轮盘点时起草的旧稿把它们当未跟踪文件，是这次快照里最需要订正的一处。

**已知偏差（有意保留，不是遗漏）：**

- [`legacy/README.md`](../legacy/README.md) 第 3 行仍指向 `docs/architecture.md`，该路径在四档
  迁移后已不存在（现为 [`architecture/architecture.md`](architecture/architecture.md)）。
  `legacy/` 是冻结区，规约是"不动它"，所以这条断链**不修**，在此记为已知偏差。
- 代码注释里的 `Task N`（如 `daemon.rs` 的"Task 10 加 watchdog"、`registry.rs` 的"Task 7 已落地"）
  指的是 **rrd 实现计划**的任务号，与本轮文档重构计划无关。两者同号不同事，读文档时别把它们
  对齐；这些注释不属于本任务订正范围。
