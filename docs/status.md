# status —— 唯一实况快照

本文件是仓内**唯一**允许回答"某能力在当前 HEAD 有没有实现"的地方。其余文档只写设计语义，
不写实况；设计文档里出现"已落地/未落地"的字样一律视为待订正（架构文档 §5 就因此整段重写过）。

- **实况锚点**：本快照按锚点提交 `dc3a1d6`（分支 `dev/hubs-01`，0.0.7 批G 落档树）逐条读代码写成，不是按规划推断；
  本文里的"当前 HEAD"一律指这个锚点提交的那棵树，锚点之外的增量不进本表。
- **状态只取三值**：`已实装` / `骨架` / `未设计`。限定条件（如"仅 Unix 参与编译""仅单元层"）
  写在证据列，不塞进状态列——否则三值就退化成自由文本，也就失去了可对比性。
  这套词表里**没有"未实现"这个值**，所以"设计已定但一行没写"的项只能落进 `未设计`（无接口形状）
  或 `骨架`（类型/计划已在、动作缺席）；不软化的保证不在状态列，而在证据列那句实话——
  例如"tcp 的线上签名已定型、实现一行没写"就写清了缺的是哪一手。
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
| 128B 定长消息头编解码（Layout C） | 已实装 | [`header.rs`](../base/runtime/rr-ipc/src/protocol/header.rs)：`RR3_IPC_HDR_SIZE = 128`、`Rr3Header::from_bytes` / `to_bytes`（reserved 自动全 0），偏移逐字段对齐契约；另有**编译期 const 断言**（偏移连续 / 头长合计 128 / magic LE 线上字节 31 45 52 41 / payload 枚举值冻结）——任意目标编译通过即成立，不依赖测试有人跑 |
| 帧读写（半帧 / 坏 magic / 长度不符即拒） | 已实装 | [`frame.rs`](../base/runtime/rr-ipc/src/transport/frame.rs)：`rr3_frame_read` / `rr3_frame_write` / `rr3_frame_to_bytes`；`Rr3FrameError` 区分"头只读了一半就 EOF"与"头本身非法"，另有 `Unsealed`（缓冲未 seal 就取整帧字节）；校验失败时不得有半帧落进流里 |
| payload 类型前缀（前 2 字节小端枚举） | 已实装 | [`body.rs`](../base/runtime/rr-ipc/src/protocol/body.rs)：`rr3_body_encode` / `rr3_body_split`；枚举 `Rr3PayloadType` 定义在 [`header.rs`](../base/runtime/rr-ipc/src/protocol/header.rs) |
| 所有权移交整帧缓冲（零拷贝落点） | 已实装 | [`buf.rs`](../base/runtime/rr-ipc/src/transport/buf.rs)：`Rr3FrameBuf` 单块连续内存（`[0..128)` 头预留区 + payload）、`rr3_seal` 原地幂等编头、`rr3_framebuf_read` 读侧同一错误语义；`Rr3Frame` 保留为纯值视图，两形状低成本互转（ADR-0007 裁定 3 / ADR-0008） |
| `trait Transport` 与 uds / shm / tcp 分层 | 已实装 | **uds 与 shm 都是真实现；tcp 只有线上签名、一行实现都没有**（ADR-0008 裁定 4 的"不写半成品"对 tcp 仍成立）。[`transport/mod.rs`](../base/runtime/rr-ipc/src/transport/mod.rs)：三 trait——`Rr3TransportConn`（`rr3_send_buf` 消费缓冲 / `rr3_recv_buf` 返回所有权缓冲；对一切 `Read + Write + Send` 有 blanket 实现，send 前幂等 seal）、`Rr3TransportListener`、`Rr3TransportEndpoint`（cleanup → bind）；[`uds.rs`](../base/runtime/rr-ipc/src/transport/uds.rs)（`#[cfg(unix)]`）：`Rr3UdsListener`（`type Conn = UnixStream`）与客户端入口 `rr3_connect`；shm 见下一行。**不引 async**（ADR-0008 裁定 1） |
| shm 传输（单写者多读者环形缓冲，实时控制面） | 已实装 | ADR-0011，[`shm.rs`](../base/runtime/rr-ipc/src/transport/shm.rs)：核心环平台无关——64B 区域头（magic `RR3S` / version / cap / **head 单共享变量**，Release 发布 Acquire 读取）+ 字节环，环内记录 = 一整帧（128B 头 + payload）；seqlock 式复核（拷出后 head 不变才接受）+ 半容量守卫（帧 ≤ cap/2）证明无撕裂；单写者多读者、**丢最旧保最新**——写者永不阻塞永不失败，读者落后过半圈重同步并计 `lost`（不静默）。角色分型 `Rr3ShmWriterConn` / `Rr3ShmReaderConn`（方向不匹配的方法恒报 Io），读者 `rr3_attach` 对位 uds 的 `rr3_connect`，阻塞 `recv_buf` = 200µs 退避轮询（无通知原语）。POSIX 附着段（`cfg(any(target_os = "linux", target_os = "macos"))`）：**手写 6 函数 extern 面**（shm_open / shm_unlink / ftruncate / mmap / munmap / close，不引 libc crate，依赖预算零增量）；macOS 的 shm fd 只支持 mmap（read/pread 报 ENXIO/ESPIPE），附着走"64B 探头映射校验 → 按 cap 全量映射"两步法。默认容量 1MiB（`rr3_bind_with_cap` 可建大区），对象名默认 `/rock3-ipc-shm`、可用 `RR3_SHM_NAME` 环境变量覆盖。**不做清单（ADR-0011）**：tcp、rrd 接线（[`conn.rs`](../base/runtime/rrd/src/conn.rs) 仍 UnixStream）、F_ZEROCOPY grant、F_URGENT 行为差异、通知原语、可靠交付、跨进程 e2e |
| IPC-T-003 跨架构对拍（x86 ↔ aarch64 字节布局一致） | 骨架 | **三方对拍 ①②本机已过、③容器真跑未发生（已接 CI 平台条件）**：①宿主 x86_64（Intel Mac）`ipct003_dump` 输出与 [`golden/event-header-128.hex`](../contracts/golden/event-header-128.hex) 逐字节一致（F5 实跑，登记于 conventions.md §2 平台条件脚本段）；②`aarch64-unknown-linux-gnu` 目标已装，交叉 `cargo check` PASS——header.rs 的编译期 const 断言已在 aarch64 目标求值；③`linux/arm64` 容器真跑 SKIP（本机无 Docker）。[`tools/ci/ipct003.sh`](../tools/ci/ipct003.sh) 缺环境打 `SKIP` 明说、不静默冒充已验证；`--strict` 已接入远端 CI——ci.yml 的 `ipct003` job（540ec0f：ubuntu runner + QEMU arm64 + 容器真跑，见 [`../.github/workflows/ci.yml`](../.github/workflows/ci.yml)），远端跑绿一次即可把本行与"本机无法收口"第 4 条结项。登记见 [`envelope.schema.json`](../contracts/envelope.schema.json) 的 `x-rock3-v0.0.1-uncovered` 与 [contracts/README.md](../contracts/README.md) 演进登记。0.0.7 批B 起 `ipct003.sh` 扩**四路**（xtensa 编译期对拍路 PASS），见下行 |
| 非 Linux 主控契约 parity（ESP32/xtensa，0.0.7 四层梯子） | 已实装（ESP32 档） | 0.0.7 四层全真板收官（R19，2026-10-11）——**层① 字节契约**：rr-ipc no_std 化（`#![no_std]` + 条件 `extern crate std`，协议层纯 core+alloc，批B `a860405`），`ipct003.sh` 四路化、xtensa 编译期 golden 对拍 PASS（实跑 3/4：宿主 PASS / aarch64 PASS / xtensa PASS / arm64 容器 SKIP 本机无 Docker）；机制核 rr-core（常量/寻址/帧组装 0.0.5 批D①、runtime 执行环 批D②、控制原语+安全核 批D③）xtensa 双线编译绿（none-elf 裸金属 + espidf std）。**层② UART 载体**：rr-ipc `transport/uart.rs`（宿主侧 raw termios）+ esp32-uart-echo 裸金属 echo 夹具（批C `46edb3c`），板级实测 128B 头 + payload 完整收发、半帧/坏 magic 双拒不进死锁（批C④⑤ `6d3a1ee`，espflash 4.x app descriptor 现场补）。**层③ 机制核真板消费者**：espidf-sentinel 内跑 `rr-core::runtime`（批D1 `22783ea`/`f72c0d4`）——harness 14 PASS（注册 / 8 拍心跳 195–204ms / round-trip 对拍），三点堆采样 roundtrip min_free=250620B ≈ 启动堆 85%（D1→D2 不触发），线程栈 16KB 钉运行下限。**层④ 桥接 e2e**：rr3-bridge UART↔UDS 中继（`955ef14`）+ 真板 e2e PASS=16 FAIL=0（批E `870cfe6`）——顺序 A/B 双序列、心跳三态、正/负路径点名、断链自愈真拔 USB 双半段实锤。**点验矩阵 esp32-mini 行七格收口（批G `dc3a1d6`）**：⑤ 安全核逐拍板上腿 bit-for-bit（[`rr-core::control::parity`](../base/runtime/rr-core/src/control.rs) 八组原语，LX6 双腿汇合宿主 harness）+ ⑦ 200 拍 min=198/avg=200/max=299ms 全落判据窗——[供给自证表](reference/供给自证表-esp32-mini.md) + [G4 时序证据](reference/G4时序证据-esp32-mini-20261011.md)（C12 证据四样）。平台形状实锤：Xtensa LX6 无 64 位原子（计数/发号 AtomicU32 域）、esp-idf-hal 半体驱动 `unsafe Send` 补齐、UART 开口 flock 排他账本（`707611b`）。余敞口：树莓派档 e2e 板上线即补、watchdog/安全核周期两键板上标定；容器路真跑已补——2026-10-11 CI ipct003 job --strict 四路全绿（run#6 38146782224 = 1c3cf41），IPC-T-003 移入 envelope.schema.json coverage（敞口表第 4 条结项） |
| manifest 校验（字段 + 跨字段规则） | 已实装 | [`schema.rs`](../base/runtime/rr-registry/src/schema.rs) + [`validate.rs`](../base/runtime/rr-registry/src/validate.rs)（入口 `rr3_registry_validate_manifest_text`）；契约面在 [`tests/contract.rs`](../base/runtime/rr-registry/tests/contract.rs)。运行期 facts 解析（[`rrd/src/manifest.rs`](../base/runtime/rrd/src/manifest.rs)）随 D9 加 `lang` 字段（演练/打包侧产物路径推导要它，`req_str` 缺失即点名；解析真值保持一册，不逼调用方另开 TOML 读取） |
| 能力格坐标系与装配单（slots.toml / profiles） | 已实装 | ADR-0012。唯一事实源 [`contracts/slots.toml`](../contracts/slots.toml)（21 格 = base 5 + body 12 + cognition 4，2026-10-04 Q1 裁定 append `act.relay`，schema_version 1；基础层 5 格随原能力名两段式、与 `base_capabilities` 表钉死等价，`power.battery` 属基础层通道格）。装配单两份：[`profiles/default.toml`](../profiles/default.toml)（RK3566 参考机，bindings 16 条 = body 12 + cognition 4 全绑 + faces 6 模块）与 [`profiles/distro.toml`](../profiles/distro.toml)（快路机型，bindings 只绑 body 12——cognition 4 格缺位恰好等于 `rr3-brain` provides 全集，faces 保留 brain 声明）。查册门在 [`slots.rs`](../base/runtime/rr-registry/src/slots.rs)；契约守护进 [`tests/contract.rs`](../base/runtime/rr-registry/tests/contract.rs)（基础层等价 / 格位形状 pin / manifest 能力词全查册 / default 与 distro 双装配单对账——distro 机三条腿：绑定面查册与 provides 对账同 default 门、未绑非 base 格位恰好等于 brain provides、faces 覆盖全部在册模块）。**rrd 装配链已接线（T3，`60fcab9`，0.0.2 方案 Q3 裁定）**：[`profile.rs`](../base/runtime/rrd/src/profile.rs) + [`probe.rs`](../base/runtime/rrd/src/probe.rs)——`--profile` 装机正门（与 `--manifest` 两种顺序同给都在 parse 处互斥拒），**faces 决定起谁**；bindings 缺格逐格点名 `拒绝启动 <模块>：缺格 <格位>`（**fail-closed 绝不带病起，缺格 ≠ 拒 boot**——被剔者不进 boot 队列、provides 仍计入可满足性宇宙，下游不连坐）；binding 被他人认领 / 基础层进 bindings / faces 引未在册格位 = 致命拒 boot；基础层进 faces 只记账不缺格（`rr3-power` faces 里的 `power.battery` 属此例）；probe 的 sim 后端对任何格位回在位——缺格只由装配单缺 binding 表达。线级证据 [`tests/e2e_profile.rs`](../base/runtime/rrd/tests/e2e_profile.rs) 3 条（P1 满配三模块全 running / P2 缺格点名且其余照常 spawn / P3 非法装配单 bind 前非 0 退出）。T5 再补 `--contracts` 运行期契约目录入口（deb 装机后编译期仓内路径不存在；缺省 None 保持仓内开发零变化，topics/slots 共用解析） |
| 主题注册表与前缀归属 | 已实装 | [`topics.toml`](../contracts/topics.toml)（内容本身不在此复述）+ [`topics.rs`](../base/runtime/rr-registry/src/topics.rs) 的 `Rr3TopicSet` / `rr3_topic_set_owner`；装载侧规范化与叠加见 [`topics_load.rs`](../base/runtime/rrd/src/topics_load.rs)。0.0.2 T2 批（`46cf28d`，Q1/Q4 裁定）、0.0.3 批C（`rr3-act/relay/state`，T3 补裁定）与 0.0.4 批D（`3682bfe`，connect `wifi/scan|connect|forget` 三 request，25→28 条 / kind 11-10-3-4）各有 append，形状 pin 三处同批（registry 集成测试 + registry lib + rrd lib 各一份独立 pin，两档同源计数断言同批订正） |
| 依赖仲裁（boot 启动序 + 降级传播） | 已实装 | ADR-0009。仲裁入口 `rr3_registry_plan_boot`（[`depends.rs`](../base/runtime/rr-registry/src/depends.rs)）：**依赖拓扑为主键、`start_priority` 为层内次键**（dependent 的低优先级压不过拓扑序——旧的全局优先级排序在 D1 场景是反的）；**环 fatal 拒整个 boot**；**不可满足只剔点名模块并重仲裁**（被剔者的 provides 仍计入可满足性宇宙，下游不连坐）。rrd 侧接线：[`daemon.rs`](../base/runtime/rrd/src/daemon.rs) 在投 boot 队列前仲裁（拒绝打 `拒绝 boot：` 并非 0 退出）；降级账本在 [`registry.rs`](../base/runtime/rrd/src/registry.rs) 的 `rr3_reg_degrade_after`（**断供即降**：任一 required 能力失去全部提供者即置位）与 `rr3_reg_undegrade_after`（解除须**全部** required 就位），supervisor 收尸 / watchdog 判死 / conn 解除三路触发，广播**不排除依赖者自己**（它最需要知道"我已降级"，且没有别的查询途径）；降级不出重启计划、不吃预算（裁定 4）。线级证据：[`tests/e2e_depends.rs`](../base/runtime/rrd/tests/e2e_depends.rs) 3 条（D1 链式启动序 / D2 环拒 boot / D3 降级 → always 自动重生 → 恢复）。不做清单（ADR-0009）：跨机、动态重仲裁、optional 传播、依赖触发重启、权限仲裁 |
| rrd 在册表：注册 / 心跳 / 死亡记账 / 扇出 / 快照 | 已实装 | **且被连接层与 supervisor 实际调用**：[`registry.rs`](../base/runtime/rrd/src/registry.rs) 的 `rr3_reg_add` / `rr3_reg_beat` / `rr3_reg_remove` / `rr3_reg_death` / `rr3_reg_child_exit` / `rr3_reg_fanout` / `rr3_reg_snapshot`。心跳回**三态** `Rr3Beat { Rejected, Kept, Promoted }`（`Promoted` = registered → running 的唯一升级，conn 据此广播一次 running 上线）；死亡回**三态** `Rr3Death { DeferredToExitStatus, Silent, Broadcast }`。表级账三件：`crash_streak`（按 name 计、跨重注册成立）、`owned`（"这个进程是我起的"所有权标记）、`stopped`（显式退场标记）。（同文件的 `rr3_reg_name_of` 仍没有跨文件调用点：已落地的发布分支改用登记期身份 `me.name`，见下文"发布归属判定"一行） |
| 在册表阶段机（`Rr3Phase`） | 已实装 | [`registry.rs`](../base/runtime/rrd/src/registry.rs) 的 `enum Rr3Phase { Registered, Running, Degraded, Failed }`。**四个相位全部有置位路径**：`Degraded` 由依赖仲裁传播置位/解除——`rr3_reg_degrade_after` 置位、`rr3_reg_undegrade_after` 解除（见"依赖仲裁"行）；`rr3_reg_beat` 对 `Degraded` **只续命、不升级**，一颗心跳不会把降级标注抹平；`as_str()` 回显 `"degraded"`，词表完整 |
| 模块 spawn / 重启执行 | 已实装 | **限定：仅 Unix**（[`supervisor.rs`](../base/runtime/rrd/src/supervisor.rs) 整模块 `#[cfg(unix)]`）。全 daemon **唯一的执行线程**（`rr3_spawn_supervisor`）：一条线程串行做 收件（boot / restart 两种请求）→ `try_wait` 扫尸 → 到期重启，**退避不是睡眠**（计划进 `due` 到期表，绝不 `sleep(delay)`）；`Command::spawn` 是全仓产品代码里唯一的进程派生。boot 序：先全部装载（`rr3_boot_job_from_manifest`——entry 存在性 + 可执行位在 boot 那一刻查，被拒打 `拒绝启动` 且**不吃预算**）→ 由 `daemon.rs` 调 `rr3_registry_plan_boot` 仲裁（拓扑主键 + 层内次键，环拒整个 boot，见"依赖仲裁"行）→ 投 boot 队列（E-7）。所有权模型（E-1/E-2）：`jobs`（"我能起谁"的全集）由 boot 装载的 manifest 填——`--manifest` 直给，或 T3 起 `--profile` 装配链按 faces 选出的装机 manifest（装配链在仲裁前定 boot 清单，见"能力格坐标系"行），**外来模块的计划一律跳过**（只打 `跳过重启 {name}：不是本守护起的进程`）；spawn 前先 `rr3_reg_own`、成功后 env 注入三件（`RR3_BUS_PATH` / `RR3_MANIFEST` / `RR3_SIM_PEER_ROLE`）、子进程 stdout 接 `Stdio::null()`。**四条已知边界**（supervisor.rs 文件头自述）：①非本守护起的模块永不重启；②own 与注册只靠 name 相关；③无 `SIGCHLD`/`waitpid`，"进程已死"到"rrd 知道"有一个 sweep 延迟（≤ tick）；④depends/degraded 不在本文件裁定——boot 序由 `rr3_registry_plan_boot` 在 daemon.rs 仲裁、降级传播只在收尸那一路被动触发（原"两条不做"随 F2/ADR-0009 解除，但裁定权不在这）。线级证据：[`tests/e2e_spawn_restart.rs`](../base/runtime/rrd/tests/e2e_spawn_restart.rs) 5 条（S1 boot→running→崩溃重启、S2 owned 崩溃 vs 外来对照、S3 never 不再起、S4 watchdog 判死挂死模块先杀再起、S5 孤儿卫生） |
| 模块重启的退避计算 | 已实装 | **且已被执行**（派生动作归上行"模块 spawn / 重启执行"）：唯一记账处 `rr3_charge`（[`registry.rs`](../base/runtime/rrd/src/registry.rs)），三条死亡路（conn EOF / watchdog 到期 / 子进程退出码）共用一本账——`crash_streak` 按 **name** 计、跨重注册不归零（活满一整条自己的窗口才清，`rr3_reg_expire` 段 2）；策略门（`always` / `on-failure` 且不干净 / 其余不重启）与预算门（`RR3_RESTART_FAILURE_CAP = 5` 到顶即 `None` 且不再增长）；`attempt` 从 1 起，`delay_ms` = `RR3_RESTART_BACKOFF[attempt-1]` × base，`checked_mul` 溢出封顶 `u64::MAX`（"永远等下去"是唯一保守解）。达上限的查询单列 `rr3_reg_hit_cap`，supervisor 打 `停止重启 {name}：连续崩溃达上限，运行时降级（spec §5.3）`（仅日志面，"整体标 degraded"要新增 topic、属改契约，未做） |
| 到期判活扫描（`rr3_reg_expire`） | 已实装 | **已实装且已被唯一 watchdog 线程按 tick 调用**。[`registry.rs`](../base/runtime/rrd/src/registry.rs) 的 `rr3_reg_expire(now_ms) -> Vec<Rr3Expiry>` **一趟扫描、两个生命周期决定**：①恢复归零——`Running` 且活过一整条自己的窗口（看 `registered_at_ms`）即清 crash_streak；②判死——`now - last_beat >= facts.watchdog_ms` 即置 `Failed` 并**当场产记账结果** `Rr3Expiry.restart: Option<Rr3RestartPlan>`（`saturating_sub` 防时钟倒退、已 `Failed` 跳过不重复回报、死连接句柄当场丢弃、回报里回带 `elapsed_ms` / `watchdog_ms` 两个判据数字）。"到期路径不产重启计划"的旧账**已结清** |
| watchdog 监视线程 | 已实装 | **限定：仅 Unix**（`daemon.rs` 整模块挂在 `#[cfg(unix)]` 下）。[`daemon.rs`](../base/runtime/rrd/src/daemon.rs) 的 `rr3_spawn_watchdog`，全进程只有这一条。tick 来自 CLI 的 `--watchdog-tick-ms`（默认值与"拒 0"的字面量归 [`reference/rrd-cli.md`](reference/rrd-cli.md) §2 §3，本行不抄）；循环**先睡后扫**、**每拍只锁一次**（扫全表那一次）、**广播在锁外**；判死日志把**判据数字本身**一起回显（渲染形状归 [`reference/rrd-registry.md`](reference/rrd-registry.md) §6.1，本行不抄）。判死那一拍广播 failed 后，把 `ex.restart` 交 `sup.rr3_restart(p)`（只做一次 send，**绝不**在这条线程里 spawn/睡眠）；**这一路不打"重启计划"日志**——A-5③ 的 needle 口径要求"重启计划"只由 supervisor 那一路打，两路不重叠 |
| CLI 参数与默认值注入 | 已实装 | [`cli.rs`](../base/runtime/rrd/src/cli.rs) + [`main.rs`](../base/runtime/rrd/src/main.rs)（套接字路径在打印横幅**之前**绝对化，横幅与 bind 取同一个值，由 [`tests/socket_banner.rs`](../base/runtime/rrd/tests/socket_banner.rs) 钉住）；T3 增 `--profile`（默认 None、与叠加层并存；与 `--manifest` 两种顺序同给都在 parse 处互斥拒）；T5 增 `--contracts`（运行期契约目录入口，parse 门四态：缺省 None / 显式值 / 缺参数带用法 / `-` 开头值拒）；M2 增 `--boot-role`：默认 `resident`、**原样透传不校验取值**（role 的合法集是夹具的约定，rrd 一校验就是第二处真值），空值与疑似开关值在 parse 处当场拒；参数与默认值的字面量归 `reference/rrd-cli.md`，此处不复述 |
| rrd UDS 监听与注册握手 | 已实装 | **限定：仅 Unix**——`conn` / `daemon` 两模块整体挂在 `#[cfg(unix)]` 下。[`daemon.rs`](../base/runtime/rrd/src/daemon.rs) 的 `rr3_daemon_run`：`Rr3UdsListener::rr3_cleanup` 清残留 → `rr3_bind`（bind/cleanup/accept 走 trait 工厂，ADR-0008 裁定 3 接缝最小化，`conn.rs` 一字未动）→ **bind 后 ACL 放行**（批F5b `rr3_socket_acl_grant`：chmod 0o660 + chgrp rockrobot3，组不在位留痕跳过不拒启——ADR-0010 修订 1；真机复验留板上线）→ 起 supervisor → 起 watchdog → **boot 清单定出**（`--profile` 装配链或 `--manifest` 装载，见"能力格坐标系"行）→ **依赖仲裁**（`rr3_registry_plan_boot`，见"依赖仲裁"行）→ 投 boot 队列 → accept 循环，每连接 spawn 一条 reader 线程（加上 [`conn.rs`](../base/runtime/rrd/src/conn.rs) 为同一条连接 spawn 的 writer 线程与它那份 `try_clone`，连接账是每连接 **2 条线程 + 2 个 fd**；全进程另有**唯一** watchdog 线程与**唯一** supervisor 执行线程，不在这个"每连接"账里）；[`conn.rs`](../base/runtime/rrd/src/conn.rs) 的 `rr3_conn_serve` / `rr3_handshake`：首帧解析时按 `is_ctl` **分流**——`rr3.ctl.*` 四方法进控制旁支（ADR-0010：ctl 连接不注册、不进在册表、一问一答即断），模块四方法才必须走 `rr3.register` 门，`src_task` 由 rrd 分配、伪造者那一帧被拒但连接不断，注册前设读超时收掉静默连接。线级用例：[`tests/register_publish.rs`](../base/runtime/rrd/tests/register_publish.rs)、[`tests/socket_banner.rs`](../base/runtime/rrd/tests/socket_banner.rs)、[`tests/e2e_sim_peer.rs`](../base/runtime/rrd/tests/e2e_sim_peer.rs)（跨进程对端夹具：由 `env!("CARGO_BIN_EXE_sim_peer")` 起的第二个真进程，证明注册→心跳→发布→退场整条链在线上走得通）、[`tests/e2e_watchdog.rs`](../base/runtime/rrd/tests/e2e_watchdog.rs)、[`tests/e2e_spawn_restart.rs`](../base/runtime/rrd/tests/e2e_spawn_restart.rs)、[`tests/e2e_depends.rs`](../base/runtime/rrd/tests/e2e_depends.rs)（F2）、[`tests/e2e_ctl.rs`](../base/runtime/rrd/tests/e2e_ctl.rs)（F3）、[`tests/e2e_profile.rs`](../base/runtime/rrd/tests/e2e_profile.rs)（T3）与 [`tests/e2e_act.rs`](../base/runtime/rrd/tests/e2e_act.rs) / [`tests/e2e_power.rs`](../base/runtime/rrd/tests/e2e_power.rs) / [`tests/e2e_connect.rs`](../base/runtime/rrd/tests/e2e_connect.rs)（T4a/b/c 与 0.0.3 批C），这些线级目标整体挂在 `#![cfg(unix)]` 门后，故 Windows 侧它们**不参与编译**（那是 cfg 边界的正确结果，不等于通过）。cfg 边界的自述见 [`lib.rs`](../base/runtime/rrd/src/lib.rs) |
| 发布归属判定（越权前缀 / 冒充内建命名空间） | 已实装 | **已接到线上**：`RrdRegistry::rr3_reg_publish_check`（返回 `Rr3PublishDeny`）不再只有单元用例——`conn.rs` 的上行分发把 `Rr3PayloadType::Event` 交 `rr3_publish`，后者依次调 `rr3_rpc_topic_param`、`rr3_reg_publish_check(&me.name, …)`、越权时回错误帧并 `rr3_reg_note_denied` 计数。发送方身份取 `me.name`（注册期 `Rr3Peer` 在册名），帧上 `src_task` 在分发前就校验：不等于 `me.task` 即那一帧被拒——`registry.rs` doc 注里"由 `rr3_reg_name_of` 反查"的表述线上换成了这套等价做法。拒帧不断连，与 `src_task` 伪造同一口径。接口字面量归 `reference/rrd-rpc.md` §7 与 `reference/rrd-registry.md` §8 |
| rrd 低电兜底（battery/low 钩子四件动作面） | 已实装 | T4b（`e87a284`，0.0.2 方案 Q4 裁定）。[`conn.rs`](../base/runtime/rrd/src/conn.rs) 的发布转发点挂 `battery/low` 钩子（**rrd 是总线本体，每帧 Event 必经，兜底不截断事件流**）→ 置 [`registry.rs`](../base/runtime/rrd/src/registry.rs) 全局低电旗（**上升沿恰一次、只升不降**——与 denied_publish 同款 conn 与 supervisor 两路共见守护态）→ 点名广播 `rr3-runtime/system/power`（帧形状单测钉死：Notification / src=rrd / dst=广播 / 体恰四字段——e2e 旁观者接入时首拍已过，帧形状只有单测能钉，两半拼成完整证据链）+ 日志环 → supervisor 的 boot 漏斗拒新 spawn（boot 与重启一问全挡；杀而不重启属关机策略，归 rr3-power 演进与 M6）。线级证据 [`tests/e2e_power.rs`](../base/runtime/rrd/tests/e2e_power.rs) 1 条（装配单正门起真 rr3-power，sim 电池 5% 起表 → 兜底触发行含透传值与广播结果 → 旁观者收周期流 → pkill 真模块后拒绝 spawn 窗断言 + 末尾破坏性收尾计数双重保险） |
| rrd 上行 JSON-RPC 十方法（模块四 + 控制六） | 已实装 | [`rpc.rs`](../base/runtime/rrd/src/rpc.rs)：`Rr3RpcMethod` = `rr3.register` / `rr3.heartbeat` / `rr3.unregister` / `rr3.call`（M3）+ `rr3.ctl.status` / `rr3.ctl.health` / `rr3.ctl.logs` / `rr3.ctl.ota`（F3 / ADR-0010）+ `rr3.ctl.counters` / `rr3.ctl.reboot`（0.0.4 批B/批C append，Q1 裁定）。六个 ctl 方法走**同一解析器**，`is_ctl` 的判据与方法面一致（模块四方法全部 `!is_ctl`）；`rr3_rpc_parse_request` 的版本与 id 校验、`rr3_rpc_build_result` / `rr3_rpc_build_error` 错误映射、`rr3_rpc_call_params`（`params.topic` 非空、`args` 可缺省、成功不回 result 只回 accepted）照旧 |
| 运维计数面（心跳 counters 双端镜像 + 聚合持久化 + ctl 读面） | 已实装 | 0.0.4 批B（`e495242`，Q1/Q2 裁定；legacy data 的"机器慢先看数字 + 断电不丢"语义承接，digest 归档行在案）。**模块侧**：[`client.rs`](../base/runtime/rr-bus/src/client.rs) 的 `rr3_counters_bump`（模块内部运维词汇自愿累加）——心跳线程每拍快照，**非空才带** `{"counters": {...}}`（可选 append，旧 rrd 忽略未知字段，双向兼容）；Python 侧 [`bus.py`](../modules/rr3-brain/rr3_brain/bus.py) 同语义镜像（ADR-0014 裁定 1），双端镜像单测各钉。**rrd 侧**：[`rpc.rs`](../base/runtime/rrd/src/rpc.rs) 形状裁决（`{string: u64}`）→ [`registry.rs`](../base/runtime/rrd/src/registry.rs) 聚合进在册表（键 = `模块名/键`；rrd 自有键 `boots` 启动自增）→ [`counters.rs`](../base/runtime/rrd/src/counters.rs) 持久化：启动加载全表 + 心跳归并 write-through，路径 `RR3_DATA_DIR`（cfg(linux) 缺省 `/data/rr3`——rr_data 分区挂载点）下 `counters.json`，**原子写**（临时文件 + rename），皆无 = 不持久化（日志一行点名不报错，sim 直跑即此态）；发行版 unit 注入 `RR3_DATA_DIR=/var/lib/rockrobot3` 且 postinst 建目录（0.0.6 批E'，`a5b3093`——真板复验 boots 跨重启累积成立，见 deb 打包链行）；`rr3.ctl.counters` 读面回全表快照。brain 对话轮计数与 act 继电器动作计数已自报（键随模块内部词汇）；**归并顺序竞态已修（0.0.6 纪律小批 `b81c986`）**——心跳分支先归并后应答，ctl 读快照不再抢在归并前（e2e 同用例钉顺序契约）；counters 落盘节流已由 0.0.5 增补轮四 R26 裁定改判心跳合并写并随批W3 实施——`Rr3CountersGate`（首拍即刷 + M 毫秒窗口至多一次，env `RR3_COUNTERS_FLUSH_MS`/`RR3_COUNTERS_FLUSH_MAX` parse-or-default）+ SIGTERM/SIGINT 关停强制 flush（self-pipe + `counters-term-flush` 线程，e2e `sigterm_forces_flush_of_pending_counters` 钉节流与关停完整性双判据）；裁定演进记录在 [`counters.rs`](../base/runtime/rrd/src/counters.rs) 模块头 |
| 模块间点名路由与应答回路（rr3.call） | 已实装 | **请求方向**：[`conn.rs`](../base/runtime/rrd/src/conn.rs) 的 `rr3_route_call`——查 [`registry.rs`](../base/runtime/rrd/src/registry.rs) 的 `rr3_reg_callee_for`（exposes 该主题的在册模块里**取 task 最小者**）→ `rr3_reg_send_to` 点对点投递，转发帧 `trace_id`=调用方 task（语义登记见 [contracts/README.md](../contracts/README.md) 演进登记）；三种拒绝（未注册主题 / 无 callee / exposes 不符）逐字文案归 [`reference/rrd-rpc.md`](reference/rrd-rpc.md) §8。**应答方向**：`rr3_route_response` 按 trace_id 原路回给调用方。线级证据：[`tests/module_call.rs`](../base/runtime/rrd/tests/module_call.rs) 4 条（点名回路 + 三种拒法）+ [`tests/e2e_audio_chain.rs`](../base/runtime/rrd/tests/e2e_audio_chain.rs) 1 条（**三真进程**：rrd + rr3-sense(sim) + rr3-interact(sim)，vad_start 事件上总线、pcm STREAM 有序转发、rr3.call 应答回到调用方） |
| 模块侧同步总线客户端 | 已实装 | [`client.rs`](../base/runtime/rr-bus/src/client.rs)：`rr3_connect`（读 manifest → UDS 连接 → 注册 → 独立线程喂心跳）、`rr3_task` / `rr3_name`、`rr3_call` / `rr3_publish` / `rr3_reply_call`、`rr3_poll` / `rr3_poll_wait` / `rr3_try_recv`、`rr3_connected` 死亡探针（断开后 `poll_wait` 返回空 Vec 与"暂时没帧"不可区分，事件循环必须探——两个真入口都探了）、`rr3_frame_body` 解帧；manifest entry 相对路径按所在目录绝对化（rrd spawn 注入的契约环境变量）。6 条用例，无平台门 |
| HAL 设备面（音频/舵机(PWM)/数字输出/显示/电源/链路） | 已实装（sim 全 + linux 真后端） | **五面 sim 后端全实装；linux 侧真后端批F5a 五件落地（PWM / GPIO / 显示占位 / 电源 / 链路，音频 ALSA 更早）——板上单测与真件证据欠（板离线，见"本机无法收口"第 7 条）**。[`lib.rs`](../base/runtime/rr-hal/src/lib.rs) 的 trait 面与 `rr3_block_on`（同步桥接，**不引 async 运行时**——ADR-0008 裁定 1）；批F5a③ 增 `Rr3Display` trait 与 `Rr3FaceFrame = [u8; 8]` 帧类型（帧类型归 HAL 事实源，[interact face.rs](../modules/rr3-interact/atoms/face/face.rs)（批C③ 重铺前为 src/face.rs） re-export——帧表留模块面，`Rr3Face` 不立，评审定案）；[`sim.rs`](../base/runtime/rr-hal/src/sim.rs)：音频三种波形（正弦 / 噪声 / 静音）+ M4 burst（1s 说/1s 停，让 VAD 周期性发 onset；门控按**帧序号**算偶秒——多通道交错样点折半缺陷批F4 `379e6de` 修，mono golden 不动）+ 批F2 双麦方位注入（`RR3_HAL_SIM_DOA_DEG`，ILD 增益模型 g=1∓sinθ）+ T4a sim 舵机（记录末次落角不判行程）与 sim 数字输出（电平 round-trip）+ T4b `Rr3SimBattery`（电量/充电态记账，电压曲线钉死 3200mV + 10mV/1%）+ T4c `Rr3SimLink`（up/ssid/rssi 三元组，**断链时设备面强制抹掉 ssid/rssi**——不许从死链读网络信息）；[`backend.rs`](../base/runtime/rr-hal/src/backend.rs) 选择器八件——`rr3_hal_backend_resolve` + 音频 source/sink + `rr3_hal_servo` / `rr3_hal_gpio_out(role)` / `rr3_hal_display` / `rr3_hal_power` / `rr3_hal_link`；**平台门订正（批F5a① `acc548e`）**：alsa 门只判音频面、能力面各判各的——`cfg(target_os = "linux")` 走 linux 后端（非 linux 平台显式 `RR3_HAL_BACKEND=linux` 回 Unsupported，模块探活 exit(1) 属诚实行为）；sim 按 env 起表（`RR3_HAL_SIM_BATTERY_PERCENT/CHARGING` 缺省 80/未充电、`RR3_HAL_SIM_LINK_UP/RSSI` 缺省在线/-50，取值裁决是独立纯函数）。linux 真后端五件全单门 `#[cfg(target_os = "linux")]`（宿主 macOS 不参与编译；验证 = aarch64 musl 交叉 check，**板上 cargo test 补跑欠账**——板离线，rsync 口径同批F1）：[`linux.rs`](../base/runtime/rr-hal/src/linux.rs) ALSA（`cfg(all(target_os = "linux", feature = "alsa"))` 门 + `RR3_HAL_ALSA_DEVICE`）；[`linux_pwm.rs`](../base/runtime/rr-hal/src/linux_pwm.rs) sysfs PWM 舵机（批F5a① `acc548e`：`RR3_HAL_PWM_CHIP/CHANNEL` 缺省 pwmchip0/pwm0=GPIO12（R22 定案硬件 PWM 脚），50Hz / 0.5–2.5ms↔0–180°，ensure 落中位 90° 与 sim 同观测，Drop 停脉冲 + 归还已导出通道）；[`linux_gpio.rs`](../base/runtime/rr-hal/src/linux_gpio.rs) chardev UAPI v1（批F5a② `da52e7f`：ioctl 号与 `#[repr(C)]` 布局单测钉死内核定值；`rr3_hal_gpio_out` 带 role 参数（"led"/"relay" 白名单）免顺序耦合；`RR3_HAL_GPIO_CHIP` + `RR3_HAL_GPIO_LED_LINE/RELAY_LINE` 缺省 4/17（R8 预算表软件路，真件档装配单可 env 覆写）；ensure 首调请求 OUTPUT 落初值 0 与 sim 同观测；EBUSY→Busy 其余→InvalidParam；Drop close 释放 line）；[`linux_power.rs`](../base/runtime/rr-hal/src/linux_power.rs) INA219 I2C 电池（批F5a④ `8f4d928`：`RR3_HAL_I2C_BUS` / `RR3_HAL_INA219_ADDR`；I2C_SLAVE 自定义常量钉死 i2c-dev.h 定值（libc musl 面不导出，照 GPIO 批先例）；bus_mv = (raw>>3)×4（13-bit / LSB 4mV）；cell_percent 3200–4200mV↔0–100% 与 sim 同一条线；charging 判定诚实恒 false 待真件档两点标定）；[`linux_link.rs`](../base/runtime/rr-hal/src/linux_link.rs) NetworkManager nmcli 子进程承载（批F5a⑤ `faf01be`：`RR3_HAL_LINK_IFNAME` 缺省 wlan0；terse 转义解析单测钉死；SSID 以 wifi list IN-USE 行优先、失败回落 CONNECTION；SIGNAL 质量百分比→dBm 经验折算注释钉死；断链强制抹 ssid/rssi 对齐 sim 语义）。libc 仅 extern 声明壳（GPIO/电源两件——musl 静态零额外 C 依赖，R13）。**板上单测与真件读数/动作证据欠（板离线 + 外设采购中，见"本机无法收口"第 7 条）；MAX7219 SPI 显示真后端归真件档（linux 侧现落占位）** |
| A/B OTA 切换与恢复出厂 | 已实装（主机面） | **限定：引导配置改写的主机面已实装；U-Boot 环境变量写侧与块设备探测属板上件不 claim**。0.0.3 批D（`68220e4`，Q3 裁定）：[`bootconf.rs`](../base/runtime/rr-ota/src/bootconf.rs)——`switch <rr_a\|rr_b>` 改写 extlinux.conf 的 `append root=LABEL=rr_X` 标签做**持久** A/B 切换（只动生效行那一个标签、注释与其余字节一字不动；同槽幂等拒 / rr_factory 目标拒走 factory / 缺·歧义·杂牌 token 报形状漂移拒）、`factory` 切 `rr_factory` 做**一次性**出厂引导（rr_factory 分区本体任何代码路径不得写入——partition-layout.md 只读约束不破；"回得来"的职责在 rr_factory 镜像自带恢复流程）、原子落盘（读 → 改写 → 同目录临时文件 → rename）、cmdline 当前槽解析（运行槽权威来源是 `/proc/cmdline`，只认 `root=LABEL=rr_` 形态，裸设备形态报"板上核对项"不覆盖；读取入口挂 `cfg(linux)` 门、解析逻辑收 `&str` 全平台单测）；extlinux.conf 真工件 `include_str!` 字节级对拍（可逆律 / 对合律 / 长度差恰为标签差）。CLI [`rr3-ota-ctl`](../base/ota/rr3-ota-ctl/src/main.rs)（薄壳：判定全在 rr-ota::bootconf）：slots / switch / factory 三子命令、`--bootconf` 必传不缺省碰任何路径、退出码 0 成功 / 1 业务拒 / 2 用法错。类型形状与分区标签守护（0.0.1 起）：[`rr-ota/src/lib.rs`](../base/runtime/rr-ota/src/lib.rs) 的 `Rr3Slot` / 对合律 / `ota_labels_match_genimage_cfg`（include_str 逐字钉死）。**未做（不 claim）**：U-Boot env 写侧（bootcount / bootlimit / altbootcmd / upgrade_available 自动回滚链——标准语义与断电安全序落 [`partition-layout.md`](../base/ota/partition-layout.md) 增补段）、块设备/挂载探测、落盘 fsync 加固、槽位镜像写入（往 rr_b 落整镜像，随升级下载链另批）、实板验证——均列 partition-layout.md 板上核对项清单（首条即 vfat rename 原子性弱于 unix 文件系统的诚实敞口） |
| Buildroot OS 层（defconfig / 分区布局 / 引导链 / 烧录文档） | 骨架 | **结构就绪、构建与实板验证未发生**（硬件未采购）。OS 层已按 ADR-0013 拆双轨（`{board,configs,patches}` 平移 `buildroot/`，`distro/` 快路轨）。buildroot 轨已落树：[`configs/rock3_zero3_defconfig`](../base/os/buildroot/configs/rock3_zero3_defconfig)（Buildroot + Linux 6.6 + aarch64，BusyBox init，ALSA/Python，post-build/post-image 挂钩）；[`genimage.cfg`](../base/os/buildroot/board/radxa-zero3/genimage.cfg)（boot + `rr_a`/`rr_b`/`rr_factory`/`rr_staging`/data 六分区；容量一律 `@XXX_SIZE@` 构建期占位符，由 [`post-image.sh`](../base/os/buildroot/board/radxa-zero3/post-image.sh) 注入并做残留检查）；[`extlinux.conf`](../base/os/buildroot/board/radxa-zero3/extlinux.conf)（root=LABEL=rr_a；A/B 持久切换由 rr3-ota-ctl 改写本文件 append 行标签——批D 主机面实装；bootcount/altbootcmd 自动回滚链属板上件，见 partition-layout.md 增补段）；[`post-build.sh`](../base/os/buildroot/board/radxa-zero3/post-build.sh) **装机布局已收口（`28c629a` 保底轨修复批）**：manifest 归位 `/usr/lib/rr3/modules/<name>/module.toml`（不再平铺 `/usr/lib/rr3/manifest/`——旧布局下模块 `rr3_config_load` 读 manifest 同目录 config 必撞同名文件、bin 相对 entry 解析必错位）、entry 装机绝对化（与 rr3-pack `rr3_manifest_install_rewrite` 同语义三道门：entry 行恰一条 / 值可装机拒相对与 `.` `..` 段 / 改写后 sed 重提复核，除 entry 行外与仓源逐字节一致）、config 从仓源随装（env `RR3_REPO_ROOT` 可覆盖仓根）、bin 按 entry 落位、S99rrd 的 manifest glob 改 `modules/*/module.toml`；**缺件口径**：缺 bin / 缺 manifest / entry 不可装机 = 警告 + **整模块跳过**（manifest 一并不装——缺件还装 manifest 会让 rrd spawn 必退出的模块成重启环；"缺席只警告"仍指镜像结构可构建），config 缺席仍只警告；装配冒烟 [`tools/ci/buildroot-assembly-smoke.sh`](../tools/ci/buildroot-assembly-smoke.sh) 13 项断言进 CI `buildroot-assembly` job；[`flash-maskrom.md`](../base/os/buildroot/board/radxa-zero3/flash-maskrom.md)（rkdeveloptool 操作序）；[`patches/README.md`](../base/os/buildroot/patches/README.md)（dts/defconfig 核对清单，无补丁本体）。distro 快路轨：[`base/os/distro/README.md`](../base/os/distro/README.md) + [`units/rrd.service`](../base/os/distro/units/rrd.service) + [`profiles/distro.toml`](../profiles/distro.toml) + [`tools/rr3-pack`](../tools/rr3-pack/src/main.rs)（见上行 deb 打包链行）。分区标签与代码常量的逐字一致性由 `rr-ota` 的 include_str 守护用例钉死。缺的手：全量 make 构建（需 Linux/容器）、dts 与 U-Boot defconfig 核对、串口出 shell、maskrom 实烧——均属板上/Linux 侧收口 |
| 发行版快路轨 deb 打包链（tools/rr3-pack） | 已实装 | T5（`a95eda7`，ADR-0013 裁定 5 首证，0.0.2 方案 D8）。独立工程（七步门 ④ 第 10 个；依赖 rrd lib 只为 manifest 解析——解析真值只有 rrd 运行期那一册，打包器不带第二份读取逻辑）。核心裁定：**entry 装机改写**（`rr3_manifest_install_rewrite`）——线上注册门禁（conn.rs）要求 manifest.entry 绝对路径，而模块开机读的就是装机 manifest 原文上报、相对值必被 AIRY_EINVAL 拒；打包器只改 entry 引号内一个值（三道门：entry 行恰一条 / 装机值可装机拒相对与 `..` 逃逸段 / 改写后重提复核），测试对"除 entry 外与注入原件逐字节一致"字节级钉死。缺件点名（rrd 缺席 / 模块缺 bin / 无模块 / 缺 distro.toml / 缺 config.toml——模块开机读 manifest 同目录 config，缺件必重启环，随包必带）+ 目录名与 manifest.name 不一致拒。`--stage-only` 供 macOS 本机装配演练（本机无法 `dpkg -i`，真装判据归 CI `distro-smoke` job：rust:bookworm 容器内构建 → 出 deb → 装机 → rrd 装配单正门起 5 模块 + brain 缺格点名（P2/D2 语义首证）+ 0 拒 boot + `denied_publish = 0`）。独立工程单测 5 条（④ 步 check，conventions §2 另登记）。0.0.6 批B（`5ecaa8d`）：deb 版本常量升 **0.0.6**（`RR3_DEB_VERSION`，单测 control 断言随改）+ 新增 [`tools/deploy/pack-arm64.sh`](../tools/deploy/pack-arm64.sh) 宿主交叉出包链（aarch64-unknown-linux-**musl** 静态 + rust-lld——macOS host 无 aarch64 glibc sysroot，musl std 自带完整 libc 是零安装路；三步：交叉编译 → 注入装配 → `rr3-pack --stage-only`，staging 断言含"aarch64 静态 ELF 体检 / entry 已装机绝对化"两条）。**真板实测（0.0.6 批D，`efd224f`）**：树莓派（pi@192.168.124.52，主机名 spharx，Debian 13 trixie / aarch64）板上 `dpkg-deb --build` + `dpkg -i` 成功（postinst 自动 enable），五模块全 running、`rr3-ctl` 三查实值、reboot 自恢复全绿——"同一 payload、不改一行产品代码，装进 Layer 0 完全不同的板"实测成立。**真板首发现两条缺陷（0.0.6 批D 实测，R3/R6 裁定归属）**：①boots 数据目录——**已修复并真板复验通过（0.0.6 批E'，`a5b3093`+`f2597d5`）**：[rrd.service](../base/os/distro/units/rrd.service) 注入 `Environment=RR3_DATA_DIR=/var/lib/rockrobot3` + postinst 建目录置属主（rr3-pack 模板，staging 单测断言）+ distro-smoke 断言链补 ⑧"counters 回写"（日志逐字 + counters.json 含 `"boots"` 键）；板上升级 restart 后 `{"boots":1}` 落盘、reboot 自恢复后 `{"boots":2}` **跨重启累积成立**；②`/var/run/rock3-ipc.sock` = `srwxr-xr-x root:root`（rrd root umask）非 root 运维连接被拒——**已实装修复（0.0.6 批F5b `fab7b6c`，ADR-0010 修订 1）**：rrd bind 后 `chmod 0o660` + `chgrp rockrobot3`（postinst 幂等 `addgroup --system rockrobot3`；组不在位留痕跳过不拒启，root 恒可连）；unit 同批注入 `RR3_REBOOT_OK=1` + `Restart=on-failure→always`/`RestartSec=2`（方案目标形态"批F5b 与 R12 同批落"）。**真机复验留板上线**：非 root `rr3-ctl` 直连不再 Permission denied，随判据 14/15 运维链验收同跑；复验通过前板上巡检仍按 `sudo rr3-ctl …` 口径（[boards §7](../base/os/distro/boards/raspberry-pi-64.md) 修复状态条目） |
| `rr3-sense` / `rr3-interact` | 已实装（sim 侧） | [`modules/rr3-sense/src/main.rs`](../modules/rr3-sense/src/main.rs)：注册/心跳 → 按后端开拾音面（批F2 `e4ccbaf` 增 `audio/doa.rs`：**下混前**双麦 ILD 反演 ±90° 分三档（|deg|≤30=center），vad_start onset 负载可选携 `doa{deg,bin}`——schema append 零新契约，R14 双麦下混 mono 前判向）→ 自 pacing 采集循环（sim read 即时返回、ALSA read 自耗一拍，两种后端同一形状）→ pcm STREAM + VAD 边沿上行 → `capture/dump` 点名应答（模块用例条数登记 conventions §2 + golden PCM 向量）；[`modules/rr3-interact/src/main.rs`](../modules/rr3-interact/src/main.rs)：注册/心跳 → 占位 TTS（440Hz burst，60ms/字符封顶 10s）+ 七态表情机（批F3 `444c764`：五态增 listen/turn 可点名，talk 仍 TTS 自动驱动不可点名；每态 8x8 帧序列，主循环 ~250ms 墙钟节拍推帧不随总线流量漂移）+ **显示出帧接线（批F5a③ `bcc4de2`）**：开机 HAL 显示面探活 + 主循环经 `Rr3Display` 出帧（sim 内存位图）应答两个 rpc 点名 + **Web 操作面板（0.0.4 批C+批D，`7aa7fc8`/`3682bfe`）**：[`panel/server.rs`](../modules/rr3-interact/src/panel/server.rs) std 手写 HTTP **十二端点**（状态/硬件/系统三页九端点 + WiFi 配网三端点——scan GET / connect POST / forget POST 全部 BusCall 透传，参数裁决在 owner 模块）、单文件静态页 `include_str!` 内嵌四页签（[`panel/www/index.html`](../modules/rr3-interact/src/panel/www/index.html)）、`rr3.ctl.reboot` 面板触发双重门、监听面 `RR3_PANEL_ADDR` 优先缺省回环（局域网可信面声明在 README）、beep/face 经 mpsc 动作通道主循环串行代执行（sink/face 所有权不出主循环）、net/link 事件缓存进状态页。两者失败语义同一：**业务性失败折 error 应答回话，传输性失败 `exit(1)` 交 supervisor**（不装"活着"）。**板上 ALSA 采集/回放未发生（硬件未采购）**；三真进程整链证据在 [`e2e_audio_chain.rs`](../base/runtime/rrd/tests/e2e_audio_chain.rs) |
| 机器人功能可达性（F-1…F-8 清单口径，2026-10-12 新增本行） | **四条可达 / 四条不可达**（骨架） | 清单与逐条判据的唯一真值 = [0.0.5 方案 §18.1](process/dev/0.0.5版本洛克三号对齐与裁定方案.md)（版本边界三窗分离后新增的验收面，见 [ADR-0020](process/adr/ADR-0020-版本边界-开发收口与实机验证与验证后整理.md)）。**可达（欠板上证据）**：F-1 舵机转向（`linux_pwm.rs` sysfs 后端 + `rr3_travel_clamp` 判定 + `servo/state` 限速流）、F-3 语音输入（ALSA 采集 + VAD 边沿上总线，欠真麦与真 key）、F-7 生命周期（supervisor `restart_policy` + 断电 boots 累积，0.0.6 批D/E'/F5b 实装）、运维面 F-7 附带的 `rr3-ctl` 六子命令。**不可达（欠码，非欠载体——这条区分是本行的全部意义）**：**F-4 真人声播报** = `composite.py` 已合成云 TTS PCM 却只发 `{"text"}`、`atoms/player/mech.rs` 播 440 Hz 占位波形按 60 ms/字符估时长（→ MC-1/MC-2）；**F-2 表情上屏** = `rr3_hal_display` 无 linux 支，真机取 `Rr3UnavailableSource` 回 `Unsupported`（施工 = 增补轮五批B/批C——R31 Q2 已裁显示件为 SSD1306 I2C 挂 ESP32 协处理器；清零判据 = MC-3，MCP-2 SPI 扩键评审随 Q2 作废登记）；**F-6 低电禁动** = `Rr3LowEdge` 原语与 rrd 低电旗广播 `rr3-runtime/system/power` 都在，act 侧**无消费点**，且 `cutoff_voltage_mv` 只回显不动作（→ MC-5/MC-8）；**F-8 微型形态板侧件** = `profiles/esp32-mini.toml` 三格无任何件承载（→ MC-7 + 0.0.7 批H）。另有两处**未登记的站替面**已按索引 R-16 落单：`Rr3SimProbe` 恒真（缺格降级环不转，MC-4）、配网凭据不落盘（MC-6）；`net.bt` 状态位记 `normal` 而实现是骨架（MC-9 二选一处置） |
| `rr3-act` / `rr3-connect` / `rr3-power` | 已实装（sim 侧） | 0.0.2 T4a/T4b/T4c（`c8297b1` / `e87a284` / `2b08994`，方案 Q1–Q4 裁定）+ 0.0.3 批C（`be19982`，T3 补裁定），失败语义与 sense/interact 同一（业务折 error 应答、传输 `exit(1)` 交 supervisor）、HAL 开机探活（Unsupported 即 exit(1)）。[`rr3-act`](../modules/rr3-act/src/main.rs)：`servo/command`、`led/effect`、`relay/set` 三事务面点名应答（缺参/类型错/越界折 error 不断链）+ `servo/state` 限速流（`Rr3RateGate`，状态变化必发、两次至少隔 state_min_interval_ms）+ 进程内反射环宿主线程（饱和记账、episode 恰报一次）；**订阅执行面已实装（批C）**：订阅 `rr3-brain/intent/result` 逐条执行载荷 `actions[]`（元素 `{topic, args}`），[`intent.rs`](../modules/rr3-act/src/intent.rs) 两道归属门（exposes 归属门 + 意图面开放门）——越权动作拒执行折日志不断链（事件语义无应答回路）；批F2 起 `servo/command` 进意图开放面（`e4ccbaf`：判向-转向编排，转向落 `servo/state` 限速流，e2e 钉跨路 seq 连贯与限速窗纪律）；`relay/state` 状态流 append 在册（RPC 与 intent 两条驱动路共用 `relay_seq` 流序，变化即发不做限速门），线级 `e2e_act` 断言 RPC 路 seq=1 → intent 路 seq=2 跨路连贯 + 越权动作有界窗口否定断言。[`rr3-power`](../modules/rr3-power/src/main.rs)：`battery/state` 周期流（首拍即采）+ `battery/low` 边沿事件（越阈首拍恰报一次、严格大于才重新武装——武装若回 true 就会发出电压高于阈值的"低电"事件）+ `charger/state`（由最近电池读数 charging 位派生，首拍电池之前不发）；订阅面为空是设计（低压关断本地决策不依赖总线）；cutoff 阈值只解析回显不动作。[`rr3-connect`](../modules/rr3-connect/src/main.rs)：`net/link` 状态机周期流（up/ssid/rssi，首拍基线必发-同态不发-翻转必发；断链时设备面强制抹掉 ssid/rssi）+ `cloud/session` open 应答面（down 折 error 点名判据词"链路"，业务性失败不断链；云侧凭据联调范围外，`RR3_BRAIN_CLOUD=mock` 注入点维持）+ **配网面三 RPC（0.0.4 批D，`3682bfe`）**：[`wifi.rs`](../modules/rr3-connect/atoms/wifi/mech.rs)（批C④ 重铺前为 src/wifi.rs）的 `wifi/scan`（sim 确定性三行列表、`backend:"sim"` 诚实标注，元素三字段照搬 legacy panel.c h_scan 形状）/ `wifi/connect`（legacy 字段名 `ssid`/`pass` 与"密码至少 8 位"原词；`connected` 如实取 HAL 链路读数不 fake"连上了"——"等连后回真实结果"的等待语义由 net/link 流承载）/ `wifi/forget`（清凭据回 ap，承接 legacy factory_reset 的 WiFi 语义、≠ M6 整机恢复出厂）+ `Rr3Provision` 配网状态机（ap|station 两态纯函数，模块内策略不进 HAL——HAL 只如实报数；凭据内存不落盘重启即忘、真持久化 wpa_supplicant/NVS 板上后置，如实声明；`mode` 字段随 net/link 流发布，契约只增不改）+ **蓝牙链路状态机骨架（0.0.4 批E，`586641d`）**：[`bt.rs`](../modules/rr3-connect/atoms/bt/mech.rs)（批C④ 重铺前为 src/bt.rs） `Rr3BtState` 三态（off/discoverable/connected）转移矩阵纯函数——非法转移（connected 直接断电等）拒绝且状态不动；sim 初值 `RR3_HAL_SIM_BT_UP` env 注入（取值非 "1" 一律按关，env 是 sim 注入口不是配置面）；主题不登记（btctl 日志回放/远程命令语义已被 rrd 日志环 + rr3-ctl 承接，D5 裁定不预先占位），BlueZ 真后端板上后置。独立工程单测 act / power / connect（条数登记 conventions §2，④ 步 check）；线级 `e2e_act` / `e2e_power` 1 / `e2e_connect` 2（装配单正门起真模块，`#![cfg(unix)]`）。各目录 `module.toml` 仍由 [`tests/contract.rs`](../base/runtime/rr-registry/tests/contract.rs) 逐目录读回判合规。**板上真件证据欠**（外设采购中 + 板离线欠账；HAL linux 真后端已就位——批F5a GPIO(led/relay)/PWM 舵机/link(nmcli)/battery(INA219)，真件读数/动作证据随采购到位，见"本机无法收口"第 7/11 条） |
| `rr3-brain` | 已实装 | **不是 Rust 模块，是 Python stdlib-only**（ADR-0014，零 pip 依赖）。入口 [`rr3_brain/__main__.py`](../modules/rr3-brain/rr3_brain/__main__.py)（注册 / 心跳 / 事件循环 / 对话轮完成时发布 intent/result）；[`bus.py`](../modules/rr3-brain/rr3_brain/bus.py)（386 行）——128B 头逐语义镜像 rr-ipc（struct 打包、Layout C 同偏移）、帧读写、注册握手、publish / subscribe / call 客户端；**意图规则面（0.0.3 批C，Q1 裁定——策略层唯一归属在 brain）**：[`intent.py`](../modules/rr3-brain/rr3_brain/intent.py) 从 `user_text`（ASR 文本，唯一输入）抽出结构化 `actions[]`（元素 `{topic, args}`，词汇即注册表零新造；精确词表 + 两字符否定护栏，3.9 兼容语法；批F2 `e4ccbaf` 词表 append："向左/向右"→`servo/command` 转向动作，mock ASR 文本注入 env 供演练）；载荷 append `actions`（空数组 = 纯对话轮）；[`cloud/`](../modules/rr3-brain/rr3_brain/cloud/__init__.py) 三家客户端（asr / llm / tts）urllib 真调用 + **`RR3_BRAIN_CLOUD=mock` 确定性后端**（默认 `real`，测试注入点）；[`dialog.py`](../modules/rr3-brain/rr3_brain/dialog.py) 对话状态机（vad_start → 采集 → asr → llm → tts 回路）；配置见 [`config.toml`](../modules/rr3-brain/config.toml)。stdlib unittest 全量条数与平台差额登记 conventions §2（本机 py3.9 实测 22 过 + 1 条 `tomllib` 门槛 error；`test_intent` 在门槛内全绿；CI brain-unittest 钉 3.12 跑全量）；线级证据：`rr3-sim --brain`（[`tools/sim/src/brain.rs`](../tools/sim/src/brain.rs)）**四真进程**（rrd + brain + sense(sim) + interact(sim)）全语音对话闭环演练实跑通过（5ab029e / 5b55620）；批F4 `rr3-sim --chain` **六真进程** mock 云全链闭环（`379e6de`，判向→转向→表情无人工干预一轮以上，真板实测 boards §9）。运行门槛：python3 ≥ 3.11（`tomllib`——rr3-sim 的 `which_python3` 以 `import tomllib` 成败做能力验证，见 rr3-sim 行）。**三家云的真实凭据联调未发生**（无 key 环境，mock 之外的云端路径零实测）。规约注记：按 conventions §3，brain 属存量非 Rust 面，功能语义不变渐进迁移为 Rust；迁移路线已由 ADR-0016 落设计裁定（首件 bus.py → rr-bus 复用、实施归 0.0.6 专项，0.0.5 只落设计裁定不等于实施解锁） |
| `rr3-ctl` | 已实装 | **限定：查询面只读；ota 恒拒（点 M6）；reboot 双重门缺省恒拒**。rrd 侧（ADR-0010）：`rr3.ctl.*` **六方法**（模块四 + 控制二——`rr3.ctl.counters`（0.0.4 批B）/ `rr3.ctl.reboot`（0.0.4 批C，Q1 裁定）append，白名单一册在 [`rpc.rs`](../base/runtime/rrd/src/rpc.rs)，`is_ctl` 判据与方法面一致）——握手在 Register 门前按 `is_ctl` 分流，**ctl 连接不注册、不进在册表、一问一答即断**；status 走在册表快照（`rr3_reg_health_snapshot`，ctl 连接自己不得进快照）；health 回显条目自己的 heartbeat / watchdog / `since_beat_ms` 三数；logs 读 `rr3_log` 单点落的 512 行内存日志环；counters 回聚合持久化的全表快照（见"运维计数面"行）；reboot 执行面**双重门**（[`conn.rs`](../base/runtime/rrd/src/conn.rs) `rr3_reboot_unlocked`：`cfg(linux)` **且** env `RR3_REBOOT_OK=1` 才真跑 `systemctl reboot`，缺省恒拒——解锁面留装机环境，本机 sim 直跑永不满足；**实况收口（0.0.6 批F5b `fab7b6c`）**：[rrd.service](../base/os/distro/units/rrd.service) 已注入 `RR3_REBOOT_OK=1`（批E 核出的注释↔unit 漂移消除），板上 ctl.reboot 解锁面就位；真机执行验收（判据 14/15）留板上线）+ 拒绝口径与解锁口径都进日志环可查；ota 恒拒 `Permission(-22)`（契约冻结集无 NotSupported，文案点 M6——0.0.3 批D 后 rr3-ota-ctl 写侧主机面已实装，rrd 侧 ctl.ota 的接入仍是后续批）；已注册连接上行 ctl 被 `Permission` 拒但**同一条连接心跳照常被接受**（拒帧不断连）。rrctl 侧：[`tools/rrctl`](../tools/rrctl/src/commands.rs) 独立工程**六子命令**与 rrd ctl 白名单逐一同名对齐（0.0.5 批B，`25e90b7`，Q1 裁定：COMMANDS 4→6，`counters`/`reboot` append + 方法名白名单钉死单测）——UDS 一次性查询连接、5s 读超时、人读表渲染；counters 按模块前缀分组成 MODULE/KEY/VALUE 表（rrd 自有键归 `rrd` 组、空表打"无计数"、非 u64 报错不渲染 `?`——与 status 的 cell 容错口径有意分开）；reboot **CLI 不加本地门**（rrd 执行面双重门是唯一真值点，CLI 加门即第二真值），拒绝文案经错误信封回显非 0 退出、成功回执一行确认。独立测试（含 fake-rrd 真 UDS 往返 4 条：status/counters 实答渲染、ota/reboot 恒拒回显）不在七步门内。线级证据：[`e2e_ctl.rs`](../base/runtime/rrd/tests/e2e_ctl.rs) 8 条（F3 六条 + 0.0.4 append 两条：`ctl_reboot_is_refused_by_default_double_gate` 钉缺省恒拒——真执行面 `systemctl reboot` 挂 cfg(linux)，本机 macOS 不参与编译、e2e 只能钉拒半边；`ctl_counters_aggregates_heartbeat_counters` 钉心跳可选字段兼容 + 同键跨拍累加 + ctl 读回；counters 持久化跨重启另有 `counters_persist_across_restart_and_boots_accumulate`）。字面量归 [`reference/rrd-rpc.md`](reference/rrd-rpc.md) §7 §8 |
| `rr3-sim` | 已实装 | [`tools/sim`](../tools/sim/src/main.rs) 脚本化全链路演练入口，**四模式**：①默认两模块演练——起 rrd（boot spawn `rr3-sense` resident）→ 轮询 status 等 running → 外部 spawn `sim_peer` 发布 `rr3-interact/asr/text` → 四命令巡检（`denied_publish=0` 作发布被接受的线上证据）；②`--brain` 四真进程对话闭环（M4）；③`--profile` 装配演练（D9，`363e4ed`）——装配单驱动 rrd 装配链正门（`--profile` + `--contracts`，与 rrd.service 的 ExecStart 同形）boot 全部 face 模块，**演练 modules-root 是装机布局**：仓源 manifest 原文只把 entry 改写为产物绝对路径（rr3-pack `rr3_manifest_install_rewrite` 同语义门：装机值可装机 / entry 行恰一条 / 旧值与 facts 交叉核验 / 改写后重解析复核——"entry 在仓相对、装机绝对"的第三面）+ config 同目录就位（Rust 模块仓源 config、brain 用 mock 演练 config），python 模块产物 = shim（`python3 -m`，包名 = 目录名连字符转下划线）；`--brain` 与 `--profile` 同给即拒（Q3 同款）；④`--chain` 六真进程全链 mock 闭环（批F4 `379e6de`，判据 10 sim 档载体）：rrd boot spawn `rr3-sense` + 外部 rr3-interact / rr3-brain / rr3-act + 演练自带总线观察者 rr3-obs，mock 云（R4）下无人工干预跑通"采集→意图→判向→转向→表情"一轮以上（真板实测 boards §9）；演练统一注入 `RR3_PANEL_ADDR=127.0.0.1:18080`（与板上系统实例面板端口避让，--brain 同享常量）；CI `sim-chain` job（第 8 job）钉远端回执。[`assemble.rs`](../tools/sim/src/assemble.rs) 的 `repo_default_faces_are_assembly_ready` 把**仓内 default 装配单↔模块目录对账**钉进单测（六模块 manifest 可解析 / name == 目录名 / entry 仓相对无 `..` / config 在）——装配面漂移让单测红，不等演练跑一半红。[`which_python3`](../tools/sim/src/brain.rs) 从"PATH 第一个"改为**能力验证**（`import tomllib` 成败 = brain ≥3.11 门槛的直接证据，不满足候选继续向后找、全不满足 fail-fast 点名）。纯逻辑单测分居 drill / devices / assemble / chain 四模块（条数登记 conventions §2，全平台可跑）；编排（spawn 真进程，Unix-only）分居 `main.rs`。**六模块真进程判据（含 brain）归 CI `sim-assemble` job 远端回执**（macOS 本机 python3 3.9 不过 tomllib 门；无 brain 子集装配单 5 Rust 模块全链绿——装配 boot → 全 running → denied_publish=0 → 四命令巡检——已实证）。五步脚本见 [`tools/sim/README.md`](../tools/sim/README.md) |
| `contracts/events/*.schema.json` | 已实装（3 个） | [`sense-audio-vad_start.schema.json`](../contracts/events/sense-audio-vad_start.schema.json)（0.0.2；批F2 `e4ccbaf` onset 增可选 `doa{deg,bin}`——append 兼容 schema_version 仍 1，仅 onset 且判出时携带、offset 不带）：schema_version 1，$schema draft 2020-12，required `["seq","edge","speech_ms"]`，`rms_peak`/`doa` 仅 onset 携带（`allOf` + `if/then`），`additionalProperties false`；事实源 `rr3_vad_event_body`。0.0.3 批C 两件（T3 补裁定）：[`brain-intent-result.schema.json`](../contracts/events/brain-intent-result.schema.json)（required `["trace","user_text","reply_text","tts_ms","actions"]`；`actions` 元素 required `["topic","args"]` 且 `additionalProperties false`——词汇即注册表零新造）+ [`act-relay_state.schema.json`](../contracts/events/act-relay_state.schema.json)（required `["seq","on"]`——RPC 与 intent 两路共用状态流的摘要体）。**pcm STREAM 的 schema 未落**——0.0.5 Q3 裁定（`c7b91e7` 登记）维持空缺不冻结：schema 冻结与 rrd shm 接线绑定同一份后续 ADR（不写半成品），[contracts/README.md](../contracts/README.md) 演进登记在案 |

