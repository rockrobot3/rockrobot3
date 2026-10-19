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
| 非 Linux 主控契约 parity（ESP32/xtensa，0.0.7 四层梯子） | 已实装（ESP32 档） | 0.0.7 四层全真板收官（R19，2026-10-11）——**层① 字节契约**：rr-ipc no_std 化（`#![no_std]` + 条件 `extern crate std`，协议层纯 core+alloc，批B `a860405`），`ipct003.sh` 四路化、xtensa 编译期 golden 对拍 PASS（实跑 3/4：宿主 PASS / aarch64 PASS / xtensa PASS / arm64 容器 SKIP 本机无 Docker）；机制核 rr-core（常量/寻址/帧组装 0.0.5 批D①、runtime 执行环 批D②、控制原语+安全核 批D③）xtensa 双线编译绿（none-elf 裸金属 + espidf std）。**层② UART 载体**：rr-ipc `transport/uart.rs`（宿主侧 raw termios）+ esp32-uart-echo 裸金属 echo 夹具（批C `46edb3c`），板级实测 128B 头 + payload 完整收发、半帧/坏 magic 双拒不进死锁（批C④⑤ `6d3a1ee`，espflash 4.x app descriptor 现场补）。**层③ 机制核真板消费者**：espidf-sentinel 内跑 `rr-core::runtime`（批D1 `22783ea`/`f72c0d4`）——harness 14 PASS（注册 / 8 拍心跳 195–204ms / round-trip 对拍），三点堆采样 roundtrip min_free=250620B ≈ 启动堆 85%（D1→D2 不触发），线程栈 16KB 钉运行下限。**层④ 桥接 e2e**：rr3-bridge UART↔UDS 中继（`955ef14`）+ 真板 e2e PASS=16 FAIL=0（批E `870cfe6`）——顺序 A/B 双序列、心跳三态、正/负路径点名、断链自愈真拔 USB 双半段实锤。**点验矩阵 esp32-mini 行七格收口（批G `dc3a1d6`）**：⑤ 安全核逐拍板上腿 bit-for-bit（[`rr-core::control::parity`](../base/runtime/rr-mech/src/control.rs)（rr-mech 下沉，批M；rr-core re-export 保路径） 八组原语，LX6 双腿汇合宿主 harness）+ ⑦ 200 拍 min=198/avg=200/max=299ms 全落判据窗——[供给自证表](reference/供给自证表-esp32-mini.md) + [G4 时序证据](reference/G4时序证据-esp32-mini-20261011.md)（C12 证据四样）。平台形状实锤：Xtensa LX6 无 64 位原子（计数/发号 AtomicU32 域）、esp-idf-hal 半体驱动 `unsafe Send` 补齐、UART 开口 flock 排他账本（`707611b`）。余敞口：树莓派档 e2e 板上线即补、watchdog/安全核周期两键板上标定；容器路真跑已补——2026-10-11 CI ipct003 job --strict 四路全绿（run#6 38146782224 = 1c3cf41），IPC-T-003 移入 envelope.schema.json coverage（敞口表第 4 条结项） |
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
| `rr3-sense` / `rr3-interact` | 已实装（sim 侧） | [`modules/rr3-sense/src/main.rs`](../modules/rr3-sense/src/main.rs)：注册/心跳 → 按后端开拾音面（批F2 `e4ccbaf` 增 `audio/doa.rs`：**下混前**双麦 ILD 反演 ±90° 分三档（|deg|≤30=center），vad_start onset 负载可选携 `doa{deg,bin}`——schema append 零新契约，R14 双麦下混 mono 前判向）→ 自 pacing 采集循环（sim read 即时返回、ALSA read 自耗一拍，两种后端同一形状）→ pcm STREAM + VAD 边沿上行 → `capture/dump` 点名应答（模块用例条数登记 conventions §2 + golden PCM 向量）；[`modules/rr3-interact/src/main.rs`](../modules/rr3-interact/src/main.rs)：注册/心跳 → 占位 TTS（440Hz burst，60ms/字符封顶 10s）+ 七态表情机（批F3 `444c764`：五态增 listen/turn 可点名，talk 仍 TTS 自动驱动不可点名；每态 8x8 帧序列，主循环 ~250ms 墙钟节拍推帧不随总线流量漂移）+ **显示出帧接线（批F5a③ `bcc4de2`）**：开机 HAL 显示面探活 + 主循环经 `Rr3Display` 出帧（sim 内存位图）应答两个 rpc 点名 + **Web 操作面板（0.0.4 批C+批D，`7aa7fc8`/`3682bfe`）**：[`panel/server.rs`](../modules/rr3-interact/src/panel/server.rs) std 手写 HTTP（端点全集唯一真值 = 同文件 `RR3_PANEL_ROUTES` 常量表——MC-8③/I-9 起端点数由表长推导、文档不手写计数；状态/硬件/系统三页 + WiFi 配网页——scan GET / connect POST / forget POST 全部 BusCall 透传，参数裁决在 owner 模块）、单文件静态页 `include_str!` 内嵌四页签（[`panel/www/index.html`](../modules/rr3-interact/src/panel/www/index.html)）、`rr3.ctl.reboot` 面板触发双重门、监听面 `RR3_PANEL_ADDR` 优先缺省回环（局域网可信面声明在 README）、beep/face 经 mpsc 动作通道主循环串行代执行（sink/face 所有权不出主循环）、net/link 事件缓存进状态页。两者失败语义同一：**业务性失败折 error 应答回话，传输性失败 `exit(1)` 交 supervisor**（不装"活着"）。**板上 ALSA 采集/回放未发生（硬件未采购）**；三真进程整链证据在 [`e2e_audio_chain.rs`](../base/runtime/rrd/tests/e2e_audio_chain.rs) |
| 机器人功能可达性（F-1…F-8 清单口径，2026-10-12 新增本行） | **六条可达 / 一条不可达**（骨架） | 清单与逐条判据的唯一真值 = [0.0.5 方案 §18.1](process/dev/0.0.5版本洛克三号对齐与裁定方案.md)（版本边界三窗分离后新增的验收面，见 [ADR-0020](process/adr/ADR-0020-版本边界-开发收口与实机验证与验证后整理.md)）。**可达（欠板上证据）**：F-1 舵机转向（`linux_pwm.rs` sysfs 后端 + `rr3_travel_clamp` 判定 + `servo/state` 限速流）、F-3 语音输入（ALSA 采集 + VAD 边沿上总线，欠真麦与真 key）、F-7 生命周期（supervisor `restart_policy` + 断电 boots 累积，0.0.6 批D/E'/F5b 实装）、运维面 F-7 附带的 `rr3-ctl` 六子命令、**F-4 真人声播报 = 已结（2026-10-13，MC-1 四提交 `eefc0de`/`ed8fb2f`/`f959e3a`/`70e8fb6` + MC-2 `1930956`）**——brain `rr3_finish_turn` 已按 20ms/640B 分帧发布 `rr3-brain/audio/tts_pcm`（seq 模块级单调、`ms` 由样点数导出）、`tts/text` 点名 args 增 `pcm_bytes` 作收流完整性唯一判据；interact 收流解码入缓冲、点名对账排空（半流/残留皆拒因 + 清缓冲 + `tts_play_reject` 计数）后写 `Rr3AudioSink`，起播水位 `prebuffer_ms`（legacy 0cf029e min 语义）与增益 `volume_pct`（整型饱和）两参数实装，占位正弦与 `RR3_TTS_MS_PER_CHAR` 已删除（**台-2 清零**；欠板上实录，闭环余量 = 本地 Piper 引擎归增补轮五批E）、**F-6 低电禁动 = 已结（2026-10-13，MC-5 `071dcad`/`8b5b57e`）**——`Rr3LowGate` 接 `rr3-runtime/system/power`，执行面 RPC/intent 两路拒动、妆面不禁（欠板上实录）、**F-2 表情上屏 = 判据面兑现（2026-10-13，MC-3 复核）**——`rr3_hal_display` 的 `RR3_BACKEND_ESP32` 桥后端支（批B `5280fe0` 出字 + 批C `60a8b81` 帧进城）+ 开机自述真后端名 + fail-closed 探活，**台-3 清零**（欠板上实录，真屏对账归 0.0.6 判据 10/接线 3；余数 = 帧表 R-12 迁移，归属待裁）。**不可达（欠码，非欠载体——这条区分是本行的全部意义）**：**F-8 微型形态板侧件** = 件与装配单真跑未发生——装配演练面在位（`profiles/esp32-mini.toml` 三格 + contract.rs 形状 pin + `rr3-pack` 正例），MC-7 复核（2026-10-13）不勾：件承载三缺（件模块未进 xtensa 门 / 板侧仍专用固件 crate / `[hardware]` 进位未补，触发条件 = 0.0.7 轻承载档真板）。另有两处**未登记的站替面**已按索引 R-16 落单并**已由批M 处置**：`Rr3SimProbe` 恒真（缺格降级环不转）→ **MC-4 已结**（`084d081`：`Rr3LinuxProbe` 五面实装 + `RR3_PROBE` 显式选择口，sim 探针不再当缺省构造）、配网凭据不落盘 → **MC-6 已结**（`084d081`：后端承载持久化，件内不自写文件）；`net.bt` 骨架格位台账已按 MC-9 裁定 (a) 改记 `placeholder`（触发条件 = 配网备选通道立项 / BlueZ 真后端批，`ba90367`） |
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
| M4 | rr3-brain：移植 asr/llm/tts | 全语音对话闭环（legacy 功能等价） | **已收口（云侧联调敞口）**。`rr3-brain` Python 实装（5ab029e，ADR-0014）：py 总线客户端 bus.py 386 行逐语义镜像 rr-ipc、三家云客户端 urllib 实装（`RR3_BRAIN_CLOUD=mock` 是测试注入点）、dialog 状态机串起 vad_start → asr → llm → tts 回路；线级证据 `rr3-sim --brain` 四真进程全语音对话闭环演练（5b55620）；批F4 `--chain` 六真进程 mock 云把对话轮扩到全链（判向→转向→表情，真板实测 boards §9）——详见能力表"`rr3-brain`"行。敞口一条：三家云真实凭据联调未发生（无 key 环境），不阻塞"legacy 功能等价"的 sim 侧验收。**2026-10-12 订正（版本边界轮）**：这句"legacy 功能等价"在**播报面**不成立——M4 按当时判据收口的是"对话闭环跑通 + TTS 验证性消费"（拿到 PCM、算出时长、入日志），而 legacy 机器人是**真的用人声说话**。今天云侧 PCM 没有通路到扬声器（`composite.py` 只发 `{"text"}`、interact 播 440 Hz 占位正弦），所以播报等价属**未达成**，落 0.0.5 批M MC-1/MC-2（F-4）。M4 的收口状态不回改，缺的是这条敞口的**指名**（**2026-10-13 补记：该敞口的通路半边已由 MC-1 结清**——brain 分帧发布 `rr3-brain/audio/tts_pcm`、interact 收流对账播放，占位正弦退役；播放面参数 `volume_pct`/`prebuffer_ms` 已随 MC-2 实装（`1930956`），闭环余量仅 = 本地 Piper 引擎（增补轮五批E）） |
| M5 | rr3-act / rr3-connect / rr3-power | 语音控继电器 + WiFi 配网 + 电量上报；shm 控制面初版 | **已收口（sim 侧；板上敞口同 M3）**。逐条对照验收标准：①"语音控继电器"——**sim 侧闭环已证（0.0.3 批C，`be19982`）**：意图→动作映射落 brain（`intent.py` 精确词表 + 否定护栏，Q1 裁定策略层唯一归属）、act 订阅执行面两道归属门（exposes 归属 + 意图面开放）执行 `actions[]`、`relay/state` 状态流 append 在册（T3 补裁定）且 e2e 断言 RPC 路 seq=1 → intent 路 seq=2 跨路连贯——语音进 → brain 抽动作 → act 执行 → 状态回读，整链在 `e2e_act` 线级绿；②WiFi 配网——T4c（`2b08994`）net/link 状态机 + cloud/session 应答面（`e2e_connect` 两场景对偶绿）；**0.0.4 批D（`3682bfe`）配网面收口（sim 侧）**：wifi/scan|connect|forget 三 RPC + `Rr3Provision` AP 配网状态机（ap|station 两态纯函数）+ net/link 流 `mode` 字段随发 + 面板 WiFi 页三端点透传（裁决面钉在 connect 模块单测，`e2e_connect` 既有两场景不动）；③电量上报——T4b（`e87a284`）battery/state 周期流 + battery/low 边沿 + rrd 低电兜底四件动作面（`e2e_power` 绿）；④"shm 控制面初版"——Q2 口径（2026-10-04）已达成：ADR-0011 transport 层落地（单写者多读者环形缓冲 + POSIX 附着），rrd 接线不纳入 M5 验收（如需纳入须立新 ADR）。**0.0.6 批F5a 进展**：HAL linux 真后端五件落地（GPIO led/relay、PWM 舵机、link nmcli、battery INA219——代码与 musl 交叉 check 就位，板上单测欠账）；批F4 `--chain` 板上 sim 侧全链闭环（boards §9）。敞口 = 真硬件动作证据（继电器/配网/电池读数，随外设采购 + 板上线，同 M3 口径）；意图词表的产品化扩展属 brain 演进，不在 M5 验收内 |
| M6 | A/B OTA + 恢复出厂 | 断电升级不 brick，坏 slot 自动回滚；genimage 预留模块独立可写分区 | **未收口（主机面已实装，板上件与升级链未做）**。0.0.3 批D（`68220e4`，Q3 裁定）：switch/factory 的判定与改写逻辑在 `rr-ota::bootconf` 主机可测（字节级真工件对拍），`rr3-ota-ctl` 三子命令实装（`--bootconf` 必传、退出码 0/1/2），genimage.cfg 的分区集合（含 rr_staging 独立可写分区）已随 M1 落树。验收标准"断电升级不 brick、坏 slot 自动回滚"依赖的 U-Boot 回滚链（bootcount / bootlimit / altbootcmd / upgrade_available）**语义已落档、写侧未实装**（板上件，partition-layout.md 增补段含标准断电安全序与板上核对项清单七条）；槽位镜像写入（往 rr_b 落整镜像）随升级下载链另批；容量定型与实板验证随硬件——三项在位前不 claim |

---

## 本机无法收口的项

这些**不是待办清单里的模糊项，是本机没有能力证明的项**。任何"应该是好的"的推断都不构成
收口证据，所以下面如实写明缺的是哪一手证据，不软化。