---

## 里程碑进度

"内容"与"验收标准"两栏按架构文档 **原 §10** 的里程碑表逐字抄录；那张表现在只有这一份——
架构文档 §10 已收口为一行指向本节（迁入与删原分两步做，是为了让删除那次改动单独可审）。
**当前实况**一栏是本快照读代码实测的结论，不是对验收标准的复述或期望。

| 里程碑 | 内容（原 §10 抄录） | 验收标准（原 §10 抄录） | 当前实况（`dc3a1d6` 实测） |
|---|---|---|---|
| M0 | 仓库骨架 + ADR-0001~0005 + legacy 冻结 | 目录树落地，rr3-mkmod 能生成合规模块 | 目录树已落地，ADR 已写到 0016（e6b13c9 板卡策略、5f67008 brain 迁移路线，均超出原验收范围），`rr3-mkmod` 的产物被回灌进契约用例 —— **已收口** |
| M1 | base/os：Buildroot 在 Radxa ZERO 3 跑通 | 串口出 shell；maskrom 自烧录镜像成功 | **结构就绪，未收口（板上敞口）**。defconfig、genimage.cfg（容量=构建期占位符）、extlinux.conf、post-image、flash-maskrom.md、patches/ 核对清单已全部落树，分区标签守护用例进 rr-ota；post-build.sh 装机布局已收口（28c629a：manifest 归位 + entry 绝对化 + config 随装，装配冒烟进 CI `buildroot-assembly` job）；缺的是：全量 make 构建（Linux/容器侧）、dts 与 U-Boot defconfig 核对、串口出 shell、maskrom 实烧——目标硬件 Radxa ZERO 3 **未采购**，板上收口项在硬件到位前不 claim（证据见能力表"Buildroot OS 层"行） |
| M2 | rrd MVP：A-IPC 协议层 + uds transport + registry + sim 双假模块 | 注册/心跳/崩溃自动重启；IPC-T-001/002/003 通过 | **已收口（留一条环境敞口）**。验收标准逐条对照：注册/心跳/崩溃自动重启——**全到位**（注册/心跳/死亡记账/扇出被连接层调用；崩溃自动重启由唯一 supervisor 线程执行：boot spawn、串行收尸、到期重启、按策略退避，线级证据 `e2e_spawn_restart` 5 条）；IPC-T-001/002——同机 golden 往返 + 契约逐字段对齐，全绿；IPC-T-003——三方对拍 **①②本机已过**（F5：宿主 dump==golden 逐字节一致 + aarch64 交叉 check 且 const 布局断言已在该目标求值），**③容器真跑 SKIP**（本机无 Docker；`--strict` 已接远端 ipct003 job（540ec0f），远端跑绿一次即结，见"本机无法收口"第 4 条），这是 M2 唯一敞口。0.0.7 批B 起 `ipct003.sh` 扩**四路**、xtensa 编译期对拍路 PASS（能力表"非 Linux 主控契约 parity"行），容器路敞口不变。`trait Transport` 已随 ADR-0008 定型并落地 uds 实现；依赖仲裁与 shm 传输为 M2 之后的增量（ADR-0009 / ADR-0011），不计入本里程碑验收 |
| M3 | rr3-sense + rr3-interact：ALSA 采集与播放 | 板上录音回放，VAD 事件上总线，音频走 STREAM | **已收口（真麦板上证据敞口）**。sim 侧证据齐：`rr3-sense` / `rr3-interact` 实装（见能力表两行）、rr-bus 同步客户端、rrd 四方法 RPC 与 rr3.call 路由应答回路、**三真进程** e2e（vad_start 事件上总线、pcm STREAM 有序转发、点名应答回到调用方，`e2e_audio_chain` 绿）、七步门禁全绿（conventions §2 台账）；ALSA 后端代码与 feature 门配置就绪。**板上进展（0.0.6 批F1/F4）**：板上 release `--features alsa` 两件构建链实测过（动态 ELF + libasound 断言，boards §8）、板上 `rr3-sim --chain` sim 音源全链闭环（boards §9）——**真麦录音回放仍未发生**：USB 麦阵列未采购（R22 定案 PS3 Eye 或同级 ≥2 麦 UAC），板上载体已是树莓派（0.0.6 批D 起），见"本机无法收口"第 7 条，真件到位前不 claim |
| M4 | rr3-brain：移植 asr/llm/tts | 全语音对话闭环（legacy 功能等价） | **已收口（云侧联调敞口）**。`rr3-brain` Python 实装（5ab029e，ADR-0014）：py 总线客户端 bus.py 386 行逐语义镜像 rr-ipc、三家云客户端 urllib 实装（`RR3_BRAIN_CLOUD=mock` 是测试注入点）、dialog 状态机串起 vad_start → asr → llm → tts 回路；线级证据 `rr3-sim --brain` 四真进程全语音对话闭环演练（5b55620）；批F4 `--chain` 六真进程 mock 云把对话轮扩到全链（判向→转向→表情，真板实测 boards §9）——详见能力表"`rr3-brain`"行。敞口一条：三家云真实凭据联调未发生（无 key 环境），不阻塞"legacy 功能等价"的 sim 侧验收。**2026-10-12 订正（版本边界轮）**：这句"legacy 功能等价"在**播报面**不成立——M4 按当时判据收口的是"对话闭环跑通 + TTS 验证性消费"（拿到 PCM、算出时长、入日志），而 legacy 机器人是**真的用人声说话**。今天云侧 PCM 没有通路到扬声器（`composite.py` 只发 `{"text"}`、interact 播 440 Hz 占位正弦），所以播报等价属**未达成**，落 0.0.5 批M MC-1/MC-2（F-4）。M4 的收口状态不回改，缺的是这条敞口的**指名** |
| M5 | rr3-act / rr3-connect / rr3-power | 语音控继电器 + WiFi 配网 + 电量上报；shm 控制面初版 | **已收口（sim 侧；板上敞口同 M3）**。逐条对照验收标准：①"语音控继电器"——**sim 侧闭环已证（0.0.3 批C，`be19982`）**：意图→动作映射落 brain（`intent.py` 精确词表 + 否定护栏，Q1 裁定策略层唯一归属）、act 订阅执行面两道归属门（exposes 归属 + 意图面开放）执行 `actions[]`、`relay/state` 状态流 append 在册（T3 补裁定）且 e2e 断言 RPC 路 seq=1 → intent 路 seq=2 跨路连贯——语音进 → brain 抽动作 → act 执行 → 状态回读，整链在 `e2e_act` 线级绿；②WiFi 配网——T4c（`2b08994`）net/link 状态机 + cloud/session 应答面（`e2e_connect` 两场景对偶绿）；**0.0.4 批D（`3682bfe`）配网面收口（sim 侧）**：wifi/scan|connect|forget 三 RPC + `Rr3Provision` AP 配网状态机（ap|station 两态纯函数）+ net/link 流 `mode` 字段随发 + 面板 WiFi 页三端点透传（裁决面钉在 connect 模块单测，`e2e_connect` 既有两场景不动）；③电量上报——T4b（`e87a284`）battery/state 周期流 + battery/low 边沿 + rrd 低电兜底四件动作面（`e2e_power` 绿）；④"shm 控制面初版"——Q2 口径（2026-10-04）已达成：ADR-0011 transport 层落地（单写者多读者环形缓冲 + POSIX 附着），rrd 接线不纳入 M5 验收（如需纳入须立新 ADR）。**0.0.6 批F5a 进展**：HAL linux 真后端五件落地（GPIO led/relay、PWM 舵机、link nmcli、battery INA219——代码与 musl 交叉 check 就位，板上单测欠账）；批F4 `--chain` 板上 sim 侧全链闭环（boards §9）。敞口 = 真硬件动作证据（继电器/配网/电池读数，随外设采购 + 板上线，同 M3 口径）；意图词表的产品化扩展属 brain 演进，不在 M5 验收内 |
| M6 | A/B OTA + 恢复出厂 | 断电升级不 brick，坏 slot 自动回滚；genimage 预留模块独立可写分区 | **未收口（主机面已实装，板上件与升级链未做）**。0.0.3 批D（`68220e4`，Q3 裁定）：switch/factory 的判定与改写逻辑在 `rr-ota::bootconf` 主机可测（字节级真工件对拍），`rr3-ota-ctl` 三子命令实装（`--bootconf` 必传、退出码 0/1/2），genimage.cfg 的分区集合（含 rr_staging 独立可写分区）已随 M1 落树。验收标准"断电升级不 brick、坏 slot 自动回滚"依赖的 U-Boot 回滚链（bootcount / bootlimit / altbootcmd / upgrade_available）**语义已落档、写侧未实装**（板上件，partition-layout.md 增补段含标准断电安全序与板上核对项清单七条）；槽位镜像写入（往 rr_b 落整镜像）随升级下载链另批；容量定型与实板验证随硬件——三项在位前不 claim |

---

## 本机无法收口的项

这些**不是待办清单里的模糊项，是本机没有能力证明的项**。任何"应该是好的"的推断都不构成
收口证据，所以下面如实写明缺的是哪一手证据，不软化。

| # | 无法收口的项 | 缺的证据 | 现状与后果 |
|---|---|---|---|
| 1 | 远端 CI 尚无一次真实运行回执 | 任一 remote 上一份全绿（或如实红的）运行记录 | 双远端：origin = atomgit（`git@atomgit.com:rockrobot/rockrobot3.git`）+ gitcode（`git@gitcode.com:rockrobot/rockrobot3.git`），`dev/hubs-01` 指针同为 `b787ba8`（0.0.7 批F 收口登记；0.0.5 起各批推送时 `git ls-remote` **三端**核对一致）。ci.yml 八 job（conventions / lint / brain-unittest 双矩阵 / ipct003 / distro-smoke / buildroot-assembly / sim-assemble / sim-chain）已全部落盘（540ec0f 起 4 个，a95eda7 / 28c629a / 363e4ed / 379e6de 增至 8 个），但**两个 remote 上一次都没有跑过**。本地门禁照旧靠 `bash tools/ci/check.sh` 门禁全绿才提交；远端首跑回执到手前，"门禁的强度等于提代码的人记不记得跑它"这半句仍然成立。**2026-10-08 批W2 迁移注记（0.0.5 增补轮四，e5a8c22）**：八 job ci.yml 已 `git mv` 至 `.gitcode/workflows/` 单真源（atomgit 只拾取该路径——0.0.5 批A' 实证；`.github/` 侧留说明行指向），**迁移后八 job 首跑回执待网页核对**——本条敞口不变，随首跑一并取数。**2026-10-11 R30 迁移注记（CI 真源迁移小批）**：用户裁定 atomgit 无流水线功能、CI 改走 GitHub Actions——真源 `git mv` 回 `.github/workflows/ci.yml`（R27 的 `.gitcode/workflows/` 路径作废，`.gitcode/` 已删除，八 job 定义零改动）；GitHub 仓库待建+SSH key 待配（`~/.ssh` 现仅 atomgit key，`ssh -T git@github.com` publickey 不通），推送 GitHub 后取八 job 首跑回执，取到即本条结项。**2026-10-09 收口批 E' 深挖注记**：九步门禁的 CI 侧执法（conventions job）与 sim 全链 e2e（sim-chain job）取证都押在本条上——第 II 轮重铺与机制核下沉五版期间，tools/sim 的 23 条 #[test] 处于零执行死角（批E' ⑥ 扩口径已封死、真跑全绿），而全链 `--chain` 本机不可复验（本机 python3 3.9 不过 brain 3.11 门）——首跑回执的核对优先级进一步上升。**2026-10-11 八 job 首跑全绿结项（CI 首跑修复批 H1–H4，run#6 38146782224 = 1c3cf41）**：GitHub 仓 rockrobot3/rockrobot3 建成转公开 + SSH key 配好后 e0a8bad 触发首跑 5 绿 4 红；五轮迭代修复：run#2 484a13c（批H1 espidf-sentinel fmt 归一 + rust-cache 工作区 pin）→ run#3 f640234（批H2 rr-hal linux 单测七败真修复 + result_large_err 契约级豁免——conventions/lint 双转绿）→ run#4 adf3d16（批H3 ci.yml 插 espup 装机步 + ipct003 第④路默认在线直连，本机网络 workaround 收进 IPCT003_CARGO_OFFLINE=1 / IPCT003_CARGO_MIRROR=1 环境开关——四路真对拍绿）→ run#5 8f33071（批H4 distro-smoke ⑤ 点名断言——回执实锤根因：脚本注释 ASCII 撇号截断 docker bash -c 单引号串、余行溢出宿主执行）→ run#6 1c3cf41（撇号清零 + 警示注释，八 job 9/9 全绿）。本条结项。步序见 [`process/conventions.md`](process/conventions.md) §2，脚本入口 [`tools/ci/check.sh`](../tools/ci/check.sh) |
| 2 | 设备侧 `idf.py build` 的密钥门未验证 | 在装有 ESP-IDF 的机器上的一次成功构建 + 一次无密钥失败的对照 | **已随验证对象消失注销（0.0.4 批F）**：`legacy/` ESP32 固件整树删除后，仓内工作树已无 `idf.py` 可构建的对象，"固件侧改一处配置会不会撞上密钥门"这条问题不再有仓内落点；固件源码仍在 git 历史（digest 归档台账在案），若未来复活固件线再重开此条并按当时的密钥口径取证 |
| 3 | 泄漏 API key 的提供方侧轮换未确认 | 提供方控制台里那把 key 已失效/已替换的凭据 | 仓内曾经出现过的 key 从 git 历史里抹不掉（0.0.4 批F 删除 `legacy/` 整树只清工作树，历史提交对象里 key 依旧在）；**从仓库角度能做的都做完了**（工作树已无 key 文件、文档不再复述），但"这把 key 现在还能不能用"只有提供方能回答，这一条不闭环，第 3 项就一直是敞口 |
| 4 | IPC-T-003 的容器真跑（三方对拍第三路） | 一次 `linux/arm64` 容器里 `ipct003_dump` 与宿主输出的逐字节 diff | **三方对拍前两路已在 F5 本机过**（98d1cc2 登记）：①宿主 x86_64 dump == golden PASS；②`aarch64-unknown-linux-gnu` 交叉 check PASS，const 布局断言已在该目标求值。缺的只剩 Docker 容器一路——本机 macOS 无 Docker，**该路已接远端 CI**：ci.yml 的 `ipct003` job（540ec0f，ubuntu runner + QEMU arm64 + `bash tools/ci/ipct003.sh --strict`）远端跑绿一次，即可把 [`envelope.schema.json`](../contracts/envelope.schema.json) 里该条移入 coverage 并在本表结项。**2026-10-11 结项注记（CI 首跑修复批）**：ipct003 job --strict 四路全绿回执到手（run#6 38146782224 = 1c3cf41，含 ③ linux/arm64 容器真跑与宿主逐字节 diff）——IPC-T-003 已同批移入 envelope.schema.json coverage（活引用同步：contracts/README、golden/README、rr-ipc header.rs 测试注释），本条结项 |
| 5 | 登记平台（Windows L1 / WSL2 Linux L2）的一轮 ①b 全绿回执 | 登记平台的一次 ①b 全绿 | 0.0.2 各批在本机 macOS 逐批实跑 ①b 全绿，0.0.3 批A–E、0.0.4 批A–F、0.0.5 批A–E 与增补轮、0.0.6 批A–F5b 同（Unix 侧逐批登记见 conventions §2）；Windows 条数为推算（POSIX 段 cfg 门控不参与编译，无 Windows 目标未实跑）。conventions §2 的两平台实测仍是唯一权威口径，macOS 单次全绿不替代它 |
| 6 | M1 的板上收口四件 | 一次全量 make 产出的镜像 + 实板串口出 shell + maskrom 实烧 + 分区容量定型 | 目标硬件 Radxa ZERO 3 **未采购**，且全量 Buildroot 构建需 Linux/容器侧（本机 macOS 无环境）。defconfig 里 `BR2_LINUX_KERNEL_INTREE_DTS_NAME` 与 `BR2_TARGET_UBOOT_BOARD_DEFCONFIG` 两个名字是否命中、genimage 引导裸区偏移是否正确，都**未核对**——核对清单在 [`buildroot/patches/README.md`](../base/os/buildroot/patches/README.md)，逐条回填前结构就绪不等于可跑。保底轨 post-build.sh 的装机布局逻辑已有 CI `buildroot-assembly` job 覆盖（夹具侧模拟目标根），但夹具不等于 Buildroot 真实 TARGET_DIR |
| 7 | M3/M5 的板上证据（真麦录音回放 + 继电器/舵机/电池/链路真件读数动作 + 板上 rr-hal 单测补跑） | 树莓派上一次真机采集/回放 + 真件读数/动作 + 板上 `cargo test -p rr-hal` 全绿回执 | HAL linux 真后端已实装（批F5a 五件：ALSA + GPIO led/relay + PWM 舵机 + link nmcli + battery INA219，见能力表 HAL 行），aarch64 musl 交叉 check 全过——但**真机一行都没跑过仍未变**：板当前离线（ssh spharx 超时），板上 `cargo test -p rr-hal` 补跑欠账（rsync 口径同批F1）；真件读数/动作随外设采购（BOM 定案 R22，采购中）；sim 侧证据（三真进程 e2e + 模块用例 + `e2e_act` 意图执行链 + 批F4 `--chain` 板上全链 mock 闭环）不能替代真件证据。板上线三项欠账（板上 `cargo test -p rr-hal` 补跑 / socket 放行真机复验 / 部署链回归演练）的执行步骤、预期输出、判定与失败处置已显式化为清单（0.0.5 增补轮四 批W7 A4）落 [boards/raspberry-pi-64.md §10](../base/os/distro/boards/raspberry-pi-64.md)，板上线照单执行 |
| 8 | 发行版 deb 的 CI 真装回执（distro-smoke） | distro-smoke job 的一次远端全绿回执 | **真装语义已被真板实测补上（0.0.6 批D，`efd224f`）**：树莓派板上 `dpkg-deb --build` + `dpkg -i` 成功、五模块全 running、`rr3-ctl` 三查 + reboot 自恢复全绿——比容器路更强的真装证据已在案（见能力表 deb 打包链行）。余下的是 CI `distro-smoke` job（a95eda7，rust:bookworm 容器单步——glibc 自洽、本机产物装本机 deb，断言链 5 模块 spawn + brain 缺格点名 + 0 拒 boot + `denied_publish = 0` + counters 回写断言（0.0.6 批E' 补 ⑧：日志逐字 + counters.json 含 `"boots"`））的远端回执，**归第 1 条远端 CI 总敞口统收**；**2026-10-11 结项注记（CI 首跑修复批）**：distro-smoke job 远端全绿回执到手（run#6 38146782224 = 1c3cf41，八 job 全绿）——归第 1 条统收同步结项；本地替代证据（rr3-pack 单测 + `--stage-only` 冒烟）不变 |
| 9 | D9 六模块真进程巡检（含 Python brain） | sim-assemble job 的一次远端全绿回执 | 本机 python3 = 3.9.6，不过 brain 的 `tomllib`（≥3.11）门，且无 brew / uv / pyenv 可装（D9 实测：`--brain` 演练在 brain 门槛处 fail-fast 点名）。本地替代证据：无 brain 子集装配单（5 Rust 模块）装配演练全链绿（装配 boot → 全 running → `denied_publish = 0` → 四命令巡检）+ default 满配单在门槛处当场点名。**六模块判据归 CI**：`sim-assemble` job（363e4ed，runner python3 ≥3.11 满足门槛），远端跑绿一次本条结项。**2026-10-11 结项注记（CI 首跑修复批）**：sim-assemble job 远端全绿回执到手（首跑即绿，run#6 38146782224 = 1c3cf41 全绿收官复核）——本条结项 |
| 10 | M6 的板上件（U-Boot 回滚链写侧 + 实板验证） | 实板上一轮 switch→重启进对侧→校验 bootcount/altbootcmd 回滚链真生效 | 0.0.3 批D 只交付主机面（引导配置改写），U-Boot env 写侧、块设备/挂载探测、fsync 加固、vfat rename 断电窗口行为、槽位镜像写入均未做——逐条列 [`partition-layout.md`](../base/ota/partition-layout.md) 增补段板上核对项清单（七条）。判据同 M6 里程碑行：三项在位前不 claim |
| 11 | 0.0.6 阶段二真机功能链的板上证据（语音采集 / DoA 判向 / 舵机转向 / 表情输出） | 树莓派上麦克风 / 舵机 / 显示真件到位后的一次全链实测 | **sim 档已收官**：批F2/F3 sim 段（`e4ccbaf` / `444c764`）+ 批F4 板上 `rr3-sim --chain` 全链 mock 闭环（boards §9，判据 10 sim 档过——R9 档位诚实标注：采购前档不充当真件收口证据）；批F5a 五件 HAL linux 真后端实装（musl 交叉 check 全过）。真件档随外设采购（BOM 定案 R22：PS3 Eye 或同级 ≥2 麦 UAC / SG90 / MAX7219——树莓派外设采购中、ESP32 硬件已齐）；真件到位只动 HAL 后端，云联调以 `RR3_BRAIN_CLOUD=mock` 先行（R4）——真件档验收是板上线后的第一优先敞口 |