**第八轮起本表有两类行，读之前先看类别**（**第十轮 裁-2 订正：原写"第 1–19 行是缺证据类"既与"18 是缺裁定类首条"互斥、又滞后于后来新增的 20/21 两行 ⇒ 按"区间写法是易失分母"的既有纪律改列全枚举**）：
本表现共 **21 行**——**缺证据类＝第 1…17 行 ＋ 第 19…21 行（共 20 行）**；**缺裁定类＝只有第 18 行（共 1 行，且其**裁定半边已于 2026-10-17 结清**：`ui.face` 拆格已拍板——先立 SPI 词形规则、批N3 拆格 ＋ 门 n 扫面 7→8，裁定记录见 [decisions.md §2](process/decisions.md) 行 2；**余量只剩批N3 代码半边，收口即结项**）**。上面那段章程管的只有缺证据类。**2026-10-19 对表：第 18 行代码半边已随批N3 件六收口（`6953222`），18 行结项 ⇒ 缺裁定类清零**（裁定记录与施工注记见 [decisions.md §2](process/decisions.md) 行 2）。
第 **18** 行是**缺裁定**类的首条——它收口不需要任何硬件，需要的是**一次用户裁定**（施工图 §6 第二问）+ 一批代码（批 N3）；**裁定半边已随 2026-10-17 拍板轮结清（未决对象唯一登记处 = [decisions.md](process/decisions.md)，裁定类预算自此为零）**。
它之所以也住这张表，是因为 [模块设计索引 §11 L-23](design/模块设计索引.md) 明文要求"必须挂敞口账，不许等它自然消失"，
而**本表是全仓唯一的敞口账**——为它另开一张表就等于造第二个真值点（那正是 L-23 自己在批的病）。
⇒ 判据：**"挂敞口账"的落点只有本表一处**；新行若不属"缺证据"类，须在行内明写它属"缺裁定"类并给出裁定归属，不许混装。

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
| 12 | 三形态的**资源事实真值**（接线档 + 能源域阈值与分压比 + `[pins]` 绑定） | 装机完成后的机器可查 `wiring.toml` + 与万用表对拍（≤2%）的域电压/分压比 + 逐形态 `[pins]` 过装载校验 | [ADR-0021](process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md) 本轮只立**对象与校验形状**，取值全在待标定态：ESP32 的 GPIO34 分压测点**尚未接**（`ADC_RAIL_DIVIDER_NUM/DEN` 仍是占位 1:1、`RAIL_GATE_ARMED = false` 不接闸，这条"无真板对拍前不接闸不 claim"的纪律随契约面一起搬进 `[[supplies.domain]].armed`）；树莓派与 RK3566 今天**没有任何机器可查的接线表**，批N1 只能先交演练实例。另记一条**未闭的互斥**：板侧 UART2 RX 用的 GPIO19 与点阵 VSPI 预留同脚（`board.rs` 自述"点阵立项时需再迁，已在案"）——点阵一立即撞 |
| 13 | **微型形态（ESP32）的产线闭环四件**：烧录封装、镜像装配、OTA、回滚 | 一次"从干净机器到能出字/转动/听到声"的可复现流程记录 + 一份失败后的回滚实录 | 复盘实测：`tools/rr3-pack` 与 `tools/deploy/` 只覆盖 host aarch64，**ESP32 侧只有一条手工 `espflash` runner**（`.cargo/config.toml`），无镜像装配、无 OTA、无回滚面；`base/ota/rr3-ota-ctl` 写侧只到"改写 extlinux `root=LABEL`"，`bootcount`/`altbootcmd`/`upgrade_available` **只存在于文档表格**（`base/os/buildroot/patches/README.md` 与 `partition-layout.md` 自述未核敞口）。本机无 ESP-IDF、板不在手 ⇒ 这条**既缺码也缺载体**。裁定见 [ADR-0022](process/adr/ADR-0022-代码事实复盘-机制核纯净度与申报面逃逸与安全语义身体.md) 裁定 6，施工 = [全案](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §17 批N5；**"该形态不可回滚"若为终态，也要写成一句结论而不是空白** |
| 14 | **rtos 形态的配置腿载体本身**（板侧 HTTP 服务 + 堆核算），连带"直连云 API"这条认知路径 | 板侧出现一个真的 HTTP 服务器（带选型结论与内存核算），或明确记"本形态不做配置面、观测腿只有守护输出" | 2026-10-15 证伪审查实测：`tools/esp32-periph-base/src/*.rs` 与 `tools/espidf-sentinel/src` 对 `HTTP`/`https`/`TLS`/`tcp`/`BLE`/`gatt` 词界 **零命中**——板上 WiFi 只用于配网与链路读数。ADR-0021 裁定 1 与 D6 要求"每形态至少一条自己的配置/观测腿"，而全案 §7 那句"承载在 WiFi（rtos 档已在册）"里的"在册"指**格状态不是能力**。工单 connect **N-13**，列**批N4 前置**；细则见 [全案](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §18 F-18，风险面记 architecture **RK-20** |
| 15 | **证据号命名空间**（D11 界面门与"mock 不得充当在位证据"的正面答案） | `docs/evidence/<形态>/<编号>.md` 规则立起来，且面板/装载期能校验所引编号存在并形态匹配 | 实测 `docs/evidence/` 与 `evidence/` 均不存在，仓内无任何证据编号规则（conventions 只有"每行带证据路径"的散文；`contracts/golden/` 是 IPC 字节向量不是板上对拍记录）⇒ D11"`armed=true` 须引用一条真板对拍证据"这条**界面门不可执行**。工单 brain **B-15**，列批N4 前置；细则见全案 §18 F-24 |
| 16 | **两条链上只能由板上证据收口的三问**：板侧运行面失败（舵机拒动/移动失败）在真机上发生过没有、`rr3-audio-rec` 的 gaps/restarts 真实量级、云失败各分支（HTTP 非 200 / 超时 / 坏 JSON / 缺 key）的实际分布 | 一次真板长跑的串口日志样本 + 一次 `RR3_BRAIN_CLOUD` 真云档的 counters 读数 | 第六轮追流（全案 §20.5/§20.6）证明这三问**在仓内无答案**：板侧 `rt_move` 的失败只 `eprintln` 且返回值被 `exec_job` 丢弃、不回宿主；`audio-rec` 的 gaps 只 println 不落 counters；`mock` 档使全部云失败分支不可达。本机无 ESP-IDF、板不在手 ⇒ **缺码与缺载体并存**，归 0.0.7 验证窗；在拿到样本前，可达终点表相应三格写"不能 + 触发条件"，不许留白 |
| 17 | **三形态的平台事实登记表取值**（`contracts/platform/<平台>.toml`：控制器词表 / ADC 脚集 / input-only 脚集 / 冲突矩阵 / 具名构造表，全案 §4.6，第七轮 T-4 逼出的对象） | 每形态一份**由芯片数据手册或 HAL 直读**的取值集 + 一次"故意把模拟测点绑到无 ADC 脚"的装载红例 | 第七轮查出：装载校验 **④-b/⑤-a/⑤-b/⑧-a/⑧-b 五个判定位一直在读这张表，而它从头到尾没有住处**（新类 L-28）——本轮给它一个可指的文件名与形状，**取值仍待器件**：ESP32 一份可由 `tools/esp32-periph-base/src/board.rs` 直读（`ADC_RAIL_PIN=34`、GPIO22/23 硬线 I2C、GPIO19 与 VSPI 预留同脚这三条已在案），树莓派与 RK3566 今天**没有任何机器可查的平台事实**，批N1 只能先交演练实例。与 D9/D10 同族（**卡取值不卡结构**）；表为空 ⇒ 基-2（门 b 装载门）的那五个判定位一律判不了，故 **基-22 必须与基-2 同批** |
| 18 | **`spi_device` 不在门 n 扫面 ⇒ L-23（新设计自带第二真值点）对 SPI 那条链未闭** | 门 n 的扫面 `RR3_PIN_DERIVED_KEYS` 由 **7** 键扩到 **8** 键（含 `spi_device`）、`[hardware]` 里再出现 `spi_device` 即红的一次装载红例；~~前提是先有施工图 §6 第二问的裁定~~（**该裁定已于 2026-10-17 拍板**：先立 SPI 词形规则、批N3 拆格，见 [decisions.md §2](process/decisions.md) 行 2） | 第八轮 实-9 + **实-15** 查实：`rr-registry/src/assemble.rs:56-67` 的 `RR3_ASSEMBLE_DESC_KEYS` **十名，含 `spi_device`**（注释写明它是 0.0.6 链④按裁定流程扩进去的）；而 `rrd/src/profile.rs:91-99` 的门 n 扫面 `RR3_PIN_DERIVED_KEYS: [&str; 7]` **不含**它 ⇒ SPI 器件（MAX7219 点阵）仍走 schema 2 的 `[hardware]` 直给旧路，**注入路径对它不唯一、门 n 对它永远不闭**。两张表相差的三名（`alsa_device`/`link_ifname`/`spi_device`）恰好就是 `contracts/roles.toml:23` 的三个非引脚键。**收口终点＝批 N3**——**裁定半边已结（2026-10-17 拍板：批N3 交 SPI 词形规则 ＋ 门 n 扫面 7→8，同批拆格；不给 SPI 显示面造角色名）**，余量只剩批N3 代码半边。**实-15 顺带查出**：施工图 Task 9 Step 2 原本预写的那行文本（"`spi_*` 派生键未入 `RR3_ASSEMBLE_DESC_KEYS`"）与本行第一句矛盾，已改判，**不得回抄原文** ／ **2026-10-16 对表**：该号位原由并行设备填入一句假账（"spi 派生键未入 `RR3_ASSEMBLE_DESC_KEYS`"与 `assemble.rs:66` 矛盾；`ui.face.matrix` 不在 `roles.toml` 四角色里），本轮就地订正文本并**保留其号**（先占者保号）；判据已升 **实-16**——抄入台账的每一行须由执行者再跑一次该行点名的命令，写侧 + 抄侧两次回执缺一不可（全案 §22.9）／**2026-10-19 结项（批N3 件六 `6953222`）**：词形规则落 `profile.rs` 派生臂（`spidev<N>`＋channel → `/dev/spidev<N>.<M>`，与旧直给值逐字节对齐）＋`RR3_PIN_DERIVED_KEYS` 7→8（`spi_device` 进门 n 扫面，直给与派生并存即红——一次装载红例落库）＋拆格半边 roles 新角色 `ui.face.spi` 与 profiles 两份 `[hardware].spi_device` 撤除迁移 `[[pin]]`；L-23 对 SPI 链闭合，本行结项（施工注记见 [decisions.md §2](process/decisions.md) 行 2） |
| 19 | **L-14 器件级探测的寄存器 ID 读回比对**（INA219 0x00 = 0x2268 等 `contracts/devices.toml` `id_reg` 判据源）：本批（n1-10）已立 sysfs 器件节点判据 + "总线在、器件不在"反例，**读回比对需 i2c-dev 事务（会碰器件），本机 macOS/CI 无法实跑** | 真板上一轮"器件没接 → 该格缺格点名"实测（sysfs 节点不出现）+ 读回比对落 HAL 后端（与 P-5 电流符号派生共用同一 INA219 句柄，不另立句柄） | 全案 §12.1 / power **P-11** + sense **S-11** 需求面；判据本体已落 [`probe.rs`](../base/runtime/rrd/src/probe.rs)（批N1 n1-10），读回归器件与真板批——**结构先行，取值随后**（D9/D10 同族纪律）／**2026-10-19 对表（批N3 件八①）**：`id_reg` 判据源随计量格补课 4→6（SHTC3/ICM-42688 演练行进场，`3025205`），结构面再实、取值随后口径不变，本行不结项（真板批） |
| 20 | **出厂轨（buildroot / `S99rrd`）上装载门真跑过一次的证据** | 一份全量 make 产出的镜像里，`/etc/init.d/S99rrd` 带 `--profile` 启动后的一次 `rrd` 日志（能看到 31 位判据里那 14 位真在判），以及一次**故意写坏接线档**让它拒起的对照 | 第八轮 实-1 / 新类 **L-31** 查出：今天出厂轨**根本不传 `--profile`**（`base/os/buildroot/board/radxa-zero3/post-build.sh:129-155` 生成的启动脚本只有 `--socket`/`--manifest`），`daemon.rs:209` 的 `None` 臂（`:248-253`）于是合法地返回空 `hal_bindings` + 空降级集 ⇒ **十条校验一个判定位都不跑，而 `rrd` 退出码 0、日志无异常**。修法是代码侧的（`--profile` 升必选 + 两轨参数集对账，**基-23**，批N1），但**"修好了"这件事只能由一次出厂轨启动日志证明**，本机无 Radxa ZERO 3 也无全量 Buildroot 环境（同第 6 行）。在拿到证据前，**不得把"装载门已建成"读成"出厂机上装载门在跑"** |
| 21 | **rtos 档"只改数据即改接线"的行为证据** | 改一次 `profiles/esp32-mini.toml` 的 `[pins].physical` 取值，板上行为随之变化的一次实录；以及一次**抽掉注入通路**让它 fail-closed 拒起的对照 | 第八轮 实-6 查出：这条链**今天只做了一档**。host 档是真的（`profiles/*.toml` → `[pins]` 派生 → `RR3_HAL_BINDINGS` → `descriptor.rs`）；rtos 档四项零命中——板侧引脚**硬编码**（`tools/esp32-periph-base/src/board.rs`，取证时点 `e9a764f` :148/150/174；原记 `:156/158/182` 系 `b11dd2f` 时点）、桥上**无配置命令集**、平台词形硬编码在 `rrd/src/profile.rs:660`、`:1006` 的档位守卫拿字面量 `"host"` 比 ⇒ **三份接线档与 `[pins]` 对 rtos 是零消费者的装饰数据**。修法是 **基-24 桥上配置腿**（归属 connect 册，批N1），验收判据是**行为判据**而非"代码里读了配置"；本机无 ESP-IDF 也无板（同第 13/14 行），故只能登记为敞口。在拿到证据前，**"裁剪与接线都只改数据"这句必须带档位限定读** |
| 22 | **单元装配位仍住实现 crate**（基-35 半切余量①）：装配选择函数 `rr3_hal_*()` 住 `rr-hal/src/backend.rs`（实现 crate），装配位今日在五单元 composite.rs 内——⑨-w 白名单第一类 `rr_hal::backend::*` 因它存在而必须放行，Cargo 级拆边需**装配上移** | **缺裁定类**（首条先例＝第 18 行）：一次用户裁定"装配位上移到哪一层"（装配层收编装配选择，单元只持 `Box<dyn 契约 trait>` 句柄）＋一批代码；裁定归属 = [decisions.md §3 在册未决](process/decisions.md) | 施工事实（`be69c09`，⑨-w 三判据）：单元 Cargo 已直依 `rr-hal-api`、契约符号 use 已切 `rr_hal_api::`（30 处），但 rr-hal 边按裁定 12 保留装配用 ⇒ **"单元零实现 crate 引用"今天不成立**。不裁的缺省态：白名单四类冻结现状，**不阻塞批N2–N5**；上移落地后 [check.sh](../tools/ci/check.sh) ⑨-w 白名单第一类同步收紧为 Cargo 禁令（该步注记在案） |
| 23 | **单元对总线承载 crate `rr-bus` 的直依不在任何门内**（基-35 半切余量②）：⑨-w 只钉能力契约面（`rr-hal-api` 必依）与板侧三禁（rr-hal/rr-bus/rrd），五单元 Cargo 对 `rr-bus`（std 承载：bus 客户端/serde_json 面）的直依不落任何判据 | **缺裁定类**：一次用户裁定"总线承载面要不要照 `rr-hal-api` 先例契约化（立承载契约层或经 `rr-core` 机制核供给）"＋一批代码；裁定归属 = [decisions.md §3 在册未决](process/decisions.md) | 事实基线（取证时点 `8f8bbce`）：五单元 Cargo.toml 均直依 `rr-bus`——这是**承载面**（与能力契约面分立），基-35 只切了后者。不裁的缺省态：直依维持现状，**不阻塞批N2–N5**；若裁"不契约化"，本条以"承载面维持实现 crate 直依"的明文结论结项 |

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

- **0.0.5 增补轮六——底座资源模型与能力面抽象补课（2026-10-14 立项，**批N1 十小步 n1-1…n1-10 全收口**；立项那轮本身零 `.rs`）**：
  **第八轮订正（实-8）＋ 2026-10-16 对表：本行原写"未开工"，今天为假**——批N1 已在树 **10 格代码提交**
  （`769967b` roles / `3524dcd` devices / `05c7529` schema 3 `[pins]` / `d1df8a1` 第七供给位⑥ / `e6d7ef2` 接线档⑦ /
  `b11dd2f` rr3-pack 真值集 / `9b7d4f7` n1-7 `contracts/platform/` 三份 + 读表器 + 组合件申报面 /
  `0abd5a3` n1-8 表情真值点 + `rr3-codegen` + 门 f 接线 / `0e7bc6d` n1-9 校验 ⑤-a/⑤-b/⑧-b/③ 三空间/④-b /
  `bbc853d` n1-10 L-14 器件级探测 + 收口文档回写）：连带进树的 `.rs` 有 `rrd/src/profile.rs`·`wiring.rs`(新 500 行)·`daemon.rs`、
  `rr-registry/src/roles.rs`·`devices.rs`(新 265 行)·`lib.rs`、`tools/rr3-pack/src/main.rs`、`tools/sim/src/assemble.rs` 与 4 份 e2e。
  **台账说谎的机制**：本行是"立项那一轮"的在制品记录，而开工事实进了下面各批的条目、没有回改这一行
  ⇒ 一个只读本行的读者会得出"契约层一行没写"的结论。修法已立为纪律（conventions §2 同日八轮）：**在制品行的状态词随开工批同批改写**。
  下面这段是**立项那轮的原文**（按 X-4 保留，只加本指针不改写）：
  裁定 [ADR-0021](process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md) + 施工编排
  [全案](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md)（批N1–N4）落档；
  architecture 升 **v0.11**（七位供给 / 八段描述符 / 接缝四 / 三独立形态两轴 / §13 待建门行）、
  模块设计索引（§3 第四条、§8 D10–D15 及 D3/D5–D8 全部落定、§10 两轴读法、
  §11 新增 L-11/L-12/L-13 与二轮的 L-14/L-15/L-16、§14.6 归属行）、
  六册新增工单（power P-9/P-10、act A-13/A-14、interact I-15/I-16、sense S-9/S-10、connect N-10、
  brain B-12）、ADR-0018/0019 各加**后续改判指针**（正文与修订记录按历史保留不改写）。
  同批订正两处**已被实机推翻的断言**：增补轮五"UART2 = GPIO16/17……与 0.0.7 实测接线零冲突"
  （板侧实为 GPIO18/19，且 19 与点阵 VSPI 预留同脚、互斥**今天仍未闭**）、
  0.0.6 §5.4 BOM 第 2/4 行"由 ESP32 协处理器承载**替代**"（三形态独立，替代关系作废）。
  本轮**不产任何板上证据**，也不改 `contracts/*.toml` 与任何 `.rs`——契约文件改动属批N1 开工后的事，
  届时 `schema_version` 2→3 与 `rr-registry`/`rr-hal`/`rr3-pack`/`profiles` 须同批。
  **同日二轮（同批，仍零 `.rs`）**：用户授权"按最优方案决策"后，**D10–D15 六条全部裁定**
  （其中仅 D10 的取值面是**可撤回的设计默认**）并顺带结清 **D3/D5/D6/D7/D8**；结论行在索引 §8、
  **细则唯一落点 = 全案 §12**。同时拿主线当尺子复查**门禁与纪律自身**，查出并登记三条
  纪律层自指失败（索引 §11 **L-14/L-15/L-16**，全部 `grep` 自核）：
  ① 器件级探测缺失——`Rr3LinuxProbe::rr3_face_present` 判 I2C 只看 `/dev/i2c-*` 存在、**从不问从机地址**，
  于是 `power.charger`/`sensor.env`/`sensor.imu` 只要总线在就**恒判在位**（器件没接也绿）；
  ② **"零策略载荷"有两套禁词表**——`RR3_BOARD_TOKENS` 含 `gpio/i2c/spi/uart/pwm`，⑦-a 的源码扫面不含，
  且 ⑦-a 执法前剥行注释，使 `Rr3FaceFrame` 注释里的"MAX7219 行寄存器直排"对门**永不可见**
  （器件事实写注释即逃逸）；
  ③ **"N 册同批"句式造无主项**——实测 `RR3_POLL_WAIT` 只存在于 act/connect/interact/power 四份
  `composite.rs`，sense 根本没有该符号，所以文档那句"五册同批"的事实基础即错。
  §15.1 据此把十条"策略住码内且无单号"的位点逐条指派（**A-15 / I-17 / S-11 / N-11 / B-13 / P-11**），
  待建门由六扩到**十**（新增 h 禁词单源 / i 注释不得独承载器件事实 / j 组合件申报面进门 /
  k "同批句式必须带单号"）。**未决的只剩两类**：D9 等器件到货（不卡批N1–N4）；
  D10 取值等采购。另有一条**不替产品定**：brain 无中止面（现场无法取消正在跑的云轮次），
  加 `dialog/abort` 属新增契约面，须单独评审。
  **批N1 施工中（2026-10-15 起，第八轮施工图，判据真值 `docs/process/dev/0.0.5增补轮六——批N1收口与装载门建成施工图.md`）**：
  n1-1 `roles.toml` 角色词表（`769967b`）/ n1-2 `devices.toml` 器件词表（`3524dcd`）/
  n1-4·n1-5 profile schema 3 `[pins]` 全链（`05c7529`）/ n1-6a `[[supplies.domain]]` 第七供给位 +
  校验⑥（`d1df8a1`）/ n1-6b `wiring.toml` 接线档全链 + devices 词表装载 + 校验⑦（`e6d7ef2`）/
  Z-4 rr3-pack 打包真值集跟装载消费集（`b11dd2f`）/ n1-7 `contracts/platform/<平台>.toml` 三份落盘
  （esp32 真形态 `board.rs` 直读 + rk3566 / raspberry-pi-4 **演练实例**）+ rr-registry `src/platform.rs`
  fail-closed 读表器（④⑤⑧ 判据前件；与 roles/devices 空表宽容的关键差异：平台表读不到回 `Err`
  拒 boot，宽容 = §18 F-6 病）+ 组合件申报面定形（`src/composite.rs`：六份 `modules/*/composite.toml`
  同批补 `handles`/`ops`/`no_injection` 三键，N-11 门 j 前件；**schema 位裁定**：组合件面
  `RR3_COMPOSITE_SCHEMA_VERSION = 1` 与件级 `RR3_ASSEMBLE_SCHEMA_VERSION = 2` 同名不同域，
  本轮增三键属首次定形只增不 bump）/ n1-8 生成物真值点定形（§4.5）：白名单唯一来源落
  `contracts/face-expressions.toml`（43 态全集 + board_subset 七态；宿主投影 face.rs
  include_str! 逐名对账）+ 生成器 `tools/rr3-codegen`（板侧 `topics_gen.rs` 生成副本，
  board.rs 四手写常量退役改 include）+ 门 f 接线（check.sh 拟 ⑨-d 重跑 diff 为空）+
  descriptor 键集改判收口（rr-registry contract.rs 机判 `Rr3HalBindings` 十键与
  `RR3_ASSEMBLE_DESC_KEYS` 两处同批，C4 撤销后 spi_device 进场的对账钉死）/ n1-9 装载校验接平台表
  （判据实判位 **5→12**）：`[platform].chip` FK 必填非空（boot 链按 `contracts/platform/<chip>.toml`
  选册，读不到/形状坏 fail-closed 拒 boot）+ 校验⑤-a（`direction=out` 落平台 input-only 集；
  `input_only_ok` 住 roles.toml 角色侧）/⑤-b（域 `sense`/`current_sense` 按 pin_id 引用的 pin，
  其 physical ∈ 平台 adc_pins；I2C 测点 physical 空 = 无判据对象）/⑧-b（controller ∈ 平台词表）/
  ③ 三资源空间双占（physical 元素 / (controller, channel) / (bus, addr)——addr="none" 显式不适值
  不占地址维）/④-b conflicts 反推互斥（命中同一冲突子集者 ≥2 且任一未申报该组即红，**组名取自
  平台表**，装机档不得自起名；单向含入——芯片事实 ≠ 现役占用）——后果一律拒（全案 §5 第 5 列）；
  10 条新用例（含 ④-b 正例：双 pin 申报 `mux_group=["i2c0"]` 放行）全绿，daemon 喂
  `contracts/platform/` 目录，profiles 三份 + e2e 四夹具（act/power/connect/sim）同批补 chip；
  施工中修 1 缺陷：③ owner 判重原用 pin_id 字符串，per_instance=false 角色的双 pin 同名会把双占
  误判成"自己撞自己"吞红，改循环序号判"同一条"（③-c 用例当场抓出）/ n1-10 收口两件：
  **① L-14 器件级探测本体**（全案 §12.1 / power P-11 + sense S-11 需求面）——探针接口加
  `rr3_probe_slot_bound`（带缺省回落），I2C 格位在装机档 `[pins]` 在册 `(bus, addr)` 时在位判据
  升为"内核已为该从机地址注册器件节点"（sysfs `devices/<bus>-<addr4hex>`，如 `1-0040`）、
  总线级降**前置条件**（收窄到该具体总线，由 controller 词形 i2c\<N\> 数据驱动）；"总线在、
  器件不在"反例用例落库（旧判据下此格恒绿——L-14 缺陷的判据身），addr 词形坏/越 7 位域一律缺
  （fail-closed）；装配单无 `[pins]` 时回落五面原样（兼容面，e2e 即此形）；**寄存器 ID 读回比对
  （INA219 0x00 = 0x2268）需 i2c-dev 事务会碰器件，归器件与真板批（敞口表第 19 行）**——
  当前 roles 表唯一 I2C 角色 power.battery.meter 属 base 层格位不进 bindings 探测环，生产形态
  待器件角色（charger/env/imu，批N3）进场即自动生效；**② 文档回写**——README 两处"已立项未开工"
  改施工实况 + 敞口表补第 18 行（spi_* 派生键，施工图 Task 9 Step 2 遗留）与第 19 行（L-14 读回
  比对）+ conventions 台账批N1 行——**批N1 十小步（n1-1…n1-10）全部收口**。

- **2026-10-15 全代码逐 crate 复盘 → [ADR-0022](process/adr/ADR-0022-代码事实复盘-机制核纯净度与申报面逃逸与安全语义身体.md) 落档**（文档轮，
  零 `.rs`、零契约文件改动）：取证 = 四路并行审查 + **每条断言我本人 grep/读码复核**（撤回与收窄项见
  [全案](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §16.2）。核实成立的主线违例五组
  登记为索引 §11 **L-17…L-21**：机制核携策略窗（`rr-mech` 的 `250u64`）、契约层携部署缺省
  （`rr-ipc` 的 socket/shm 路径缺省对 `none` 目标仍再导出）、跨形态契约携器件缺省（拾音 `channels: 1`
  对上实机双麦，另 `audio` 上行 `seq` 宿主 `u64` / 板侧 `u32` 宽度分叉）、基座 crate 钉某块板
  （`rr-ota` 两处 `include_str!(…radxa-zero3…)`）、**申报面可内联逃逸**（哨兵内联 manifest exposes
  `rr3-sentinel/diag/parity` 不在册 + 装配扫描不含组合件层）。另两组运维空洞（生效绑定无外部读者、
  日志环 512 行不落盘）与一组产线缺口（打包/回滚只覆盖 host aarch64，**ESP32 零烧录封装/零 OTA/零回滚**）。
  同批订正两处计数与文案：`board.rs` 板常量 **50**（ADR-0021 原写"约三十个"）、
  `rr3-act` 的 `move_timeout_ms` 注释与 println 降级为"入参时限裁决"（唯一功能读者是入参裁决，
  反射环内**无到时停轴路径**）。回写：architecture **v0.12**（第四条增长律 / `[hardware]` 条件必填 /
  装配单自洽 + 可达终点表 / §13 三把新待建门）、索引（L-17…L-21 + §14.6 两行）、
  六册新工单 **A-16 / S-12 / I-18 / N-12 / P-12 / B-14**、待建门十→**十五把**（拟 ⑨-i…m）、
  施工追加**批N5（生产与运维闭环）**——其首件"按形态分档可达终点表"不依赖新契约，**可先于 N1 开工**。
  **本批复盘只核代码文本，不产任何板上证据**；`contracts/README.md` 的历史计数行加现值注而不改写历史。

- **2026-10-15 同日第四轮：逐项证伪审查 → 全案 §18 落档**（文档轮，零 `.rs`、零 `contracts/*.toml`）。本轮不查完备性（§15/§16 已各一轮），只逐条回答"**什么观测能推翻这条设计**"，反例构造到"不改任何硬件形状、只改装机档或一句话就能让门变绿"的粒度。**32 条命中，其中 14 条打在 ADR-0021/ADR-0022 与全案自己写的裁定与编排上**；撤回 7 处单列 §18.8、审过成立 10 条单列 §18.7。三类失败形态升格进索引 §11：**L-22 自愿填写的字段是注释搬了家** / **L-23 新设计自带第二真值点** / **L-24 判据恒真是判据缺失**。**六处改判**：① `[hardware]` 六物理键**下移**为 `[pins]` 派生视图 + 新门 ⑨-n 注入路径唯一（实读 `rr-hal/src/backend.rs:327` 取 `b.pwm_chip?` 与 `atoms/servo/assemble.toml:9-11`：`[pins]` 若只是旁挂第八段，则装载期说 A 份、运行时用 B 份，而门只判 A 份）；② **`pin_id` 等式**定义，接线档 / 能源域绑定位 / 轴档案 `pin` 外键一律引它（首版 `sense="adc:gp34"` 违了同格注记"角色名，非裸引脚"）；③ 新立契约文件 **`contracts/devices.toml`** + 新门 ⑨-o（"速率取值域按器件在册"、L-14 器件级探测的寄存器 ID、门 ⑨-g 的型号词表**三处的共同前件**）；④ §5 校验补**缺失语义**与**后果三态**两列（首版十条**全部可被留空绕过**）、口径写成"十条 / **24 个判定位**"，另补 ③ 三资源空间、④ 双向互斥、⑤ ADC 能力半、⑦ `feeds` 加总（1A 线载 2.2A 峰值原本全绿）、⑧ 三元键、⑨ 缺省全重叠、⑩ 栈堆两条；⑤ 门 ⑨-a 由四表改判**六表**、`roles.toml` 补 `iface`/`requires`、`kind` 补 `logic`、**撤名 `Rr3PowerDomain`**；⑥ 门 ⑨-f 改"一套词表 + 两张在册豁免域"、门 ⑨-h 补组合件申报字段前件、门 ⑩ 分母降字段级 + 分子须端点自报 `covers`、门 ⑪ 加"集合宣称必须可数"、门 ⑨-l/⑨-m 分判"申报文件合法集"与"**读者三态标注**"。待建门 **十五→十七把**；architecture 升 **v0.13**；六册新工单六个 **P-13 / A-17 / I-19 / N-13 / S-13 / B-15**；敞口表新增第 14、15 行（rtos 配置腿载体、证据号命名空间）。**两处撤回值得单列**：ADR-0022 裁定 2.4 的"同格双件静默并存"**前提为假**（`rr-registry/src/atom.rs:530-536` 当场 `panic!("格 {slot} 被两件抢占")`），真位点是 `daemon.rs:357-369` 的"providers **含被拒模块**"（ADR-0009 有意语义，缺的是可观测项）；上一轮"批N5 首件可先于 N1 开工"与其自身判据"由结构化开放项数据生成"矛盾，改判为**首版手写 + 逐格标 `文件:符号`**。**本轮不产任何板上证据**，也不动任何契约文件——devices 表与键下移都属批N1 开工后的契约面施工

- **2026-10-15 同日第五轮：跨文档一致性 + 运行期语义审查 → 全案 §19 落档**（文档轮，零 `.rs`、零契约文件）。
   前四轮打"代码事实 / 判据可判定性"，本轮打两件没碰过的事。**(A) 口径传播**：机械数旧字面量查出
   **X-1**"十项"有两个所指（装载侧 vs 可达终点表）⇒ 统一为"装载期十条校验（24 个判定位）"；
   **X-2** 待建门有两个分母（设计编号 13 行 vs `check.sh` 拟步 17 把，字母**错位一位**）⇒ §16.4 分列 +
   纪律"引用门写全前缀"；**X-3** 批N5 立项后 5 处仍写"批N1–N4"⇒ 全改；**X-4** architecture **正文留着旧分母 9 处**
   （§1 核心口号"两件码 + 七段"、§12.1 分档判据"四类供给齐"、**§12.2 标题本身**、§12.8 移植第一步）而速查表已是七位
   ⇒ 9 处全改、4 处历史注记只加指针，收口判据"旧字面量在现行文字中为 0"；**X-5 由这次修改当场暴露**：
   把判据写成"七位供给齐不齐"之后才发现 `[supplies]` 今天只有六键（`rrd/src/profile.rs:45,228`），
   **七位里只有 2 位可机判** ⇒ 要么批N1 补 `timebase`/`heap` 键，要么明写不可判，三态不许被一句话抹平。
   **(B) 运行期语义**（索引 **L-26**，四条设计只写了正常路径）：① **面板写阈值没有通路**（阈值与 `armed` 是
   `pub(crate) const`、`SIGHUP`/`reload` 零命中、14 端点无阈值写口）⇒ 可写量必填 `applies = live｜reboot｜reflash`，
   这句话在 rtos 档的真实含义是"重烧固件"；② **面板唯一写落盘不是原子的**（`secrets.rs:95-105` truncate 原地覆盖、
   无 fsync/rename，读侧坏 JSON 回 `Err` 且不覆盖 ⇒ 一次掉电卡在写中间就从此坏死且不能自愈；仓内正确范式只有一份
   `counters.rs:110-124` tmp→fsync→rename）⇒ **写落盘只许一份实现，出现第二条即红**；③ **闸门复位即开闸**
   （`Rr3LowGate{blocked:false}` 纯内存，`rr-mech/src/control.rs:99-125`）而 BOD 复位循环正是
   "欠压→复位→开闸→再推舵机"，"回阈不自动重放"的真身是"没有重放实现" ⇒ §6 **复位三义扩为四义**：
   复位后默认取保守侧，须先有一次合法样本；④ **用于判定的读数没有龄期**（档位是 RAM 快门值；
   `rr3_now_ns()` 取不到 epoch 折 `0` = 假时间戳）⇒ `age_ms`/`max_age_ms` 并入校验⑥-c，**刻意不新增判定位**
   （新增位会立刻制造 X-4 那类 21 处改名）。另核出 **Y-5**：经桥链路两侧三种做法（宿主 `rr3_ensure` 只试一次无退避、
   桥进程有 1s→10s 退避、板侧重连零命中），"板死则宿主阻塞"至今挂在注释里 ⇒ 必须进可达终点表经桥行。
   落档：全案 §19（X 类 5 条 + Y 类 8 条 + 判据 6 条 + **纪律 2 条**：改一处口径必扫全仓同名字面量并逐处判性质、
   新增一层必改所有范围表述）；索引 §11 新增 **L-25 / L-26**；六册新工单 **P-14 / A-18 / I-20 / N-14 / S-14**；
   architecture §1/§4.3/§12.1/§12.2/§12.8 分母订正。待建门数不变（17 把）。
   **本轮不产任何板上证据**；`Rr3Store` 与 NVS 三项载体性质（掉电原子性 / 写放大 / 容量上限）的实测归验证窗，
   本机无 ESP-IDF 与板 —— 在它测完之前，rtos 档的 `calibrated` 只许是运行期内存态，
   且可达终点表必须明写"标定状态重启即失"这一句。

- **2026-10-15 同日第六轮：端到端流 + 变更爆炸半径 → 全案 §20 落档**（文档轮，零 `.rs`、零契约文件）。
   方法：`flow-visualizer` 追两条链（P1 面板→舵机、P2 麦→云→播放）+ 自己做一次读者计数
   （`dependency-impact-analyzer` 的那一问：**"同批改 N 处"从来没人数过读者**）；子代理给的行号逐处 `sed` 复核。
   **爆炸半径**：绑定物理键有 **22 个文件**在读（含 `rrd/src/profile.rs`、`rr-registry/tests/contract.rs`、
   **板侧 4 文件**、6 份含键 `assemble.toml`），而 N1 改动面只列了 5 处 ⇒ 新门 **⑨-p 物理键读者集对账**。
   另查出两个会让门失效的细节：**`i2c_bus` 同名跨命名空间**（描述符键 vs 板侧变量 12 处 ⇒ 假阳性读者）；
   **`schema_version` 四域、其中两域当前同为 2**（`RR3_PROFILE_SCHEMA_VERSION` 与 `RR3_ASSEMBLE_SCHEMA_VERSION`），
   而 5 处文档裸写"2→3" ⇒ 改错常量当场毁 17 份装配清单，表述已全部改成带常量名。
   **申报面缺第四维**：call 路由只看被调方 `exposes`（`registry.rs:953-964`），`rr3-interact` 的 4 次调用
   在申报面上不存在 ⇒ "改一条 request 主题谁会断"不可答；新门 **⑨-q** 先管住 `bus.rpc` 申报一致
   （brain 申报了、interact 没申报），`[rpc] calls` 维度**待契约级评审**（不擅自新增申报面）。
   **流上的四个硬事实**：`supervisor.rs:319` 把子进程 stdout 接成 `Stdio::null()`，而模块生产码有
   **45 处 `println!`**（`eprintln!` 仅 13）⇒ 舵机写入失败/配网成功/拒动点名**按设计进不了任何去处**，
   六单元里只有 brain 看得见（新门 **⑨-r**）；板侧控制面在物理执行**之前**回成功、回显记账角、
   `move_ms` 恒 0 ⇒ 宿主"回显一致性"门**结构性地发现不了拒动**；`rr3_low_gate_release` **零调用方**
   ⇒ 低电闸门一旦置位永不解除（与第五轮"复位即开闸"合起来 = 同一闸门两端都错）；
   播放点名失败被吞成一行 print 后照常 `chat_rounds+1`、`integrity(0,0)=Ok` ⇒ **"完成一轮"与"播出声"解耦**。
   **一处宣称订正**：**"板侧双麦 → 宿主 VAD"这一跳结构上不存在**（`rr3-audio-rec` 只依赖 `rr-ipc`+`serde_json`
   不落总线、`rr3_hal_audio_source()` 无 esp32 分支）⇒ §2 表"四件已在板上真跑"已改为如实表述，
   可达终点表 rtos 行"拾音"按此填。**面板也没有舵机端点**（14 端点无 servo、interact 全目录 `servo` 零命中），
   §7 的"逐轴点动/回中/标定"是**待建**不是现状。
   待建门 **17→20 把**；索引 §11 新增 **L-27**；六册新工单 **A-19 / I-21 / B-16**。
   **本轮不产任何板上证据**：板侧运行面失败是否发生过、`rr3-audio-rec` 的 gaps/restarts 真实量级、
   云失败各分支的实际分布，全部归 0.0.7 验证窗（见敞口表第 16 行）。

- **2026-10-15 同日第七轮：设计逻辑闭合性审查 → 全案 §21 落档**（文档轮，零 `.rs`、零 `contracts/*.toml`）。
   前六轮问"是不是真的 / 能不能判 / 口径一致吗 / 跑起来是什么 / 能不能推翻 / 流与爆炸半径"，本轮问一件不同的事：
   **每条设计有没有终点**。尺子是"闭合五问"（对象有住处 / 建设有单号 / 缺失有语义 / 状态词有不信任态 / 红有终点）
   ——它是 §0 收口判据的**反向用法**：那条问"坑落到哪一处"，这把问"落下的那一处自己闭合了没有"。
   **十一条断头 T-1…T-11**，其中三条打在**本轮之前几轮自己的产物**上：
   ① **T-4（最重）判据的前件对象未立**——装载校验 ④-b/⑤-a/⑤-b/⑧-a/⑧-b **五个判定位共引一张"平台侧数据"**
   （`conflicts` 集 / ADC 能力集 / input-only 集 / 控制器词表 / 具名构造表），而 §4 六小节里从头到尾**没有它**，
   §9 批N1 只把它列为"三份新数据"⇒ 建成即全红而施工者无处改绿，与 F-32 的"无判据对象"同型（新类 **L-28**）。
   修法 = 新立 **§4.6 `contracts/platform/<平台>.toml`**（四段合一：控制器词表 / `[capabilities]` / `[[conflict]]` /
   `[construction]`；缺失即红、空集须显式写、栈堆与器件能力**不许并表**——"芯片有什么 / 这块板供多少 / 这颗器件叫什么"三问三个主人）；
   ② **T-8 已撤回的名字在编排表里还活着**——`Rr3PowerDomain` 的撤名落在 DoD / 回写面 / 口径 / conventions 四处，
   唯独 **§9 批N2 行仍把它列为交付物**，而编排表是施工者唯一通读的那张（L-25 的新实例，落点最危险）；
   ③ **T-10 口径登记表自己没有被闭合**——全案 §16.4 停在"17 把 / L-1…L-24"，实为 20 把 / L-1…L-27（第五/六轮没登记）
   ⇒ 升格为纪律：**§16.4 / conventions §2 / architecture §14 三处事实源同批改**，执法位 = 基-20（拟步 ⑪）。
   **本轮最大的一条不是命中清单，是 T-1：基座轨没有工单命名空间**——改机制核、契约层、`check.sh`、装配单本身
   这条轨在全仓没有任何号可引，十六条门里"建这把门"这件事**一条工单都没有**（挂的六个号全是**前件对象**的单元单），
   这解释了为什么 L-17…L-21 那五组基座违例只能靠"拟 ⑨-x"当归属：**拟步号是执法位，不是交付承诺**（新类 **L-29**）。
   修法 = 新立 **`基-1…基-22`**（与拟步号一一对应，逐门补齐 批次 / 前置 / "建成即红 → 转绿批或敞口行"三列，
   表见全案 §21.4 与索引 §14.7）；DoD 4 由"门 a–o"改写为**待建 20 把 + 拆三条义务**（T-7），DoD 2 的四类数据结构扩为**五类**。
   另两条同类：**T-11 撞名**——第六轮给 §20.7 起的发现号 `S-1…S-5` 与 sense 的工单序列 `S-1…S-14` 撞，
   整串改名 `流-1…流-5`（5 条定义 + 5 处引用一次改完，不留兼容层）；新序列拟名 `G-` 也在落笔前查出被 §15.2 占用而弃用
   ⇒ 新立 [conventions §6 标识符命名空间总表](process/conventions.md)（11 个序列的唯一登记处；起新名前必须 grep 确认该前缀为空，新类 **L-30**）。
   运行期一条：**T-6 `Rr3RailTier` 没有"读数不可信"态**——`Rr3RailState{mv, tier}` 无龄期字段，
   面板 / brain / 日志读到的 `tier` 与"陈旧 / 从未取到"不可区分 ⇒ 补 `age_ms` 与 **`Unknown`**，
   规则 `age_ms > max_age_ms ⇒ Unknown ⇒ 执行面按 Severe 处置、展示面原样显示`（power **P-15**）；
   `concurrency_group`（校验⑨ 的唯一放松源）落 `[[atom.demand]]`（P-10），`id_reg` 缺省态定 `unknown`
   且明写"只能报『总线可达』不得报『在位』"（sense S-13）。
   **本轮清白账**（照实说）：§1 十六条坑 16/16 都有五处落点；六册各 15 节无空节；两套门编号（a–r 与 ⑨-x）对得上；
   状态词 12 个里 7 个闭合、2 个半闭合（`age_ms` / `failsafe`）、3 个不闭合已立单。
   分母只动两处：**契约 5→6 类**、**接缝四 4→5 件**；**24 判定位与 20 把待建门不动**（补的是前件不是条数）。
   回写落点：ADR-0021 **修订 3**、ADR-0022 **修订 2**、architecture **v0.14**（§12.8 / §12.3 / §13 / §14）、
   索引 §11 **L-28/L-29/L-30** + §14.6 第七轮行 + **§14.7 基座轨归属总表**、conventions §2/§6、本表敞口第 17 行。
   平台表**取值待器件**，**本轮不产任何板上证据**。

- **2026-10-15 同日第八轮：设计文档 vs 真在跑的代码 → 全案 §22 落档**（文档轮，零 `.rs`、零 `contracts/*.toml`）。
   前七轮读的是 `a98a891`，本轮读 `b11dd2f`：中间 **6 格在建提交**把 `schema_version` 从 2 推到 3、新增
   `rrd/src/wiring.rs`(500 行) 与 `rr-registry/src/devices.rs`(265 行)、落盘 3 份接线档与 `contracts/devices.toml`(54 行)，
   而 `git diff --name-only 813a5d0..HEAD -- docs/ | wc -l` = **0**——**三格提交零文档回写**，本轮补这笔账。
   **十六条发现 `实-1…实-16`**，最重的三条：
   ① **实-1（生产阻碍，新类 L-31）装载门的入口条件是一个可省略的参数**——`rrd/src/daemon.rs:209` `match &cfg.profile` 的
   `None` 臂（`:248-253`）返回 `cfg.manifests` + 空 `hal_bindings` + 空降级集，**不是错误路径**（`rrd` 正常起、退出码 0、日志无异常）；
   而 RK3566 出厂轨 `base/os/buildroot/board/radxa-zero3/post-build.sh:129-155` 生成的 `/etc/init.d/S99rrd`
   **只传 `--socket`/`--manifest`**（deb 快路轨 `base/os/distro/units/rrd.service:18` 传了 `--profile`）
   ⇒ **出厂路径上十条校验一个判定位都不跑**。前七轮审"门里判什么"（§18）与"门的对象住哪"（§21），
   **没有一轮审"门在什么条件下被跳过"**，而它**绿着发生**。修法三件：`--profile` 升必选 + 出厂轨与 systemd 参数集对账（同属 **基-23**）
   + 门 ⑩ 分母为 0 一律判红（**基-19**）；连带查出 `rrd.service:4` 那句"与保底轨同一组安装位…注入约定同源"**是假的**
   （`/usr/lib/rockrobot3` vs `/usr/lib/rr3`）——**一句已经为假的注释比没有注释更危险**。
   ② **实-6（抽象只做一档）"裁剪与接线都只改数据"今天只对 host 档成立**——host 侧那条链是真的
   （`profiles/*.toml` → `[pins]` 派生 → `RR3_HAL_BINDINGS` → `descriptor.rs`）；rtos 侧四项零命中：
   板侧引脚**硬编码**（`tools/esp32-periph-base/src/board.rs`，取证时点 `e9a764f` :148/150/174；原记 `:156/158/182` 系 `b11dd2f` 时点，漂移即基-37 活体）、桥上**无配置命令集**、
   平台词形硬编码在 `profile.rs:660`、`:1006` 的档位守卫拿字面量 `"host"` 比 ⇒ **三册对 rtos 是零消费者的装饰数据**。
   这一条正打在**跨芯片跨平台快部署**这个核心价值主张上。修法 = 新立 **基-24 桥上配置腿**（归属 connect 册，不另起 N 号），
   验收是**行为判据**："改 `profiles/esp32-mini.toml` 的 `physical` 值后板行为发生变化" + 读不到注入值即 fail-closed 拒起。
   ③ **实-14（反向失真，打在本轮自己的回写产物上）"执法位"必须拆三层报**——我在本轮回写时写下"装载门**零反例用例**、
   门 n **无 CI 执法位**"并铺到 9 处，随后读 `tools/ci/check.sh:79-80`（步 ①b `cargo test --workspace`）+
   `base/runtime/Cargo.toml:3`（members 含 `rrd`）+ `profile.rs:1271-1272` 的 **42** 个 `#[test]`（10 处以"反例"注释逐条点名判定位）
   ⇒ **两半都假**。立三层判据：① **判据实体** ② **反例用例** ③ **专属执法步**，各自可缺、**混报即失真**；
   门 b/门 n 今天是 ①② 有、③ 缺。把已绿着跑在 CI 里的 14 位报成"完全没执法"是**文档小于代码**，
   与 实-13（文档大于代码：§4.1 字段集缺第 11 键 `derives`、示例 `iface` 写了不在册的 `Rr3Actuator`）同为违尺子，只是方向相反。
   **真正缺的第三样**：反例**不按判定位编号组织** ⇒ "31 位每位至少一条反例"今天机判不了（基-2 那 62 条清单的余量所在）。
   **分母动两处**：装载期判定位 **24 → 31**（+4 追认代码已实装而设计未登记的 ①-c…①-f、+1 ⑦-c 拆器件侧/件侧、
   +1 ②-a0 元判据、+1 ⑥-c 拆装载期/运行期），**今天实判 14、未判 17**；基座轨 **基-22 → 基-25**（三件都不占拟步号）。
   **同轮自纠一处造名**：原稿写的 `disp.matrix.spi` **推不出来**（违反 slots 两段式 + roles 前缀＝格名动作侧两条例），
   已在全案 §4.2/§21.4、architecture `:718`、索引 L-23 四处就地收回，不给住处——这正是 实-13 登记的 **F-5 病形**
   （文档示例值会被装机档抄）的活体，由我自己犯了。
   **同轮第二条打在施工图的"预写台账文本"上（实-15，同一 F-5 病形的加重形态）**：施工图 Task 9 Step 2 已经替下一个执行者
   写好了要往本表加的那一整行字，而那行断言"`spi_*` 派生键**未入** `RR3_ASSEMBLE_DESC_KEYS`"与
   `base/runtime/rr-registry/src/assemble.rs:56-67` **直接矛盾**——十名里第十名就是 `spi_device`
   （其注释还写明它是 0.0.6 链④按裁定流程扩进去的）。真正的缺口在**另一张表**：门 n 的扫面
   `rrd/src/profile.rs` 的 `RR3_PIN_DERIVED_KEYS: [&str; 7]`（取证时点 `e9a764f` :92；原记 `:91-99` 系 `b11dd2f` 时点）**不含**它，
   而两张表相差的三名（`alsa_device`/`link_ifname`/`spi_device`）恰好就是 `[hardware]` 的三个非引脚键。
   **它比 F-5 更重的地方**：F-5 抄的是**值**，这里抄的是**一句关于代码状态的断言**，而施工图的执行纪律是
   "每行带证据、无证据的句子删掉"——执行者按纪律照抄时**不会质疑那行断言本身**，于是假账**带着证据号进台账**。
   修法：Task 9 Step 2 的行号 18→**20**（18/19 已被本轮占用）、文本改判为"不在门 n 扫面"，
   并立可复用判据"**施工图里凡预写'将来要往某张表加一行'的句子，写施工图时就要回表核一次**"。
   **实-14 与 实-15 都撞在回写阶段而非通读阶段** ⇒ 同批立"回写不是誊抄，每处被回写的文本都要回证据源核一次"（全案 §22 抬头）。
   **本轮清白账**（照实说）：**②-a 元判据今天建即红 14 条**（`roles.toml` 只有 4 角色覆盖 4 格，而 `status = normal` 的格是 18
   ⇒ 其余 **14 格**的空集包含恒真、永远绿而从未判过，实-7）——如实登记为欠账，归属 基-2 + 各册角色扩册，**禁缩分母让它绿**；
   `contracts/devices.toml` 54 行已落盘但三个访问器里只有 `rr3_device_table_mates()` 有非 test 调用者，
   `rr3_device_table_contains()` 与 `rr3_device_table_peak_ma()` **0 调用者** ⇒ **表已落盘 ≠ 表已被执法**（实-4/实-12）。
   回写落点：ADR-0021 **修订 4**（+ 裁定 1 后补"配置通路是一件交付物"整段）、ADR-0022 **修订 3**、architecture **v0.15**、
   索引 §11 **L-31** + §14.6 第八轮行 + §14.7 **基-23/24/25** 三行、conventions §2（三层执法位判据）/§6（**11→13 行**）、
   connect 册 **基-24** 行、power **P-13/P-14/P-15**（⑥-c 拆 c1/c2）、sense **S-13**（装载器错误面点名到字段，实-10）、
   本表 :127 改正"未开工" + 敞口第 **17–21** 行（**2026-10-16 行号对表**：17 平台表取值／18 `spi_device`（**缺裁定类首条**）／19 L-14 读回比对／20 出厂轨装载门旁路／21 rtos 档只改数据的行为证据；18/19 两号由并行设备先占，本轮**只订正 18 的文本、不动它的号**，本轮两条缺证据类因此落到 20/21）、
   **施工图**（§1 基线与逐 Task 执行态、Task 1/2/3/6/7/8/9/10 订正块、§4 DoD 四行改判、§6 第二问补 `spi_device` 依赖）。
   **回写复验阶段又查出四处"宣称已改而实际没改 / 该列而没列"**（登记为全案 §22.4 **自指第 11 条**，与 实-14/实-15 同族、
   **不新立发现号**；结论升为 全案 §22.7 **第 6 句**）：
   ① architecture 两处**现行文字**仍写"六个物理绑定键"（§12.3 `[hardware]` 行与 §14 速查描述符行），
   而 实-9 那句"现行文字逐处改毕"**点名了后者却没改它**——收工前 `grep -rn "六个物理" docs/` 才命中，两处现已真改毕，
   `:39-40` 历史指针同步扩到覆盖 v0.13 段；ADR-0021 修订 2 那处按 X-4 **只加指针不改写**。
   ② **conventions §2 的轮次日志第七轮与本轮双双断档**（第四/五/六轮各有一条叙事）。根因不在这两处文字，
   在 §22.8 那张回写清单是按"我这轮改了什么"枚举的 ⇒ **清单缺一行时，拿这张清单做的对账会全绿**。两条已补，
   并就地立规矩"本日志只记改了 §2 的哪一条规矩，叙事真值点归全案与 status"——第四/五/六轮那种同一段叙事三面各抄一遍，
   正是第七轮 T-10 查出"口径表滞后两轮"的成因，不能一边立案一边再犯。
   ③ 施工图 **Task 1 Step 2 预写的那个 markdown 块作废**：其中"6 类中已落 **2**"连它自己声称的基线 `05c7529` 都不符
   （`git ls-tree --name-only 05c7529 contracts/` 实测 `topics`/`slots`/`roles`/`devices` **四类**已在，原文只数了本批新落的两类），
   "24 位中实判 **5**"与 §22.6 的 10／14 不等 ⇒ 这是 实-15 那条判据的**第五个同族活体**，也是五个里唯一一处
   **数字看着合理、只有回表跑 `git ls-tree` 才翻得动**的（前四个都带明显字面矛盾）。
   ④ 索引 §14.7 **基-14 行前置列首版漏 `derives`**，收工前按该自己的关键词回 grep 才发现（实测 0 命中），已补并写明它是
   门 a 第 5/6 张表的对账域与门 n 的另一侧真值。
   ⇒ §22.8 抬头据此立**三条执法位**：清单按"**文件 × 节**"枚举 · 收工前
   `git diff --stat b11dd2f HEAD -- docs/` 与清单逐行对、**清单外出现文件即红** · 清单点名的每一处收工前用**它自己的关键词**
   回 grep 一次并**报实测命中数**——**"改毕"这个词只准由 grep 说，不准由记忆说**。
   **act / interact / brain 三册本轮零新单**（如实写，不硬凑）。
   施工图 §6 四处待用户拍板**本轮不代答**。**本轮不产任何板上证据**。
   **2026-10-16 对表（收工推送时发现并行设备已推 `bbc853d`：n1-7…n1-10 四格，批N1 十小步全收口）**——
   本轮取证基线是 `b11dd2f`，代码在两天里往前走了四格，所以 §22 的"0 命中 / 未落盘 / 未执行"必须逐处重取。
   **作废六格**：契约 **5/6 → 6/6**（`contracts/platform/` 三份 + `platform.rs:112` fail-closed 读表器 ⇒ 基-22 对象件已落，余量缩为"平台词形仍硬编码 `profile.rs:660`"）；
   专属执法位 **0 → 1 把**、待建门 **20 → 19 把**（`check.sh` 新增 `step ⑨-d 生成物一致性`＝门 f）；施工图 **Task 3 / Task 4 / Task 5 由"未执行"改判"已执行"**；
   敞口第 17 行收窄为"host 两份仍是演练实例、`esp32.toml` 已是真形态"（该表把施工图示例 `channels = 8` 按 ESP32 TRM 改判 **16**，差异登记在 commit body——**这是执行侧回表核过的好例**）。
   **继续成立五格**：`RR3_PIN_DERIVED_KEYS` 仍 **7** 键（`spi_device` 不在门 n 扫面）· `roles.toml` 仍 **4** 角色（②-a0 建即红 14 条）·
   **`daemon.rs:209` 的 `None` 臂 + `S99rrd` 只传 `--socket`/`--manifest` ⇒ L-31 出厂轨旁路一格没收，基-23 仍欠**·
   `tools/esp32-periph-base` 对 `profiles/`｜`roles.toml`｜`wiring` 仍零命中 ⇒ 基-24 未落、rtos 档仍是零消费者装饰 ·
   devices 表三条腿仍只建成一条（`contains`/`peak_ma` 的调用点全在 `#[cfg(test)]` 内）。
   **新增发现号 `实-16`**：施工图 Task 9 Step 2 预写的那行敞口**已被逐字抄进本表第 18 行并被下游消费**（`git show b11dd2f:施工图` 的 `:509` 与之比对），
   而那两句断言都是假的 ⇒ 实-15 由预言变事实，判据补上执行侧那一半：**抄进台账/契约/代码表的每一行，落笔时由执行者再跑一次该行点名的命令，写侧 + 抄侧两次回执缺一不可**。
   本表第 18 行就地订正、**保留并行设备的号**（先占者保号），本轮两条缺证据类落到 **20/21**。
   逐格实测与命令见 [全案 §22.9](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md)。
   **另一处口径并存如实登记**：远端条目写"判据实判位 **5→12**"，本轮写 **14/31**（口径＝全案 §5）——同一批代码在两个口径下报 12 与 21，
   而**仓内没有逐位清单能机判谁对** ⇒ 已立规矩"分母宣称必须带口径号"（conventions §2），逐位重取归 **基-2 / 门 b**，本轮**不猜数、不重报**。

- **2026-10-17 同日第九轮：模块化尺子（内聚 / 耦合 / 极小性）→ 全案 §23 落档**（文档轮，零 `.rs`、零 `contracts/*.toml`；代码基线 `80af1a9`）。
   前八轮的尺子依次是可推翻性 / 跨文档口径 / 运行期语义 / 端到端流与爆炸半径 / 闭合五问 / 设计承诺 ≤ 代码事实，
   **没有一轮问过"这个切分是不是最小的"**——为什么是七位供给、四段窄缝、八段描述符、六类契约、十条校验、三档板卡。
   取证方式：三路并行子代理（内聚 / 抽象充分性 / 耦合与覆盖面）+ **每条我自己复跑**；**拦下两条未复跑即拟收录的转述**
   （一条说 `DESC_KEYS` 机判是"自指、无实体"——实测 `contract.rs:1817-1838` 确有双向机判；一条说面板 option 42 个——我复跑得 2，作废不写）。
   **新发现号序列 `块-1…块-22`**（已在 conventions §6 挂号；`块-` 起号前 grep 全仓 0 命中）。**最重的两条**：
   ① **块-15（改变施工顺序）**：n1-7 把平台表变成**装载必需件**（`profile.rs:894-901` 读不到即拒 boot），
   而 `rr3-pack:324-327` 只打四册、`distro-smoke.sh:91-100` 六条 `must_file` 不含它、buildroot `post-build.sh` 一册都不装
   ⇒ **今天出的包装到板上 rrd 必然起不来，而装机验证全绿**；sim 轨把整个 `--contracts` 目录传进去，把这个洞完全遮住。
   **结论：基-25（打包与装机真值集判等）必须先于 基-23（`--profile` 升必选）**，否则"修门"这个动作本身锁死出厂机。
   ② **块-1/块-2/块-3 同根：分类没有判据，于是每个文档各自分类各自数**——同一个"契约六类"在三处指**三个互不包含**的集合
   （architecture §14 含 `README.md` 漏 `face-expressions.toml`；全案 §22.6 把不在 `contracts/` 下的 `wiring.toml` 算一类；
   `contracts/README.md` 自己在册表 9 行）；"实判位"之争（远端 12 / 本轮 14）的根因也在这——
   **31 位里代码只有 16 位带锚点**（本条首稿写"锚点实测 19 个"，同日按**双向集合差**复跑订正，逐家族对账表＝全案 **§23.6**）：缺锚 **15** 位＝`②-a0`/`②-a`/`②-b`、`①-c…①-f`、`④-a`、`⑨-a`、`⑩-a`/`⑩-b`、两个 `c1`、两个 `c2`，
   即第八轮"追认 ①-c…①-f"只追认在文档侧、**代码一个编号都没补**；反向代码还**多出 5 位无 §5 归属**（`⑥-c`/`⑦-c` 是拆分后没删的合写残名，`⑨-d`/`⑨-n` 是拟步号与校验号共用圈号字形，**`⑧-c` 被 §8 门 j、§15.2 G-3、§19 F-22 三处当校验位引用而 §5 行 ⑧ 只列 a/b**，补它即改分母 31→32，本件不自改）⇒ 新立 **基-28（文档位号集 △ 代码位号集，两个方向各自非空即红、不许折成一个标量）**，
   它比 基-2 的"每位一条反例"更便宜且更前置。
   另两条结构性：**块-4** 档位 `tier` 是**单源自证字符串**同时是三处行为的入口条件（`:949/:971/:1053`），
   全仓无第二侧对账 ⇒ 是 **L-31 的加重形**（旁路不再需要少传参数，只要填错一个字符串），修件 **基-29**；
   **块-6** "只改数据"混了两件事——在册维度内改值只改 toml，**增补一个维度**实测 **10–17 个文件 / +500～1564 行**
   （逐提交 `--numstat`），而 esp32 平台表**没有 `i2s0` 在册**、⑧-b 按词表拒 ⇒ **现役语音上行的 GPIO25/27/26 今天不可用数据表达**。
   **分母四处同批改**：待建门 **19 → 22 把**（+⑨-s 枚举单源对账 / ⑨-t 契约版本位必读 / ⑨-u 运行期绑定键表单源）、
   基座轨 **基-1…基-33**（`基-26` **有意空置不给号**）、教训类 **+L-32 过度抽象没有账**（全仓只有"缺抽象"的账，没有"多抽象"的账）。
   **开发环境口径修正**（我本机 `cargo check --target x86_64-pc-windows-msvc` 复现）：Windows 断点是**两处**不是一处——
   `rr-bus/src/client.rs:21`（`std::os::unix`，`lib.rs:11` 漏门）**以及** `rr-hal/src/esp32_uart.rs:34`
   （与 unix 无关：它无条件 import 的三枚 uart 项在 `rr-ipc/src/lib.rs:51` 被钉成 `cfg(any(linux,macos))`）
   ⇒ 拆法是两枚门 + 7 个消费点同批改，归 **基-33**；"十步门禁在 Windows 编译不过 = rr-bus 的锅"这句老口径**不完整**。
   **本轮不新增敞口行**（块-15 与 块-6 都是**缺码 + 工单号**，不是缺板上证据，按敞口表章程不许混装）；
   六册新单只有 **act A-20**（`esp32_uart.rs:8,9,75,79` 四处引用不存在的 `contracts/events/*_*.schema.json` 下划线路径），
   **其余五册本轮零新单**（如实写，不硬凑）。architecture 升 **v0.16**（§12.1/§12.2 分档判据改**两轴**、
   §12.2 首句"四类东西"订正为七、§14 契约面**不再以数命名**、§13 装载门行加基-25→基-23 顺序约束、待建门 22 把）。
   **本轮同样不产任何板上证据**；施工图 §6 四处待用户拍板**继续不代答**
- **2026-10-17 同日第十轮：决策闭合 + 依赖面极小性 → 全案 §24 落档**（文档轮，**零 `.rs`、零 `contracts/*.toml`**；基线 `ced8946`；发现号 `裁-1…裁-14`）。
  尺子换成"**待裁这件事本身闭合没有**"——前九轮审门、审对象、审口径、审运行期、审流与半径、审切分，没审过"决定"这个物件；
  本轮逐条问四句：有没有唯一登记处 / 今天能不能判 / 不裁有没有缺省态 / 裁完落点在哪。**用户本轮要求给决策意见**，故全案 §24.4 十四项各写足
  六段（事实／选项／代价／**推荐**／可判终态／不裁的缺省态），**但状态词一律未改"已裁"**，等用户拍板。
  **四条最重的**：① **决策没有唯一登记处**（新类 **L-33**，是 L-30 的决策版）：`待裁` 散在 **18 文件 / 70 处**，无处能答"悬着几个决定"，
  ADR 侧对"待评审／待定／未决"三词零命中 ⇒ 后果不是撞名而是**状态腐烂**；② **施工图 §6 第三问四轮前就裁完并落了数据却仍挂账**——
  `adc_pins` 不住在 profile 而住在 `contracts/platform/*.toml`，`raspberry-pi-4.toml:43` 与 `rk3566.toml:35` 已是显式空集、
  `platform.rs:259-262` 只有缺键才红（错误串自带"裁定②"）、四处 `is_empty()` 断言钉住 ⇒ **该问建议撤下**（裁-4）；
  ③ **能力面已抽象、依赖面未解耦**（裁-7）：`rr-hal-api` 已落盘（373 行 / 7 trait / `no_std`），可 **5/5 单元 Cargo 依赖写 `rr-hal`、
  `modules/**` 内 `use rr_hal_api` 零行**，全靠 re-export 编译 ⇒ "换板只改数据"在依赖层不成立（修件 **基-35** ＋ 新拟步 **⑨-w**）；
  ④ **第七位供给没有数据面**（裁-9）：`[[demand]]` toml **0 份**、`demand` 在 `.rs` 里 7 处**全是注释** ⇒ 校验 ⑨ 无判据对象、无读者（修件 **基-36**）。
  **门 a 实跑数**（裁-6，本人 node）：`roles.toml` 4 角色只覆盖 **4／18** normal 格；今天置红即 **22 条**（全格 25 条）＝未覆盖格 14 ＋
  `roles.iface` 缺的 trait 4（`Rr3AudioSource`/`Rr3AudioSink`/`Rr3Link`/`Rr3Display`）＋ `requires` 缺的键 4（`role`/`instance`/`direction`/`mux_group`），
  roles 侧零悬空 ⇒ 一条也消不掉；"契约层 6/6 齐"须读作**文件数齐、覆盖面 22%**，转绿件 **基-39**。
  **两处自招（复跑推翻我自己上一轮的断言）**：第八轮写"文档对 批N6/批N7 零命中"今天实测 **文档侧 9＋10 行在用、代码侧只剩 3 行注释**（裁-5）；
  第九轮 块-20 写"44 处爬路"实测 **≥3 级 54 行、剔 test 后 14 行、其中 5 处已收口 ⇒ 未收口 4 处**，而 44 恰是 **Cargo 边数**（裁-11）。
  另拦下取证代理一处误报（它称六册 §10 的"待裁"是已裁未回写，经核 D3/D5/D6/D7/D9 确实未裁）⇒ **没写进文档**。
  **分母**：待建门 **22→23 把**（＋⑨-w）、基座轨 **基-1…基-39**（＋基-34…基-39，`基-26` 仍空置）、教训类 **＋L-33**、新前缀 **`裁-*`**（起号前 grep 0 命中）。
  **本轮不新增敞口行**（裁-2 是表头分类错，已就地订正为全枚举；裁-4 是撤一条已裁项；其余缺口全部落在 基-34…基-39 与 ⑨-w 上＝缺码缺门，不缺板上证据）。
  **零新单元工单**（缺口都在基座轨，不借单元号——L-29 纪律第二次兑现）。。

- **2026-10-17 拍板轮：全案 §24.4 十六项决策全裁 ＋ 批N2–N5 开发计划落档**（文档轮，零 `.rs`、零 `contracts/*.toml`）：
  用户裁定 A 组四项（施工图 §6 四问）＋ B 组六项（D3/D5/D6/D7/D8/D9）＋ C 组六项（结构决定）**共十六项全部按推荐**——
  裁定记录唯一落点 = [批N2至批N5开发计划与第十轮决策拍板.md](process/dev/0.0.5增补轮六——批N2至批N5开发计划与第十轮决策拍板.md) §2；
  批N2–N5 施工顺序（第 0 步 基-34 未决登记处建表 → 第 0.5 步 基-35＋⑨-w / 基-32 / 基-37 → 批N2 能源 → 批N3 执行件 →
  批N4 面板 → 批N5 生产运维）见同件 §3，各批范围原文仍以全案 §9/§10/§17 为准。敞口表无增减；
  D3/D5/D6/D7/D9 状态词改"已裁"随第 0 步施工同批落。

- **2026-10-17 第 0 步施工：决策拍板落档**（文档轮，零 `.rs`、零 `contracts/*.toml`，提交 `6bffed2`）：
  新建 [decisions.md](process/decisions.md)（**基-34 未决对象唯一登记处**，L-33 的收口件，六列必填；首批 = 十六项已裁 + 1 条在册未决）；
  施工图 §6 / 本表敞口段 / 索引 §8 三处"待裁"改**引用不复制**（各加指针指向 decisions.md §2）；
  **裁定 1 落地**：批N6/批N7 正式登记（conventions §6 批次行 / 全案 §9 / 索引 §14.6 / docs/README 四处全枚举），
  "批N1–N5"区间写法（**现行范围表述类**）改全枚举——终态核对：`grep -rn "N1–N5" docs/` 余量 13 处全部属白名单
  （判据自指：decisions.md 裁定行、施工图裁定行与取证数据、计划终态定义、conventions 台账；历史保留：全案 X-3 教训行与 §24.4 正文；
  被作废注记覆盖：施工图 §4 首条验收项 / §5"新增批次号"行 / Task 1 收编落字，本体按历史保留、指针在 §6-① 裁定行）；
  D3/D5/D6/D7/D9 状态词改"已裁"（D3 值更新、D6 值反转、D9 转自动可判）。敞口表无增减。

- **2026-10-18 批N2 开工·首件：基-36 demands 最小集**（裁定 13"只交一件"；申报面＋判据实体＋反例三层同批，
  详录 [conventions §2](process/conventions.md) 同日条目）：①申报面定形 `[assemble].demands = [{ rail, peak_ma, critical }]`
  （rr-registry `assemble.rs` 条目级解析件级专用、fail-closed 逐条点名；缺键 = 空申报合法，"kind=power 未申报 → 红"
  执法面归门 c；增键不 bump——17 份已激活数据语义不变）；②判据实体 `rr3_demand_budget_check` 落
  `rrd/src/profile.rs`（`Σ demands.peak_ma (critical=true) ≤ domains[rail].current_cap_ma`）；③反例 #[test]
  进 ①b（正例＋超 cap 红＋ghost rail 红＋critical=false 不进 Σ）；首条真实声明落 servo 件
  （`rail5v peak_ma=700 critical=true`——act.axis.pwm 是全仓唯一 kind=power 角色）。**三层诚实登记（实-14）**：
  ①②落、**③专属执法步 = ⑨-c/门 c 仍待建**——判据不接装载拒链，可数 step 13 行、待建门 21 把**计数均不变**；
  裁-9 推翻条件（"写了 demands 且 ⑨ 在跑"）未满足。`contracts/*.toml`、`check.sh` 零改动；敞口表无增减。

- **2026-10-18 批N2 件一：⑨-c 门 c 接链（校验⑨ 装载拒链）**：①申报面升五键定形（ADR-0021 裁定 2——
  `sustained_ma` 正整数且 ≤ peak（物理矛盾 fail-closed）、`concurrency_group` 缺键即红（T-5 同 no_injection
  范式，显式 `[]` = 不分组 = 最坏口径合法）；servo 申报升 `rail5v 700/120/true/[]`）；②判据接 boot 链
  致命汇合点——`rr3_demand_declared_expand` per-instance 扩算（实例数 = 角色 iface ∈ 件 handles.trait 且
  slot 绑本模块的 pin 数，归因打空地板 1）＋`rr3_demand_budget_check` 并发口径（恰一组归属 = 组内互斥取
  max，空组/多组归属按不收窄计——F-12 收窄必须是数据）＋`rr3_demand_kind_power_gaps`（kind=power pin
  挂轨而绑定模块未申报 ⇒ 红）；③专属执法步 `check.sh` 建 `step "⑨-c 电流预算装载拒链"`（显式过滤器
  `demand_budget` 封 fail-open，4 用例）。反例组扩：并发三态（同组互斥绿 / 多组不收窄 2100 红）＋装载
  拒链三用例 `demand_budget_chain_*`（三件叠加 2100 红 / 未申报红 / 同布局组收窄 1400 绿）；
  **可数 step 13→14 行、专属执法位 3→4 把、待建门 21→20 把**（conventions §2 对表段 / architecture §14 /
  全案 §16.4 同批）；decisions.md 基-36 行回填首件提交号 `bcc13fd`、裁-9 推翻条件满足；
  `contracts/*.toml` 零改动、敞口表无增减。

- **2026-10-18 批N2 件二：能源能力面 Rr3Rail ＋ 两格一主题（ADR-0021 裁定 2，契约三册同批扩格）**：
  ①`rr-hal-api` 立 `Rr3Rail` trait 三方法（`rr3_read`/`rr3_current_ma`/`rr3_switch`——F-32 撤名收口，
  `Rr3Power` 保留不动）＋ `Rr3RailState{mv,tier,age_ms}` ＋ `Rr3RailTier` 四态含 `Unknown`
  （T-6/P-15：`age_ms > max_age_ms` 或时基取不到 ⇒ Unknown；执行面按 Severe 处置、展示面原样显示，
  执法归⑥-c2 件三，契约层只供数）；占位后端显式 `Unsupported` 不静默退化，rr-hal re-export 三名。
  ②`slots.toml` schema_version 2→3、基础层 append `power.rail`（域电平观测，bare 档经守护输出行
  不发主题）/`power.budget`（能源预算仲裁），三档全在、`power.battery` 保留不动；`topics.toml`
  30→31 条 append `rr3-power/rail/state` event L2（消费走 trait 本地决策不经总线，同 battery 语义）；
  base_capabilities 5→7 等价面同步；五处形状 pin 同批（rr-registry `slots.rs`/`contract.rs`/
  `topics.rs`＋rrd `topics_load.rs` 三处）＋atom 承担账两格进 `BUILTIN_NO_ATOM`（机制切片承载、
  无原子目录，D15 防空壳）。③本件不建执法步（check.sh 零改动、可数 step 14 行/待建门 20 把不变），
  角色覆盖扩表 4→18 归件六基-39；敞口表无增减。详录 [conventions §2](process/conventions.md) 同日条目。

- **2026-10-18 批N2 件三：校验⑥-c2 运行期判据实体 ＋ D3 无器件合法态（decisions.md 裁定 5）**：
  ①`rr-hal-api` 落判据实体 `rr3_rail_tier_effective(tier, age_ms: Option<u32>, max_age_ms)`——
  `age > max_age_ms` 或时基取不到（`None`，零新类型）⇒ 折 `Unknown`、边界 `age == max` 不折；
  红例五断言（新鲜原样/边界不折/超龄 Ok⇒Unknown/超龄 Severe⇒Unknown 陈旧不许冒充新鲜/None⇒Unknown），
  rr-hal re-export 补函数名——单真值点，产出侧与消费侧共用不许手抄判据。②D3 落地 rrd `profile.rs`：
  `[slots].device_absent` 表（格名 → 非空理由）＋三查 fail-closed（在册 / 必须 base 层格——非 base
  格缺席表达＝不进 present，第二套语法即红 F-2 同型 / 理由非空）；`present` 语义不动、schema_version
  = 3 不动（可选键 append 非破坏性）。③测试：rrd 正例一绿（power.battery 带理由登记 boot 过、不进
  缺格判定）＋三红一组＋空 map 断言，rr-hal-api 红例进 lib 测试；**本件不建 check.sh 执法步**
  （可数 step 14 行/待建门 20 把不变）。④decisions.md D3 行施工注记同批（提交号随批N2 收口对账
  回填）；运行期对照（"无器件 vs 坏了"分辨）归件五/批N4；敞口表无增减。

- **2026-10-18 批N2 件四：校验⑩ 内存预算——[supplies.budget] 分母 ＋ [assemble] 分子 ＋ schema 4**：
  ①分母 `[supplies.budget]` 四键（`not_applicable`/`task_stack_budget_bytes`/`heap_total_bytes`/
  `heap_reserved_bytes`，ADR-0021 修订 3 三份数据三个主人）＋分档自洽 parse 层裁（host ⇒ 显式真且
  数字键不得并存；rtos/bare ⇒ 必假且三数键必填、`total ≤ reserved` 红；段缺点名"不许留空"——F-6）；
  `schema_version` 3→4（破坏性变更，批N1 `[[supplies.domain]]` 先例；supplies 段缺不级联）。②分子
  `[assemble]` 新增 `task_stack_bytes`/`heap_peak_bytes` 两键（**件级语义非每实例**——双轴共享任务栈
  直加不扩算，与 ⑨ 每实例扩算相反；负数红；缺省 0 = 未申报不进 Σ，申报义务未触发归 0.0.7 批G/
  批D1，红例合成申报防 L-24 假绿；组合件级点名即红）。③判据实体 `rr3_memory_budget_check` ＋装载
  拒链（坏形件不进加总——可见面 = assemble_broken 点名，同 ⑨ 诚实注记）；红文案点名 ⑩-a/⑩-b 与
  F-13（渲染缓冲/队列/板侧配置全是堆账）。④数据：default/distro（host）显式 `not_applicable = true`；
  esp32-mini（rtos）声明预算位 65536/327680/16384（board.rs 任务栈同源 + S3 内部 SRAM 推算 +
  `UART2_TX_RING` 首位落账，对账归 0.0.7 真板）；四份 e2e 夹具 + tools/sim + wiring 路径推导夹具
  同批 schema 4。⑤测试：rrd 六条新用例（实体绿/红含等号边界、分档自洽七红、两超限拒 boot、
  只超堆断言不含 ⑩-a、边界和过 boot、双件直加）＋ rr-registry 两新＋ contract.rs 三处形状 pin
  同批 3→4；⑩ 经步①b cargo test --workspace 进 CI，**不建 check.sh 新步**（计数均不变）。
  ⑥两腿对拍用例落 rr3-power composite.rs（⑥-c2 真值面：腿一确定性 + 腿二裁定语义逐条点名：
  None⇒Unknown、边界不折、档名 "unknown" 同源、Unknown 独立第四态不折 Ok——power 非 atom 名
  不占 ⑧-a 分母）；全案 §5 ⑩ 行 X-4 加注、decisions.md §4 变更记录同批（提交号随批N2 收口
  对账回填）；敞口表无增减。

- **2026-10-18 批N2 件五：RailMonitor/RailJudge 接能力面——机制不动、归属搬家**：
  ①选择入口 `rr3_hal_rail()`（rr-hal backend.rs，与 `rr3_hal_power` 同族）今日全后端
  占位 fail-closed（`Rr3UnavailableSource`，使用时 Unsupported 显式回错）——板侧
  RailMonitor/RailJudge 机制本体 esp32-periph-base 真跑零改动；宿主真件（INA219）归
  0.0.7（敞口表 12 行）。②rr3-power 接面：config `[rr3-power.rail]` 段必填＋全键必填
  （up=false 惰性但显式写——F-6；`severe < mild` 严格校验相等也红；`max_age_ms = 0`
  合法＝恒 Unknown 诚实语义）；主循环第 4 拍（up=true 才跑：读域压 → ⑥-c2 折档 →
  判定核 → `rail/state` 四键流 `{seq, mv, age_ms, tier}`＋越阈首拍边沿守护行）；
  up=true 探活与周期读数失败均 exit(1)（fail-closed，counter `rail_read_fail` 先行）；
  module.toml publishes 加 `rr3-power/rail/state`（topics.toml 件二已在册）；仓
  config.toml 与 e2e_power 夹具同形状 up=false（rail5v 域 4600/4400/0 同源）。③机制
  切片归属搬家：`rr3_rail_tier_of`（纯 mV 比较）＋`Rr3RailJudge`（双 Rr3LowEdge 一核
  两条线，event ⇔ 向下档变）＋`rr3_rail_beat`（⑥-c2 消费执法点：Unknown 不喂核原样
  透传）逐字对照板侧 power.rs 迁组合件层。④测试九条（config 五向含倒挂相等红、
  body 四键无加戏、判定核序列对照板语义、两腿对拍、⑥-c2 执法两条各一
  `rail_beat_unknown_not_refed_and_passthrough`）；rr-hal/rrd/rr3-power cargo test
  全绿，**不建 check.sh 新步**（可数 step 14 行、待建门 20 把计数均不变）。⑤
  decisions.md §4 变更记录同批（提交 `53e243a`，批N2 收口对账 2026-10-18）；rr3-power 模块设计 P-9
  行 X-4 加注。⑥板侧 power.rs 零改动；敞口表无增减。

- **2026-10-18 批N2 件六：⑨-a 门 a 报告模式 ＋ 基-39 N2 那一刀角色扩表（decisions.md §2
  施工图§6-④ 裁定）**：①`contracts/roles.toml` append 两角色 power.rail.sense/
  power.current.sense（slot=power.rail、iface=Rr3Power、requires=controller+addr、
  per_instance=true、droppable=false——形状与批N3 归属注记见 conventions §2 件六条目；
  covered normal 格 4→5，校验②不涉——power.rail 是 base 层格不进 enabled_slots∩present）；
  ②`check.sh` 建 `step "⑨-a 基-39 角色覆盖六表对账·报告模式"`——报告三段 A（未覆盖
  normal 格）/B（roles 四外键悬空，应恒 0、硬红不进基线）/C（TRAITS·PIN_KEYS 差集），
  红账＝A+B+C 对低水位基线 `tools/ci/gate-a-baseline.txt`（初值 23＝15+4+4，配套 c
  "红条数单调不增"：count>基线即红、文件缺/坏即红 fail-closed、count 下降打印须同批下调；
  配套 a＝报告走 stdout 可见面）；键集四腿全部自代码常量导出不抄第二份（⑨-n 先例，
  `RR3_PIN_KEYS` 声明数/实解析数判等）；③rr-registry roles.rs 契约测试同批 4→6。
  **三事实源同批对表：可数 step 14→15 行、专属执法位 4 把不变＋报告位 1 把、待建门
  20→19 把**（门 a 转入"已建·报告模式"，转红随基-39 全刀落完——switch/charger 归批N3、
  logic 内建格覆盖口径归转红收口件另裁）；decisions.md §2 施工图§6-④ 行施工注记＋§4
  变更记录同批（提交 `e918b76`，批N2 收口对账 2026-10-18）；contracts/README 演进登记同批；敞口表无增减。

- **2026-10-19 批N3 件一～件五：执行件契约面＋档案装载＋拒动＋标定写入＋行程收敛（⑨-e 门 g 建成）**：
  ①件一 `c5220f5` rr-hal-api 落 `Rr3Actuator`/`Rr3Position{source}`/`Rr3ActProfile` 契约面＋`Rr3Store`
  窄缝＋句柄面 trait 在册扩名；②件二 `a5bf8ea` rrd 档案装载器 `hardware/<形态>/axis/*.toml`
  （外键对账＋原子写＋坏件不覆盖）＋判据用例；③件三 `be8ba8c` 拒动硬约束——闸门复位保守默认＋
  motion 词面＋判据两路径接线＋rrd 轴状态注入；④件四 `0df6969` 标定路线 A/C 写入面 `axis_write`
  （② bin 薄壳与③手册回写挂账未完）；⑤件五行程三处收敛（② host 撤 config 行程键＋注入面五键扩形
  `bfe3505` / ③ 板侧撤手写行程字面量改 `axis_gen.rs` 生成表＋esp32-mini 双轴档案＋profile 补 tilt pin
  `ee18519` / ④ `check.sh` 建 `step "⑨-e 行程真值单点…"` `af61210`）。**三事实源同批对表：可数 step
  15→16 行、专属执法位 4→5 把、待建门 19→18 把**（门 g 转入"已建"——四字面全仓零命中 fail-closed，
  注释/断言提及同红；行程唯一真值点 = 轴档案 center/lo/hi，host 注入/板侧生成表两路同源）；
  conventions §2 对表段＋architecture §14 门表 ⑨-e 行"待建"改"已建"（相邻 ⑨-d 行同批订正）＋
  全案 §16.4 待建门链＋基-5 行施工回执（两处索引）＋act 册 §0/§3 行程口径与 A-13/A-17 状态同批；
  敞口表无增减（brain 档位角 B-14② 当日已评估改判批N4——落档 `e07bbe7`，注记见 brain 册 B-14 行；挂独立 todo 不占敞口行）。

- **2026-10-19 批N3 件六～件九：SPI 词形拆格＋D5 up 语义落码＋计量格补课＋收口对账**：
  ⑥件六 `6953222` ui.face SPI 词形拆格——`profile.rs` 派生臂（`spidev<N>` 词形＋channel 片选号，
  与旧 `[hardware]` 直给逐字节对齐）＋`RR3_PIN_DERIVED_KEYS` 7→8＋roles 新角色 `ui.face.spi`＋
  profiles 两份 `[hardware].spi_device` 撤除迁移 `[[pin]]`＋CI contract/roles 判据同批＋gate-a
  基线 23→21 同批下调＋architecture 三处对表——**敞口 18 行结项**；⑦件七 `4e77e1f` D5 up 语义
  定案落码——`contracts/events/connect-net-link.schema.json` 补齐（§5 点名的缺 schema：up 只表
  链路层已关联不表可出网、出网性由 `net.online` 另证、AP 态恒 `up=false`、`up=false ⇒
  ssid/rssi_dbm` 必 null 红线进 allOf）＋`cloud/session` 未就绪文案按配网态分流（AP 态点名
  `mode=ap`，不折叠 `up=false`；station 原词保留）＋topics.toml note＋connect 册五处对表——
  connect 册 N-4 随结；⑧件八 `3025205`＋`5018e20` charger/env/imu 计量格补课——rr-hal-api
  五型契约面（Rr3SampleKind/Rr3SampleQuality/Rr3Sample/Rr3Sensor/Rr3DigitalIn）＋assemble
  traits 7→9＋roles 三计量角色（derives=[] 显式空语义）＋profile `device` 键（RR3_PIN_KEYS 11）
  ＋③-c 共址精化（同 (bus,addr) 须同 device 且型号在册）＋devices.toml SHTC3/ICM-42688 进场
  （现役六器件）＋profiles 两份 charger/env/imu 三 pin 补 device＋architecture 四处对表；附加
  评估 `e07bbe7` B-14② brain 档位角派生改判批N4（语义位进 payload 属契约级变更／通道与
  A-13③ 同链未立／映射待裁定，三理由落 brain 册 B-14 行）。**四项推荐代行待追认**（AskUserQuestion
  四问用户跳过，按全案推荐项代行施工、事实已在案，追认前不改裁决力）：charger 形状＝meter
  采样面（件八①）；env/imu 深度＝器件在册＋契约面先行、真件探测归器件批（件八①，D9 同族）；
  家族E 立格＝charger/env/imu 三计量角色进场（件八①）；D5＝up 只表链路关联、出网性
  `net.online` 另证（件七，decisions §2 D5 行施工注记同批）。**三事实源同批对表：红账 21 与
  基线持平**（A=13＋B=0＋C=8＝trait 差 4＋pin 键差 4；roles 现役 10 角色、devices 现役 6 器件、
  RR3_ASSEMBLE_HANDLE_TRAITS=9、RR3_PIN_KEYS=11）；decisions §2 行 2＋D5 行施工注记、
  conventions §2 批N3 条目、敞口 18 行结项＋19 行对表注记＋:90 缺裁定类清零注记、本行同批
  （件九收口对账，本提交，2026-10-19）。

**0.0.6 版本批次全景（修订版 6 口径）与在制品**：

- ~~**0.0.6 阶段一收尾件（批E'）**~~：**已落库并真板复验结清（`a5b3093` + `f2597d5`）**——boots 三件修复（unit 注入 `RR3_DATA_DIR` + postinst 建目录置属主 + distro-smoke 断言补 ⑧"counters 回写"）+ boards §7 补录 ctl socket root/sudo 现状；板上 restart 落盘 `{"boots":1}`、reboot 自恢复 `{"boots":2}` 跨重启累积成立；ctl socket 加组放行牵动 ADR-0010 权限模型，归阶段二小批（同批核对 unit 补注 `RR3_REBOOT_OK`）；
- ~~**0.0.6 阶段二（批F1–F4 + 批F5a/F5b，修订版 6 重排）**~~：**已全部收官**——批F1 板上验证四项（boards §8）、纪律小批 `b81c986`、批F2/F3 sim 段、批F4 板上 `rr3-sim --chain` 全链 mock 闭环（boards §9，判据 10 sim 档过）、批F5a 五件 HAL linux 真后端、批F5b 运维链 R12 两件（`fab7b6c`，ADR-0010 修订 1）；
- ~~**板上线后待补**~~：**已收官（2026-10-15，boards §11 回执）**——板上 `cargo test -p rr-hal` **79 passed / 0 failed**（板上 rustc 1.85.1；宿主 50 + 29 个 linux-only 真后端用例）+ 判据 13/14/15 真机验收全过（断链 30s 自愈三断言 / SIGSTOP+SIGKILL 两路判死重启 / 断电恢复 boots 累积 + 真 reboot 两档行为相反各自成立）+ socket 放行真机复验结清（非 root 巡检免 sudo，§7 复验状态划线）+ 链④真件档首件 MAX7219 显示收口（linux spidev 后端 + descriptor `spi_device` 十键批，interact 进程 `fd -> /dev/spidev0.0` 硬证据）；判据 12（真电池）仍随电池组采购，链④其余真件（真麦 DoA / 真舵机）随采购逐件验收；
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
② `tier_scope` 与 ⑦-c 的件名 `if` 一起数据化（L-2）——"门只读数据"此前只对 ⑧-a 成立（**已结：批M MC-10 L-2**，⑧-a 读 status+tier_scope、⑦-c/⑦-d 按 `module.toml` `lang` 数据位化）；
③ 已收口批次**不重开**，余数另立 **0.0.5 批L（L-1…L-10）**，六册逐条回指，一个余数只有一个落点；
④ `rr-hal` 选择器仍返 `Box<dyn>` 判为**正确落位**（契约面才禁 dyn，承载层允许 std 形状）；
⑤ ⑦-a 文件清单三处不一致按"脚本为准"回抄 `conventions.md` 与 `tools/ci/README.md` 并补自指检查（**已结：批M MC-10 L-4**，`check.sh` ⑦-e 自指判等落码——扫面名集由 ⑦-a 数组归一，文档两处 ⑦ 行与 xtensa 门目标集同判）；
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
  battery/charger/bt 三份已填，错因是用单行 grep 读多行 TOML 数组（**L-5 本体已结：批M MC-10 L-5，2026-10-13——14 份补齐/明写 `no_injection`，`rr-registry` 形状级空面规则，门认声明**）；② power P-2 被我记成"后半未做"——
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
- **0.0.5 增补轮五 批D 采集冒烟已过（2026-10-13，真板）**：INMP441 ×2 双麦 @ I2S0
  （BCLK=25/WS=26/SD=27，L/R 时隙复用——甲 GND 乙 3V3）接入 [src/mic.rs](../tools/esp32-periph-base/src/mic.rs)
  冒烟面（philips 16kHz 32bit stereo，16KB 线程纪律）。真板实录：实测 ~16.6kHz（I2S
  时钟分频精度面，wav 头按实测写）；静音 rms 本底 ~34k/24bit，拍手+说话 A/B 分段
  rms 抬至 45–68k、peak 冲 123k——幅度响应实锤；L==R ≤1%（双通道独立）。两处排障
  实录在案：**`rx_enable()` 必须显式调**（esp-idf-hal 0.44.1 i2s.rs L803，驱动 new 后
  不自动 enable，缺它 read() 永挂无 Err 无统计行）；外设 token/引脚 cross-thread move
  需显式 `+ Send` bound（编译器实爆驱动补，非 unsafe impl）。余口：peak 低位 0x…0FF
  结构化触顶疑 1bit 时隙错位（留 wav 真录音判据核）；921600 参数化 + 音频主题上行 +
  宿主 wav 落盘 + DoA 出角 = 批D 后续小步。接线表落 [README 批D 节](../tools/esp32-periph-base/README.md)。
- **0.0.5 增补轮五 批D②/D③ 音频主题上行收官——判据① 过 + 双根因排障（2026-10-13，提交
  `f947ddb` + `c5d318c` + 修复批 `dd1e55a`）**：D② 921600 参数化（宿主 transport/桥构造面
  `_with_baud` + `RR3_HAL_PERIPH_BAUD` env，缺省仍 115200 向后兼容；Q1=(a) 带宽账：7.8 片/s
  ≈87KB/s 落 921600 有效 92.16KB/s 内，板侧 `UART2_BAUD_HZ` 烧录即 deliberate）；D③ 板侧
  `AUDIO ON` 起 mic 线每 2048 帧（128ms @16k）逐通道减本片 DC → 24bit 截 16bit s16le →
  b64 → JSON 体（`rr3-sense/audio/pcm` 在册主题，形状对齐 rr3-sense `rr3_pcm_event_body`）
  → Event 帧整帧入有界 audio_q（4 深，满丢**最旧**计 `audio_dropped`）→ 控制面 drain 整帧
  写 UART2；宿主工具 [rr3-audio-rec](../tools/rr3-audio-rec)（单测 4/4）收流解码落 wav
  （**wav 头按实测率写**——I2S 实际率与标称差，按实测写回放音高才对）。**判据① 真板实录过**：
  15s 收流 117 片 seq 连续 gaps=0 err=0（实测 16138Hz / 14.85s / 958KB），wav 双通道 16bit
  L==R 仅 1.65%（非单声道假象）零饱和，板面 `drop=0 q=0` 对账一致。排障双根因实录：
  ① **CP2102 三线物理断线**——0001 全静默用双判别探针实锤（0001 在 921600/115200 双判别档
  0 字节 + 1410 控制台完整 boot 对照），重插即愈，行/帧/音频三面全通；② **dev 缺省
  opt-level=0 下组帧 680ms/片 → I2S DMA 环溢出丢数**——分阶段计时插桩一烧定案（pcm≈280/
  b64≈200/json≈150ms，memcpy 面 body/frame 仅 1-3ms：全段无病态，纯 -O0 逐元素堆写慢两个
  数量级；此前"b64/JSON ms 级"的排除按 -O3 直觉估算全不成立），[mic] 16.5kHz→2.8kHz 崩塌
  与"128ms 音频 + 680ms 组帧 = 2.5kHz"精确自洽。修复：Cargo.toml dev profile 提
  **opt-level=2**（门禁构建=烧录产物同位形，根治"-O0 不代表真板性能"失真）+
  `UART2_TX_RING=16KB` TX 环（[board.rs](../tools/esp32-periph-base/src/board.rs) 策略面；
  缺省 256B 下整帧写 11.4KB 同步等线速排空 ~120ms/帧，≥ 单帧后 write 降 memcpy 入环异步
  排水），实测组帧 **680→11.7ms（58×）**、AUDIO ON 期间 [mic] 稳定 16.2kHz；[mic] 统计窗
  加 `build=` 组帧耗时面（批D④ 稳态观测沿用）。rail_tier=severe 系 GPIO34 浮空伪读数
  （未接闸，预期内）；判据②③（10 分钟稳态 / DoA 出角）= 批D④⑤。
- **0.0.5 增补轮五 批D④ 600s 稳态收口——判据② 被宿主侧 CP2102 硬件链路阻塞，等 FT232
  采购（2026-10-14，修复批 `61743f0`）**：①宿主侧落盘线程化（mpsc 通道 + 写线程独占
  wav，读循环**零磁盘 I/O**；`WatchState` 双真相——`ingested` 供看门狗判停滞 / `disk_bytes`
  供恢复头 data_len；恢复头三路全走 `write_at` 位置写）+ 重连骑行窗加码
  `HANDSHAKE_TRIES` 3→10（~90s）。②五轮真板实录定案：冻结样本 **38 / 51 / 184 / 310 /
  369s（/dev/null）**横跨固件版本（板面旧 `a83c9f8-dirty` 乌龙查 boot banner 破案，重烧
  `61743f0` 干净树后仍冻）与落盘配置（/dev/null 决定性实验**证伪 flush 根因论**——磁盘
  I/O 只是 ~7× 放大器非触发器）；板面五轮洗冤（`seq=sent`、`drop=0 q=0`、重试期无 boot
  banner 复现、CH340 控制台同挂单 XHCI 全程健康）→ 定性 = **宿主侧 CP2102@921600 RX
  路径楔死**（板→宿主字节全无；宿主→板 TX 间歇存活——run#5 重试期板面 ×N ~6-7s 振荡
  即每轮 handshake 防御 OFF+ON 间歇到达板面所致；渐变丢字节先于硬冻结：run#5 冻结前
  gaps=7 / 撕裂 18 帧重同步救回）；自愈 >92s（10 轮重试窗全败）≤11min（历史观测）。
  ③硬件手段逐项排除：CH340 **板载焊死 UART0** 互换不可行；降档 460800 被 Q1=(a) 带宽账
  否定（46KB/s < 87KB/s 必背压丢片）；本机**单 XHCI 控制器**（`AppleUSBXHCIWPT`）换口
  无信息量。④处置：**FT232 系适配器采购中**，到货后接 GPIO18/19 收流位重跑 d4（600s
  gaps=0 + INFO 对账 + wav 落盘）→ d5 DoA 冒烟（`--secs 40 --doa`；拍手时间表 0–8s 左侧
  / 10–18s 右侧，启动前发 "d5 开始"）→ RAM 三点收口。RAM 点1/点2 已随 run#5 到手
  （boot 行 02:07:18 + INFO `free=191072B min_free=105136B`）。
- **0.0.5 增补轮五 批D④ 续——FT232 接入首跑病害根因定案：GPIO18 驱动档不足（CAP_3 修复）+ 收流落 wav 证据面复测过（2026-10-16，提交
  `d5d9398`+`158bc4f`+`055cf48`+`63212c7`）**：FT232 到货接 GPIO18/19 收流位后 921600 抓流帧头仍脏——trace_id/reserved
  长零区收成 0xff 族 + payload 头 212B 冻结垃圾，payload 尾 b64 完好；损坏按字节数定界不按时间（921600/460800 对照同窗口）=
  内容依赖（长零串）。三重证据定案**第三根因 = GPIO18 默认驱动档 CAP_2（≈20mA）推不动长跳线 + FT232 接收链路——长零串（线上
  持续低电平）期间电平被耦合/漏电抬高，接收端把 0 读成 1**：① 板面片内 TX→RX 环回探针（`uart_set_loop_back`，2KB 四段图案
  长零串/'A'/递增/0xFF，64B 交叉写读防 RX FIFO 溢出）逐字节全对上——芯片内排除；② mic.rs 改**自定位图案探针**（L=seq 帧恒定
  / R=帧内样本序号，任何损坏字节自带坐标）后 CAP_3 拉满抓流 27 完整帧**位级全对**（payload 整体逐字节 + PCM 图案逐样本
  L==seq/R==0..2047 核验，seq 连续无断）；③ 边沿密集内容（'A' 段/递增段）全程完好。修复 = `gpio_set_drive_capability(GPIO18,
  CAP_3)`（≈40mA）一行转正（[main.rs](../tools/esp32-periph-base/src/main.rs) 定案注释在位），与 I2C 被迫 50k 同族（长跳线
  电气边际）但驱动档即治；临时面（环回探针/460800 对照/图案探针）定案即撤。宿主证据面复测过：图案流 wav（chunks=78 gaps=0
  err=0 实测 16010Hz / 9.98s）+ 真声流 wav（chunks=94 gaps=0 err=0 resyncs=0 **实测 15995Hz / 12.04s / 770KB**，双通道
  RMS≈74 峰值 369 = 安静室底噪；`rr3-audio-real-mic.wav` / `rr3-audio-pattern-probe.wav` 随收流落盘不入库）。过程实录一段
  宿主侧 FT232 瞬态噪声插字（一窗 err=186 resyncs=62 但 seq 零丢失、wav 无损——同时刻原始抓流 46/46 帧净对表，噪声字节穿插
  帧间非换帧，三级防护重同步按设计护住对齐），与 d4 已档宿主侧 RX 异态自愈同族，非板面回归。正式判据②（600s 稳态）与判据③
  （DoA 拍手）待跑。
- **0.0.5 增补轮五 批D④ 续 2——收流线程化改造（reader/decoder split）+ 判据② run3/run4 判读（2026-10-18，提交
  `a0126d8`+`292cb76`）**：分相读病灶定案后 `rr3-audio-rec` 收流线程化——读线程只做大块裸读
  （poll+read 64KB，A/B 已证该节奏干净）推裸字节通道，解码主循环 `FrameAssembler` 纯内存拆帧
  （magic 对齐 + `Rr3Header::from_bytes` 单点语义走 rr-ipc + plen 1MiB 护栏 + 坏帧弃 4B 续扫零新 I/O），
  断流重连搬进读线程（见过数据后 1.6s 零读 → 重开 fd ×10 + 防御停流握手），9 单测守恒。
  **run3（改造后首跑）**：起流 17.4s 零 ReservedNotZero（分相读病灶拔除实锤），随后板→宿主 RX 全盲
  （重连 10 次"3000ms 无字节"全败，exit 4 fail-honest 收兵）；INFO 对账钉死宿主→板 TX 畅通（seq 算术证明
  板收到重连发的 AUDIO OFF 停流）、板面健康（seq 连续 drop=0），CP2102 同款 RX 冻结形状在 FT232 上 17s 即现、
  自愈超 90s 重连预算；15s 冒烟复跑满速零瑕疵 = 链路自愈实锤。**B 类握手竞态（物理层排查冒烟 ×3 抓获）**：
  板 `OK audio on` 应答与首帧字节背靠背进线，读窗口先抓到 seq 帧的 b64 payload 再扫得 OK 同行——
  "首行必须 OK 开头"把合法起流误判为拒收；修复 = ON 应答判据放宽（3s 预算内逐行扫 `OK audio on` 全串，
  防 b64 字符表 O/K 裸撞，`ok_reply_line` 纯函数 + 单测），修复后冒烟 3/3 干净。**run4（判据② 首次完整跑）**：
  588s 连续收流无断流无失步（reconnects=0 resyncs=0 restarts=0，4594 片），但 gaps=92（自 chunk=88/11s 起
  散布，占总片 2%）+ err=46 载荷级孤立滑移——**判据② 未过（口径 gaps=0）**，链路定性"可用但有损"：
  断流型病灶（A/B/分相读）已全数拔除，余量为低频散布丢片（长跑才现，15s 冒烟不可见，与 d4 已档宿主侧
  FT232 瞬态噪声同族嫌疑）。wav 落盘在位（`rr3-audio-600s-run4.wav` 37.6MB 实测 16002Hz）。判据② 重跑
  与判据③（DoA 拍手）仍待跑；功能开发（brain 语音交流 / DoA 听声识位）按"可用但有损"口径可并行开工，
  不以判据② pass 为前置。
- **A 批（语音自由交流）c1–c5——板麦进城四步上提 + esp32 音频源后端 + 板麦档与 runbook（2026-10-18，提交
  `1d00cef`+`5cb7f98`+`5dce1cd`+`406b1ef`+本批记）**：立项计划
  .trae/documents/A语音自由交流实施计划.md（用户拍板"先 A 后 B"），M4 四进程闭环已在
  （brain 对话编排 ASR→LLM→TTS→tts_pcm→interact 播放），唯一缺口 = 板侧麦克风流进总线的承载
  （rr-hal 音频源仅 linux/sim/mac 三档，esp32 静默落 sim）——五步收口：
  ①c1 `rr_ipc::framing::FrameAssembler` 拆帧器上提单点（rr3-audio-rec 改用，四类测试随迁）；
  ②c2 `rr_ipc::audio_session` 板面会话握手上提单点（read_reply_line/ok_reply_line/共享常量，
  B 类 ON 应答判据修复随迁）；③c3 **核心**：rr-hal `esp32audio.rs` `Rr3Esp32UartAudioSource`
  （`RR3_HAL_PERIPH_UART/BAUD` 开口 → 握手 → 读线程 64KB 裸读 → FrameAssembler 四道门 →
  Mutex<VecDeque> 环形缓冲 ~1s 溢丢最旧；断流重开口重握手永不退；backend.rs 两选择函数补
  esp32/sim 显式臂（E0004 裁定 `&str` match 无法穷举，`_` 臂保留为防御性落占位，白名单收敛
  `rr3_hal_backend_resolve` 单点），esp32 sink 落 placeholder——板无扬声器，fail-closed）；④c4 rr3-sim `--sink-backend`
  （源/回放分列，`--audio-backend esp32` 无 `--sink-backend` 拒收 + `--sink-backend esp32` 拒收
  两条 fail-closed；interact 进程 env 单独透传，既有 sim/mac 链零改动）；⑤c5
  `modules/rr3-sense/config-esp32.toml`（双通道 16k/20ms，R14 双麦期语义：DoA 吃原始双通道、
  总线承载下混 mono，brain 零改动）+ 本条 runbook。**runbook（mock 档真板冒烟：宿主四进程 +
  板面真麦）**：`RR3_HAL_PERIPH_UART=/dev/cu.usbserial-A98KN77U RR3_HAL_PERIPH_BAUD=921600
  RR3_SENSE_CONFIG=<repo>/modules/rr3-sense/config-esp32.toml rr3-sim --brain --audio-backend
  esp32 --sink-backend mac --keep`——对板说话 → brain「对话轮完成」（mock 固定文本）→ mac
  扬声器出声；onset 事件带 doa 字段（B 铺垫）。注记两条：**flock 独占**——音频流期间
  舵机/显示/rr3-audio-rec 与音频源互斥不可并存（B 延后自洽）；**~2% 时钟滑移**（run4 gaps
  口径）→ ring 溢丢稳态损耗，VAD/ASR 可容忍。真 key 就位后 `--brain-cloud real` 切档即
  全语音自由对话（acc④，运维动作不属本轮代码）。
- **A 批真板冒烟——分层验收过（sense 面）+ 板麦口径标定 120 + 物理层两课（2026-10-18，本提交）**：
  分层冒烟（无 brain，rrd --manifest 直起 sense）：`denied_publish = 0`、pcm 62 万+ 帧事件在流、
  板面流就绪 16000/2ch/s16le 与钉定值一致、**VAD 真板成轮 8 次 onset**（说话 → vad_start 上总线）
  ——A 批新增面（板麦 → esp32 源 → VAD → 总线）真件验证通过。**板麦口径标定**：`rms_start 1500→120`
  （config-esp32.toml）——VAD 不成轮的根因不是阈值略高而是电平差一个量级：临时探针（RR3_VAD_PROBE，
  已撤）+ wav 对拍实测说话 RMS 峰值 163 / 环境底噪 73（INMP441 半米距离真实物理量级，宿主真麦的
  1500/800 口径在板麦上不适用；板固件 `>>8` 24bit→16bit 处理复核无误）；**板端数字增益（+18dB
  拉开信噪余量）板固件小批立项**。**物理层两课（当日流劣化排查）**：①UART2 杜邦线接触不良 =
  劣化主因（20s 实收 5.4s/73% 丢 + 112 resyncs → 压实后 14.0s/15s 实收 + 7 resyncs，恢复 run4
  同量级）；②FT232R 通道容量余量课题——音频 b64 组帧需求 ~90KB/s vs 921600 上限 92.16KB/s
  余量仅 ~7%（板钟实测 16010–16019Hz 漂移在吃余量；桥型号已核：A98KN77U=FT232R 上限 3M、
  1420=CH340），提波特率板固件小批评估。**全链（brain→mac 出声）仍被本机 python 3.9 门槛卡**
  （本表第 9 行在案，`--brain` 演练 fail-fast 同款），brain 腿归 CI/装机环境。**VAD feed 毫秒账
  与 esp32 非阻塞源部分读的耦合**（`vad/mech.rs` `above_ms += frame_ms` 假设每 feed 恒 20ms）：
  探针实证 92% 拍满 320 帧（esp32audio ring 已把 128ms 突发摊平），实际影响小，如实登记不改判。
- **A 批真板冒烟（二）——全链完全体验收过（2026-10-18，提交 `623a526`）**：`rr3-sim --brain
  --audio-backend esp32 --sink-backend mac`（mock 档）全语音对话闭环 **OK**——板麦语音 →
  VAD 真起音真判停（vad_start ×12）→ brain 成轮 ×6（trace=1..6，ASR/LLM 为 mock 固定文本，
  轮次由真板音频驱动）→ interact mac 扬声器出声 ×6（tts 1520ms/轮）→ `rr3-ctl status` 三模块
  running、`denied_publish = 0`。工具随批小改：`--brain` 演练就绪上界 env 可覆盖
  （`RR3_BRAIN_READY_DEADLINE_MS`，缺省 60s 零改动）——sim burst 源自动说话 60s 够用，
  真板档等真人响应节奏不可预置（60s 硬上限首轮演练超时暴露）。本机 python 门槛随批解决：
  Miniforge x86_64 用户目录静默装（`/Users/apple/miniforge3`，python 3.14，不碰系统
  python3/shell 配置；brew 源被墙 + arm64 包 Bad CPU type（本机 Intel）两绕行），调用时
  `PATH=/Users/apple/miniforge3/bin:$PATH` 显式前插。A 批（语音自由交流）至此**代码 + 配置 +
  工具 + 真板验收全收官**：c1–c6（`1d00cef`→`5cb7f98`→`5dce1cd`→`406b1ef`→`ebb0c81`）+
  冒烟(一)（`7438dcc`）+ 本批。真 key 就位后 `--brain-cloud real` 切档即全语音自由对话。
- **0.0.6 模型连接抽象批——AgentRT v3 式连接行 + 密钥双源 + 面板密钥面（2026-10-14，提交
  `d69e918`+`89eb71e`+本批记）**：用户需求"抽象化 API 客户端连云端/本地模型；操作面板填 key，
  代码无明文密钥"（立项计划 .trae/documents/rock3-model-client-abstraction-and-panel-keys.md，
  本地工作区文档）
  → ①**云适配层**（[rr3-brain/cloud](../modules/rr3-brain/rr3_brain/cloud/__init__.py)）——
  `config.toml` 新增 `[[rr3-brain.model]]` 连接行（name/mode(api|local)/api_format(本批仅
  openai，anthropic 预留)/base_url/model_id/api_key_ref/active，恰一 active 由解析器强制，
  白名单外 fail-closed，无表回退旧 `[rr3-brain.cloud]` 通用键不破）；`Rr3LlmClient` 加
  key_ref/mode，密钥双源解析：`env:VAR` 前缀走环境变量（缺省 None = 旧 `RR3_CLOUD_API_KEY`
  不破）、裸名走仓外 secrets 文件；`mode="local"` 免鉴权（请求不带 Authorization 头）；
  ②**面板密钥面**（[panel/secrets.rs](../modules/rr3-interact/src/panel/secrets.rs) 新建）——
  端点 `GET /api/model/keys`（清单只回 ref 名永不回值）+ `POST /api/model/key`（upsert，
  value 空=删，应答只回 ref）；落仓外 `~/.rr3/secrets.json`（0600，父目录 0700，
  `RR3_SECRETS_FILE` 与 brain 侧同名同义双端单一契约），坏文件回错**不静默覆盖**（防一把抹掉
  用户其他 key）；brain 每轮现读 secrets，面板保存后下一轮对话即生效；页签"模型密钥"卡片
  （textContent 防 XSS，保存后清空密码框）；端点总表 12→14，漂移门同批 pin；③**演练 real 档**
  （tools/sim brain.rs）真云 config 镜像补连接行（`--brain-base-url` 旗标→行 base_url，密钥
  引用 `env:RR3_CLOUD_API_KEY` 沿用演练面旧契约）。rr3-brain 59 tests OK（skipped=3）、
  rr3-interact 47 tests OK、tools/sim 25 tests OK；开源自查 `git grep "sk-"` 命中均为测试
  假值（sk-test-value 等 fixture）与 "task-4" 子串误中，无真实密钥入库。
- **0.0.6 模型连接抽象批验收补记——面板密钥面真连过 + 演练面板地址透传（2026-10-14，提交
  `eed8d61`+`efb8ad6`+`90b19a9`）**：①用户裁定 key 供给方 = DeepSeek 官方（仅 LLM 腿，
  ASR/TTS 维持 mock/半真环前提）——active 行切 `https://api.deepseek.com` + `deepseek-chat`；
  ②用户裁定 key 不走 shell 环境变量（面板路径最小暴露面）——api_key_ref 切裸名
  `DeepSeek官方` → 仓外 secrets 现读；③**面板密钥面实操真连过**：手机/局域网面板填 key →
  `~/.rr3/secrets.json`（0600）→ brain 现读 → DeepSeek 真连应答（生产路径：模型行 → 客户端 →
  裸名 ref → secrets，值全程零回显）；④演练修复：RR3_PANEL_ADDR 原被硬钉回环 18080（手机
  永远够不着），改用户显式设置优先；interact 旧二进制（12 端点）重建至 14 端点。**acc③
  （LLM 腿真云）就此通过**；敞口改记：acc④ 全语音真云环待全栈云 key（SiliconFlow）或本地
  模型（mode=local）——DeepSeek 官方仅 LLM，演练 mock/real 是全局档无 per-leg 开关。
- **Mac 宿主真麦全环首通 + 真云 opt-in 冒烟落库（2026-10-14）**：四步收口——① rr-hal 新增
  [macaudio.rs](../base/runtime/rr-hal/src/macaudio.rs)：CoreAudio AudioQueue **手写 extern 面**
  （零 crates.io 增量，同 shm 先例），源/汇 16k mono s16、回调锁内零分配、Drop 时序
  Stop(true)→Dispose(true)→回收 Shared；选择面 `RR3_HAL_BACKEND=mac` **显式启用**（缺省仍
  sim，CI/e2e 零改动）；② mac 宿主档显示面落 sim 同款虚拟点阵（宿主无 LED 矩阵是平台事实
  非能力缺失，R11③ 的探活回错退出保留给 linux 真件档——首跑 interact 显示面探活判死即修，
  [backend.rs](../base/runtime/rr-hal/src/backend.rs) `rr3_hal_display` mac 臂）；③ tools/sim
  `--brain` 三旗标 `--audio-backend / --brain-cloud / --brain-base-url`（取值白名单 +
  real 缺端点 fail-closed 拒起）；④ 真云 base_url 修正（云客户端自拼 `/v1/<路径>`，契约 =
  主机根；config.toml 与 sim 演练镜像值去 `/v1` 尾缀，防 `/v1/v1/...` 404）+
  [test_cloud_real.py](../modules/rr3-brain/tests/test_cloud_real.py) opt-in 冒烟
  （skipUnless key：无 key 整模块 skip 日常单测零网络零红，本机 3.13.5 44 tests OK
  skipped=3）。**acc② 实录过**：`rr3-sim --brain --audio-backend mac` 全环——sense(mac 真麦)
  → brain mock 云「对话轮完成」trace=1 user='你好，今天天气怎么样' tts_pcm_ms=1520 →
  interact `播放面就绪: backend=mac` `tts: 19 字符播了 1520ms`（48640B gain 70%，talk 挂起），
  三模块 running denied_publish=0。排障链存档：真麦探针曾全静音（用户已说话 + TCC 已授权 +
  协商流格式 rate=16000 ch=1 精确一致），加强探针（`AudioQueueGetProperty` 诊断面 + 10s 包络）
  定位真因 = **正常说话 rms ~700 低于 sense VAD 阈 rms_start=1500**（输入音量 50%），非拾音
  链路问题——输入音量提 85% + 靠近说话即破阈成轮；真麦探针升级为包络观测面沿用。敞口：
  真云两测（acc③④）等 SiliconFlow key 到位；判据② 板麦仍等 FT232（CP2102 RX 楔死定性不变，
  今日复测 0001@921600 0 字节；板载 1410 静默期无自发帧，未复位不计证据）。
- **0.0.5 第 IV 轮 MCP-0 契约级评审批（2026-10-12）**：P1 评审批开工门兑现——MCP-1
  `topics.toml` 29→30（append `rr3-brain/audio/tts_pcm` stream L2，载荷体逐字段复用
  `rr3-sense/audio/pcm` 已定形状，F-4 下行通路契约半边；与批C/批D 合成一次改表，批C/批D
  复用在册主题零新增登记）；MCP-2 SPI 扩键评审随 R31 Q2 作废留痕；MCP-3 `rr3-act` 申报面
  增订阅 `rr3-runtime/system/power`（MC-5 消费点，C14/R-3 同程序）。同批同步 rr-registry
  形状 pin（30 条三元组 / kind 12-10-4-4）、rrd topics_load 计数（33=30+3）、
  [contracts/README](../contracts/README.md) 演进段 / [conventions §2](process/conventions.md) /
  [0.0.5 方案 §2.7](process/dev/0.0.5版本洛克三号对齐与裁定方案.md) 裁定登记；
  MC-1 / MC-5 解锁施工；评审记录唯一落点 contracts/README 演进段；
- **0.0.5 增补轮五 批C 收官——能力帧进城 + 宿主格位 + e2e + 胶囊眼（2026-10-12，提交
  `60a8b81` + `bbc60e4` + `ff9de83`）**：路线二落地——板上 esp32-periph-base 加**帧执行面**
  （双面分流：首字节 0x31 走帧面 rr3.call 分发应答、字母首字节走批B 行面），宿主 HAL 桥后端
  [rr-hal::esp32_uart](../base/runtime/rr-hal/src/esp32_uart.rs) 长连接直发（src=1/dst=2，
  舵机/表情复用在册主题零改表）；e2e 真板两连全绿：判据① display.draw(idle) applied=true、
  判据② frame() 读回一致、判据③ servo round-trip 90/45/135/90 逐度一致，console 八帧零
  丢弃——"帧面失步"破案结项（排障脚本把 payload 写进 128B 头保留区，板侧 ReservedNotZero
  正确拒帧，教训登记 [conventions §2](process/conventions.md)）；修复批：rr-ipc 新增
  `rr3_flush_input` settle 补冲 + draw() 置 probed 订正 frame() 探活擦屏；**胶囊眼增量**
  （用户裁定"有生命感"）：圆角竖矩形胶囊眼渲染 + 自主眨眼/扫视微动作（anim_tick 搭 cmd_loop
  1-tick 窥视空档驱动，esp_random 硬件熵），真板 idle/think/idle 三轮 round-trip 应答全对 +
  OLED 目视确认，固件 v0.2.0→v0.3.0；专用门 + 十步门禁 GATE_EXIT=0 全绿；敞口：判据④
  （堵转 UBEC≥4.7V/板子不复位）待舵机接线窗口，读侧阻塞无超时归批E 桥面统一收口；
- **0.0.5 增补轮五 批M 推进——P1 评审批 / M-α / M-β / M-γ 收官（2026-10-12，提交 `8d05590a` / `ba90367`+`1958ce8` / `3bc30f1`+`91b473c` / `084d081`）**：
  P1 评审批（MCP-0）落 `topics.toml` 29→30（tts_pcm）与 MCP-3 申报面，M-α（MC-9 台账诚实化 +
  MC-8②③ 回显型参数与端点真值）；**M-β（MC-11 四件，判据"已实装行为有护栏、卡死与静默停摆看得见"）**：
  L-6 relay/state 同态抑制 RPC 路 e2e 断言（`e2e_act` ④b，破坏性验证证牙齿；intent 路 ⑤b 批I 已有）+
  A-6 `reflex_ticks` / S-3 `pcm_frames`·`pcm_empty_frames` 心跳自报记账（e2e 经 `rr3.ctl.counters`
  有界轮询点验）+ I-4 面板 spawn 失败改弃连点账继续服务（`Rr3ConnSlot` RAII 守卫防 panic 泄漏，
  必败注入 `stack_size(usize::MAX)`——macOS 对 `stack_size(1)` 静默 clamp 使 spawn 成功）；
  四件全按"先跑红再接门"纪律闭环；十步门禁直跑 GATE_EXIT=0（54 PASS）全绿，推三远端四端核对一致；
  A-7 守恒核清：`tools/sim/chain.rs` 与 legacy 面板均不按帧数 relay/state，零改落档注记。
  **M-γ（MC-6 配网凭据持久化 + MC-4 探测真值接线）**：MC-6 配网凭据改由**后端承载持久化**
  （`rr-hal` 新增 `nmcli_support` 公共机制层 + `provision_store`（`Rr3WifiStore` 三后端 + 选择
  函数 `rr3_hal_wifi_store`）；`rr3-connect` connect/forget 先过 store 再转状态机（fail-closed）、
  `Rr3Provision` 只管态不管存、新增 `rr3_provision_restored` 重启恢复 station——件内不写文件）；
  MC-4 `Rr3LinuxProbe` 五面在位性实装 + `RR3_PROBE`（sim/linux/absent）显式选择口（**sim 探针
  不再当缺省构造**，台-1 清零）+ `profile.rs` 注入缝与 2 条"缺格 → 点名件与键"门禁用例；e2e 与
  `tools/sim` 装配演练显式 `RR3_PROBE=sim`（真 rrd 子进程走非 test 编译面，不赌平台默认）；
  十步门禁直跑 GATE_EXIT=0（54 PASS）全绿，推三远端四端核对一致。
  [索引 §13.4](design/模块设计索引.md)/设计册判定列对账同批落档；教训登记 [conventions §2](process/conventions.md)。
  余批：M-ζ（MC-5 + MC-8① + MC-10）最后；
- **0.0.5 增补轮五 批M M-ζ 收官——MC-5 + MC-8① + MC-10（2026-10-13，提交 `071dcad`+`8b5b57e`（MC-5）/
  `a5ce663`（MC-8①）/ `c1bd370`+`48e6145`+`1b1768a`+`98e032d`（MC-10 取证基础设施）
  + `9d3043a`+`b6966fe`+`5245ba9`（MC-10 策略出件三连））**：
  **MC-5 低电禁动闸门**（A-5 ⊕ power P-8 ⊕ 批L L-8 三单合一）落机制核闸门 `Rr3LowGate`，
  `rr3-act/src/composite.rs` 接 `rr3-runtime/system/power` 广播——执行面 RPC/intent 两路拒动、妆面不禁、
  回阈 + 迟滞解禁（欠板上实录）；**MC-8① 判 (c)**：`cutoff_voltage_mv` 键保留为策略数据、
  启动日志显式"声明未生效"，本版不关断（关断动作归 M6 关机链）；
  **MC-10 取证基础设施 + 策略出件**：L-1 `matrix.sh` 进 CI 第九 job、L-2 ⑦-c/⑧-a 数据位化、
  L-4 ⑦-e 自指判等、L-5 14 份 `assemble.toml` 补齐/明写 `no_injection`；策略出件三连
  A-9（开放门落 `[rr3-act.intent] open_topics` + 灯效白名单落 ops 槽 `led_effects`）/
  I-11（interact talk 不可点名改 ops `non_pointable`）/ S-5（sense DoA 分档阈值迁 ops），三处均 fail-closed。
  **mz-dod 收口台账已结**（§22 DoD 勾点 + §18.2 站替台账清零核对 + 本锚重写 + 敞口登记）。
  十步门禁 `bash tools/ci/check.sh` 全绿（尾行"所有规约检查通过"），推三远端四端 `ls-remote` 核对一致。
  **批M 余数如实登记（批次未收口）**：**MC-1 已结（2026-10-13，四提交 `eefc0de`/`ed8fb2f`/`f959e3a`/`70e8fb6`）**——brain 分帧发布 `rr3-brain/audio/tts_pcm` + interact 收流对账排空播放 + `contracts/golden/tts-pcm-frames.json` 两腿对拍，占位正弦与 `RR3_TTS_MS_PER_CHAR` 退役（**台-2 已清零**）；
  MC-2 已结（2026-10-13，`1930956`）——`volume_pct`/`prebuffer_ms` 经 config `[rr3-interact.player]` + ops 槽实装（`rr3_tts_apply_gain` i64 整型饱和、`rr3_tts_prebuffer_ready` 起播水位 legacy 0cf029e min 语义，interact 42 用例全绿）；
  MC-3 复核（2026-10-13）——判据三条全兑现（`RR3_BACKEND_ESP32` 桥后端支 fail-closed + 开机自述真后端名 + 格位三源一致），**台-3 清零、F-2 改「可达」**；余数 = 帧表 R-12 迁移（`RR3_FRAMES_*` 件内唯一真值、板侧 `expr_eye_base` 已落而生产链未切 `expression` 变体），MC-3 不勾整单、归属待裁；
  MC-7 复核（2026-10-13）——**不勾，缺什么已写**：装配演练面在位（esp32-mini.toml + contract.rs pin + rr3-pack 正例），件承载三缺（件模块未进 xtensa 门 / 板侧仍专用固件 crate / `[hardware]` 进位未补），F-8 维持不可达。
  §22 DoD 相应条目：站替台账五条全部清零（**已勾**）；F 组条目仅剩 F-8 不勾，缺项与触发条件见
  [方案 §22](process/dev/0.0.5版本洛克三号对齐与裁定方案.md)。
- **0.0.5 增补轮五 现场处置续批（二）——OLED 表情不出的根因修复（2026-10-12，本提交）**：
  用户现场裁定（"OLED 屏幕没有显示出表情 + 舵机没有正常转向 + 考虑整板电压 + 极限资源下
  也能正常系统运行"）。**真板实验链把根因定死**：长度阶梯探针排除"单笔长度"、逐页轨迹排除
  "数据内容"、舵机前后对拍定位到 **Move 之后恒败**、**逐轴掉 duty 试探一步定案——舵机保持电流
  与 SSD1306 电荷泵抢同一 5V 轨**（掉 duty 同一笔立刻 Ok，重新激磁立刻又败）；并登记
  **地址单笔"假 ACK"欺骗性判据**（总线已死时 0x3C 仍"在列"，必须看多字节页写）。
  **修复 = 供电分时仲裁**：`RtPeriph::servo_release()` 在 `runtime_loop` 每次动作收尾把两轴
  duty 归零（5V 轨整段让给 OLED）+ `i2c_bus_recover(true)` **无条件** 9 时钟 + STOP 强制
  重同步（旧条件版只在 SDA 低时打时钟，救不回"空闲期被电机噪声顶进半途态"的从机）+ 亮度
  降级阶梯 `0x4F→0x3F→0x2F→0x1F` + 每轴 2°/20ms 缓速软启动。**boot 姿态按要求恢复：
  pan=60（中位左转 30°）/ tilt=98（中位上抬 8°）**，队列序"先表情后舵机"（可见性优先）。
  真板复验 3 次 35–40s：`anim_fail=0 capsule_fail=0 resets=0 bod=0`、`姿态到位 pan=60 tilt=98
  （LEDC 回读一致）`——修复前同条件 `anim_fail` 达 15–25 次/30s。UART2 引脚**定案 18/19**
  （16/17 一装驱动即打死 I2C0，真板实锤）。**遗留如实登记**：舵机不保持激磁（位姿靠齿轮
  摩擦，长时间可能下沉；WANDER 退化为碎步）、板载 5V 直供下 BOD 偶发未闭、UART2 指令面
  现场仍不可测（CP2102 回环，待用户核查接线）、Q1/Q2 维持未施工。全文见
  [方案A §11](process/dev/方案A——整板电压监测与分轴限幅立项评审.md)、runbook 见
  [esp32-periph-base README](../tools/esp32-periph-base/README.md)。
- **0.0.5 现场处置 方案C——真板云台故障的软件面收口（2026-10-12，提交 `f6e92d6` / `8f750b2`，
  计划外修正非批M 工单）**：真板实测云台舵机卡死不动 + 发烫（SG90 堵转 ~700mA/只持续流动），
  5V 取自板载 USB/5V 排针经拓展坞直供 → 5V 轨跌落 → **BOD 复位循环**（`Brownout detector
  was triggered`）→ 串口节点与 OLED 反复掉线；软件加剧因素 = 板侧 WANDER 全行程 0–180 折返 +
  SCAN 五点 0/45/90/135/180 把舵机推过机械死点并持续施力。**落点（板侧固件为主 + 宿主 config
  同步）**：① 板侧 `main.rs` 立**分轴行程**（`AXIS_CENTER_DEG=90` / `PAN_LIMIT_DEG=20` /
  `TILT_LIMIT_DEG=10` + `Axis::range` + `axis_scan_points`），`servo_set` 越程**点名拒收**
  （fail-closed，不静默钳位）；WANDER/SCAN 端点一律取自 `Axis::range`（pan[70,110]/
  tilt[80,100]）；INFO 增 `lim=` 字段、HELP 加行程区间；② 宿主 `modules/rr3-act/config.toml`
  `angle_limit_deg [0,180]→[70,110]` + composite 夹具 + brain 词表(45/135/90→70/110/90) +
  `--chain`(45→70) + `e2e_act`([0,180]→[70,110]) 必要连带；③ **同批修正**（`8f750b2`）：
  `TILT_LIMIT_DEG 5→10`，tilt 行程放宽 [85,95]→[80,100]，纳现场所需俯仰上抬 98°（原值会被
  fail-closed 拒收）；连带补 0.0.5 方案C 批漏改的 `README.md` 指令表与验收判据（旧文仍写
  「单轴 0–180」与五点 0/45/90/135/180）。xtensa 专用门 PASS（真编译 25.78s）+ 十步门禁直跑
  `GATE_EXIT=0` 全绿，推三远端四端核对一致，板上已烧录（460800），HELP 回读证实「行程
  pan 70-110 / tilt 80-100」。**敞口**：板载 USB-UART 在复位/欠压瞬间掉出 USB 总线需物理重插；
  UART2 指令面（外部 CP2102）双向字节污染（非固件 bug，自查清单已出）；板载 USB 掉总线时
  OLED 第 4 行实时角度行为唯一可靠读数。**方案 A**（整板电压监测 + 分轴限幅）另行立项评审，
  与本处置并行。[0.0.5 §19](process/dev/0.0.5版本洛克三号对齐与裁定方案.md) /
  [conventions §2](process/conventions.md) / rr3-act 设计册判定列同批落档。
- **0.0.5 现场处置 板侧验收 + 方案A 裁定（2026-10-12）**：真板验收（板载 1410 控制台 + 外部
  CP2102 0001 UART2）——控制台状态行/boot 全档取证（新镜像 ELF SHA256 `1e330dd12…`、编译
  `Oct 12 2026 10:51:03`、`rst:0x1 POWERON_RESET`）；OLED 硬件通（I2C 扫描 `[3C]`，400k 失败 →
  100k 第 2 次重试过，boot 4 行已出）；LEDC 置中 `duty pan=1229 tilt=1229`；**UART2 限幅字段取证
  `lim=pan[70,110]/tilt[80,100]`**；UART2 双向字节污染仍在（钳制宿主驱动面）。**两症状定位（均非
  固件 bug）**：①OLED 无胶囊眼 = `oled_last="(boot)"`（自 boot 未收 face/expression，胶囊眼为帧面
  `rr3-interact/face/expression` 专有）；②舵机发烫/未到位 = `pan=tilt=90 wander=off`（板侧未驱动）
  + UART2 上行污染，发烫属供电/装配（疑未走 UBEC 独立 5V 轨，0.0.6 BOM #3 记待采购）。**方案A 裁定**：
  用户授权"按最优方案裁决" → Q1–Q6 逐条采纳建议（Q1 5V 轨 ESP32 ADC1 测点=与判据④共用 / Q2 双阈值
  +迟滞复用 `Rr3LowEdge` / Q3 板侧为主+宿主留位 / Q4 config 升 pan/tilt 双区间〔形状变更，契约评审
  前置〕 / Q5 先占位阈值+真板两点标定 / Q6 并入 0.0.5 续批并与 MC-5 合并），详见
  [方案A §8/§9](process/dev/方案A——整板电压监测与分轴限幅立项评审.md)。
- **0.0.5 现场处置续批——控制面/运行面分离 + 舵机串行降级（2026-10-12，提交 `03824f6`）**：用户现场
  裁定"舵机与 OLED 的控制面/运行面都分离……舵机先左右再俯仰、最大化利用电源而不卡顿，这是降级策略"
  → 落 `tools/esp32-periph-base/src/main.rs`：①**双平面拆分**——控制面线程独占 UART2（解析/校验/
  改状态/推作业/回包，不碰慢外设）、运行面线程独占 I2C0+LEDC（执行硬件作业 + 自主节拍），经
  `Arc<Mutex<Bus>>` 通信、锁不跨 I2C 持锁（消"卡顿"）；②**舵机串行降级**（即[方案A §8 Q3](process/dev/方案A——整板电压监测与分轴限幅立项评审.md)
  (a) 板侧为主）——复合动作先 pan 到位 → 整定 200ms → 再 tilt，SCAN 逐点串行、WANDER 同拍单轴；
  ③**I2C 健壮性配套**——400k→100k + GDDRAM 分块写（≤32B=硬件 FIFO）+ 写重试 6×5ms + boot init 5 次
  重试。**板上证据**：boot SURFACE 行 `planes=ctrl+runtime serial-servo`、10/10 复位 boot 画面全出、
  `INFO` 在 SCAN（~6s）期间即时应答（分离性直证）、`lim=pan[70,110]/tilt[80,100]` 仍取证。**未决**：
  UART2（0001）链路物理劣化（板→Mac 前导乱码 + 自收包，非固件）+ OLED 运行期偶见 I2C 瞬态失败
  （电源/总线边际，重试窗内自愈）——均转用户物理核查。Q1（5V 轨观测）仍未施工。
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