---

## 在制品

**本快照只记锚点提交 `dc3a1d6` 那一棵树的实况；锚点之后新进的提交、以及以未提交状态存在的在途
改动，都不进本表，重锚时随锚点一并更新。**

锚点 `95d37ef`（上一版快照，0.0.6 全版本收口，载体提交 `405bef3`）之后进树的提交（60+），按批分组全部已按实况计入上文能力表与下方批次全景段：

- **0.0.5 增补轮四（批W0–W7，2026-10-08 收官）**：`55b0234`（R23–R29 落档）/ `a0d83d4`（两方案承接）/ `e5a8c22`（CI 单真源迁 `.gitcode/workflows/`）/ `9f2abba`（counters 节流）/ `48e047d`+`a1dd7d7`（espidf std 构建链冒烟）/ `020be7b`（真件装配与标定手册）/ `db72f79`（0.0.7 修订版 5）/ `65ed883`（批W7 收官）；
- **0.0.5 第 II 轮原子重铺（批A–E'，ADR-0018/0019）**：`b211b95`（两 ADR 落档）/ `4c97ac8`+`14f1972`（批A）/ `7a3c277`（批B）/ 批C 五重铺 `4002303`/`a82d8b6`/`e17974b`/`a1723b5`/`708e5ad`/`b3aafa2` + `793e98f`（批C⑥ 对账）/ 批D①–⑤ `d09c382`/`f635fcd`/`a767ab9`/`2925847`/`ff1024b` / 批E `2bd1a80`（七步扩九步）/ 批E' `9ae8889` / 设计 docs 十件（`72c688d`/`face127`/`b61c845`/`d4c6c83`/`f298e7a`/`00d719a`/`a9fb167`/`fcd8672`/`8989e9b`/`1510cb9`）；
- **0.0.5 第 III 轮（批I/J/K，2026-10-09/10）**：`d21839a`（批I rr-hal-api no_std 契约层）/ `4216d7f`（批J ⊙通路+七段描述符+retired 首例）/ `f397329`+`d61011c`（深挖收口）/ `5962084`（批K 收口，第 III 轮收官）；
- **0.0.6 深挖收口批**：`35c3338`+`d1a0d54`（rrd.service 装机位随 `--profile` 改写 + ⑧-b L0 门拓全限定内联）；
- **0.0.7 ESP32 四层梯子（批A–G，2026-10-07/11）**：批A `8285677`（方案落档+Q4/Q5）/ 批A' `942d486`（xtensa 工具链前置）/ 批B `a860405`（层① no_std parity）/ 批C `46edb3c`（层② UART 传输+echo 夹具，另 `baf67a6`/`4511641` 两 docs）/ 两板收口批 `6d3a1ee`+`3c467c2`（批C④⑤ 板级收官）/ 批D1 `22783ea`+`f72c0d4`+`c6e3a9c`（层③ 上板，另设计对表 `4895d1d`）/ 批E 七件 `7147890`/`955ef14`/`bab64f5`/`73547a9`/`d30d4eb`/`df914ea`/`870cfe6` + `567c832`（层④ bridge e2e）/ 批G 六件 `707611b`/`dfb55ff`/`bf20d15`+`5c16b15`/`7122cee`/`2372299`/`9fcbb67`+`dc3a1d6`（矩阵行七格收口）——以上各批均在 conventions §2 逐批登记在案。

以上提交都已按实况计入上文能力表与下方批次全景段；**锚点树内在途的未提交改动只有本收口批（0.0.7 批F）的文档面**
（本文件重锚 / conventions §2 批F 汇总条），随本批落库。

**0.0.6 版本批次全景（修订版 6 口径）与在制品**：

- ~~**0.0.6 阶段一收尾件（批E'）**~~：**已落库并真板复验结清（`a5b3093` + `f2597d5`）**——boots 三件修复（unit 注入 `RR3_DATA_DIR` + postinst 建目录置属主 + distro-smoke 断言补 ⑧"counters 回写"）+ boards §7 补录 ctl socket root/sudo 现状；板上 restart 落盘 `{"boots":1}`、reboot 自恢复 `{"boots":2}` 跨重启累积成立；ctl socket 加组放行牵动 ADR-0010 权限模型，归阶段二小批（同批核对 unit 补注 `RR3_REBOOT_OK`）；
- ~~**0.0.6 阶段二（批F1–F4 + 批F5a/F5b，修订版 6 重排）**~~：**已全部收官**——批F1 板上验证四项（boards §8）、纪律小批 `b81c986`、批F2/F3 sim 段、批F4 板上 `rr3-sim --chain` 全链 mock 闭环（boards §9，判据 10 sim 档过）、批F5a 五件 HAL linux 真后端、批F5b 运维链 R12 两件（`fab7b6c`，ADR-0010 修订 1）；
- **板上线后待补**：板上 `cargo test -p rr-hal` 补跑（rsync 口径同批F1）+ 判据 12/13/14/15 真机验收 + socket 放行真机复验 + 真件档（随外设采购）——板当前离线（ssh spharx 超时），到位即补；
- **0.0.5 增补轮四——板载开发前技术落地（批W0–W7，2026-10-08 收官）**：R23–R29 裁定落执行——批W0 落档 `a0d83d4` / 批W2 CI 迁 `.gitcode/workflows/` 单真源 `e5a8c22`（八 job 首跑回执待网页核对，敞口表第 1 条）/ 批W3 counters 落盘节流 `9f2abba` / 批W1 espidf std 冒烟 `48e047d`（0.0.7 §9.3 ① std 链实证）/ 批W5 真件装配与标定手册 `020be7b` / 批W6 0.0.7 修订版 5 `db72f79`（桥形状钉死 + S3 预案 + e2e 双档判据）/ 批W7 收口（HAL env 总表落 distro README + 板上线触发清单落 boards §10 + 0.0.6 修订版 7 + 本行注记）——执行对账落 [0.0.5 方案 §2.5](process/dev/0.0.5版本洛克三号对齐与裁定方案.md)；W4a/W4b 作废（R25：批F5a④⑤ 实况超越）；板上欠账与真件档维持触发态；
- **0.0.5 第 II 轮——原子重铺施工（承 [ADR-0018](process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md)/[ADR-0019](process/adr/ADR-0019-原子功能模块模型-格件面与组合代数.md)，0.0.8 并入不再独立成批，方案 §7–§11 批A–E）**：批A 坐标系对齐**已收官**——批A① 二十一格原子归属清单落 [0.0.5 方案 §12](process/dev/0.0.5版本洛克三号对齐与裁定方案.md)（`4c97ac8`）；批A② `[faces]`→`[enabled_slots]` 消歧改名（ADR-0019 裁定 1 / R16：profiles 两份 + rrd/profile.rs + 查册门 + sim/assemble.rs + 架构/ADR/参考文档当前节，历史存档节不改写，`14f1972`）。批B 原子件契约随本批落库——批B① Atom manifest 契约定形（`atom.schema.toml` + `atom.rs`：查册门共用 slots 真值、每格一件恰一项、板无关禁词断言）；批B② `rr3-mkmod` 扩产原子五件套生成（回灌 `rr3_registry_validate_atom`，bin `--atom` 子命令）；批B③ `rr3-power` 打样重铺（`atoms/battery` + `atoms/charger` 各五件 + `composite.toml` ⊗ 链 + `main.rs` 248→17 行，SURFACE/module.toml/config.toml/Cargo.toml 零改动）。批C 组合件与四面**已收官**（2026-10-09）——批C① sense（base64 塌缩 `4002303` + 重铺 `a82d8b6`：capture/vad/env/imu 四原子，main.rs 133→16 行）/ 批C② act `e17974b`（servo/led/relay 三原子，intent/reflex 留组合件）/ 批C③ interact `a1723b5`（face/player 两原子，panel 状态面留组合件）/ 批C④ connect `708e5ad`（wifi/bt 两原子，session 留组合件）/ 批C⑤ brain `b3aafa2`（dialog/asr/llm/tts 四原子两件式 manifest——Python 面只落 atom.toml + assemble.toml、机制本体 .py 原位，`__main__.py` 组合件分装吸收、机制主体迁 `rr3_brain/composite.py`）；六单元入口全部瘦入口、17 份 atom.toml 钉进 rr-registry 的 `#[cfg(test)]` 面（13 Rust + 4 brain 两件式；跑在 ①b 全量测试，② 契约目标本身不读件声明）、SURFACE 与六红线逐批复核、每批门禁直跑 GATE_EXIT=0；批C⑥ 收官对账（方案 §12 落点列实况 + conventions §2 批C 条目，本行同步）随本批落库。批D∥E 已解锁：批D① 机制核总线语义下沉**已落库**（ADR-0018 裁定 1/5——`rr-core` crate 落地：常量/寻址/帧组装时钟注入纯函数，no_std + core+alloc 12 测试；`rr-ipc` 帧形状下沉 `protocol/wire.rs`、re-export 门控拆分，58+4 测试零波及；`rr-bus` 改消费 rr-core、对外路径与线上字节零变化 8+1 测试；xtensa no_std 线 `cargo +esp check -Zbuild-std=core,alloc` 双绿；conventions §2 批D① 条目与本行同批）。批D② 模块运行时执行环下沉**已落库**（`rr-core` runtime：连接态转移/心跳拍点/分发分类纯函数状态机——net.online 格机制核切片 = `Rr3LinkState`，7 测试；`rr-bus` client 变 std 驱动器消费拍点真值与分发分类，线上字节与拍点序列逐字不变 12 测试守恒，xtensa 线复验绿；方案 §12 net.online 行落位注记同批）。批D③ 控制原语+安全核下沉**已落库**（`rr-core` control：限速门/低电边沿/饱和检测三件真值自模块面迁入 + 限位钳位新原语 `rr3_travel_clamp`，9 测试含逐拍一致性判据；rr3-act/rr3-power re-export 保路径、RPC/intent 两路落角接安全核钳位，xtensa 线复验绿；方案 §12 reflex 行落位注记同批）。批D④ 板特定缺省收口进描述符**已落库**（ADR-0018 裁定 4——profiles 两份 `[hardware]` 物理绑定段、rrd 装配链解析 spawn 注入 `RR3_HAL_BINDINGS`、rr-hal `descriptor` 合成生效绑定（描述符为底/env 应急覆盖），五选择函数接绑定、代码缺省全删 fail-closed 不回落树莓派引脚；conventions §2 批D④ 条目、architecture §12.3 注记与 §3 落点表划线同批）。批D⑤ xtensa 对拍门 + 无 OS 启动形态点验**已落库**（`tools/ci/rr-core-xtensa-check.sh` 专用门三查全绿：机制核 tripwire（无全局分配器/std 逃逸）+ xtensa none-elf 裸机面编译（rr-ipc golden 头 const 断言该目标重求值）+ 宿主面 24 测试对拍，SKIP 纪律同 espidf-std-check.sh；0.0.7 层③④对表结论随批登记、architecture §12.6 注记同批）。批E §13 静态门接线**已落库**（七步扩九步：⑥ 模块测试接线（modules 逐工程 cargo test，打空 FAIL）+ ⑦ 静态断言四件——机制核零策略载荷（外部板卡厂商/器件禁词，rock3 自有标识不在禁词）/ F⊗F 非法 / 五件套计数（brain 两件式豁免）/ 入口 ≤80 行，全 fail-closed；每格一件反向断言落 rr-registry 仓扫描测试——21 格唯一覆盖账平，BUILTIN_NO_ATOM 四格豁免钉死；随批 rustfmt 全仓归一 + clippy 全仓清零复验，CI lint 对两 esp 平台条件工程排除与 ④ 同口径；conventions §2 批E 条目与本行同批）。**第 II 轮批A–E 全部收官**（0.0.8 并入不再独立成批，方案 §11 DoD 九项随批E 收官勾定）。批E' 深挖收口**已落库**（批E 后对 0.0.5 交付面漏洞深挖，三实锤同批结清——① sim 23 测试死角封死（⑥ 扩为 ④ 同口径全量独立工程，PASS 25→30，真跑全绿证重铺无行为漂移）/ ② manifest 未知字段反向断言（atom+module 共用实现，上线即抓出 atom.schema 漏登记 schema_version 活案例，补登记修复）/ ③ atom.name == 目录名钉死；CI 回执核对与 ⑦-a 禁词表双份维护两项登记敞口未结；conventions §2 批E' 条目与本行同批）；**第 III 轮已立项·未开工**（2026-10-09，方案 §13–§17 批I–K）：第 II 轮收官复核查出四处"设计承诺 > 代码事实"——① ⊙ 在无 std 的板上无路（Rr3TransportConn 唯一实现路径是 impllt;T: std::io::Read + Writegt;，transport/uart.rs 整模块 cfg(any(linux, macos)) 属宿主侧）；② 能力面契约带平台形状（rr-hal 七 trait 全 #[async_trait] + Send + Sync，件侧经 rr3_block_on 自旋，Rr3LinkState/Rr3PcmStream 把平台类型焊进件签名）；③ L3 注入面零实现（17 份 assemble.toml 全 handles=[]/ops=[] 且无读者，缺格降级环因 Rr3SimProbe 恒真而不转）；④ 承载切线按"有没有 OS"二分，使 ESP32+FreeRTOS 无档可归。落点 = ADR-0018 修订 1（承载三档 host/rtos/bare · 机制核外延定名 rr-ipc protocol + rr-core + 待建 rr-hal-api · 三处窄缝 · 描述符六段 · 点验矩阵六列定形 · 无堆降敞口）与 ADR-0019 修订 1（Facade 降为职责视图、不建第四份 manifest · 双达标② 由扇入改判为件级逐拍对拍 + 依赖闭包 · L3 建实体 · 求和公式改写 · 撤"业务阈值 grep 即红"的过高宣称），架构展开落 architecture v0.8 §12.1/§12.2/§12.3/§12.6/§12.8/§13/§14 与 §4.0/§4.0.3/§4.0.4/§4.3/§5。0.0.6 随修订版 9 承接点验矩阵全承载档首行，0.0.7 随修订版 7 定位订正为轻承载档（原"无 OS 端"标签系第 II 轮改写时写错，已订正）并新增批J 收口该行。本轮立项零 .rs 改动；三处窄缝与两条新判据的实施归批I–K，立项不等于实施解锁；跨版依赖（0.0.6/0.0.7 矩阵第③④列）在批J 落地前按"前置未落"如实登记。同日二次对表再落 **ADR-0018 修订 2/3** 与 **ADR-0019 修订 2/3**：补齐无基座（自足 bare）形态的**寻址与生命周期**（task 静态分配、运行环预置入口、不宣称发布主题）、能力面契约的**增长律与对象形状**（新执行器=新 trait、bare 禁 trait 对象、能力面签名禁 alloc）、**时序预算位**（并明确"逐拍一致"只保证数值语义不保证实时性）、件级对拍判据的**第二条腿**（期望值引自契约，只有确定性记未达标）、**retired 号位下线机制**（只增不改冻结的是历史号位、不是永久义务）、依赖闭包 **L0/L1 两级**、**门查门**（新机制核面必须同批进 ⑦-a 扫面与 xtensa 门清单）、描述符段数订正（六段→**七段**，含既有 `[os]`）、窄缝落点与 `transport` 层 `cfg(not(target_os="none"))` 门解除。architecture 升 **v0.9**；十处施工阻塞（装机包不含件级声明、`profiles/` 机型集被 contract.rs 硬 pin、`rr3-pack` 只认 distro.toml 无 `--profile`、mkmod 与 ②b 形状成对、`[hardware]` 扩键需契约级评审、禁词作用域须与板数据面分离、11/13 件依赖 serde_json 且 7 件测试依赖 rr_hal::sim、执行宿主无 esp/xtensa 与 musl target 且板离线外设未采购、CP2102 不在 BOM、批次代号与 0.0.6/0.0.7 撞名）全部登记为第 III 轮 **§13.1 前置任务 C1–C17（各批开工门）**，批次代号改 **批I/J/K**；0.0.6 随修订版 10、0.0.7 随修订版 8 各补**执行前置与 runbook 断点订正**（板 SSH 不可达、musl target 缺、`pgrep` 路径错、`raspi.toml` 不存在、`--features uart` 无效、`frame.bin` 无生成器），未就位判据一律记**"无载体"**，禁把 SKIP 读成 PASS。本轮仍零 `.rs`、零契约文件改动。**同日三轮复核（docs/design 全量对代码回抄）**：六册逐符号核对后修掉
24 条不实现状断言（签名/枚举变体/载荷键名/用例存在性/参数有无读者），框架升 **v1.1**（推测值独占 §11、
§3 现状与目标分栏、件面导出 vs `mech` 内 `pub` 的"内松外紧"必须立单），并落五条裁定 R-1…R-5 入
[模块设计索引 §9](design/模块设计索引.md)——R-1 face 权威序（显式点名 > 播放自动态，代码为准）、
R-2 `charging` 只由句柄派生（撤"恒 false"红线，标定对象改 percent 曲线）、R-3 四条"在册不实"的边一次走
retired（C14 扩围，含 `dialog/turn` **全仓零生产点**）、R-4 批I 拆 led 线程前**先**把三条用例改合成时钟、
R-5 panel 的 `live` 泄漏降为敞口、改立"spawn 失败令 accept 退出"这条可达缺陷；全仓"六段/六列/七步/批I–H"
旧口径扫清（唯余 `.gitcode/workflows/ci.yml` 的 job 步骤名，属 CI 配置，留批K）。新增缺陷工单：sense S-6、
act A-7、interact I-7、power P-6、connect N-6、brain B-7/B-8，另立**孤儿参数清零**一条 DoD（六册同纪律）。**同日三轮（模块设计细化 + 前提推理）**：框架升 **v1.2**，
每册新立两件必修工件——**§12 跨档动作**（逐件判 host/rtos/bare 能不能跑、卡在哪个符号、改什么形状）与
**§13 策略字面量清单**（逐字面量登值、有无读者、去处与**契约字面量豁免**），六册全部填满；索引再加
§9 裁定 R-1…R-6、§10 形态裁剪三例（F-A 全承载 / F-B 桥接微型 / F-C 自足微型）、§11 逻辑闭环审计
L-1…L-10。推理产物三份修订：ADR-0018 **修订 4**（`[supplies]` 平台看门狗位、自足档本地守护输出、
经桥档定**注册态**）、ADR-0019 **修订 4**（**载荷组装上移承载层**，依赖闭包按**发布权**收窄——据此
**收回**"件必须无 `serde_json`"的一刀切：线上字节本就是 JSON，而自足档根本不宣称发布主题，
那个 no_std 编码器今天没有消费者）、ADR-0012 增补注记（base 层"恒在位"按档分族 base-supply/base-bus，
并订正正文 20→**21 格**）。新工单 S-7/S-8、A-8…A-11、I-8…I-12、P-7/P-8、N-7…N-9、B-9/B-10；
0.0.5 前置清单扩 **C1–C16**（C15 看门狗与守护输出、C16 载荷上移）、批I 加 F5、批J G3 补看门狗位、
DoD 加三项（§12/§13 齐、F-B 形态成立、base 分族落地）；architecture 加第六供给位与形态三例速查。
本轮仍零 `.rs`、零契约文件改动；⑤ 文档门复跑 0 空文档 / 0 断链 / 0 导航漏链。
**同日四轮（D4 裁定 = 执行宿主）**：问法从"指定哪台机器"改判为**标准化环境集**——0.0.5 新增
[§13.2](process/dev/0.0.5版本洛克三号对齐与裁定方案.md) 定形表：**E1** host-x86_64
（`rust:*-bookworm` + musl/arm64 target + qemu-user + docker）、**E2** xtensa-none-elf
（esp-rs fork v1.90.0.0，`build-std=core,alloc`）、**E3** espidf-std（`espressif/idf` + esp-clang）
出 ①②③④⑤⑥ 六列，**E4 真板不可被替代**（⑦ 时序与真外设证据只能来自它——云 runner 是共享 x86、
qemu-user 是指令级模拟，两者的时间都不是硬件时间，拿来充 ⑦ 就是"以设计已覆盖充当已验证"）。
矩阵每行必带**环境指纹**（镜像 digest + `rustc -Vv` + 目标三元组），指纹缺失或换代不重跑按"无载体"计；
判据真源仍是 `tools/ci/*.sh`，workflow 只是调用清单（多宿主 ≠ 多套判据）。C11/C12 同步改判、
architecture §12.6 承载体段重写、索引 §9 加 R-6。**两条资源动作未结，如实登记**：本仓今天只有 atomgit
一个远端，走 GitHub-hosted 需你授权建镜像仓并推送；私有仓 Actions 额度有限且 esp 镜像首次拉取很贵，
须先配缓存。二者未清前 E1 以本地 WSL2 形态承载，矩阵照 `--strict` 记"实跑几路 + 其余无载体"。
**同日五轮（可开发性审查）**：改用三条硬尺量每一条结论——**有没有批次归属、有没有载体、有没有可证伪判据**。
查出并当场结清 **五件无主项**：① 自足档本地守护输出（原文写"随批K 定"而批K H1–H5 无人认领）→ **批K H6**；
② "无 WDT 不得归 bare"只有三处散文、零执法点 → **批J G6**（描述符装载门拒 boot 并点名）；
③ base 分族与三态标记**没有数据位**（实测 `contracts/slots.toml` 只有 `name/layer/note`、`Rr3SlotTable`
只解析 name→layer，裁定要的"记不适用"无处可记）→ 新门前置 **C17**（契约评审；建议加 `status`/`tier_scope`
字段，而非把语义藏进 `note`）；④ ⑦ 时序列**没有生产者**（四份文档各写一半，没人定义人肉实测怎么进矩阵行）
→ C12 补 **`--manual <列> --evidence <路径>` 回填契约**，证据文件须带环境指纹 + 采样窗口与样本数 + 原始数据，
缺一判"证据不足"不判 PASS；⑤ `mkmod` 模板仍产 `handles = []`/`ops = []` 与件面 `*_body`，与批J G4/C16
不成对 → **C3 扩写**为"模板 × 形状测试 × 件面形状"三者同批。**四处口径互斥**消解：P-4 并入 P-7、
L-6 归 H2（不再与卡批I 的 C8 互指）、L-8 分层（串值单点批I / 页面向后端取批K）、N-1 定批I。
**两处结构谎报**订正：§13.1 承诺把失败处置"写进 §14 每批验收四件套的失败处置栏"，而 §14 表只有五列、
**那一栏根本不存在** → 改为表后统一口径三档（C 项未结 / 载体不在 / 门与数据互斥各有处置）；
§17 的 18 条 DoD 里 **C3/C4/C9/C12/C15 从未有 tick**，"C 未清不得勾 DoD"对这五条不可执行 → 补两条勾点、
开工门由五条改列七条（补 C15/C17）。**挂着却没工单的 D8** 结清：act A-12 + interact I-13 立同一张
失败语义分类表，框架通用约定 1 改引该表、不许各册自定例外。architecture §13 新增两行门（档位判定、
⑦ 证据回填）并标注格状态标记"数据位今天不存在"；索引新增 **§12 可开发性审查**（含五条可证伪命题：
换 OS 不改码 / 换板只改装配单 / 安全核跨档生效 / 裁剪=改数据 / 微型形态可诊断，各配一个能推翻它的实验）。
**结论如实**：批I 现在可开工（F4 记无载体）；批J 差 D1 + C17 + 两个契约评审；批K 差 C17 数据位与 E4 真板。
差的是两个裁定、一次评审和物理设备，**不是再写一轮设计**。
**同日六轮（代码前进后的基线对表，基线 `22783ea`）**：另一台设备把**第 III 轮批I–K 全部落库**
（`rr-hal-api` 七 trait 同步化、`Rr3ByteStream` 窄缝 + `rr-ipc` 变 `#![no_std]`、件面载荷上移、
`assemble.toml` 注入面与校验门、`matrix.sh` 七列、`esp32-mini.toml` 第二形态、看门狗三位与装载门拒
`tier=bare` 无 WDT、`Rr3GuardBuf` 守护输出语法、`rrd.service` 随 `--profile` 改写、门禁十步）。
本集"还没做"那批断言因此大面积过期——**过期不回改正文**（第 I/II 轮正文同样按存档冻结），
而是按新立纪律逐条点名作废：框架升 **v1.3**、每册加**第十五节 §14 基线对表**（六册全填，带符号级证据
与去向），索引加 **§13 基线对表**（作废清单 + "文档说有、代码没有"清单 + 本轮决策 R-7…R-13）。
**自决六处冲突**（依既有工程思想，未回头问）：① `matrix.sh` 定性为**报告聚合器**、不进 `check.sh`，
改由 CI 加 job 调用（批L L-1）——塞进门禁会让无容器机器恒红，可没有调用方就是没有主语；
② `tier_scope` 与 ⑦-c 的件名 `if` 一起数据化（L-2）——"门只读数据"此前只对 ⑧-a 成立；
③ 已收口批次**不重开**，余数另立 **0.0.5 批L（L-1…L-10）**，六册逐条回指，一个余数只有一个落点；
④ `rr-hal` 选择器仍返 `Box<dyn>` 判为**正确落位**（契约面才禁 dyn，承载层允许 std 形状）；
⑤ ⑦-a 文件清单三处不一致按"脚本为准"回抄 `conventions.md` 与 `tools/ci/README.md` 并补自指检查；
⑥ brain 的 `net/link` 主张以**撤申报**方式结清（R-3 允许的二选一），文档改述为"主张已撤销"而非"已实现"。
**两处谎勾订正**：`esp32-mini` 的 DoD 拆成"数据与装配段成立 [x]"与"板侧真跑 [ ]（无载体）"；
功能守恒红线的"申报面零改动"与 R-3 自相矛盾，**改述为"机制面与字节面零改动 + 申报面变更须经裁定在册"**
后如实勾上。另查出 **6 个新孤儿配置项**并回 §17 该条（保持未勾）。**还欠的东西没变**：
E4 真板运行半边（`tools/espidf-sentinel/README.md` 实测报告表仍全 `—`）、板侧低电兜底动作（L-8）、
0.0.6 BOM 的六件外设。本轮仍零 `.rs`；⑤ 文档门复跑 0 空文档 / 0 断链 / 0 导航漏链。本轮仍零 `.rs`；⑤ 文档门复跑 0 空文档 /
0 断链 / 0 导航漏链。
**批I 能力面契约下沉已落库**（2026-10-10，方案 §14 批I 行 + C7/C8/C16 结清，ADR-0018 修订 1 /
ADR-0019 修订 3·4）：`rr-hal-api` no_std 契约层落地（7 trait 同步签名、值类型去 String/Vec、
占位后端随迁），rr-hal 退 Linux/sim 实现层 re-export 保路径、全仓调用点同步化去 `rr3_block_on`；
led 去线程改合成时钟状态机（R-4 顺序：用例先改注入时钟再拆线程，行为序列不变）、bt 去 env 改
config `[rr3-connect.bt] up` 注入（N-1）、sense config 与 power/connect 同口径（S-1）、假 AP 表
迁 HAL 后端（N-9）、sim 波形 LUT 迁后端（I-10）、S-6 整型化——框架 v1.1 违例清零；载荷组装上移
承载层（C16）件面 serde_json 11→0 且线上字节逐字不变；**F4 xtensa 有载体真跑绿**（本机 esp
fork 工具链在位，专用门三查全绿——上文"批I 现在可开工（F4 记无载体）"的预判未发生，方案 §17
已如实改记）；C7 闭包口径钉死、C8 门查门两处清单接线、C5 维持批J。九步门禁直跑 GATE_EXIT=0
（30 PASS）；clippy 全仓清零复验（rr-hal-api result_large_err 契约形状带注豁免 + 五处 lint 修）；
e2e 两处夹具随新口径对齐（audio_chain 的 sense config env 指仓内缺省档、connect 的 timeout_ms
迁 wifi 段）。conventions §2 批I 条目同批落库
- **批J `⊙` 通路 + 描述符七段 + 注入面已落库**（2026-10-10，方案 §14 批J 行 + C1/C3/C4/C5/C6/C9/
  C14/C15/C17 结清，ADR-0018 修订 1 / ADR-0019 修订 1 ③·修订 2 ②）：G1/G2 `Rr3ByteStream` 两方法
  trait 落 bytestream.rs、conn blanket impl 换底 + std 单向 adapter，uds/shm/uart 线上字节零改动，
  transport 层门解除、xtensa 门真绿（C6）；G3+C17 slots.toml schema v2 增 status/tier_scope 两字段
  （C17 裁定 (a)）+ profiles 扩 [platform]/[supplies]/[slots].present 三段（C15 看门狗位）+ G6 档位
  ↔watchdog 一致性拒 boot；G4 装配校验真值单点（rr-registry assemble.rs，七 trait × 九键在册表副本 +
  契约测试钉死）+ rrd 缺项逐条点名到件、模块在册即 `Degraded` 不被解除传播捞回 + 17 份 assemble.toml
  升 v2（battery 真值样板 / bt 兑现迁移承诺 / 其余如实空）+ rr3-pack staging 扩整树（C1）+ mkmod 模板
  升 v2 回灌校验（C3）；G5 六选择函数 `.ok()` 吞错改 eprintln 点名（R3-D3）；C14 首批 retired 走通
  （asr/text / dialog/turn 置位 retired + 两订阅撤申报 + brain 只增补 pcm/vad_start + sim 演练换主题；
  主题计数 28 与 128B 线上字节零改动）；C4 不扩键裁定落 descriptor.rs 头注释。九步门禁直跑
  GATE_EXIT=0（30 PASS）；rustfmt 全仓 + clippy 全仓清零复验。方案 §17 勾点（R3-D2/D3、C14、前置
  清单批J 五条 + 三条注记）与 conventions §2 批J 条目同批落库；**第 III 轮余批K**（H1–H6：对拍用例、
  三门接 check.sh、点验矩阵、第二形态、文档收口、守护输出）
- **批I/批J 深挖收口已落库**（2026-10-10，提交 `f397329`，双远端三端核对一致）：独立代理对拍两批差异
  + 人工复核，两处 P1 结清——①`rr3_slot_table_check` 原无生产消费点（运行期只查空表，形状门只在 CI），
  照 topics 装载同构模式接线进 `rr3_profile_boot_inputs`（越界契约 boot 即拒逐条点名 + 反向断言测试）；
  ②capture/mech.rs 非 test 面引 `Rr3PcmStream`（承载层数据类型，C7 白名单外），stream 级下混上移
  composite（逐字等价），件面回归零 rr_hal 引用。P2 登记敞口四条：注入缺值类型裁决归真注入器批、
  led tick 重锚停滞期差异（妆面件）、SSID 32-octet 截断对超长注入串的字节差异（802.11 语义正确）、
  read() 契约注释 0 语义两侧未兑现（归 USB 麦真件换装批）。门禁 GATE_EXIT=0（30 PASS）+ clippy 清零。
  批K H2 件级依赖闭包 grep 门落地后 C7 约束转由门执法
- **批K 判据上门 + 点验矩阵 + 第二形态已落库**（2026-10-10，方案 §14 批K 行 + C2/C10/C11/C12 结清，
  ADR-0018 / ADR-0019）：**H1** 11 件两腿对拍补齐（led/relay/servo/bt/wifi/face/player/battery/charger/
  capture/vad——腿一确定性 + 腿二裁定语义期望；env/imu 两占位格 + cognition 四件 Python 两件式记
  不适用，不冒领 PASS）；**H2** 三门接 check.sh ⑧（⑧-a 对拍计数门含 C10 stub 反例自证、⑧-b 依赖闭包
  grep 门 L0/L1——39 份 atom 源非 test 面零命中、⑧-c 注入面校验门显式过滤器；⑦-a 扫面扩
  modules/*/atoms/*/）；**H3** `tools/ci/matrix.sh` 七列出数入口（FAIL=1/--strict 未跑列=2/默认如实=0；
  `--manual --evidence` 回填契约：证据三样齐才收、不覆盖自动 FAIL 格）+ 供给自证表模板落
  [docs/reference/供给自证表模板.md](reference/供给自证表模板.md)；**H4+C2** `rr3-pack --profile` 参数化
  + 机型集 pin 同批 + `profiles/esp32-mini.toml` 第二形态（F-B 三格微型：capture/vad/servo，件码与
  slots.toml 零改动）+ rrd 装载正例与 contract 形状 pin 装配演练；**H6** rr-core::runtime 本地守护
  "起动态+喂拍+安全核触发/拒动原因"行格式定形 + 对拍用例（architecture §12.4）；**H5** 文档口径收口
  （architecture §3 树补 rr-hal-api + 落点表四行翻已落盘、ci.yml job 步骤名改十步、tools/ci/README ⑤
  行撤除 legacy/ 语 + ⑧ 三门补录、六份模块 README 目录树对齐、全仓复扫）。门禁直跑 GATE_EXIT=0
  （**50 PASS**——⑧ 三门上线即全绿，十步口径）+ rustfmt/clippy 全仓清零复验。**点验矩阵首跑回执**
  （E1 形态行；行指纹：本机直跑无容器镜像 ｜ Darwin 21.6.0 x86_64 ｜ rustc 1.99.0 ｜
  x86_64-apple-darwin）：② xtensa 三查、③ rrd profile 24、④ rr-registry contract 38、⑤ 五 crate 两腿
  对拍 22 条（act 6/connect 4/interact 4/power 4/sense 4）四列 PASS；① 3/4 路（本机无 Docker，归上表
  第 4 条）、⑥ python3 3.9.6<3.11（归上表第 9 条）、⑦ E4 真板专属（归上表第 7/11 条真件敞口）三列
  如实记未跑，FAIL=0。方案 §17 勾点（前置清单七条硬开工门、C10/C12/L-10、对拍与三门、矩阵、第二
  形态、指纹、文档收口）与 conventions §2 批K 条目同批落库。**第 III 轮（批I→批J→批K）收官，停等**
  ——批K 新增敞口仅两笔：C12 首条手测行待 E4 真板（⑦ 手测回填契约定形、证据模板在 docs/reference/）；
  CP2102 进 0.0.6 §5.4 BOM 第 9 行（C11），到货前置 0.0.7 批C④⑤ 板级 round-trip
- **两板收口批——批C④⑤ 板级收官落档**（2026-10-10，提交 `6d3a1ee`，0.0.7 方案状态行 +
  conventions §2 条目随批）：CP2102 到位，ESP32 devkit 按 0.0.7 §9.2 runbook 板级实测——④ 判据过
  （128B 头 + payload 完整收发，cmp 逐字节一致）、⑤ 判据过（半帧截断与坏 magic 两类注入均拒收、
  注入后不进死锁、正常帧收发不受影响；现场实测，用户口述确认，串口日志摘要未留档，boards 回填时
  补——R2 诚实标注）。随批现场补丁 `6d3a1ee`：espflash 4.x 构镜像硬校验 ELF 内 ESP-IDF app
  descriptor，esp-bootloader-esp-idf 0.1 宏段名 `.rodata_desc.appdesc` 与 esp-hal 1.1.2 KEEP 段名
  `.flash.appdesc` 错代、宏产物被 gc-sections 丢弃——按对表段名手挂 `#[used]` 静态量修复
  （xtensa 构建绿 + ELF 节/magic/project_name 三要素物证核验齐）。**层② 必达全绿，批C 整批收官
  ——两板载体验证态成立**（ESP32：层② UART round-trip 板级跑通；树莓派：部署链此前已实证——
  批D 真装 `efd224f` / 批F4 `--chain` 板上全链 / 批F5b 运维链）。0.0.6 §5.4 BOM 第 9 行 CP2102
  翻"已到位"；C12 首条手测行不冒领——本批板级实测无 ⑦ 列证据三样（指纹+采样窗口+原始数据）
  留档，敞口维持待按 [供给自证表模板](reference/供给自证表模板.md) 补采
- **批K/两板收口批深挖收口**（2026-10-10，两独立代理对拍 + 人工复核）：P1 一处同批结清——
  rrd.service ExecStart 硬编码 `distro.toml` 装机位与批K H4 `--profile` 参数化矛盾（非 distro
  机型出包 deb 内无 distro.toml、systemd 拉起即败），rr3-pack 补第二处内容改写：ExecStart
  装机位随 `--profile` 同步改写（缺省 distro 恒等、模板锚点缺席 fail-closed、反例断言钉死）；
  ⑧-b L0 门拓全限定内联与 L1 对称（活体三份 atom mech 五处 `std::` → `core::` 清退）；活文档
  口径同步（conventions §2 ⑧ / architecture §13 / tools/ci/README）+ `.github/README.md` 订正
  （指路存根、八 job、十步）+ 五处落档日期 2026-10-11 订正为提交时间戳在案的 2026-10-10。
  P2 敞口四笔：⑧-c 注入面过滤器按用例名子串绑定（空壳可顶替保绿，改名会红、增假不防）；
  matrix.sh 三小项（①列 "4/4" 硬编码 / --manual 证据仅 grep 标记词 / ⑧-b test 块剥离 `{{`
  转义可失衡，当前文件平衡）；esp32-uart-echo 的 esp-bootloader-esp-idf 未关默认特性
  （validation→md-5 拉入，类型面用不到——下次触碰该工程同批处理）；ELF appdesc 三要素核验
  未自动化进 esp-uart-echo-check.sh + 上游 0.1/0.2 idf_ver 误读 MMU 页配置键（装饰性，
  无参宏同源字段一致）
- **批D1 层③ 板上实测收官**（2026-10-10，提交 `f72c0d4`，双远端三端核对一致；
  十步门禁直跑 GATE_EXIT=0（52 PASS）+ 专用门绿）：哨兵定稿两修正——reader/心跳
  线程栈 8KB→16KB 根因实锤（serde_json + json! + FrameBuf 峰值溢出踩堆 → std Mutex
  惰性 pthread 初始化崩，EXCVADDR=0xb；16KB 钉为运行下限）+ harness parse_upstream
  method 门改 Option<String>（Response 帧合法无 method，原强制门误杀 round-trip
  应答）+ 4 行诊断插桩清理。板上对跑 HARNESS_EXIT=0（14 PASS / 0 FAIL：①注册
  ②8 拍心跳 195–204ms 无停摆 ③round-trip id=9001 回显 echo 逐字对拍）；三点 RAM：
  boot free=295672B / registered min_free=269476B / roundtrip min_free=250620B ≈
  启动堆 85% >> 20% 阈值——**D1→D2 不触发，层③ 必达全绿**（实测报告回填
  [tools/espidf-sentinel/README.md](../tools/espidf-sentinel/README.md)）；附实证
  harness 退场后心跳 5s 超时 → mark_down → esp_restart → 重注册，Down 转移路径
  真板跑通。敞口一笔：UART 持续流开口帧同步（CP2102 驱动输入缓冲残留上轮帧字节 +
  `Rr3UartConn::open` 不 tcflush → harness 开口 BadMagic，复跑需复位+清缓冲组合
  时序）——**批E rr3-bridge 同题**，桥件开口路径按此处置
- **批E 层④ 收官——0.0.7 四层梯子全收官**（2026-10-11，哨兵 dst 修复件 `870cfe6` + 本落档提交，
  双远端三端核对一致）：E6 本机彩排（假哨兵对拍 PASS=18 FAIL=0）揪出 harness 盲区——假哨兵应答
  dst 取 src_task 字面回抄、彩排环境 src 恒 1 恰蒙混 rrd 上行 dst 门，真 rrd 在链后才暴露；哨兵
  应答 dst 改钉 `RR3_TASK_RRD` + harness bus_frame 补 src_task 入参变回归网（`870cfe6`，xtensa
  专用门绿 + 十步门禁 GATE_EXIT=0（54 PASS））。E7 真板 e2e（§9.4 ⑤ macOS 档）PASS=16 FAIL=0
  全绿：顺序 A/B 双序列注册在册、心跳 hb=200 三态正常、正路径点名 `rr3-sentinel/diag/probe` 回带
  物证（板侧 `round-trip 完成（id=2）`）、负路径点名被拒不记账、`denied_publish = 0` 全程保持、
  断链自愈真拔 USB 双半段实锤（拔线：桥存活+上行 EOF+关 UDS+ENOENT 指数退避+哨兵摘除 failed；
  插回：`uart link up` 重开+哨兵重注册 task=3 running+心跳恢复）。E7 排障四实锤落方案 §9.4 ⑥
  （修订版 12，判据不动）：macOS 串口须用 `cu.*` callout 节点（`tty.*` callin open EBUSY 与占用
  无关）；stty 配置 close 即丢、串口参数须与读写同 fd 生效（判别法：cat 直听见 magic
  `31 45 52 41` 开头即物理层通）；macOS 允许重复 open 同一节点、残留旧桥静默截走数据（清场须
  `ps` 核 0 残留，"桥进程存活"须与 `uart link up` 日志同验）；桥 open CP2102 的 DTR/RTS 翻转
  复位 ESP32、哨兵 10s 注册窗口要求 rrd socket 先于任何复位点就绪。批D1 开口帧同步敞口随批E
  实证关闭（E7 全程无 BadMagic 复发——`Rr3UartConn::open` configure 末尾 tcflush(TCIFLUSH) 在案）。
  **层④ 必达全绿（层①②③④ 全收官）；树莓派档 e2e（§9.4 ⑤ 右列）板上线即补，不冒领**
- **批G 点验矩阵轻承载档行收官——esp32-mini 七格收口**（2026-10-11，代码面 G0 `707611b` /
  G1 `dfb55ff` / G2 `bf20d15`+勘误 `5c16b15` / G3 `7122cee` 四独立提交 + 本落档提交，双远端
  三端核对一致；方案修订版 13）：⑤ 列板上载体经裁定扩 espidf-sentinel（否决独立固件 crate）。
  G1 机制核 `rr-core::control::parity`（真实原语 runner + 契约期望向量 + 守门测试，宿主/xtensa
  同源）；G2 哨兵扩 `diag/parity` 点名——**勘误实锤：rrd 转发不改写 method、点名主题在
  params.topic**（conn.rs `rr3_route_call`），初版分发形状必漏接已修；G3 harness ④ 两腿对拍 +
  ⑦ 直方图对表 + C12 证据写出；G0 UART 开口 flock 排他账本（E7"重复 open 静默截流"实锤变机制
  门）。**G4 板上实测全绿**：④ 板回 parity 向量与宿主 runner bit-for-bit（LX6，八组原语，契约/
  确定性双腿过）；⑦ 200 拍 54s 窗口 min=198/avg=200/max=299ms 全落 [150,300] 判据窗（首拍
  299ms 注册对齐点）；C12 证据四样落仓
  [G4时序证据](reference/G4时序证据-esp32-mini-20261011.md)（rustc -Vv 操作面追记）；
  matrix.sh ⑦ PASS[手测] + ⑤ 注记核验过。矩阵七格终态：①skip(实跑3/4) ②③④⑤⑥⑦ pass。
  排障三实锤落修订版 13：ESP-IDF Compile time 系 C 组件宏缓存 Rust-only 重编不刷新（判别固件
  用 ELF SHA256）；cargo "Finished"≠重编（勘误后 touch 强制重编核 mtime）；板↔宿双向全断
  头号嫌疑 GND 未共地。供给自证表 tick_period_ms/tick_jitter_ms 升级真板实测（≤6ms 抖动）。
- **0.0.7 收官（批F 版本收口，2026-10-11，本落档）**：四层梯子 + 点验矩阵 esp32-mini 行全收官——
  批A' 工具链绿（2026-10-07）→ 批B 层① 绿（`a860405`）→ 批C 宿主侧（`46edb3c`）→ 批C④⑤ 板级收官（`6d3a1ee`）→
  批D1 层③ 板上实测收官（`f72c0d4`）→ 批E 层④ 收官（`870cfe6`）→ 批G 矩阵行七格收口（`dc3a1d6`）→
  批F 收口重锚（本落档）。**0.0.7 收官停等（R19 达成：四层全真板验收无受阻态）**
  （**2026-10-12 版本边界改判**：此处"收官"的范围 = 跨 OS 通路四层梯子 + 点验矩阵 esp32-mini 行，
  且该行的**证据对象是诊断哨兵**，不是"微型形态的功能在跑"——`profiles/esp32-mini.toml` 声明的三格
  今天无任何件承载。功能验收半边（F-8 与 F-1 板侧）**新开批H**，本版状态由"收官停等"改回
  **执行中**；已收官的四层证据与 commit 事实一件不动。见
  [0.0.7 修订版 15](process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md) 与
  [ADR-0020](process/adr/ADR-0020-版本边界-开发收口与实机验证与验证后整理.md)）。余敞口移交：
  树莓派档 e2e 板上线即补（档位注记维持）、①列容器路真跑（无 Docker）、
  `watchdog_timeout_ms`/`safety_core_interval_ms` 板上标定（触发条件在册）、
  CI 八 job 首跑回执（R30 迁 GitHub Actions——真源已回归 `.github/workflows/ci.yml`，
  GitHub 仓库待建+SSH key 待配，敞口表第 1 条统收）、ESP32 板卡独立笔记未建
  （Q5 版本组合已落 [0.0.7 方案](process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md) 批W1 登记——
  esp-idf-sys 0.35.0 ↔ ESP-IDF v5.2.2 ↔ esp-clang 19.1.2 ↔ ldproxy 0.3.5 + rsproxy 获取方式，
  批A"随批D1 新建 ESP32 笔记"的载体演变为此登记，欠账如实注记不阻塞收口）；
- **CI 真源迁移小批（R30，2026-10-11，0.0.7 收官后工程基础面）**：用户裁定 atomgit
  无流水线功能、CI 改走 GitHub Actions——R27 的 `.gitcode/workflows/` 单真源路径作废、
  `.gitcode/` 整目录删除，真源 `git mv` 回 `.github/workflows/ci.yml`（八 job 定义零改动，
  头注释 R27→R30 订正）；`.github/README.md` 改写单真源文档（三远端口径）+ 活引用全仓
  同步（tools/ci/README、check.sh ⑥ 注释、conventions §2 CI 侧步骤、brain 模块设计门禁
  口径、本档敞口表第 1 条与 IPC-T-003 行、.gitignore）+ R30 登记 0.0.5 方案 §2.4；
  八 job 首跑回执已到手并结项（CI 首跑修复批，见下条）；
- **CI 八 job 首跑修复批（H1–H4，2026-10-11，run#6 38146782224 = `1c3cf41` 全绿收官）**：
  GitHub 仓 rockrobot3/rockrobot3 建成转公开 + SSH key 配好后 e0a8bad 触发首跑 5 绿 4 红，
  五轮迭代修复至八 job 9/9 全绿：run#2 `484a13c`（批H1 espidf-sentinel fmt 归一 +
  rust-cache 工作区 pin）→ run#3 `f640234`（批H2 rr-hal linux 单测七败真修复 +
  result_large_err 契约级豁免——conventions/lint 双转绿）→ run#4 `adf3d16`（批H3
  ci.yml 插 espup 装机步 + ipct003 第④路默认在线直连、本机网络 workaround 收进
  IPCT003_CARGO_OFFLINE/MIRROR 环境开关——四路真对拍绿）→ run#5 `8f33071`（批H4
  distro-smoke ⑤ 点名断言——回执实锤根因：脚本注释 ASCII 撇号截断 docker bash -c
  单引号串、余行溢出宿主执行）→ run#6 `1c3cf41`（撇号清零 + 警示注释，全绿）。
  敞口表第 1/4/8/9 条同批结项（第 4 条含 IPC-T-003 移入 envelope.schema.json
  coverage，活引用三处同步：contracts/README、golden/README、rr-ipc header.rs
  测试注释）；
- **批H5 审查修复批（2026-10-11，`b2c28da` + `749f7ba`）**：对六提交
  （e0a8bad..e0cb31a）执行 TRAE-code-review 审查出 4 问题（1 major + 3 minor），
  用户裁定全修——rr-hal linux_link 三测试 ETXTBSY 加固（temp 目录 pid+纳秒
  唯一后缀 + 瞬态重试，run#7 attempt1 flake 的裁定落地）+ nm() 读管道 deadline
  守护（O_NONBLOCK 轮询，堵"幸存孙进程持写端拖垮超时兜底"旁路）；ci.yml espup
  钉 v0.17.1（latest 漂移破可重现性）+ distro-smoke ⑧ grep 恢复 JSON 键精确
  匹配；登记随 docs 落档提交；
- **0.0.5 增补轮五立项 + 批A 全案落档（R31，2026-10-11，本落档提交）**：用户裁定
  版本制——"所有的功能完成都放在 0.0.5，0.0.5 如果没有提及的话就增加新一轮次的
  大改，0.0.6 是树莓派跟 0.0.7 是 ESP32 是验证功能的可成功运行"（功能归 0.0.5
  轮次制，0.0.8 不设功能版、brain 迁移既有排期不动）；同批裁定 Q1=(a) UART 波特率
  参数化 921600 + INMP441 双通道原始音频过桥（宿主 DoA/ASR 自算，超判据切板上
  DoA 预案）/ Q2=四件全挂 ESP32 协处理器 / Q3=本地 vosk+Piper（brain adapter 缝）/
  Q4=USB 声卡+喇叭立采。全案（批A–批F）落
  [0.0.5增补轮五——交互功能落地全案](process/dev/0.0.5增补轮五——交互功能落地全案.md)，
  裁定记录落 [0.0.5 方案 §2.6](process/dev/0.0.5版本洛克三号对齐与裁定方案.md)；
  0.0.7 移交敞口两笔同批结项：R2 批C④⑤ 板级证据归档
  [esp32板卡笔记](reference/esp32板卡笔记.md)（console.log 拒收旁证 + 160B 物证 +
  当日 cmp 逐字节复验一致）+ ESP32 板卡独立笔记新建（同一载体）；0.0.6 §5.4 BOM
  增第 10 行（USB 声卡+喇叭，九行→十行口径三处同步）；
- **版本边界重划轮（0.0.5 第 IV 轮 R32–R37，2026-10-12，纯 docs、零 `.rs`）**：与上一条**同一句用户
  口径的两个面**——R31 管"哪些功能、挂哪块板、用什么引擎"（功能立项面），本条管"一个版本只产哪一类
  交付物"（纪律面）。按四条代码事实核对后落
  [ADR-0020](process/adr/ADR-0020-版本边界-开发收口与实机验证与验证后整理.md)：**版本按交付物类型分窗**
  （0.0.5 产码 + 功能级板上冒烟 / 0.0.6 与 0.0.7 只产**平台级点验**、**本版不新增功能码** / 0.0.8 产结构）。
  五件连带：① 0.0.5 新增 **§2.7 裁定记录（R32–R37）** 与 **第 IV 轮 §18–§22（批M，MC-0…MC-11）**——
  收回挂在验证版名下的开发欠账（真人声 PCM 通路、探测真值、低电禁动、凭据持久化、回显型参数、
  `net.bt` 三态、门与证据链接调用方），**与增补轮五重叠的条目按 §18.4 对表让位**（显示与外设施工归
  批B/批C，本轮只持判据与清零口径；MCP-2 SPI 扩键评审随 Q2 作废登记；契约 append 合并为一次）；
  ② 0.0.6 **修订版 13**（F-1…F-7 进 DoD 第 11 条 + "缺码 ≠ 无载体"两种败法分开登记）；
  ③ 0.0.7 **修订版 15**（收口范围订正 + 新开批H，见上一条）；④ 0.0.8 **恢复独立成批**（"并入 0.0.5 第 II 轮"
  作废；物证 = `base/runtime/` 下至今无 `rr3mod` crate），[ADR-0017](process/adr/ADR-0017-模块内部分层-机制与策略分离.md)
  状态行作第二次变更登记；⑤ [模块设计索引 §14](design/模块设计索引.md) 余数归属总表（一个余数一个落点）
  与索引 R-14/R-15/R-16 三条自决——**R-15 把 A-5"低电禁动"的"待裁"就地结案**（四条语义可从既有原则推出，
  不该占用裁定预算）。本轮同批订正三条过期宣称：孤儿参数余数从"七项"收窄为
  "`cutoff_voltage_mv` 只回显不动作 + 三处自述"（interact 六键 / brain `max_turn_chars` 已被批I 删除，
  见 0.0.5 §17 订正注记）、主题计数 28 → **29 → 30 条**（0.0.7 批E append 哨兵探针主题、第 IV 轮 MCP-0 append `rr3-brain/audio/tts_pcm`）、
  以及 **R31 那句"0.0.6 已收官"不成立**（0.0.6 §6 DoD 第 6–10 条未做，前置是采购与板上线——本档
  能力表与敞口表第 7 条按"阶段一收官、阶段二待载体"读）；
- **工单落点复验轮（2026-10-12，纯 docs，基线 `78625ae`）**：六册 §10 工单表里有 **40 行的"归批"列指向
  已收口的批I/批J/批K**——收口即定格、旧批不重开，所以未做的余数必须换主人。逐条对代码复验后落
  [模块设计索引 §13.4](design/模块设计索引.md)（全仓总表）+ 六册各一节 §14.1（本册增量）：
  **判结 11 条**（act A-2/A-3/A-10、interact I-3、sense S-6、connect N-8、brain B-4/B-5、power P-2、
  批L L-4 清单一致半边、批L L-7）；**重挂活落点 11 条**（I-4→MC-11、I-9/I-12→MC-8、A-9/I-11/S-5→MC-10
  策略出件三连、S-3/A-6→MC-11 可观测面、N-9 与 L-3 与 L-10→0.0.8 W-B、L-1/L-2/L-5/L-6→MC-10/MC-11）；
  A-5/P-8/L-8 由索引 R-15 结案后统一落 **MC-5**。
  **本轮同时收回我自己写错的三条**（点名，不静改）：① L-5 份数被我"订正"成 16/17——实况 14/17 空、
  battery/charger/bt 三份已填，错因是用单行 grep 读多行 TOML 数组；② power P-2 被我记成"后半未做"——
  实况 `composite.rs` 已有 `rr3_counter_bump("battery_read_fail", 1)`，错因是 grep 漏了下划线键名；
  ③ connect N-8 上一版记"栈名仍打 SURFACE"——实况已改为"后端=动态，由 HAL 选择面交名"，
  连带批L L-10 降为可选清理。三条同族：**工具给的计数不等于实况**，跨行结构必须按结构解析。
- **下一在制品（2026-10-12 合并两条排期）**：**0.0.5 增补轮五 批B**（ESP32 外设底座档——OLED SSD1306
  I2C 出字 + 舵机 LEDC 逐度转向独立固件，不依赖桥）为**下一条施工批**；**第 IV 轮批M**（MC-0…MC-11，
  立项已落档 [0.0.5 方案 §18–§22](process/dev/0.0.5版本洛克三号对齐与裁定方案.md)）与之并行可推，
  开工门 = MCP-1（与批C/批D 合并的那次契约评审）+ MCP-3；其后 **0.0.7 批H** 与 **0.0.6 F 组真件档**
  取平台级点验（前置 = 器件到位 + 板上线 + 增补轮五批C/批D/批E 交付）；**0.0.8** 维持挂账待排期
  （内功批不与新功能轮混排，R31 与 ADR-0020 裁定 2 在此同向）；
- **0.0.5 增补轮五 批B 落库 + 真板判据①②过（2026-10-12，提交 `5280fe0` + 修复批 `d98ff5e`）**：
  esp32-periph-base 独立固件（SSD1306 I2C0@21/22 出字 + LEDC 双轴舵机 GPIO32/33 + UART2 八
  命令指令面，专用门 + 十步门禁全绿）落库推三远端四端核对一致；真板验收**判据① I2C 扫描
  [3C] ✓、判据② OLED boot 画面两行目视与串口回读一致 ✓**——中途排障实锤 `oled_cmd` 8B 缓冲
  护栏误拒 init 25B 流式命令序列（SSD1306 Co=0 流式命令面单笔写合法，数据手册 §8.1.5.1），
  修复批扩 32B + init 诊断面非 panic 化（Err 明细 + 100k 档 5×2s 重试点名 + 终败继续跑指令面）；
  LEDC 中位 90° duty 1229/16384、RAM 三点 free=294256/290996B 无异常。判据③④（舵机五点
  回读/堵转 UBEC≥4.7V）待舵机接线；教训登记见 [conventions §2](process/conventions.md)；
- **0.0.5 增补轮五 批B 增量批——OLED 4 行扩行 + Mac 实时数据演示（2026-10-12，提交 `81b30f1`）**：
  用户裁定"固件扩 4 行"路 B：`OLED_ROWS` 2→4（文本 3 行 ≤63 字符 21 列硬切 + 角度行移
  row3，`OLED <text>` 命令形状不变向后兼容，可见 ASCII 过滤前置防 char boundary 劈裂）；
  boot 画面 4 行全出真板目视确认；配套演示件 [scripts/mac_monitor.py](../tools/esp32-periph-base/scripts/mac_monitor.py)
  （Mac CPU 温度/内存/IP/时间 1Hz 上屏，真板连发全 OK 回执 + 目视确认实时跳动）；
  开口帧同步教训登记 [conventions §2](process/conventions.md)（UART2 open 后首行指令被
  boot RX 毛刺残行污染，宿主脚本空行冲行规避）；
- **0.0.5 第 IV 轮 MCP-0 契约级评审批（2026-10-12）**：P1 评审批开工门兑现——MCP-1
  `topics.toml` 29→30（append `rr3-brain/audio/tts_pcm` stream L2，载荷体逐字段复用
  `rr3-sense/audio/pcm` 已定形状，F-4 下行通路契约半边；与批C/批D 合成一次改表，批C/批D
  复用在册主题零新增登记）；MCP-2 SPI 扩键评审随 R31 Q2 作废留痕；MCP-3 `rr3-act` 申报面
  增订阅 `rr3-runtime/system/power`（MC-5 消费点，C14/R-3 同程序）。同批同步 rr-registry
  形状 pin（30 条三元组 / kind 12-10-4-4）、rrd topics_load 计数（33=30+3）、
  [contracts/README](../contracts/README.md) 演进段 / [conventions §2](process/conventions.md) /
  [0.0.5 方案 §2.7](process/dev/0.0.5版本洛克三号对齐与裁定方案.md) 裁定登记；
  MC-1 / MC-5 解锁施工；评审记录唯一落点 contracts/README 演进段；
- **0.0.7 方案 Q1–Q6 已裁（0.0.5 §2.4 R20，2026-10-06）**：Q1=UART / Q2=esp-rs std / Q3=按 R19 / Q4=transport 级连接多路器（R15 订正口径成立）/ Q5=ESP-IDF 外置 + 密钥门注销 / Q6=macOS+树莓派双档；**层③④升必达（R19）**——经典 ESP32 实测不可达即批D2 采购 ESP32-S3 移植续做，四层全真板验收无受阻态；修订版 4 已落 [§9 四层真板执行手册](process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md)；
- **采购 BOM 已定案（0.0.5 §2.4 R22，2026-10-06）**：USB 麦克风阵列（≥2 麦 UAC——I2S HAT 存档备选，R8/R14 互斥消失）+ SG90×2（硬件 PWM GPIO12/13）+ 独立 5V UBEC + 显示件（MAX7219 SPI / SSD1306 I2C 定一）+ 锂电池组 + ADC 分压板 + PCA9685（备选）+ ESP32-S3 devkit（R19/批D2 备料）+ CP2102 USB-UART 适配器（0.0.5 批K C11 补进，0.0.7 批C④⑤ 前置）；十行明细唯一落点 [0.0.6 方案 §5.4](process/dev/0.0.6版本洛克三号树莓派验证方案.md)（第 10 行 USB 声卡+喇叭为 R31 增补——增补轮五 批E 前置），到位前不 claim 真硬件证据。采购实况（2026-10-07；2026-10-10 增补）：ESP32 硬件已齐——CP2102 已到位并已用于批C④⑤ 板级
实测（见上两板收口批 bullet）；树莓派外设采购中。
- **0.0.8 模块内部分层立项已落档（0.0.5 §2.4 R17，2026-10-06）**：立项件
  [0.0.8 方案](process/dev/0.0.8版本洛克三号模块内部分层实施方案.md)——rr3mod
  公共骨架签名级规格（§2）+ 五模块目标目录树与迁移映射（§4）+ 批0–批6 实施序列
  （§5）与每批验收四件套（§6），零 `.rs` 改动随立项落档；实施批待用户排期，
  与 brain 迁移（[ADR-0016](process/adr/ADR-0016-brain迁移路线-python转Rust渐进首裁定.md)）
  同窗、与 0.0.6 / 0.0.7 真件批不混排（R17 原义）。

**已知偏差（有意保留，不是遗漏）：**

- 代码注释里的 `Task N`（如 `daemon.rs` 的"Task 11b 加了唯一的 supervisor 线程"、`registry.rs` 的
  "Task 7 已落地"）指的是 **rrd 实现计划**的任务号，与本轮文档重构计划无关。两者同号不同事，
  读文档时别把它们对齐；这些注释不属于本任务订正范围。
