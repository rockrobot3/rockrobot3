# status —— 唯一实况快照

本文件是仓内**唯一**允许回答"某能力在当前 HEAD 有没有实现"的地方。其余文档只写设计语义，
不写实况；设计文档里出现"已落地/未落地"的字样一律视为待订正（架构文档 §5 就因此整段重写过）。

- **实况锚点**：本快照按锚点提交 `8985677`（分支 `dev/hubs-01`，0.0.8 版本收官树——批N33 件一
  四组翻面后；批N32 件四已对表 brain 六处断链改指继任物、批N33 件一续对表 sim/M4 三处；前锚
  `9c07b3e`＝0.0.8 W-A 批5 落档树见 git 历史）逐条读代码写成，不是按规划推断；
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
| `trait Transport` 与 uds / shm / tcp 分层 | 已实装 | **uds 是唯一真实现（shm 已摘面见下一行，0.0.8 批N19）；tcp 只有线上签名、一行实现都没有**（ADR-0008 裁定 4 的"不写半成品"对 tcp 仍成立）。[`transport/mod.rs`](../base/runtime/rr-ipc/src/transport/mod.rs)：三 trait——`Rr3TransportConn`（`rr3_send_buf` 消费缓冲 / `rr3_recv_buf` 返回所有权缓冲；对一切 `Read + Write + Send` 有 blanket 实现，send 前幂等 seal）、`Rr3TransportListener`、`Rr3TransportEndpoint`（cleanup → bind）；[`uds.rs`](../base/runtime/rr-ipc/src/transport/uds.rs)（`#[cfg(unix)]`）：`Rr3UdsListener`（`type Conn = UnixStream`）与客户端入口 `rr3_connect`。**不引 async**（ADR-0008 裁定 1） |
| shm 传输（单写者多读者环形缓冲，实时控制面） | **已摘面**（原「已实装」，L-32 裁定摘面，0.0.8 批N19 件一 `9f0639b`） | ADR-0011 历史实装记录（shm.rs 845 行已随摘面移除，原文存档见 git 历史）：核心环平台无关——64B 区域头（magic `RR3S` / version / cap / **head 单共享变量**，Release 发布 Acquire 读取）+ 字节环，环内记录 = 一整帧（128B 头 + payload）；seqlock 式复核（拷出后 head 不变才接受）+ 半容量守卫（帧 ≤ cap/2）证明无撕裂；单写者多读者、**丢最旧保最新**——写者永不阻塞永不失败，读者落后过半圈重同步并计 `lost`（不静默）。角色分型 `Rr3ShmWriterConn` / `Rr3ShmReaderConn`（方向不匹配的方法恒报 Io），读者 `rr3_attach` 对位 uds 的 `rr3_connect`，阻塞 `recv_buf` = 200µs 退避轮询（无通知原语）。POSIX 附着段（`cfg(any(target_os = "linux", target_os = "macos"))`）：**手写 6 函数 extern 面**（shm_open / shm_unlink / ftruncate / mmap / munmap / close，不引 libc crate，依赖预算零增量）；macOS 的 shm fd 只支持 mmap（read/pread 报 ENXIO/ESPIPE），附着走"64B 探头映射校验 → 按 cap 全量映射"两步法。默认容量 1MiB（`rr3_bind_with_cap` 可建大区），对象名默认 `/rock3-ipc-shm`、可用 `RR3_SHM_NAME` 环境变量覆盖。摘面裁定＝[decisions §2 L-32 行](process/decisions.md)（零消费者：全仓 rust 面 shm 唯一命中＝rr-ipc 自身 re-export；transport 两 trait 各 2 实现却零多态调用点＝L-32 首批两例）；**不做清单（ADR-0011 历史保留）**：tcp、rrd 接线（[`conn.rs`](../base/runtime/rrd/src/conn.rs) 仍 UnixStream）、F_ZEROCOPY grant、F_URGENT 行为差异、通知原语、可靠交付、跨进程 e2e |
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
| rrd 低电兜底（battery/low 钩子四件动作面） | 已实装 | T4b（`e87a284`，0.0.2 方案 Q4 裁定）。[`conn.rs`](../base/runtime/rrd/src/conn.rs) 的发布转发点挂 `battery/low` 钩子（**rrd 是总线本体，每帧 Event 必经，兜底不截断事件流**）→ 置 [`registry.rs`](../base/runtime/rrd/src/registry.rs) 全局低电旗（**上升沿恰一次；解除沿＝battery/ok 回阈下降沿，批N5 件二笔3 起两个沿一个对**——`rr3_reg_low_power_raise`/`rr3_reg_low_power_lower` 同一张表同一面，与 denied_publish 同款 conn 与 supervisor 两路共见守护态）→ 点名广播 `rr3-runtime/system/power`（帧形状单测钉死：Notification / src=rrd / dst=广播 / 体恰四字段，批N5 件二笔3 起 reason 两值 battery-low/battery-ok——e2e 旁观者接入时首拍已过，帧形状只有单测能钉，两半拼成完整证据链）+ 日志环 → supervisor 的 boot 漏斗拒新 spawn（boot 与重启一问全挡；杀而不重启属关机策略，归 rr3-power 演进与 M6）→ battery/ok 回阈清旗恢复新 spawn 并广播 battery-ok（falling 才广播，重复 ok 不重复广播；rr3-act 订阅臂 reason 分流——解除广播不闭锁不解禁，`69b2b47`）。线级证据 [`tests/e2e_power.rs`](../base/runtime/rrd/tests/e2e_power.rs) 1 条（装配单正门起真 rr3-power，sim 电池 5% 起表 → 兜底触发行含透传值与广播结果 → 旁观者收周期流 → pkill 真模块后拒绝 spawn 窗断言 + 末尾破坏性收尾计数双重保险）＋ [`tests/e2e_power_ok.rs`](../base/runtime/rrd/tests/e2e_power_ok.rs) 1 条（批N5 件二笔3：测试连接伪造沿事件钉全链——触发→battery-low 广播→恢复→清旗＋battery-ok 广播→重复 ok 静默） |
| rrd 上行 JSON-RPC 十二方法（模块四 + 控制八） | 已实装 | [`rpc.rs`](../base/runtime/rrd/src/rpc.rs)：`Rr3RpcMethod` = `rr3.register` / `rr3.heartbeat` / `rr3.unregister` / `rr3.call`（M3）+ `rr3.ctl.status` / `rr3.ctl.health` / `rr3.ctl.logs` / `rr3.ctl.ota`（F3 / ADR-0010）+ `rr3.ctl.counters` / `rr3.ctl.reboot`（0.0.4 批B/批C append，Q1 裁定）+ `rr3.ctl.wiring`（批N4 件九笔4，装配快照读口）+ `rr3.ctl.contracts`（批N5 件五笔4，契约完整性审计读口）。八个 ctl 方法走**同一解析器**，`is_ctl` 的判据与方法面一致（模块四方法全部 `!is_ctl`）；`rr3_rpc_parse_request` 的版本与 id 校验、`rr3_rpc_build_result` / `rr3_rpc_build_error` 错误映射、`rr3_rpc_call_params`（`params.topic` 非空、`args` 可缺省、成功不回 result 只回 accepted）照旧 |
| 运维计数面（心跳 counters 双端镜像 + 聚合持久化 + ctl 读面） | 已实装 | 0.0.4 批B（`e495242`，Q1/Q2 裁定；legacy data 的"机器慢先看数字 + 断电不丢"语义承接，digest 归档行在案）。**模块侧**：[`client.rs`](../base/runtime/rr-bus-api/src/client.rs)（批N9 件一承载面契约化后住 rr-bus-api，语义见"模块侧同步总线客户端"行）的 `rr3_counters_bump`（模块内部运维词汇自愿累加）——心跳线程每拍快照，**非空才带** `{"counters": {...}}`（可选 append，旧 rrd 忽略未知字段，双向兼容）；Python 侧 `bus.py` 同语义镜像（ADR-0014 裁定 1；镜像随批N32 brain Rust 迁移退役，Rust 侧计数契约见 [`client.rs`](../base/runtime/rr-bus-api/src/client.rs)），双端镜像单测各钉。**rrd 侧**：[`rpc.rs`](../base/runtime/rrd/src/rpc.rs) 形状裁决（`{string: u64}`）→ [`registry.rs`](../base/runtime/rrd/src/registry.rs) 聚合进在册表（键 = `模块名/键`；rrd 自有键 `boots` 启动自增）→ [`counters.rs`](../base/runtime/rrd/src/counters.rs) 持久化：启动加载全表 + 心跳归并 write-through，路径 `RR3_DATA_DIR`（cfg(linux) 缺省 `/data/rr3`——rr_data 分区挂载点）下 `counters.json`，**原子写**（临时文件 + rename），皆无 = 不持久化（日志一行点名不报错，sim 直跑即此态）；发行版 unit 注入 `RR3_DATA_DIR=/var/lib/rockrobot3` 且 postinst 建目录（0.0.6 批E'，`a5b3093`——真板复验 boots 跨重启累积成立，见 deb 打包链行）；`rr3.ctl.counters` 读面回全表快照。brain 对话轮计数与 act 继电器动作计数已自报（键随模块内部词汇）；**归并顺序竞态已修（0.0.6 纪律小批 `b81c986`）**——心跳分支先归并后应答，ctl 读快照不再抢在归并前（e2e 同用例钉顺序契约）；counters 落盘节流已由 0.0.5 增补轮四 R26 裁定改判心跳合并写并随批W3 实施——`Rr3CountersGate`（首拍即刷 + M 毫秒窗口至多一次，env `RR3_COUNTERS_FLUSH_MS`/`RR3_COUNTERS_FLUSH_MAX` parse-or-default）+ SIGTERM/SIGINT 关停强制 flush（self-pipe + `counters-term-flush` 线程，e2e `sigterm_forces_flush_of_pending_counters` 钉节流与关停完整性双判据）；裁定演进记录在 [`counters.rs`](../base/runtime/rrd/src/counters.rs) 模块头 |
| 模块间点名路由与应答回路（rr3.call） | 已实装 | **请求方向**：[`conn.rs`](../base/runtime/rrd/src/conn.rs) 的 `rr3_route_call`——查 [`registry.rs`](../base/runtime/rrd/src/registry.rs) 的 `rr3_reg_callee_for`（exposes 该主题的在册模块里**取 task 最小者**）→ `rr3_reg_send_to` 点对点投递，转发帧 `trace_id`=调用方 task（语义登记见 [contracts/README.md](../contracts/README.md) 演进登记）；三种拒绝（未注册主题 / 无 callee / exposes 不符）逐字文案归 [`reference/rrd-rpc.md`](reference/rrd-rpc.md) §8。**应答方向**：`rr3_route_response` 按 trace_id 原路回给调用方。线级证据：[`tests/module_call.rs`](../base/runtime/rrd/tests/module_call.rs) 4 条（点名回路 + 三种拒法）+ [`tests/e2e_audio_chain.rs`](../base/runtime/rrd/tests/e2e_audio_chain.rs) 1 条（**三真进程**：rrd + rr3-sense(sim) + rr3-interact(sim)，vad_start 事件上总线、pcm STREAM 有序转发、rr3.call 应答回到调用方） |
| 模块侧同步总线客户端 | 已实装 | [`client.rs`](../base/runtime/rr-bus-api/src/client.rs)（**批N9 件一基-35 窄化②：自 `rr-bus` 逐字迁入承载契约面 crate `rr-bus-api`，`#[cfg(unix)]` 门随迁——非 unix 目标本面编译为空壳，基-33 断点①随之消解；语义常量真值单点在 `rr-core`，rr-bus 收窄为常量 re-export 层**）：`rr3_connect`（读 manifest → UDS 连接 → 注册 → 独立线程喂心跳）、`rr3_task` / `rr3_name`、`rr3_call` / `rr3_publish` / `rr3_reply_call`、`rr3_poll` / `rr3_poll_wait` / `rr3_try_recv`、`rr3_connected` 死亡探针（断开后 `poll_wait` 返回空 Vec 与"暂时没帧"不可区分，事件循环必须探——两个真入口都探了）、`rr3_frame_body` 解帧；manifest entry 相对路径按所在目录绝对化（rrd spawn 注入的契约环境变量）。6 条用例，无平台门 |
| HAL 设备面（音频/舵机(PWM)/数字输出/显示/电源/链路） | 已实装（sim 全 + linux 真后端） | **五面 sim 后端全实装；linux 侧真后端批F5a 五件落地（PWM / GPIO / 显示占位 / 电源 / 链路，音频 ALSA 更早）——板上单测与真件证据欠（板离线，见"本机无法收口"第 7 条）**。[`lib.rs`](../base/runtime/rr-hal/src/lib.rs) 的 trait 面与 `rr3_block_on`（同步桥接，**不引 async 运行时**——ADR-0008 裁定 1）；批F5a③ 增 `Rr3Display` trait 与 `Rr3FaceFrame = [u8; 8]` 帧类型（帧类型归 HAL 事实源，[interact face.rs](../modules/rr3-interact/atoms/face/face.rs)（批C③ 重铺前为 src/face.rs） re-export——帧表留模块面，`Rr3Face` 不立，评审定案）；[`sim.rs`](../base/runtime/rr-hal/src/sim.rs)：音频三种波形（正弦 / 噪声 / 静音）+ M4 burst（1s 说/1s 停，让 VAD 周期性发 onset；门控按**帧序号**算偶秒——多通道交错样点折半缺陷批F4 `379e6de` 修，mono golden 不动）+ 批F2 双麦方位注入（`RR3_HAL_SIM_DOA_DEG`，ILD 增益模型 g=1∓sinθ）+ T4a sim 舵机（记录末次落角不判行程）与 sim 数字输出（电平 round-trip）+ T4b `Rr3SimBattery`（电量/充电态记账，电压曲线钉死 3200mV + 10mV/1%）+ T4c `Rr3SimLink`（up/ssid/rssi 三元组，**断链时设备面强制抹掉 ssid/rssi**——不许从死链读网络信息）；[`backend.rs`](../base/runtime/rr-hal/src/backend.rs) 选择器八件——`rr3_hal_backend_resolve` + 音频 source/sink + `rr3_hal_servo` / `rr3_hal_gpio_out(role)` / `rr3_hal_display` / `rr3_hal_power` / `rr3_hal_link`；**平台门订正（批F5a① `acc548e`）**：alsa 门只判音频面、能力面各判各的——`cfg(target_os = "linux")` 走 linux 后端（非 linux 平台显式 `RR3_HAL_BACKEND=linux` 回 Unsupported，模块探活 exit(1) 属诚实行为）；sim 按 env 起表（`RR3_HAL_SIM_BATTERY_PERCENT/CHARGING` 缺省 80/未充电、`RR3_HAL_SIM_LINK_UP/RSSI` 缺省在线/-50，取值裁决是独立纯函数）。linux 真后端五件全单门 `#[cfg(target_os = "linux")]`（宿主 macOS 不参与编译；验证 = aarch64 musl 交叉 check，**板上 cargo test 补跑欠账**——板离线，rsync 口径同批F1）：[`linux.rs`](../base/runtime/rr-hal/src/linux.rs) ALSA（`cfg(all(target_os = "linux", feature = "alsa"))` 门 + `RR3_HAL_ALSA_DEVICE`）；[`linux_pwm.rs`](../base/runtime/rr-hal/src/linux_pwm.rs) sysfs PWM 舵机（批F5a① `acc548e`：`RR3_HAL_PWM_CHIP/CHANNEL` 缺省 pwmchip0/pwm0=GPIO12（R22 定案硬件 PWM 脚），50Hz / 0.5–2.5ms↔0–180°，ensure 落中位 90° 与 sim 同观测，Drop 停脉冲 + 归还已导出通道）；[`linux_gpio.rs`](../base/runtime/rr-hal/src/linux_gpio.rs) chardev UAPI v1（批F5a② `da52e7f`：ioctl 号与 `#[repr(C)]` 布局单测钉死内核定值；`rr3_hal_gpio_out` 带 role 参数（"led"/"relay" 白名单）免顺序耦合；`RR3_HAL_GPIO_CHIP` + `RR3_HAL_GPIO_LED_LINE/RELAY_LINE` 缺省 4/17（R8 预算表软件路，真件档装配单可 env 覆写）；ensure 首调请求 OUTPUT 落初值 0 与 sim 同观测；EBUSY→Busy 其余→InvalidParam；Drop close 释放 line）；[`linux_power.rs`](../base/runtime/rr-hal/src/linux_power.rs) INA219 I2C 电池（批F5a④ `8f4d928`：`RR3_HAL_I2C_BUS` / `RR3_HAL_INA219_ADDR`；I2C_SLAVE 自定义常量钉死 i2c-dev.h 定值（libc musl 面不导出，照 GPIO 批先例）；bus_mv = (raw>>3)×4（13-bit / LSB 4mV）；cell_percent 3200–4200mV↔0–100% 与 sim 同一条线；charging 判定诚实恒 false 待真件档两点标定）；[`linux_link.rs`](../base/runtime/rr-hal/src/linux_link.rs) NetworkManager nmcli 子进程承载（批F5a⑤ `faf01be`：`RR3_HAL_LINK_IFNAME` 缺省 wlan0；terse 转义解析单测钉死；SSID 以 wifi list IN-USE 行优先、失败回落 CONNECTION；SIGNAL 质量百分比→dBm 经验折算注释钉死；断链强制抹 ssid/rssi 对齐 sim 语义）。libc 仅 extern 声明壳（GPIO/电源两件——musl 静态零额外 C 依赖，R13）。**板上单测与真件读数/动作证据欠（板离线 + 外设采购中，见"本机无法收口"第 7 条）；MAX7219 SPI 显示真后端归真件档（linux 侧现落占位）** |
| A/B OTA 切换与恢复出厂 | 已实装（主机面） | **限定：引导配置改写的主机面已实装；U-Boot 环境变量写侧与块设备探测属板上件不 claim**。0.0.3 批D（`68220e4`，Q3 裁定）：[`bootconf.rs`](../base/runtime/rr-ota/src/bootconf.rs)——`switch <rr_a\|rr_b>` 改写 extlinux.conf 的 `append root=LABEL=rr_X` 标签做**持久** A/B 切换（只动生效行那一个标签、注释与其余字节一字不动；同槽幂等拒 / rr_factory 目标拒走 factory / 缺·歧义·杂牌 token 报形状漂移拒）、`factory` 切 `rr_factory` 做**一次性**出厂引导（rr_factory 分区本体任何代码路径不得写入——partition-layout.md 只读约束不破；"回得来"的职责在 rr_factory 镜像自带恢复流程）、原子落盘（读 → 改写 → 同目录临时文件 → rename）、cmdline 当前槽解析（运行槽权威来源是 `/proc/cmdline`，只认 `root=LABEL=rr_` 形态，裸设备形态报"板上核对项"不覆盖；读取入口挂 `cfg(linux)` 门、解析逻辑收 `&str` 全平台单测）；extlinux.conf 真工件 `include_str!` 字节级对拍（可逆律 / 对合律 / 长度差恰为标签差）。CLI [`rr3-ota-ctl`](../base/ota/rr3-ota-ctl/src/main.rs)（薄壳：判定全在 rr-ota::bootconf）：slots / switch / factory 三子命令、`--bootconf` 必传不缺省碰任何路径、退出码 0 成功 / 1 业务拒 / 2 用法错。类型形状与分区标签守护（0.0.1 起）：[`rr-ota/src/lib.rs`](../base/runtime/rr-ota/src/lib.rs) 的 `Rr3Slot` / 对合律 / `ota_labels_match_genimage_cfg`（include_str 逐字钉死）。**未做（不 claim）**：U-Boot env 写侧（bootcount / bootlimit / altbootcmd / upgrade_available 自动回滚链——标准语义与断电安全序落 [`partition-layout.md`](../base/ota/partition-layout.md) 增补段）、块设备/挂载探测、落盘 fsync 加固、槽位镜像写入（往 rr_b 落整镜像，随升级下载链另批）、实板验证——均列 partition-layout.md 板上核对项清单（首条即 vfat rename 原子性弱于 unix 文件系统的诚实敞口） |
| Buildroot OS 层（defconfig / 分区布局 / 引导链 / 烧录文档） | 骨架 | **结构就绪、构建与实板验证未发生**（硬件未采购）。OS 层已按 ADR-0013 拆双轨（`{board,configs,patches}` 平移 `buildroot/`，`distro/` 快路轨）。buildroot 轨已落树：[`configs/rock3_zero3_defconfig`](../base/os/buildroot/configs/rock3_zero3_defconfig)（Buildroot + Linux 6.6 + aarch64，BusyBox init，ALSA/Python，post-build/post-image 挂钩）；[`genimage.cfg`](../base/os/buildroot/board/radxa-zero3/genimage.cfg)（boot + `rr_a`/`rr_b`/`rr_factory`/`rr_staging`/data 六分区；容量一律 `@XXX_SIZE@` 构建期占位符，由 [`post-image.sh`](../base/os/buildroot/board/radxa-zero3/post-image.sh) 注入并做残留检查）；[`extlinux.conf`](../base/os/buildroot/board/radxa-zero3/extlinux.conf)（root=LABEL=rr_a；A/B 持久切换由 rr3-ota-ctl 改写本文件 append 行标签——批D 主机面实装；bootcount/altbootcmd 自动回滚链属板上件，见 partition-layout.md 增补段）；[`post-build.sh`](../base/os/buildroot/board/radxa-zero3/post-build.sh) **装机布局已收口（`28c629a` 保底轨修复批）**：manifest 归位 `/usr/lib/rr3/modules/<name>/module.toml`（不再平铺 `/usr/lib/rr3/manifest/`——旧布局下模块 `rr3_config_load` 读 manifest 同目录 config 必撞同名文件、bin 相对 entry 解析必错位）、entry 装机绝对化（与 rr3-pack `rr3_manifest_install_rewrite` 同语义三道门：entry 行恰一条 / 值可装机拒相对与 `.` `..` 段 / 改写后 sed 重提复核，除 entry 行外与仓源逐字节一致）、config 从仓源随装（env `RR3_REPO_ROOT` 可覆盖仓根）、bin 按 entry 落位、S99rrd 的 manifest glob 改 `modules/*/module.toml`；**基-23 装机面齐装改判（2026-10-20，`ff2f318`）**：安装根全迁 `/usr/lib/rockrobot3`（与 deb 轨同名——modules-root 由装配单路径上两级推导是物理前件；`/var/lib/rr3/staging` 是分区挂载点不属安装位、不随根动）、S99rrd 改传 `--socket`/`--contracts`/`--profile` 三参数（与 rrd.service ExecStart 逐字同源，`--manifest` 循环删除——与 `--profile` 同给 parse 即拒）＋机型档 distro.toml（RK3566 host 档）随装、真值集三目录（profiles/contracts/hardware）落盘断言进场（装配冒烟 22 项断言）；rrd 侧 `--profile` 升必选双缺即拒（`8b1390c`）；**缺件口径**：缺 bin / 缺 manifest / entry 不可装机 = 警告 + **整模块跳过**（manifest 一并不装——缺件还装 manifest 会让 rrd spawn 必退出的模块成重启环；"缺席只警告"仍指镜像结构可构建），config 缺席仍只警告；装配冒烟 [`tools/ci/buildroot-assembly-smoke.sh`](../tools/ci/buildroot-assembly-smoke.sh) 13 项断言进 CI `buildroot-assembly` job；[`flash-maskrom.md`](../base/os/buildroot/board/radxa-zero3/flash-maskrom.md)（rkdeveloptool 操作序）；[`patches/README.md`](../base/os/buildroot/patches/README.md)（dts/defconfig 核对清单，无补丁本体）。distro 快路轨：[`base/os/distro/README.md`](../base/os/distro/README.md) + [`units/rrd.service`](../base/os/distro/units/rrd.service) + [`profiles/distro.toml`](../profiles/distro.toml) + [`tools/rr3-pack`](../tools/rr3-pack/src/main.rs)（见上行 deb 打包链行）。分区标签与代码常量的逐字一致性由 `rr-ota` 的 include_str 守护用例钉死。缺的手：全量 make 构建（需 Linux/容器）、dts 与 U-Boot defconfig 核对、串口出 shell、maskrom 实烧——均属板上/Linux 侧收口 |
| 发行版快路轨 deb 打包链（tools/rr3-pack） | 已实装 | T5（`a95eda7`，ADR-0013 裁定 5 首证，0.0.2 方案 D8）。独立工程（七步门 ④ 第 10 个；依赖 rrd lib 只为 manifest 解析——解析真值只有 rrd 运行期那一册，打包器不带第二份读取逻辑）。核心裁定：**entry 装机改写**（`rr3_manifest_install_rewrite`）——线上注册门禁（conn.rs）要求 manifest.entry 绝对路径，而模块开机读的就是装机 manifest 原文上报、相对值必被 AIRY_EINVAL 拒；打包器只改 entry 引号内一个值（三道门：entry 行恰一条 / 装机值可装机拒相对与 `..` 逃逸段 / 改写后重提复核），测试对"除 entry 外与注入原件逐字节一致"字节级钉死。缺件点名（rrd 缺席 / 模块缺 bin / 无模块 / 缺 distro.toml / 缺 config.toml——模块开机读 manifest 同目录 config，缺件必重启环，随包必带）+ 目录名与 manifest.name 不一致拒。`--stage-only` 供 macOS 本机装配演练（本机无法 `dpkg -i`，真装判据归 CI `distro-smoke` job：rust:bookworm 容器内构建 → 出 deb → 装机 → rrd 装配单正门起 5 模块 + brain 缺格点名（P2/D2 语义首证）+ 0 拒 boot + `denied_publish = 0`）。独立工程单测 5 条（④ 步 check，conventions §2 另登记）。0.0.6 批B（`5ecaa8d`）：deb 版本常量升 **0.0.6**（`RR3_DEB_VERSION`，单测 control 断言随改）+ 新增 [`tools/deploy/pack-arm64.sh`](../tools/deploy/pack-arm64.sh) 宿主交叉出包链（aarch64-unknown-linux-**musl** 静态 + rust-lld——macOS host 无 aarch64 glibc sysroot，musl std 自带完整 libc 是零安装路；三步：交叉编译 → 注入装配 → `rr3-pack --stage-only`，staging 断言含"aarch64 静态 ELF 体检 / entry 已装机绝对化"两条）。**真板实测（0.0.6 批D，`efd224f`）**：树莓派（pi@192.168.124.52，主机名 spharx，Debian 13 trixie / aarch64）板上 `dpkg-deb --build` + `dpkg -i` 成功（postinst 自动 enable），五模块全 running、`rr3-ctl` 三查实值、reboot 自恢复全绿——"同一 payload、不改一行产品代码，装进 Layer 0 完全不同的板"实测成立。**真板首发现两条缺陷（0.0.6 批D 实测，R3/R6 裁定归属）**：①boots 数据目录——**已修复并真板复验通过（0.0.6 批E'，`a5b3093`+`f2597d5`）**：[rrd.service](../base/os/distro/units/rrd.service) 注入 `Environment=RR3_DATA_DIR=/var/lib/rockrobot3` + postinst 建目录置属主（rr3-pack 模板，staging 单测断言）+ distro-smoke 断言链补 ⑧"counters 回写"（日志逐字 + counters.json 含 `"boots"` 键）；板上升级 restart 后 `{"boots":1}` 落盘、reboot 自恢复后 `{"boots":2}` **跨重启累积成立**；②`/var/run/rock3-ipc.sock` = `srwxr-xr-x root:root`（rrd root umask）非 root 运维连接被拒——**已实装修复（0.0.6 批F5b `fab7b6c`，ADR-0010 修订 1）**：rrd bind 后 `chmod 0o660` + `chgrp rockrobot3`（postinst 幂等 `addgroup --system rockrobot3`；组不在位留痕跳过不拒启，root 恒可连）；unit 同批注入 `RR3_REBOOT_OK=1` + `Restart=on-failure→always`/`RestartSec=2`（方案目标形态"批F5b 与 R12 同批落"）。**真机复验留板上线**：非 root `rr3-ctl` 直连不再 Permission denied，随判据 14/15 运维链验收同跑；复验通过前板上巡检仍按 `sudo rr3-ctl …` 口径（[boards §7](../base/os/distro/boards/raspberry-pi-64.md) 修复状态条目） |
| `rr3-sense` / `rr3-interact` | 已实装（sim 侧） | [`modules/rr3-sense/src/main.rs`](../modules/rr3-sense/src/main.rs)：注册/心跳 → 按后端开拾音面（批F2 `e4ccbaf` 增 `audio/doa.rs`：**下混前**双麦 ILD 反演 ±90° 分三档（|deg|≤30=center），vad_start onset 负载可选携 `doa{deg,bin}`——schema append 零新契约，R14 双麦下混 mono 前判向）→ 自 pacing 采集循环（sim read 即时返回、ALSA read 自耗一拍，两种后端同一形状）→ pcm STREAM + VAD 边沿上行 → `capture/dump` 点名应答（模块用例条数登记 conventions §2 + golden PCM 向量）；[`modules/rr3-interact/src/main.rs`](../modules/rr3-interact/src/main.rs)：注册/心跳 → 占位 TTS（440Hz burst，60ms/字符封顶 10s）+ 七态表情机（批F3 `444c764`：五态增 listen/turn 可点名，talk 仍 TTS 自动驱动不可点名；每态 8x8 帧序列，主循环 ~250ms 墙钟节拍推帧不随总线流量漂移）+ **显示出帧接线（批F5a③ `bcc4de2`）**：开机 HAL 显示面探活 + 主循环经 `Rr3Display` 出帧（sim 内存位图）应答两个 rpc 点名 + **Web 操作面板（0.0.4 批C+批D，`7aa7fc8`/`3682bfe`）**：[`panel/server.rs`](../modules/rr3-interact/src/panel/server.rs) std 手写 HTTP（端点全集唯一真值 = 同文件 `RR3_PANEL_ROUTES` 常量表——MC-8③/I-9 起端点数由表长推导、文档不手写计数；状态/硬件/系统三页 + WiFi 配网页——scan GET / connect POST / forget POST 全部 BusCall 透传，参数裁决在 owner 模块）、单文件静态页 `include_str!` 内嵌四页签（[`panel/www/index.html`](../modules/rr3-interact/src/panel/www/index.html)）、`rr3.ctl.reboot` 面板触发双重门、监听面 `RR3_PANEL_ADDR` 优先缺省回环（局域网可信面声明在 README）、beep/face 经 mpsc 动作通道主循环串行代执行（sink/face 所有权不出主循环）、net/link 事件缓存进状态页。两者失败语义同一：**业务性失败折 error 应答回话，传输性失败 `exit(1)` 交 supervisor**（不装"活着"）。**板上 ALSA 采集/回放未发生（硬件未采购）**；三真进程整链证据在 [`e2e_audio_chain.rs`](../base/runtime/rrd/tests/e2e_audio_chain.rs)。**0.0.8 W-A 内部分层已收口（批N25–30）**：本行所列模块已按五层标准拆分——main ≤80 行装配＋`boot.rs`（接入）/`rpc.rs`（协议）/`dispatch.rs`（机制）分层＋公共骨架 crate `rr3mod`（`bootstrap`/`serve`/`Dispatcher`/`BusHandle`，[ADR-0017](process/adr/ADR-0017-模块内部分层-机制与策略分离.md) 裁定 1 API 面）；线上语义与装配面零改动，SURFACE 启动自述逐字 diff 空白存档，六条核销表落 [0.0.8 方案状态行](process/dev/0.0.8版本洛克三号模块内部分层实施方案.md) |
| 机器人功能可达性（F-1…F-8 清单口径，2026-10-12 新增本行） | **六条可达 / 一条不可达**（骨架） | 清单与逐条判据的唯一真值 = [0.0.5 方案 §18.1](process/dev/0.0.5版本洛克三号对齐与裁定方案.md)（版本边界三窗分离后新增的验收面，见 [ADR-0020](process/adr/ADR-0020-版本边界-开发收口与实机验证与验证后整理.md)）。**可达（欠板上证据）**：F-1 舵机转向（`linux_pwm.rs` sysfs 后端 + `rr3_travel_clamp` 判定 + `servo/state` 限速流）、F-3 语音输入（ALSA 采集 + VAD 边沿上总线，欠真麦与真 key）、F-7 生命周期（supervisor `restart_policy` + 断电 boots 累积，0.0.6 批D/E'/F5b 实装）、运维面 F-7 附带的 `rr3-ctl` 六子命令、**F-4 真人声播报 = 已结（2026-10-13，MC-1 四提交 `eefc0de`/`ed8fb2f`/`f959e3a`/`70e8fb6` + MC-2 `1930956`）**——brain `rr3_finish_turn` 已按 20ms/640B 分帧发布 `rr3-brain/audio/tts_pcm`（seq 模块级单调、`ms` 由样点数导出）、`tts/text` 点名 args 增 `pcm_bytes` 作收流完整性唯一判据；interact 收流解码入缓冲、点名对账排空（半流/残留皆拒因 + 清缓冲 + `tts_play_reject` 计数）后写 `Rr3AudioSink`，起播水位 `prebuffer_ms`（legacy 0cf029e min 语义）与增益 `volume_pct`（整型饱和）两参数实装，占位正弦与 `RR3_TTS_MS_PER_CHAR` 已删除（**台-2 清零**；欠板上实录，闭环余量 = 本地 Piper 引擎归增补轮五批E）、**F-6 低电禁动 = 已结（2026-10-13，MC-5 `071dcad`/`8b5b57e`）**——`Rr3LowGate` 接 `rr3-runtime/system/power`，执行面 RPC/intent 两路拒动、妆面不禁（欠板上实录）、**F-2 表情上屏 = 判据面兑现（2026-10-13，MC-3 复核）**——`rr3_hal_display` 的 `RR3_BACKEND_ESP32` 桥后端支（批B `5280fe0` 出字 + 批C `60a8b81` 帧进城）+ 开机自述真后端名 + fail-closed 探活，**台-3 清零**（欠板上实录，真屏对账归 0.0.6 判据 10/接线 3；余数 = 帧表 R-12 迁移，归属待裁）。**不可达（欠码，非欠载体——这条区分是本行的全部意义）**：**F-8 微型形态板侧件** = 件与装配单真跑未发生——装配演练面在位（`profiles/esp32-mini.toml` 三格 + contract.rs 形状 pin + `rr3-pack` 正例），MC-7 复核（2026-10-13）不勾：件承载三缺——缺① xtensa 门已随批N8 件三收、缺③ `[hardware]` 进位已随批N9 件五收、缺② 板侧固件 crate 收编**码面半边已随批N23 收**（`atoms/{servo,face,capture}/mech.rs` 三原子件目录形态＋格位对齐申报 servo↔act.servo / face↔ui.face / capture↔audio.capture），件与装配单**真跑未发生**（触发条件 = 0.0.7 轻承载档真板窗）。另有两处**未登记的站替面**已按索引 R-16 落单并**已由批M 处置**：`Rr3SimProbe` 恒真（缺格降级环不转）→ **MC-4 已结**（`084d081`：`Rr3LinuxProbe` 五面实装 + `RR3_PROBE` 显式选择口，sim 探针不再当缺省构造）、配网凭据不落盘 → **MC-6 已结**（`084d081`：后端承载持久化，件内不自写文件）；`net.bt` 骨架格位台账已按 MC-9 裁定 (a) 改记 `placeholder`（触发条件 = 配网备选通道立项 / BlueZ 真后端批，`ba90367`） |
| `rr3-act` / `rr3-connect` / `rr3-power` | 已实装（sim 侧） | 0.0.2 T4a/T4b/T4c（`c8297b1` / `e87a284` / `2b08994`，方案 Q1–Q4 裁定）+ 0.0.3 批C（`be19982`，T3 补裁定），失败语义与 sense/interact 同一（业务折 error 应答、传输 `exit(1)` 交 supervisor）、HAL 开机探活（Unsupported 即 exit(1)）。[`rr3-act`](../modules/rr3-act/src/main.rs)：`servo/command`、`led/effect`、`relay/set` 三事务面点名应答（缺参/类型错/越界折 error 不断链）+ `servo/state` 限速流（`Rr3RateGate`，状态变化必发、两次至少隔 state_min_interval_ms）+ 进程内反射环宿主线程（饱和记账、episode 恰报一次）；**订阅执行面已实装（批C）**：订阅 `rr3-brain/intent/result` 逐条执行载荷 `actions[]`（元素 `{topic, args}`），[`intent.rs`](../modules/rr3-act/src/intent.rs) 两道归属门（exposes 归属门 + 意图面开放门）——越权动作拒执行折日志不断链（事件语义无应答回路）；批F2 起 `servo/command` 进意图开放面（`e4ccbaf`：判向-转向编排，转向落 `servo/state` 限速流，e2e 钉跨路 seq 连贯与限速窗纪律）；`relay/state` 状态流 append 在册（RPC 与 intent 两条驱动路共用 `relay_seq` 流序，变化即发不做限速门），线级 `e2e_act` 断言 RPC 路 seq=1 → intent 路 seq=2 跨路连贯 + 越权动作有界窗口否定断言。[`rr3-power`](../modules/rr3-power/src/main.rs)：`battery/state` 周期流（首拍即采）+ `battery/low` 边沿事件（越阈首拍恰报一次、严格大于才重新武装——武装若回 true 就会发出电压高于阈值的"低电"事件）+ `charger/state`（由最近电池读数 charging 位派生，首拍电池之前不发）；订阅面为空是设计（低压关断本地决策不依赖总线）；cutoff 阈值只解析回显不动作。[`rr3-connect`](../modules/rr3-connect/src/main.rs)：`net/link` 状态机周期流（up/ssid/rssi，首拍基线必发-同态不发-翻转必发；断链时设备面强制抹掉 ssid/rssi）+ `cloud/session` open 应答面（down 折 error 点名判据词"链路"，业务性失败不断链；云侧凭据联调范围外，`RR3_BRAIN_CLOUD=mock` 注入点维持）+ **配网面三 RPC（0.0.4 批D，`3682bfe`）**：[`wifi.rs`](../modules/rr3-connect/atoms/wifi/mech.rs)（批C④ 重铺前为 src/wifi.rs）的 `wifi/scan`（sim 确定性三行列表、`backend:"sim"` 诚实标注，元素三字段照搬 legacy panel.c h_scan 形状）/ `wifi/connect`（legacy 字段名 `ssid`/`pass` 与"密码至少 8 位"原词；`connected` 如实取 HAL 链路读数不 fake"连上了"——"等连后回真实结果"的等待语义由 net/link 流承载）/ `wifi/forget`（清凭据回 ap，承接 legacy factory_reset 的 WiFi 语义、≠ M6 整机恢复出厂）+ `Rr3Provision` 配网状态机（ap|station 两态纯函数，模块内策略不进 HAL——HAL 只如实报数；凭据内存不落盘重启即忘、真持久化 wpa_supplicant/NVS 板上后置，如实声明；`mode` 字段随 net/link 流发布，契约只增不改）+ **蓝牙链路状态机骨架（0.0.4 批E，`586641d`）**：[`bt.rs`](../modules/rr3-connect/atoms/bt/mech.rs)（批C④ 重铺前为 src/bt.rs） `Rr3BtState` 三态（off/discoverable/connected）转移矩阵纯函数——非法转移（connected 直接断电等）拒绝且状态不动；sim 初值 `RR3_HAL_SIM_BT_UP` env 注入（取值非 "1" 一律按关，env 是 sim 注入口不是配置面）；主题不登记（btctl 日志回放/远程命令语义已被 rrd 日志环 + rr3-ctl 承接，D5 裁定不预先占位），BlueZ 真后端板上后置。独立工程单测 act / power / connect（条数登记 conventions §2，④ 步 check）；线级 `e2e_act` / `e2e_power` 1 / `e2e_connect` 2（装配单正门起真模块，`#![cfg(unix)]`）。各目录 `module.toml` 仍由 [`tests/contract.rs`](../base/runtime/rr-registry/tests/contract.rs) 逐目录读回判合规。**板上真件证据欠**（外设采购中 + 板离线欠账；HAL linux 真后端已就位——批F5a GPIO(led/relay)/PWM 舵机/link(nmcli)/battery(INA219)，真件读数/动作证据随采购到位，见"本机无法收口"第 7/11 条）。**0.0.8 W-A 内部分层已收口（批N25–30）**：本行所列模块已按五层标准拆分——main ≤80 行装配＋`boot.rs`（接入）/`rpc.rs`（协议）/`dispatch.rs`（机制）分层＋公共骨架 crate `rr3mod`（`bootstrap`/`serve`/`Dispatcher`/`BusHandle`，[ADR-0017](process/adr/ADR-0017-模块内部分层-机制与策略分离.md) 裁定 1 API 面）；线上语义与装配面零改动，SURFACE 启动自述逐字 diff 空白存档，六条核销表落 [0.0.8 方案状态行](process/dev/0.0.8版本洛克三号模块内部分层实施方案.md) |
| `rr3-brain` | 已实装 | **批N32 起 lang=rust**（ADR-0016 迁移路线收口：Python stdlib-only 前史见下——零 pip 依赖承 ADR-0014，Python 树已退役，现状见 [README](../modules/rr3-brain/README.md)；下述 Python 面为历史陈述，文件已删）。入口 `rr3_brain/__main__.py`（注册 / 心跳 / 事件循环 / 对话轮完成时发布 intent/result）；`bus.py`（386 行）——128B 头逐语义镜像 rr-ipc（struct 打包、Layout C 同偏移）、帧读写、注册握手、publish / subscribe / call 客户端；**意图规则面（0.0.3 批C，Q1 裁定——策略层唯一归属在 brain）**：`intent.py` 从 `user_text`（ASR 文本，唯一输入）抽出结构化 `actions[]`（元素 `{topic, args}`，词汇即注册表零新造；精确词表 + 两字符否定护栏，3.9 兼容语法；批F2 `e4ccbaf` 词表 append："向左/向右"→`servo/command` 转向动作，mock ASR 文本注入 env 供演练）；载荷 append `actions`（空数组 = 纯对话轮）；`cloud/` 三家客户端（asr / llm / tts）urllib 真调用 + **`RR3_BRAIN_CLOUD=mock` 确定性后端**（默认 `real`，测试注入点）；`dialog.py` 对话状态机（vad_start → 采集 → asr → llm → tts 回路）；配置见 [`config.toml`](../modules/rr3-brain/config.toml)。stdlib unittest 全量条数与平台差额登记 conventions §2（本机 py3.9 实测 22 过 + 1 条 `tomllib` 门槛 error；`test_intent` 在门槛内全绿；CI brain-unittest 钉 3.12 跑全量——**job 随批N32 Python 树退役**）；线级证据：`rr3-sim --brain`（[`tools/sim/src/brain.rs`](../tools/sim/src/brain.rs)）**四真进程**（rrd + brain + sense(sim) + interact(sim)）全语音对话闭环演练实跑通过（5ab029e / 5b55620）；批F4 `rr3-sim --chain` **六真进程** mock 云全链闭环（`379e6de`，判向→转向→表情无人工干预一轮以上，真板实测 boards §9）。运行门槛：python3 ≥ 3.11（`tomllib`——随 Python 树退役，rr3-sim 的 `which_python3` 能力验证链同批退役）。**三家云的真实凭据联调未发生**（无 key 环境，mock 之外的云端路径零实测）。规约注记：ADR-0016 迁移路线已于批N32 件四收口——module.toml lang=rust，Python 树与 brain-unittest job 退役，brain cargo test 全绿（终值登记 conventions §2） |
| `rr3-ctl` | 已实装 | **限定：查询面只读；ota 恒拒（点 M6）；reboot 双重门缺省恒拒**。rrd 侧（ADR-0010）：`rr3.ctl.*` **六方法**（模块四 + 控制二——`rr3.ctl.counters`（0.0.4 批B）/ `rr3.ctl.reboot`（0.0.4 批C，Q1 裁定）append，白名单一册在 [`rpc.rs`](../base/runtime/rrd/src/rpc.rs)，`is_ctl` 判据与方法面一致）——握手在 Register 门前按 `is_ctl` 分流，**ctl 连接不注册、不进在册表、一问一答即断**；status 走在册表快照（`rr3_reg_health_snapshot`，ctl 连接自己不得进快照）；health 回显条目自己的 heartbeat / watchdog / `since_beat_ms` 三数；logs 读 `rr3_log` 单点落的 512 行内存日志环；counters 回聚合持久化的全表快照（见"运维计数面"行）；reboot 执行面**双重门**（[`conn.rs`](../base/runtime/rrd/src/conn.rs) `rr3_reboot_unlocked`：`cfg(linux)` **且** env `RR3_REBOOT_OK=1` 才真跑 `systemctl reboot`，缺省恒拒——解锁面留装机环境，本机 sim 直跑永不满足；**实况收口（0.0.6 批F5b `fab7b6c`）**：[rrd.service](../base/os/distro/units/rrd.service) 已注入 `RR3_REBOOT_OK=1`（批E 核出的注释↔unit 漂移消除），板上 ctl.reboot 解锁面就位；真机执行验收（判据 14/15）留板上线）+ 拒绝口径与解锁口径都进日志环可查；ota 恒拒 `Permission(-22)`（契约冻结集无 NotSupported，文案点 M6——0.0.3 批D 后 rr3-ota-ctl 写侧主机面已实装，rrd 侧 ctl.ota 的接入仍是后续批）；已注册连接上行 ctl 被 `Permission` 拒但**同一条连接心跳照常被接受**（拒帧不断连）。rrctl 侧：[`tools/rrctl`](../tools/rrctl/src/commands.rs) 独立工程**六子命令**与 rrd ctl 白名单逐一同名对齐（0.0.5 批B，`25e90b7`，Q1 裁定：COMMANDS 4→6，`counters`/`reboot` append + 方法名白名单钉死单测）——UDS 一次性查询连接、5s 读超时、人读表渲染；counters 按模块前缀分组成 MODULE/KEY/VALUE 表（rrd 自有键归 `rrd` 组、空表打"无计数"、非 u64 报错不渲染 `?`——与 status 的 cell 容错口径有意分开）；reboot **CLI 不加本地门**（rrd 执行面双重门是唯一真值点，CLI 加门即第二真值），拒绝文案经错误信封回显非 0 退出、成功回执一行确认。独立测试（含 fake-rrd 真 UDS 往返 4 条：status/counters 实答渲染、ota/reboot 恒拒回显）不在七步门内。线级证据：[`e2e_ctl.rs`](../base/runtime/rrd/tests/e2e_ctl.rs) 8 条（F3 六条 + 0.0.4 append 两条：`ctl_reboot_is_refused_by_default_double_gate` 钉缺省恒拒——真执行面 `systemctl reboot` 挂 cfg(linux)，本机 macOS 不参与编译、e2e 只能钉拒半边；`ctl_counters_aggregates_heartbeat_counters` 钉心跳可选字段兼容 + 同键跨拍累加 + ctl 读回；counters 持久化跨重启另有 `counters_persist_across_restart_and_boots_accumulate`）。字面量归 [`reference/rrd-rpc.md`](reference/rrd-rpc.md) §7 §8 |
| `rr3-sim` | 已实装 | [`tools/sim`](../tools/sim/src/main.rs) 脚本化全链路演练入口，**四模式**：①默认两模块演练——起 rrd（boot spawn `rr3-sense` resident）→ 轮询 status 等 running → 外部 spawn `sim_peer` 发布 `rr3-interact/asr/text` → 四命令巡检（`denied_publish=0` 作发布被接受的线上证据）；②`--brain` 四真进程对话闭环（M4）；③`--profile` 装配演练（D9，`363e4ed`）——装配单驱动 rrd 装配链正门（`--profile` + `--contracts`，与 rrd.service 的 ExecStart 同形）boot 全部 face 模块，**演练 modules-root 是装机布局**：仓源 manifest 原文只把 entry 改写为产物绝对路径（rr3-pack `rr3_manifest_install_rewrite` 同语义门：装机值可装机 / entry 行恰一条 / 旧值与 facts 交叉核验 / 改写后重解析复核——"entry 在仓相对、装机绝对"的第三面）+ config 同目录就位（Rust 模块仓源 config、brain 用 mock 演练 config），python 模块产物 = shim（`python3 -m`，包名 = 目录名连字符转下划线；**sim 演练侧已随批N32 退役**——brain lang=rust 后仓内无 python 模块，rr3-pack 打包侧 shim 仍为机制保留位）；`--brain` 与 `--profile` 同给即拒（Q3 同款）；④`--chain` 六真进程全链 mock 闭环（批F4 `379e6de`，判据 10 sim 档载体）：rrd boot spawn `rr3-sense` + 外部 rr3-interact / rr3-brain / rr3-act + 演练自带总线观察者 rr3-obs，mock 云（R4）下无人工干预跑通"采集→意图→判向→转向→表情"一轮以上（真板实测 boards §9）；演练统一注入 `RR3_PANEL_ADDR=127.0.0.1:18080`（与板上系统实例面板端口避让，--brain 同享常量）；CI `sim-chain` job（第 8 job）钉远端回执。[`assemble.rs`](../tools/sim/src/assemble.rs) 的 `repo_default_faces_are_assembly_ready` 把**仓内 default 装配单↔模块目录对账**钉进单测（六模块 manifest 可解析 / name == 目录名 / entry 仓相对无 `..` / config 在）——装配面漂移让单测红，不等演练跑一半红。`which_python3` 探测链**已随批N32 退役**（2026-10-24：brain Rust crate 化后 `--brain` 直起真二进制 `modules/rr3-brain/target/debug/rr3-brain`，Python ≥3.11 门槛句为批E 时点历史陈述；原"PATH 第一个→能力验证"两段演进见 git 历史）。纯逻辑单测分居 drill / devices / assemble / chain 四模块（条数登记 conventions §2，全平台可跑）；编排（spawn 真进程，Unix-only）分居 `main.rs`。**六模块真进程判据（含 brain）归 CI `sim-assemble` job 远端回执**（macOS 本机 python3 3.9 不过 tomllib 门；无 brain 子集装配单 5 Rust 模块全链绿——装配 boot → 全 running → denied_publish=0 → 四命令巡检——已实证）。五步脚本见 [`tools/sim/README.md`](../tools/sim/README.md) |
| `contracts/events/*.schema.json` | 已实装（3 个） | [`sense-audio-vad_start.schema.json`](../contracts/events/sense-audio-vad_start.schema.json)（0.0.2；批F2 `e4ccbaf` onset 增可选 `doa{deg,bin}`——append 兼容 schema_version 仍 1，仅 onset 且判出时携带、offset 不带）：schema_version 1，$schema draft 2020-12，required `["seq","edge","speech_ms"]`，`rms_peak`/`doa` 仅 onset 携带（`allOf` + `if/then`），`additionalProperties false`；事实源 `rr3_vad_event_body`。0.0.3 批C 两件（T3 补裁定）：[`brain-intent-result.schema.json`](../contracts/events/brain-intent-result.schema.json)（required `["trace","user_text","reply_text","tts_ms","actions"]`；`actions` 元素 required `["topic","args"]` 且 `additionalProperties false`——词汇即注册表零新造）+ [`act-relay_state.schema.json`](../contracts/events/act-relay_state.schema.json)（required `["seq","on"]`——RPC 与 intent 两路共用状态流的摘要体）。**pcm STREAM 的 schema 未落**——0.0.5 Q3 裁定（`c7b91e7` 登记）维持空缺不冻结：schema 冻结与 rrd shm 接线绑定同一份后续 ADR（不写半成品），[contracts/README.md](../contracts/README.md) 演进登记在案 |

---

## 里程碑进度

"内容"与"验收标准"两栏按架构文档 **原 §10** 的里程碑表逐字抄录；那张表现在只有这一份——
架构文档 §10 已收口为一行指向本节（迁入与删原分两步做，是为了让删除那次改动单独可审）。
**当前实况**一栏是本快照读代码实测的结论，不是对验收标准的复述或期望。

| 里程碑 | 内容（原 §10 抄录） | 验收标准（原 §10 抄录） | 当前实况（`9c07b3e` 实测） |
|---|---|---|---|
| M0 | 仓库骨架 + ADR-0001~0005 + legacy 冻结 | 目录树落地，rr3-mkmod 能生成合规模块 | 目录树已落地，ADR 已写到 0016（e6b13c9 板卡策略、5f67008 brain 迁移路线，均超出原验收范围），`rr3-mkmod` 的产物被回灌进契约用例 —— **已收口** |
| M1 | base/os：Buildroot 在 Radxa ZERO 3 跑通 | 串口出 shell；maskrom 自烧录镜像成功 | **结构就绪，未收口（板上敞口）**。defconfig、genimage.cfg（容量=构建期占位符）、extlinux.conf、post-image、flash-maskrom.md、patches/ 核对清单已全部落树，分区标签守护用例进 rr-ota；post-build.sh 装机布局已收口（28c629a：manifest 归位 + entry 绝对化 + config 随装，装配冒烟进 CI `buildroot-assembly` job）；缺的是：全量 make 构建（Linux/容器侧）、dts 与 U-Boot defconfig 核对、串口出 shell、maskrom 实烧——目标硬件 Radxa ZERO 3 **未采购**，板上收口项在硬件到位前不 claim（证据见能力表"Buildroot OS 层"行） |
| M2 | rrd MVP：A-IPC 协议层 + uds transport + registry + sim 双假模块 | 注册/心跳/崩溃自动重启；IPC-T-001/002/003 通过 | **已收口（留一条环境敞口）**。验收标准逐条对照：注册/心跳/崩溃自动重启——**全到位**（注册/心跳/死亡记账/扇出被连接层调用；崩溃自动重启由唯一 supervisor 线程执行：boot spawn、串行收尸、到期重启、按策略退避，线级证据 `e2e_spawn_restart` 5 条）；IPC-T-001/002——同机 golden 往返 + 契约逐字段对齐，全绿；IPC-T-003——三方对拍 **①②本机已过**（F5：宿主 dump==golden 逐字节一致 + aarch64 交叉 check 且 const 布局断言已在该目标求值），**③容器真跑 SKIP**（本机无 Docker；`--strict` 已接远端 ipct003 job（540ec0f），远端跑绿一次即结，见"本机无法收口"第 4 条），这是 M2 唯一敞口。0.0.7 批B 起 `ipct003.sh` 扩**四路**、xtensa 编译期对拍路 PASS（能力表"非 Linux 主控契约 parity"行），容器路敞口不变。`trait Transport` 已随 ADR-0008 定型并落地 uds 实现；依赖仲裁与 shm 传输为 M2 之后的增量（ADR-0009 / ADR-0011），不计入本里程碑验收 |
| M3 | rr3-sense + rr3-interact：ALSA 采集与播放 | 板上录音回放，VAD 事件上总线，音频走 STREAM | **已收口（真麦板上证据敞口）**。sim 侧证据齐：`rr3-sense` / `rr3-interact` 实装（见能力表两行）、rr-bus 同步客户端、rrd 四方法 RPC 与 rr3.call 路由应答回路、**三真进程** e2e（vad_start 事件上总线、pcm STREAM 有序转发、点名应答回到调用方，`e2e_audio_chain` 绿）、七步门禁全绿（conventions §2 台账）；ALSA 后端代码与 feature 门配置就绪。**板上进展（0.0.6 批F1/F4）**：板上 release `--features alsa` 两件构建链实测过（动态 ELF + libasound 断言，boards §8）、板上 `rr3-sim --chain` sim 音源全链闭环（boards §9）——**真麦录音回放仍未发生**：USB 麦阵列未采购（R22 定案 PS3 Eye 或同级 ≥2 麦 UAC），板上载体已是树莓派（0.0.6 批D 起），见"本机无法收口"第 7 条，真件到位前不 claim |
| M4 | rr3-brain：移植 asr/llm/tts | 全语音对话闭环（legacy 功能等价） | **已收口（云侧联调敞口）**。`rr3-brain` Python 实装（5ab029e，ADR-0014；**批N32 起 lang=rust，Python 树退役——本行为 0.0.2 里程碑收口时点历史陈述，现状见能力表 `rr3-brain` 行**）：py 总线客户端 bus.py 386 行逐语义镜像 rr-ipc、三家云客户端 urllib 实装（`RR3_BRAIN_CLOUD=mock` 是测试注入点）、dialog 状态机串起 vad_start → asr → llm → tts 回路；线级证据 `rr3-sim --brain` 四真进程全语音对话闭环演练（5b55620）；批F4 `--chain` 六真进程 mock 云把对话轮扩到全链（判向→转向→表情，真板实测 boards §9）——详见能力表"`rr3-brain`"行。敞口一条：三家云真实凭据联调未发生（无 key 环境），不阻塞"legacy 功能等价"的 sim 侧验收。**2026-10-12 订正（版本边界轮）**：这句"legacy 功能等价"在**播报面**不成立——M4 按当时判据收口的是"对话闭环跑通 + TTS 验证性消费"（拿到 PCM、算出时长、入日志），而 legacy 机器人是**真的用人声说话**。今天云侧 PCM 没有通路到扬声器（`composite.py` 只发 `{"text"}`、interact 播 440 Hz 占位正弦），所以播报等价属**未达成**，落 0.0.5 批M MC-1/MC-2（F-4）。M4 的收口状态不回改，缺的是这条敞口的**指名**（**2026-10-13 补记：该敞口的通路半边已由 MC-1 结清**——brain 分帧发布 `rr3-brain/audio/tts_pcm`、interact 收流对账播放，占位正弦退役；播放面参数 `volume_pct`/`prebuffer_ms` 已随 MC-2 实装（`1930956`），闭环余量仅 = 本地 Piper 引擎（增补轮五批E）） |
| M5 | rr3-act / rr3-connect / rr3-power | 语音控继电器 + WiFi 配网 + 电量上报；shm 控制面初版 | **已收口（sim 侧；板上敞口同 M3）**。逐条对照验收标准：①"语音控继电器"——**sim 侧闭环已证（0.0.3 批C，`be19982`）**：意图→动作映射落 brain（`intent.py` 精确词表 + 否定护栏，Q1 裁定策略层唯一归属）、act 订阅执行面两道归属门（exposes 归属 + 意图面开放）执行 `actions[]`、`relay/state` 状态流 append 在册（T3 补裁定）且 e2e 断言 RPC 路 seq=1 → intent 路 seq=2 跨路连贯——语音进 → brain 抽动作 → act 执行 → 状态回读，整链在 `e2e_act` 线级绿；②WiFi 配网——T4c（`2b08994`）net/link 状态机 + cloud/session 应答面（`e2e_connect` 两场景对偶绿）；**0.0.4 批D（`3682bfe`）配网面收口（sim 侧）**：wifi/scan|connect|forget 三 RPC + `Rr3Provision` AP 配网状态机（ap|station 两态纯函数）+ net/link 流 `mode` 字段随发 + 面板 WiFi 页三端点透传（裁决面钉在 connect 模块单测，`e2e_connect` 既有两场景不动）；③电量上报——T4b（`e87a284`）battery/state 周期流 + battery/low 边沿 + rrd 低电兜底四件动作面（`e2e_power` 绿）；④"shm 控制面初版"——Q2 口径（2026-10-04）已达成：ADR-0011 transport 层落地（单写者多读者环形缓冲 + POSIX 附着），rrd 接线不纳入 M5 验收（如需纳入须立新 ADR）。**0.0.6 批F5a 进展**：HAL linux 真后端五件落地（GPIO led/relay、PWM 舵机、link nmcli、battery INA219——代码与 musl 交叉 check 就位，板上单测欠账）；批F4 `--chain` 板上 sim 侧全链闭环（boards §9）。敞口 = 真硬件动作证据（继电器/配网/电池读数，随外设采购 + 板上线，同 M3 口径）；意图词表的产品化扩展属 brain 演进，不在 M5 验收内 |
| M6 | A/B OTA + 恢复出厂 | 断电升级不 brick，坏 slot 自动回滚；genimage 预留模块独立可写分区 | **未收口（主机面已实装，板上件与升级链未做）**。0.0.3 批D（`68220e4`，Q3 裁定）：switch/factory 的判定与改写逻辑在 `rr-ota::bootconf` 主机可测（字节级真工件对拍），`rr3-ota-ctl` 三子命令实装（`--bootconf` 必传、退出码 0/1/2），genimage.cfg 的分区集合（含 rr_staging 独立可写分区）已随 M1 落树。验收标准"断电升级不 brick、坏 slot 自动回滚"依赖的 U-Boot 回滚链（bootcount / bootlimit / altbootcmd / upgrade_available）**语义已落档、写侧未实装**（板上件，partition-layout.md 增补段含标准断电安全序与板上核对项清单七条）；槽位镜像写入（往 rr_b 落整镜像）随升级下载链另批；容量定型与实板验证随硬件——三项在位前不 claim |

---

## 本机无法收口的项

这些**不是待办清单里的模糊项，是本机没有能力证明的项**。任何"应该是好的"的推断都不构成
收口证据，所以下面如实写明缺的是哪一手证据，不软化。

**第八轮起本表有两类行，读之前先看类别**（**第十轮 裁-2 订正：原写"第 1–19 行是缺证据类"既与"18 是缺裁定类首条"互斥、又滞后于后来新增的 20/21 两行 ⇒ 按"区间写法是易失分母"的既有纪律改列全枚举**）：
本表现共 **21 行**——**缺证据类＝第 1…17 行 ＋ 第 19…21 行（共 20 行）**；**缺裁定类＝只有第 18 行（共 1 行，且其**裁定半边已于 2026-10-17 结清**：`ui.face` 拆格已拍板——先立 SPI 词形规则、批N3 拆格 ＋ 门 n 扫面 7→8，裁定记录见 [decisions.md §2](process/decisions.md) 行 2；**余量只剩批N3 代码半边，收口即结项**）**。上面那段章程管的只有缺证据类。**2026-10-19 对表：第 18 行代码半边已随批N3 件六收口（`6953222`），18 行结项 ⇒ 缺裁定类清零**（裁定记录与施工注记见 [decisions.md §2](process/decisions.md) 行 2）。**2026-10-22 对表（批N11 件一）**：第 22 行（基-35 窄化①装配位上移——基-35 收口入表的缺裁定类第二例）已结项（不上移定案＋⑨-w 白名单条目数基线钉数，裁定记录见 [decisions.md §3](process/decisions.md) 第二行，**裁定预算清零**）⇒ 缺裁定类再次清零；第 23 行（窄化②）已随批N9 件一结项（见该行）；本段"共 21 行"计数系第 18 行结项时点口径，22/23 两行入表未回改分母（易失分母如实留痕，全枚举以行号为准）。
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
| 11 | 0.0.6 阶段二真机功能链的板上证据（语音采集 / DoA 判向 / 舵机转向 / 表情输出） | 树莓派上麦克风 / 舵机 / 显示真件到位后的一次全链实测 | **sim 档已收官**：批F2/F3 sim 段（`e4ccbaf` / `444c764`）+ 批F4 板上 `rr3-sim --chain` 全链 mock 闭环（boards §9，判据 10 sim 档过——R9 档位诚实标注：采购前档不充当真件收口证据）；批F5a 五件 HAL linux 真后端实装（musl 交叉 check 全过）。真件档随外设采购（BOM 定案 R22：PS3 Eye 或同级 ≥2 麦 UAC / SG90 / MAX7219——树莓派外设采购中、ESP32 硬件已齐）；真件到位只动 HAL 后端，云联调以 `RR3_BRAIN_CLOUD=mock` 先行（R4）——真件档验收是板上线后的第一优先敞口。**2026-10-28 T1 真机首验（Pi 4B 宿主 × ESP32 classic，条目 1/2/3/7/8）**：四真进程（rrd + sense(esp32) + brain + interact(linux sink)）全起、`denied_publish = 0`、3 轮 `对话轮完成` 且 Pi 扬声器出声（人耳）；OLED 写屏 + 胶囊眼 idle 与 pan/tilt 逐点落角肉眼确认；侧向说话触发 `rr3-sense` 反射环 `pan=60°/120°` 转头 + `tilt 90°→98°` 点头、板面逐条 ack（id=1..6）且**音频流未中断**——即「听声识位 → 转头」真机成立、命令下行走 `esp32audio` 命令旁路（flock 单消费者唯一通路）。余敞口：条目 6 唤醒率 9/10、7 三方向分箱 8/10、9 30 分钟滑移、4 轨压两点标定（板侧 GPIO34 分压未接，读数为悬空 395mV）、5 千次 ping |
| 12 | 三形态的**资源事实真值**（接线档 + 能源域阈值与分压比 + `[pins]` 绑定） | 装机完成后的机器可查 `wiring.toml` + 与万用表对拍（≤2%）的域电压/分压比 + 逐形态 `[pins]` 过装载校验 | [ADR-0021](process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md) 本轮只立**对象与校验形状**，取值全在待标定态：ESP32 的 GPIO34 分压测点**尚未接**（`ADC_RAIL_DIVIDER_NUM/DEN` 仍是占位 1:1、`RAIL_GATE_ARMED = false` 不接闸，这条"无真板对拍前不接闸不 claim"的纪律随契约面一起搬进 `[[supplies.domain]].armed`）；树莓派与 RK3566 今天**没有任何机器可查的接线表**，批N1 只能先交演练实例。另记一条**未闭的互斥**：板侧 UART2 RX 用的 GPIO19 与点阵 VSPI 预留同脚（`board.rs` 自述"点阵立项时需再迁，已在案"）——点阵一立即撞 |
| 13 | **微型形态（ESP32）的产线闭环四件**：烧录封装、镜像装配、OTA、回滚 | 一次"从干净机器到能出字/转动/听到声"的可复现流程记录 + 一份失败后的回滚实录 | 复盘实测：`tools/rr3-pack` 与 `tools/deploy/` 只覆盖 host aarch64，**ESP32 侧只有一条手工 `espflash` runner**（`.cargo/config.toml`），无镜像装配、无 OTA、无回滚面；`base/ota/rr3-ota-ctl` 写侧只到"改写 extlinux `root=LABEL`"，`bootcount`/`altbootcmd`/`upgrade_available` **只存在于文档表格**（`base/os/buildroot/patches/README.md` 与 `partition-layout.md` 自述未核敞口）。本机无 ESP-IDF、板不在手 ⇒ 这条**既缺码也缺载体**。裁定见 [ADR-0022](process/adr/ADR-0022-代码事实复盘-机制核纯净度与申报面逃逸与安全语义身体.md) 裁定 6，施工 = [全案](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §17 批N5；**"该形态不可回滚"若为终态，也要写成一句结论而不是空白** ／ **2026-10-22 批N9 件二：四件之"烧录封装"半边已落**——[flash-esp32.sh](../tools/deploy/flash-esp32.sh)（版本钉 espflash 4.x＋ELF 内 app descriptor 三查：节在册/尺寸 0x120/magic 0xABCD5432，`6d3a1ee` 先例口径；`--check-only` 干跑面供无板机器），脚本面本机干跑绿，烧录全链验证挂批N9 件三上板会话；镜像装配/OTA/回滚三件仍敞 ／ **2026-10-23 批N18：三件码面与语义全落**——件一 `1f03bb5`（镜像装配件 [image-esp32.sh](../tools/deploy/image-esp32.sh)：验证面复用 flash-esp32.sh --check-only 真值单点＋espflash save-image --merge 整片装配＋布局三点 magic/整片 4MB 自校验，本机干跑全 PASS）／件二 `90ac149`（OTA 语义明文裁定＝**重烧即升级**，NVS 凭据保留＝升级后免重配网；真增量 OTA 不做三理由＋触发条件，落 [partition-layout.md](../base/ota/partition-layout.md) 批N18 段；侥幸面实测登记＝debug app image 1,130,224 B 超 factory 槽 8.2%、release 444,576 B 槽内 42.4%，产线一律 --release；可达终点表升级格改判「能做到（重烧档）」）／件三 `63255c2`（回滚语义终态裁定＝**不可回滚**（无自动回滚链）：factory 单槽＋生成 sdkconfig 实测 `CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE is not set` ⇒ 自动回滚链为零，"回得来"两半边（重烧重试＋NVS 凭据保留）如实登记不与自动回滚混称，触发条件＝双槽分区表＋rollback enable 契约级评审——全案 §17 原文"不可回滚也是一句合格答案，但必须写进表"即本件；可达终点表回滚格改判「不能（终态裁定）」＋烧录格批N9 件二逾期补改判）；**本行维持不结项**：判据"从干净机器到能出字/转动/听到声的可复现流程记录"码面已齐（flash/image 两脚本＋本批干跑绿），"失败后的回滚实录"在不可回滚终态下语义改为重烧恢复实录，随下次上板会话取——不冒领 ／ **2026-10-23 批N20 真板实录收官，本行结项**（[批N20 计划册](process/dev/0.0.5增补轮六——批N20开发计划与产线真板实录.md) §2 逐件证据行原文）——判据两半边均取：①「从干净机器到能出字/转动/听到声」流程记录＝件一 `5222368`（image-esp32.sh --release 装配全链 PASS→`espflash write-bin 0` 直烧 merged 4MB 98s→boot 实录三达：OLED 4 行出字＋LEDC 姿态 60°/98° 回读＋双麦 16kHz rms 连续流；fail-honest 配置腿缺席点名实证＝批N13 语义真板回声）；②重烧恢复实录＝件二 `cecff5c`（POST 写口写 NVS→改横幅去注入重烧→`凭据源=nvs` 保持＋自动重连＝「重烧即升级」行为实证；`erase-flash` 全片擦→重烧→回落编译期源→重新在线＝「回得来」半边）。同批两笔事实订正（订正正文落 partition-layout.md 批N18 段＋decisions §4 批N20 行）：批N18 尺寸记录 release 444,576 B 系测量/记录错误——worktree 挂 `c6d9200` 冷构建 1,086,784 B＋esptool 段表双源验证定案 release 1,086,848 B（IDF 表语义超槽 3.66% 可跑＝bootloader 按段表加载批N9 先例；merged 模板表语义槽内 26.32%）；分区表双表事实＝espflash save-image --merge 未显式传 `--partition-table` 时用内置默认模板表（factory 0x3F0000 撑满 4MB 尾）≠ IDF 构建表 0x100000——nvs/phy 起点长度两表完全一致语义兼容，装配面「镜像表≠构建表」缺口如实登记。产线实录段落 [esp32板卡笔记](reference/esp32板卡笔记.md) |
| 14 | **rtos 形态的配置腿载体本身**（板侧 HTTP 服务 + 堆核算），连带"直连云 API"这条认知路径 | 板侧出现一个真的 HTTP 服务器（带选型结论与内存核算），或明确记"本形态不做配置面、观测腿只有守护输出" | 2026-10-15 证伪审查实测：`tools/esp32-periph-base/src/*.rs` 与 `tools/espidf-sentinel/src` 对 `HTTP`/`https`/`TLS`/`tcp`/`BLE`/`gatt` 词界 **零命中**——板上 WiFi 只用于配网与链路读数。ADR-0021 裁定 1 与 D6 要求"每形态至少一条自己的配置/观测腿"，而全案 §7 那句"承载在 WiFi（rtos 档已在册）"里的"在册"指**格状态不是能力**。工单 connect **N-13**，列**批N4 前置**；细则见 [全案](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §18 F-18，风险面记 architecture **RK-20**——**2026-10-19 批N4 件二：结构面已立**，载体选型 esp_http_server（TLS 不做四理由、凭据面不接 N-11，落 connect 册 N-13 行注记）、堆核算进 ⑩-b（`esp32-mini.toml` heap_reserved 16384→98816 逐项留痕：UART2_TX_RING＋audio_q 峰值＋渲染＋配置服务预留）；板上 HTTP 服务器实证与真板堆对账归真板批，本行证据半边维持敞口／**2026-10-22 对表（批N13）：板侧配置腿码面半边已收**——件一 `d274530` 服务本体落树（[httpd.rs](../tools/esp32-periph-base/src/httpd.rs)：`RR3_BOARD_ROUTES` 白名单表两行只读端点——`GET /api/board/state` 运行态快照 JSON（16 观测字段出锁组包，**不持锁出 JSON** 纪律）与 `GET /api/board/bindings` 注入绑定十键真值（批N9 件五对账锚同源读数）；路由纯函数＋`route_table_matches_dispatch` 表↔分派零漂移单测＋covers/applies 全空形制单测；WiFi STA 起栈全链 nvs_flash_init→esp_netif→esp_wifi_init（**手抄 `WIFI_INIT_CONFIG_DEFAULT` 宏**逐字段对表 v5.2.2 真值并注释留痕——bindgen `Default` 是全零不能用于该结构）→httpd_start；凭据 `option_env!` 编译期注入不落仓、缺席即 fail-honest 点名不装配；白名单外 404 fail-closed；甲面构建＋单测编译两步 PASS 本机 esp 工具链真跑非 SKIP）＋件二 `fbbc98c` 板侧 covers 申报进 ⑩（[check.sh](../tools/ci/check.sh) covers 扫描源扩入板侧 `RR3_BOARD_ROUTES` 区段（两源同扫）＋covers ⊆ 分母五路并集对拍执法位（反向 comm 分子−分母，越界即红硬红不入基线——申报面校验只挡词表外形制、本位补齐申报制语义另一半）；板侧两端点 covers/applies 全空＝门 d 数字零变动对账成立：分母 82／covers 申报 88／红账 0／gate-d-baseline.txt 零改动）；**curl 实测与 `esp_get_free_heap_size` 取值对账归真板验证窗，本行不结项**／**2026-10-22 对表（批N14）：写口与凭据面半边已收**——件一 `c28c85d` 板侧凭据存储对象落树（[cred.rs](../tools/esp32-periph-base/src/cred.rs)：connect 册 N-10 ② 主体半边——namespace `"rr3"`/`wifi.ssid`/`wifi.pass` 单真值常量（全案 §3.6 凭据行 `nvs:rr3` 板侧实现值）＋`Rr3Cred` 值类型**不派生 Debug/Display**（零回显机器墙：格式化路径编译期不可达）＋pass 私有读取窄缝＋`rr3_cred_read` 读时现取两键齐才 Some＋形制域 SSID 1..=32B/密码 ≤64B 空合法；boot 读链切换＝NVS init 前移读链前件＋**NVS 优先→编译期 `option_env!` 回落→都缺 fail-honest**——批N13 机制订正在案：码面误用 `std::env::var` 运行期读而板上 IDF newlib 把 `environ` 初始化为空数组＝配置腿真机永不装配，本批按文档口径落实）＋件二 `9d44903` HTTP 写口两枚（`POST /api/board/wifi` 事务性配网 fail-safe：读旧值→写 NVS→STA 重配置重连→有界等连 15s（500ms 步进 `esp_wifi_sta_get_ap_info` 探测）→败即回退旧值写回/erase 回原空态重连→如实应答四字段 `{ok, connected, reverted, stage}`——`connected` 永不因请求被接受而置真，全程不持 bus 锁；`POST /api/board/reboot` 动作面应答先行落链再 `esp_restart`；handler 真实 method 从 `req.method` 读＋注册面 method 映射随表行；单测扩臂四枚）；**键半边（`cred_store` 键进绑定域）缓批**（契约级三处同批＋桥上注入晚于起栈，见 decisions §4 批N14 收口行）；**2026-10-23 批N15 真板行为取证收官（[批N15 计划册](process/dev/0.0.7实现窗——批N15开发计划与真板行为取证.md)五件实录在案：缺席路径 fail-honest 点名逐字命中／编译期源 boot＋关联 WXYYBG 得 IP 192.168.0.167／curl 读面 state 零凭据字段＋404/400 形制面全命中／写口事务两回退分支实录（erase 生效＋写回自动恢复在线＝砖路径消灭真板实证）＋NVS 读链切换「凭据源=nvs」两处实锤／heap 对账：WiFi+httpd 常驻实测 42852B ＜ 预留 98816 充足不溢）＋真板抓出批N14 两枚缺陷已修（缺陷一 `745b598`：boot 起栈链缺 `esp_wifi_connect` 永不关联；缺陷二 `9b348f5`：等连判据 `esp_wifi_sta_get_ap_info` 关联瞬态误报 ESP_OK→坏凭据不回退→NVS 残留坏凭据真砖路径——改判 DHCP 得 IP 物理可靠）；余量两条如实登记不冒领：①写口/reboot 动作面应答物理不可达（reconnect/restart 杀 TCP 必然——取读数半边真板成立、送达半边不成立，是否延迟应答属行为裁定另批）；②键半边（`cred_store` 键进绑定域）缓批维持原排期——**本行结项**（行为半边取证全毕，键半边随契约级扩批评审另批自找落点） |
| 15 | **证据号命名空间**（D11 界面门与"mock 不得充当在位证据"的正面答案） | `docs/evidence/<形态>/<编号>.md` 规则立起来，且面板/装载期能校验所引编号存在并形态匹配 | 实测 `docs/evidence/` 与 `evidence/` 均不存在，仓内无任何证据编号规则（conventions 只有"每行带证据路径"的散文；`contracts/golden/` 是 IPC 字节向量不是板上对拍记录）⇒ D11"`armed=true` 须引用一条真板对拍证据"这条**界面门不可执行**。工单 brain **B-15**，列批N4 前置；细则见全案 §18 F-24——**2026-10-19 批N4 件一：结构面已立**，规则落 conventions §6.1、容器 `docs/evidence/`（README 模板）建成、校验器唯一实现住 `rr-ipc::evidence`（形制＋存在＋frontmatter 六键＋form 与目录一致，四道 fail-closed，单测钉死）；写面挂接随件九端点（`armed=true`/`severe_mv`/`current_ceil_ma`），装载期核验随落盘数据 evidence 引用字段（件九同批）；证据产出归 0.0.6/0.0.7 验证窗，本行证据半边维持敞口／**2026-10-23 对表（批N21 件二）：装载期半边已收**——`[[supplies.domain]]` 增 `evidence` 键（rrd `RR3_DOMAIN_KEYS` 11→12），与 `armed` 成对（同 switch/switchable 范式）：`armed = true` 必携形制合法引用、`armed = false` 显式 `"none"` 不携过期引用（形状半边＝装载校验⑥-e，判据同源 `rr3_evidence_ref_wellformed`）；文件半边 `rr3_profile_domain_evidence_check` 由 boot 装载链持证据容器根（modules-root 布局纪律推导 `docs/evidence`）跑四道核，fail-closed：容器缺＋有武装申报即红、无武装申报零波及；红绿单测五枚落库（armed 无引用拒／形制坏拒／未武装携引用拒／文件缺红例＋落形制合法文件后 boot 过绿例，[profile.rs](../base/runtime/rrd/src/profile.rs)）；首批证据文件 `实测/0001`/`实测/0002`（批N20 产线真板实录）随本批落盘进容器，rrd 仓内测试锁定其四道核全过（形制漂移即刻红）；**写面半边维持敞口**（`armed=true` 置位／`severe_mv`/`current_ceil_ma` 三写口随件九端点批挂接），本行维持不结项／**2026-10-23 对表（批N21 件四）：读面申报半边补齐**——`GET /api/wiring` covers 补申报 `domain/evidence`（assembly 快照批N21 件二起渲染该键，读面可读即申报；同源计数 pin 测试 43→44；门 d 分母 83 定案＝domain 11→12 件二笔＋ops 36→39 B批 b3b `f0678b7` 三槽欠账补记，[gate-d-baseline.txt](../tools/ci/gate-d-baseline.txt) 行 2 上调 79→83；伴随账＝⑨-y `rr-ipc::evidence` 模块门拓 windows 修 rrd Windows 编译红＋⑩越界执法位 comm 旗标订正／**2026-10-23 对表（批N22 件一）：写面半边已收，本行结项**——rrd 新增装配侧写口 `rr3.ctl.domain.set`（RPC 白名单第 9 个 ctl 方法；面板 `POST /api/profile/domain` 透传端点同批挂接，路由表 22→23、covers 补申报 4 项全在分母内，门 d 分母 83 不变红账 0），三写口（`armed` 置位／`severe_mv`／`current_ceil_ma`）经同一端点全执法：`armed=true` 必携证据且联动入档（⑥-e 成对纪律，档内 evidence 随写落出成对形状）、`armed=false` 联动写 `"none"`、`severe_mv`/`current_ceil_ma` 证据仅留痕不入档（写动作证据住 rrd 日志审计）；写口门序单点＝rrd [domain_write.rs](../base/runtime/rrd/src/domain_write.rs)（confirm 门＋key 白名单＋证据门＋路径自真值集记账推导＋tmp 产物过 `rr3_profile_boot_inputs` 装载链全量预核＝新验收事实，Err 即拒写清 tmp 零字节进正档；机制全在既有单点复用——文本手术/原子落盘/装载预核/证据四道核/轴档 `rr3_axis_write_run`，第二条写路径即红）；生效语义 `effective:"boot"` 如实申报；写口成功后契约记账重记（contracts 快照 OnceLock→Mutex，自检账跟住最新验收事实，写口不再自造常驻"sha 漂"红）；ADR-0021 裁定 6 二次确认以 `params.confirm = true` 落地（三键统一保守侧）；规则唯一落点 conventions §6.1 挂接对表同批追注 |
| 16 | **两条链上只能由板上证据收口的三问**：板侧运行面失败（舵机拒动/移动失败）在真机上发生过没有、`rr3-audio-rec` 的 gaps/restarts 真实量级、云失败各分支（HTTP 非 200 / 超时 / 坏 JSON / 缺 key）的实际分布 | 一次真板长跑的串口日志样本 + 一次 `RR3_BRAIN_CLOUD` 真云档的 counters 读数 | 第六轮追流（全案 §20.5/§20.6）证明这三问**在仓内无答案**：板侧 `rt_move` 的失败只 `eprintln` 且返回值被 `exec_job` 丢弃、不回宿主；`audio-rec` 的 gaps 只 println 不落 counters；`mock` 档使全部云失败分支不可达。本机无 ESP-IDF、板不在手 ⇒ **缺码与缺载体并存**，归 0.0.7 验证窗；在拿到样本前，可达终点表相应三格写"不能 + 触发条件"，不许留白／**2026-10-22 对表（批N12 件一）：第一问缺码半边已收**——板侧 `rt_move` 失败可见化落树（[runtime.rs](../tools/esp32-periph-base/src/runtime.rs) `exec_job` Job::Move **失败半臂**：稳定前缀行 `[periph] Move 失败 pan=… tilt=…（累计 N）`＋照 SCAN/SampleRail 先例 outbox `ERR Move 失败 …` 回宿主；WANDER 扫摆失败静默计数；[state.rs](../tools/esp32-periph-base/src/state.rs) `move_fail_events` 计数观测面，同 `selfheal_skipped` 口径；协议面不动——"不回宿主"的同步应答架构事实维持，全案 §20.5）；板侧专用门甲面构建 PASS 本机实证（esp 工具链在位真跑非 SKIP，`8996af1`）；**量级/分布取证仍归真板验证窗，本行不结项**／**2026-10-22 对表（批N12 件二）：宿主读面原始流 tee 工具面已立**（批N10 件二定案"字节级机制取证归 0.0.7（原始流 tee 双录板面/宿主读面）"的宿主半边）——[esp32audio.rs](../base/runtime/rr-hal/src/esp32audio.rs) 读线程 `asm.feed` 前镜像（拆帧前＝病灶"帧内尾随残渣"取证层）：开关 env `RR3_HAL_ESP32_TEE`＝落盘路径，**默认关**（未设/空＝构造面零 fd 零文件）；追加写＋断流重连续写同一文件（一次取证会话不撕账）；写失败观测面降级不反噬链路面；单测 `tee_disabled_noop_and_enabled_mirrors_bytes_in_order`（tee 流与消费流逐字节一致＋关闭态 no-op，经 ①b 进 CI）——字节级机制取证随下次链路会话取（板面半边并行走板面 USB 口外部录，flock 单消费者宿主 fd 唯一），**本行不结项** |
| 17 | **三形态的平台事实登记表取值**（`contracts/platform/<平台>.toml`：控制器词表 / ADC 脚集 / input-only 脚集 / 冲突矩阵 / 具名构造表，全案 §4.6，第七轮 T-4 逼出的对象） | 每形态一份**由芯片数据手册或 HAL 直读**的取值集 + 一次"故意把模拟测点绑到无 ADC 脚"的装载红例 | 第七轮查出：装载校验 **④-b/⑤-a/⑤-b/⑧-a/⑧-b 五个判定位一直在读这张表，而它从头到尾没有住处**（新类 L-28）——本轮给它一个可指的文件名与形状，**取值仍待器件**：ESP32 一份可由 `tools/esp32-periph-base/src/board.rs` 直读（`ADC_RAIL_PIN=34`、GPIO22/23 硬线 I2C、GPIO19 与 VSPI 预留同脚这三条已在案），树莓派与 RK3566 今天**没有任何机器可查的平台事实**，批N1 只能先交演练实例。与 D9/D10 同族（**卡取值不卡结构**）；表为空 ⇒ 基-2（门 b 装载门）的那五个判定位一律判不了，故 **基-22 必须与基-2 同批** ／ **2026-10-22 对表（批N11 件二）：ESP32 半边已收**——上句"批N1 只能先交演练实例"按施工事实订正：批N1 n1-7（`9b7d4f7`，platform 表落盘＋rr-registry 读表器＋组合件申报面定形）交的 [esp32.toml](../contracts/platform/esp32.toml) 是**真值表非演练实例**（取值来源＝board.rs 引脚真值段＋ESP32 数据手册在案事实，"不许编"纪律；与施工图示例的差异——ledc 通道上界 16、I2C0 实接 GPIO21/22——逐条行注＋commit body 登记）；批N4 件七(a)（`0082114`）补 `[controller_wordform]` 显式空段（rtos 档不走词形派生，空 ≠ 缺）；**红例半边已落**：⑤-b `analog_sense_pin_off_adc_set_is_rejected`（模拟测点 34→GPIO23 非 ADC 脚拒 boot——即本行"缺的证据"栏点名的那条红例）＋⑤-a `output_on_input_only_pin_is_rejected`＋⑧-b `controller_off_platform_vocabulary_is_rejected` 等，全部以 esp32 平台文本经 `rr3_profile_from_text_with_platform` 装载实证（[profile.rs](../base/runtime/rrd/src/profile.rs) 单测，经 ①b 进 CI）。**待核对项补登（本件）**：esp32 模组变体 GPIO37/38 是否出脚 board.rs 未记，`input_only_pins`/`no_open_drain` 暂按四脚收窄（[esp32.toml](../contracts/platform/esp32.toml) 表内注记自述"登 status.md 敞口"，实测此前未登）——模组型号核对后回填。**本行维持不结项**：raspberry-pi-4 / rk3566 两份仍是演练实例（无机器可查平台事实，卡板上线取值） |
| 18 | **`spi_device` 不在门 n 扫面 ⇒ L-23（新设计自带第二真值点）对 SPI 那条链未闭** | 门 n 的扫面 `RR3_PIN_DERIVED_KEYS` 由 **7** 键扩到 **8** 键（含 `spi_device`）、`[hardware]` 里再出现 `spi_device` 即红的一次装载红例；~~前提是先有施工图 §6 第二问的裁定~~（**该裁定已于 2026-10-17 拍板**：先立 SPI 词形规则、批N3 拆格，见 [decisions.md §2](process/decisions.md) 行 2） | 第八轮 实-9 + **实-15** 查实：`rr-registry/src/assemble.rs:56-67` 的 `RR3_ASSEMBLE_DESC_KEYS` **十名，含 `spi_device`**（注释写明它是 0.0.6 链④按裁定流程扩进去的）；而 `rrd/src/profile.rs:91-99` 的门 n 扫面 `RR3_PIN_DERIVED_KEYS: [&str; 7]` **不含**它 ⇒ SPI 器件（MAX7219 点阵）仍走 schema 2 的 `[hardware]` 直给旧路，**注入路径对它不唯一、门 n 对它永远不闭**。两张表相差的三名（`alsa_device`/`link_ifname`/`spi_device`）恰好就是 `contracts/roles.toml:23` 的三个非引脚键。**收口终点＝批 N3**——**裁定半边已结（2026-10-17 拍板：批N3 交 SPI 词形规则 ＋ 门 n 扫面 7→8，同批拆格；不给 SPI 显示面造角色名）**，余量只剩批N3 代码半边。**实-15 顺带查出**：施工图 Task 9 Step 2 原本预写的那行文本（"`spi_*` 派生键未入 `RR3_ASSEMBLE_DESC_KEYS`"）与本行第一句矛盾，已改判，**不得回抄原文** ／ **2026-10-16 对表**：该号位原由并行设备填入一句假账（"spi 派生键未入 `RR3_ASSEMBLE_DESC_KEYS`"与 `assemble.rs:66` 矛盾；`ui.face.matrix` 不在 `roles.toml` 四角色里），本轮就地订正文本并**保留其号**（先占者保号）；判据已升 **实-16**——抄入台账的每一行须由执行者再跑一次该行点名的命令，写侧 + 抄侧两次回执缺一不可（全案 §22.9）／**2026-10-19 结项（批N3 件六 `6953222`）**：词形规则落 `profile.rs` 派生臂（`spidev<N>`＋channel → `/dev/spidev<N>.<M>`，与旧直给值逐字节对齐）＋`RR3_PIN_DERIVED_KEYS` 7→8（`spi_device` 进门 n 扫面，直给与派生并存即红——一次装载红例落库）＋拆格半边 roles 新角色 `ui.face.spi` 与 profiles 两份 `[hardware].spi_device` 撤除迁移 `[[pin]]`；L-23 对 SPI 链闭合，本行结项（施工注记见 [decisions.md §2](process/decisions.md) 行 2） |
| 19 | **L-14 器件级探测的寄存器 ID 读回比对**（INA219 0x00 = 0x2268 等 `contracts/devices.toml` `id_reg` 判据源）：本批（n1-10）已立 sysfs 器件节点判据 + "总线在、器件不在"反例，**读回比对需 i2c-dev 事务（会碰器件），本机 macOS/CI 无法实跑** | 真板上一轮"器件没接 → 该格缺格点名"实测（sysfs 节点不出现）+ 读回比对落 HAL 后端（与 P-5 电流符号派生共用同一 INA219 句柄，不另立句柄） | 全案 §12.1 / power **P-11** + sense **S-11** 需求面；判据本体已落 [`probe.rs`](../base/runtime/rrd/src/probe.rs)（批N1 n1-10），读回归器件与真板批——**结构先行，取值随后**（D9/D10 同族纪律）／**2026-10-19 对表（批N3 件八①）**：`id_reg` 判据源随计量格补课 4→6（SHTC3/ICM-42688 演练行进场，`3025205`），结构面再实、取值随后口径不变，本行不结项（真板批） |
| 20 | **出厂轨（buildroot / `S99rrd`）上装载门真跑过一次的证据** | 一份全量 make 产出的镜像里，`/etc/init.d/S99rrd` 带 `--profile` 启动后的一次 `rrd` 日志（能看到 31 位判据里那 14 位真在判），以及一次**故意写坏接线档**让它拒起的对照 | 第八轮 实-1 / 新类 **L-31** 查出：今天出厂轨**根本不传 `--profile`**（`base/os/buildroot/board/radxa-zero3/post-build.sh:129-155` 生成的启动脚本只有 `--socket`/`--manifest`），`daemon.rs:209` 的 `None` 臂（`:248-253`）于是合法地返回空 `hal_bindings` + 空降级集 ⇒ **十条校验一个判定位都不跑，而 `rrd` 退出码 0、日志无异常**。修法是代码侧的（`--profile` 升必选 + 两轨参数集对账，**基-23**，批N1），但**"修好了"这件事只能由一次出厂轨启动日志证明**，本机无 Radxa ZERO 3 也无全量 Buildroot 环境（同第 6 行）。在拿到证据前，**不得把"装载门已建成"读成"出厂机上装载门在跑"**／**2026-10-20 对表（批N5 基-23）：修法半边已落**——S99rrd 改传 `--socket`/`--contracts`/`--profile` 三参数（与 `rrd.service` ExecStart 逐字同源，`--manifest` 循环删除）＋安装根全迁 `/usr/lib/rockrobot3`（与 deb 轨同名）＋机型档 distro.toml 随装＋真值集三目录落盘断言进场（装配冒烟 22 项断言，镜像级）（`ff2f318`）；rrd 侧 `--profile` 升必选、双缺即拒（`8b1390c`）——证据半边（全量镜像真跑启动日志＋坏接线档拒起对照）仍归本行真板批，**本行不结项** |
| 21 | **rtos 档"只改数据即改接线"的行为证据** | 改一次 `profiles/esp32-mini.toml` 的 `[pins].physical` 取值，板上行为随之变化的一次实录；以及一次**抽掉注入通路**让它 fail-closed 拒起的对照 | 第八轮 实-6 查出：这条链**今天只做了一档**。host 档是真的（`profiles/*.toml` → `[pins]` 派生 → `RR3_HAL_BINDINGS` → `descriptor.rs`）；rtos 档四项零命中——板侧引脚**硬编码**（`tools/esp32-periph-base/src/board.rs`，取证时点 `e9a764f` :148/150/174；原记 `:156/158/182` 系 `b11dd2f` 时点）、桥上**无配置命令集**、平台词形硬编码在 `rrd/src/profile.rs:660`、`:1006` 的档位守卫拿字面量 `"host"` 比 ⇒ **三份接线档与 `[pins]` 对 rtos 是零消费者的装饰数据**。修法是 **基-24 桥上配置腿**（归属 connect 册，批N1），验收判据是**行为判据**而非"代码里读了配置"；本机无 ESP-IDF 也无板（同第 13/14 行），故只能登记为敞口。在拿到证据前，**"裁剪与接线都只改数据"这句必须带档位限定读** ／ **2026-10-19 对表（批N4 件七）：代码面三件全落**——(a) host 词形规则下移平台表（`0082114`）；(b) 桥上配置腿 `rr3.set_bindings`（`b9cc482`：rr-ipc 绑定真值单点＋描述符三键扩名＋esp32-mini `[hardware]` 三键直给＋桥上 method 下发）；(c) 板侧消费腿（`frame.rs` 分发＋state 注入单点＋`board.rs` 三常量删除＋自愈路径 fail-closed：boot 解卡点名／运行面 `selfheal_skip` INFO 计数／重试终败 Err 冒泡，不回退硬编码）——四项零命中/硬编码/无配置命令集三断言至此为历史时点登记；本行验收判据是**行为判据**（改档案实录＋抽注入拒起对照），仍归真板批，**本行不结项**／**2026-10-22 批N9 件五：抽注入半边真板实证**——板复位清注入态＋宿主无 `RR3_HAL_BINDINGS` → 板侧零注入/零对账行＋boot 横幅"I2C boot 解卡拒（绑定未注入）"fail-closed 点名＋舵机能力面照常，另附失配对照 fail-visible 点名（`ledc_pan=30` → 对账✗ 注入 Some(30) ≠ 装配 GPIO32）与正向对账✓十键全齐（实录见 [批N9 计划册件五行](process/dev/0.0.5增补轮六——批N9开发计划与真板收尾.md)）；注记：本实证走的是 `[hardware]` 注入对账链（批N4 件七(b) 桥上配置腿），改 `[pins].physical` 取值板上行为随之变化的另一半边仍敞（归真板批后续），**本行维持不结项**／**2026-10-22 批N10 件三：改值半边真板实证，本行结项（行为半边取值对象按消费路径事实定形为 [hardware] i2c_sda/scl 键——[[pin]].physical 板侧无消费点（装配锚只对账不进消费路径，批N9 件五定案），其变化面（宿主载荷＋对账✗）已随批N9 件五② ledc_pan=30 失配对照实录）**——改值实录：`i2c_sda` 21→20，宿主链 rrd `--manifest` 调试后门＋`RR3_HAL_BINDINGS` 平表透传 → rr3-act esp32 桥 ensure 推送 → 板面 14:07:18.354 `绑定注入 ✓ i2c_sda=20 …`（十键全列）＋`绑定对账 ✗ i2c_sda: 注入 Some(20) ≠ 装配 GPIO21`（恰一条，余九键齐）；**消费面行为变化**：注入前 29s OLED 渲染零失败 → 注入后 2.0s 起 `表情微动作渲染失败`／`降亮写失败`／`渲染到亮度地板仍败 → 重走 oled_init 仍败` 持续点名（运行面每拍 force 重同步按注入对 (20,22) `i2c_set_pin` 把 I2C 外设改路由离装配 GPIO21——消费点 [oled.rs :170/:230](../tools/esp32-periph-base/src/oled.rs)），对账✗ fail-visible 副证同屏；复原实录：改回 21 再注入 → `绑定对账 ✓ 十键全齐` → 失败流当场终止渲染恢复（同 boot 连续对照，板全程未复位）。改值半边（本批）＋抽注入半边（批N9 件五）两半边齐，**本行结项** |
| 22 | **单元装配位仍住实现 crate**（基-35 半切余量①）：装配选择函数 `rr3_hal_*()` 住 `rr-hal/src/backend.rs`（实现 crate），装配位今日在五单元 composite.rs 内——⑨-w 白名单第一类 `rr_hal::backend::*` 因它存在而必须放行，Cargo 级拆边需**装配上移** | ~~缺裁定类~~ **已结（2026-10-22 批N11 件一）**：按 [全案 §25.8 #1](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) 推荐定案**不上移**——换板的 Cargo 收益已由 5/5 单元直依 `rr-hal-api` 拿到（`be69c09`），上移会把装配知识推给新层属多抽象（L-32 同族）；装配位是组合件层（composite）既有职责而非"绑实现 crate"违例，白名单四类（`backend::*`/`sim`/`provision_store`/`Rr3PcmStream`）冻结为**设计态**，终态"条目数只降不升"机器化＝[check.sh](../tools/ci/check.sh) ⑨-w 扩判据 6)——白名单正则交替项数（自执法面自身导出，同 ⑨-n 键名单自代码常量导出口径）对基线文件 [gate-w-baseline.txt](../tools/ci/gate-w-baseline.txt)（初值 4：count>基线红＝扩名单须同批上调、文件缺/坏/非整数红 fail-closed、count 下降须同批下调，同 gate-a 纪律）；裁定结案见 [decisions.md §3](process/decisions.md) 第二行（**裁定预算清零**，§3 余 brain 中止面一条维持） | 施工事实（`be69c09`，⑨-w 三判据）：单元 Cargo 已直依 `rr-hal-api`、契约符号 use 已切 `rr_hal_api::`（30 处），rr-hal 边按裁定 12 保留装配用——"单元零实现 crate 引用"以**装配位设计态**明文收账（原"上移落地后同步收紧为 Cargo 禁令"的注记随不上移裁定作废） |
| 23 | **单元对总线承载 crate `rr-bus` 的直依不在任何门内**（基-35 半切余量②）：⑨-w 只钉能力契约面（`rr-hal-api` 必依）与板侧三禁（rr-hal/rr-bus/rrd），五单元 Cargo 对 `rr-bus`（std 承载：bus 客户端/serde_json 面）的直依不落任何判据 | ~~缺裁定类~~ **已结（2026-10-22 批N9 件一）**：§25.8 #2 拍板"做，但排在基-40 之后"，本批同兑——承载契约面 crate `rr-bus-api` 立起（client.rs 逐字自 rr-bus 迁入＋`#[cfg(unix)]` 门随迁，照 rr-hal-api＋⑨-w 先例，"单元绑实现 crate"同族第二次出现），五单元 Cargo 直依 `rr-bus-api`、语义常量直引 `rr-core`（真值单点不经 re-export 中介，interact/connect/sense 同批补 rr-core 依赖），rr-bus 收窄为语义常量 re-export 层（rrd/tools 零波及）；⑨-w 步内扩判据 4)/5)（Cargo 面必依 rr-bus-api 且禁列 rr-bus＋use 面 `rr_bus::` 零命中）冻结执法；基-33 断点①（client.rs:21 无门直用）随之消解，⑨-y 基线红 1→0 同批下调（rd 写盘面漏门浮出即补，Windows 复跑红 0）——裁定结案见 [decisions.md §3](process/decisions.md) 第三行 | 事实基线（取证时点 `8f8bbce`）：五单元 Cargo.toml 均直依 `rr-bus`——这是**承载面**（与能力契约面分立），基-35 只切了后者。~~不裁的缺省态：直依维持现状，**不阻塞批N2–N5**；若裁"不契约化"，本条以"承载面维持实现 crate 直依"的明文结论结项~~（旧缺省态已由结案取代——实际裁定"立承载契约层 `rr-bus-api`"且同批施工完毕，施工事实见中列；结案取代注记补记 2026-10-23 批N16 件一） |

---

## 在制品

**本快照只记锚点提交 `9c07b3e` 那一棵树的实况；锚点之后新进的提交、以及以未提交状态存在的在途
改动，都不进本表，重锚时随锚点一并更新。**

锚点 `dc3a1d6`（上一版快照，0.0.7 批F 重锚 `b787ba8` 所锚，0.0.7 全版本收口）之后进树的提交（347），按批分组全部已按实况计入上文能力表与下方批次全景段（批号全程单序连续，各批版本归属以 conventions §2 登记口径为准）：

- **0.0.7 批F 版本收口重锚**：`b787ba8`（重锚载体，锚定 `dc3a1d6`）；
- **CI 首跑修复批 H1–H5＋CI 单真源回迁（R30）**：`e0a8bad`/`484a13c`/`f640234`/`adf3d16`/`8f33071`/`1c3cf41`/`e0cb31a`/`b2c28da`/`749f7ba`/`1ff2219`/`fa8897f`；
- **0.0.5 增补轮五 批A–E＋B批（R31 交互功能全案：批A 语音链路真板、批B 外设底座、批C 表情/扫摆、批D 音频上行、B批 听声识位 b1–b3c＋EMI 终验）**：立项 `d463046`/`78625ae`（三窗分离 ADR-0020）／批A `1d00cef`/`5cb7f98`/`5dce1cd`/`406b1ef`/`c5220f5`/`7438dcc`/`2ad6569`／批B `5280fe0`/`d98ff5e`/`edf31b3`/`81b30f1`/`46977bf`／批C `60a8b81`/`bbc60e4`/`ff9de83`/`a19aa01`/`a721c1a`/`8268752`／批D `8f7ddcb`/`f947ddb`/`c5d318c`/`a0126d8`/`181dfcb`/`14e7dcc`/`bb9be1c`／B批 `971bdb6`/`3febbb9`/`f0678b7`/`c74afa9`/`54ce893`/`31f18e1`/`4d13a12`/`f7dc520`/`5e161a3`/`64e111e`/`4bc7ea3`/`2a801c0`/`c8eefa9`/`16e949c`；
- **0.0.5 第 IV 轮 MCP-0 契约级评审批＋批M（M-α/M-β/M-γ）**：`8d05590`/`22a5e37`/`b33d4f2`/`ba90367`/`3bc30f1`/`91b473c`/`084d081`/`7c45566`；
- **0.0.5 现场处置（方案C/方案A）＋rr-mech 机制核下沉＋MC 批（MC-1/2/3/5/8①/10）**：`f6e92d6`/`8f750b2`/`6d76a99`/`03824f6`/`1002844`/`5984697`/`72f0400`/`9393952`/`39da2fe`/`fb744b4`/`071dcad`/`8b5b57e`/`eefc0de`/`ed8fb2f`/`f959e3a`/`70e8fb6`/`1930956`/`a83c9f8`/`b6966fe`/`5245ba9`/`4b5debe`/`9d3043a`/`98e032d`；
- **0.0.5 增补轮六 立项与八轮设计审查＋批N1–N8**：立项 `bbfecbd`（ADR-0021）/`09d75e9`（ADR-0022）／批N1 `769967b`/`3524dcd`/`9b7d4f7`/`05c7529`/`e6d7ef2`/`0e7bc6d`/`0abd5a3`/`bbc853d`／批N2 `bcc13fd`/`0e6d8e6`/`d7e60ea`/`f47d6c7`/`53e243a`/`e918b76`/`b252b65`／批N3 `a5bf8ea`/`be8ba8c`/`6953222`/`3025205`/`4e77e1f`/`a386b98`／批N4 `947af3d`/`1c30b9b`/`6d53a1f`/`322b875`/`0082114`/`b9cc482`/`9565747`/`e0a7c1a`/`d7df465`／批N5 `105929f`/`f8329f0`/`8e02be8`/`8a336e5`/`ff2f318`/`8b1390c`/`b6252b5`／批N6/N7（借号收口归批N8 件四/件五）／批N8 `078b807`/`b2cc925`/`2f58f36`/`5dbc606`/`8a2e5ca`/`2688da6`/`f78868d`；
- **0.0.5 增补轮六 批N9–N18（真板会话批N9/N10/N12/N13/N15/N18＋码面批N11/N14/N16/N17）**：批N9 `83f97da`/`2e81dfd`/`4bb283f`/`0be8fe6`/`0979c53`/`5c087e7`／批N10 `c685114`/`fed1d26`/`e71a9cc`/`3bac55f`/`00deda4`／批N11 `8a179d2`/`0b3c141`/`23823c5`/`7e0edd4`／批N12 `7af9b94`/`8996af1`/`3702cd7`/`6ceb05a`／批N13 `7bb42be`/`d274530`/`fbbc98c`/`ff4175a`／批N14 `55d0261`/`c28c85d`/`9d44903`/`0d319f5`／批N15 `b564e61`/`71f8f45`/`745b598`/`9b348f5`/`d07de82`／批N16 `7af4948`/`e801ab3`/`b25491d`/`3e4f055`／批N17 `d7b2c14`/`58c495c`/`5157480`/`d678621`/`ea25714`/`21e6487`／批N18 `8cd294e`/`1f03bb5`/`90ac149`/`63255c2`/`6a59d2b`/`c6d9200`；
- **0.0.8 整理窗 批N19（L-32 摘面·基-33 结案）**：`de13d49`/`9f0639b`/`e3eaecb`/`e69b244`/`296f33b`/`0e9cac0`/`f2ed302`；
- **0.0.5 增补轮六尾段 批N20–N22（敞口13 真板实录收官/本机纯码面清池/四版本盘点）**：批N20 `01c15f2`/`5222368`/`cecff5c`/`15d0b5b`／批N21 `b210040`/`9ac196b`/`15dc201`/`50733ad`/`5269bc8`／批N22 `8f39f99`/`0ada921`/`9375587`；
- **0.0.7 实现窗 批N23（MC-7 缺②板侧固件 crate 收编原子模块形态）**：`75532f1`/`9ef77d1`/`9158612`/`233707d`/`8413755`；
- **0.0.8 整理窗 批N24（W-B 形状清池 A3+A4+A5）**：`1e28507`/`8c758d4`/`b92e767`/`0272363`/`e3f8bf4`/`98a24a7`；
- **0.0.8 W-A 批N25–N30（批0 公共骨架 rr3mod＋批1–批5 五模块拆分）**：批N25 `47e3e66`/`fe4022d`/`c27c350`／批N26 `52d8285`/`f17c475`/`1c3ed34`/`3ced70d`／批N27 `9ababbb`/`2c10e06`/`7e68072`／批N28 `b542a8c`/`009d24d`/`8ac382a`／批N29 `aebad30`/`5a8b889`/`b40e661`／批N30 `3343e3a`/`5f48c33`/`9c07b3e`——批N1–N30 各批 conventions §2 逐批登记在案。

以上提交都已按实况计入上文能力表与下方批次全景段；**锚点树内在途的未提交改动只有本收口批（0.0.8 W-A 批6＝批N31）的文档面**
（本文件重锚 / 0.0.8 方案状态行收口段 / conventions §2 批N31 登记段），随本批落库。

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
   播放点名失败被吞成一行 print 后照常 `chat_rounds+1`、`integrity(0,0)=Ok` ⇒ **"完成一轮"与"播出声"解耦**（2026-10-20 对表：两处假通过口已随批N4 件六一笔闭合——`chat_rounds` 拆账 `rounds_replied`/`rounds_audible`、零字节下界进 `rr3_tts_pcm_integrity`，本行取证时点写法保留）。
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
- **2026-10-21 第十一轮：新基线重测与第十轮对表 → 全案 §25 落档**（文档轮，**零 `.rs`、零 `contracts/*.toml`**；取证基线 **`14e7dcc`**，与第十轮取证基线 `ced8946` 之间隔 **89 个提交**）。
  尺子不变、**对象换成"上一轮的结论还成立吗"**：`裁-1…裁-14` 全判（**已闭 4／半闭 3／仍成立 5／加重 2**）＋新发现续号 `裁-15…裁-23`。
  **头号新发现 裁-15**：**两份"该带哪些文件"的清单不同源**——装载面 `rr3_runtime_truth_set`（`rrd/src/profile.rs:1176-1192`，7 文件＋1 目录，含平台册／装机档／接线档）
  vs 出厂自检腿五 `RR3_CONTRACT_FILES: [&str;4]`（`rrd/src/contracts.rs:25-26`，只 topics/slots/roles/devices）；平台册缺失是**拒 boot** 级（`profile.rs:302` 自述 fail-closed）
  ⇒ **自检五腿全绿而机器起不来是可达状态**，与 块-15 同形状换层复发＝按 ADR-0021 裁定 5 记为**抽象失败**级别；收口 **基-40** ＋ 新拟步 **⑨-z**。
  **次条 裁-17**：`Rr3Sensor`/`Rr3DigitalIn` 的 prod 实现数＝0、多态调用点＝0，而 `contracts/roles.toml` 已有两角色以 `iface="Rr3Sensor"` 在册——门 a 的 B 段只判"名字在不在册"，**判不出契约合法引用了一张空能力面，所以它是绿的**（RK-20"格在册≠能力在板"的对偶）。
  **裁-18 体量反向**：`profile.rs` 2900→**4641** 行、装载门函数 513→**622**（`:1226-1847`）、读表样板仍 6 遍、同形函数 16→22、未知键循环 16→21、`#[cfg]` 96→224、trait 14→18（零多态 4→6），`roles.rs` 仍缺未知键拒斥。
  **分母重取**：`check.sh` step **11→18**、专属执法位 **1→8 把**（`⑨-a/c/d/e/n/r/w`＋`⑩`）、**待建门 23→16 把**，本轮再新立 `⑨-y 跨平台可编面`／`⑨-z 真值集成员判等` ⇒ **18 把**；
  基座轨 **基-1…基-40**（＋基-40；基-35/基-36 结案、基-39/基-23/基-25 半闭、基-32 未收口爬路 4→**2 处**）；闭集全数重取（roles 4→10、slots 21→23（20 normal）、TRAITS 7→9、DESC_KEYS 10→13、PIN_KEYS 10→11、**PIN_DERIVED 7→8 含 `spi_device`**）；
  位号锚点集合差 **31/16/15/5 → 31/23/8/10**（**§23.6 那张表的数全废**，六处文档引用本轮**未收敛**，见全案 §25.7）。
  **决策状态**：第十轮 §24.4 十六项**已全按推荐裁**（2026-10-17 拍板轮，落点 [decisions.md](process/decisions.md) §2 ＋ [批N2–N5 开发计划](process/dev/0.0.5增补轮六——批N2至批N5开发计划与第十轮决策拍板.md) §2），
  其中 **D6 值反转、D3 值更新**未照我的原值；⇒ 本轮 §25.8 只对**真正还悬着的 9 条**给推荐（在册 3 ＋ 新出 6）。
  **两处本轮自招**：(a) 我在 §25 初稿写"基-34 改判为扩 `docs/openitems.toml`"——**该建议当场收回**，它会造出第二个决定登记处（`openitems.toml` 是门 d 差集归因账，两表分工不同、不可并表），正是 裁-1/L-23 要拦的形状；
  (b) **§25.4 的 A/B 组已失去时效**（四问全部已裁），本轮把它降级为复核记录——**这正是 裁-4 那一族病在我自己身上的复现**。
  **本轮不新增敞口行、不删行**；敞口表第 18 行由另一台设备判"结项"，本轮实测证实（`RR3_PIN_DERIVED_KEYS: [&str; 8]` 含 `spi_device`，`profile.rs:122/:130`）⇒ **L-23 对 SPI 闭合，第 18 行状态与事实一致**。

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

**2026-10-19 批N4 件一～件三**（证据号命名空间＋板侧配置腿选型＋门 d 覆盖率门建成，三件分笔
落库、本条目随件三同批补登）：①件一 `947af3d`（brain B-15）证据号命名空间——规则落
conventions §6.1 新段、容器 `docs/evidence/`（README 模板）建成、校验器唯一实现住
`rr-ipc::evidence`（形制＋存在＋frontmatter 六键＋form 与目录一致，四道 fail-closed 单测钉死）；
敞口表 15 行结构面注记同批，证据产出归 0.0.6/0.0.7 验证窗，写面挂接随件九端点。②件二
`1c30b9b`（connect N-13）板侧配置腿选型——载体 esp_http_server（TLS 不做四理由落 connect 册
N-13 行注记、凭据面不接 N-11）＋堆核算全账进装载校验⑩-b（`profiles/esp32-mini.toml`
heap_reserved 16384→98816 逐项留痕：UART2_TX_RING＋audio_q 峰值＋渲染＋配置服务预留）＋
architecture 对表；板上 HTTP 实证与真板堆对账归真板批，敞口表 14 行证据半边维持敞口。③件三
（本提交，interact I-19）门 d 覆盖率门建成——`panel/server.rs` `RR3_PANEL_ROUTES` 升三元组带
`covers` 数据化申报（申报制：未申报不计分子；今日 14 端点读配置者 0＝I-15 实况，covers 全
`&[]` 如实）＋词表常量 `RR3_PANEL_COVER_SETS` 五名（ops/pin/role/domain/axis）码内单真值点＋
形制单测；`check.sh` 建 `step "⑩ 门 d 面板覆盖率对账·报告模式…"`：分母降字段级五路并集
（`ops[].slot` ∪ `RR3_PIN_KEYS` ∪ roles.requires 每项 ∪ `RR3_DOMAIN_KEYS` ∪
`RR3_ACT_PROFILE_FIELDS`）＝70（27+11+7+11+14，五路自代码/契约常量导出）、差集 comm 逐条
" D " 行打印（ADR-0022 裁定 6.3 开放项的差集产出位）、双行基线 `tools/ci/gate-d-baseline.txt`
（行 1 红账上限回涨 FAIL／行 2 分母下限缩分母 FAIL——"红项数不许靠缩分母减少"机器化）初值
70/70 建即红如实在账；fail-closed 四路实测（covers 形制乱写／基线坏／基线缺／分母缩水各一次红）；
可数 step 16→17 行、专属执法位 5 把不变＋报告位 1→2 把、待建门 18→17 把；差集红账不进敞口表
（:435 章程"缺码＋工单号不许混装"），机器真值＝步 ⑩ 打印，开放项数据结构表（裁定 6.3）随件九
面板与运维面同批立；interact 册 I-19 行对表注记＋conventions §2 批N4 条目＋architecture §14
（门 d 行改已建·报告模式＋计数行补登批N3 件五 ⑨-e 漏更）＋全案 §16.4 同批（件三收口对账，
本提交，2026-10-19）。**随批补修件一/件二两处漏更**（独立提交，全量门禁回执抓出）：evidence
README 进 docs/README.md 库地图（⑤ 导航漏链红）＋contract pin `heap_reserved_bytes` 16384→98816
同批（`esp32_mini_profile_binds_bridged_minimal_shape` 红）。

**2026-10-19 批N4 件四**（interact I-20 面板写面两硬要求，两半全结）：①**落盘原子序单点**
（全案 §19.3 判据 2，索引 L-26/L-23 写路径形态）——`rr3_atomic_store` 实现自 `rrd/src/counters.rs`
原样迁 `rr-ipc/src/persist.rs`（凭据面 rr3-interact 是独立 workspace 不依赖 daemon crate，
共享件住双方公共最低层——evidence.rs 同径先例；rrd 侧 `pub use` re-export 保 counters/axis
调用点零改动；模块注释登记三调用方，改面须同批过注释）；`panel/secrets.rs` 写侧退役
`truncate(true)` 原地覆盖（掉电卡写中间 ⇒ 凭据文件恒 Err 不能自愈），改走共享序＋父目录
0700（tmp 窗口护城河）＋rename 后收 0600；persist 单测 1 条（原子覆盖＋tmp 无残留）。②**applies
生效路径三态申报**（全案 §19.3 判据 1"缺失即红"）——码内词表 `RR3_PANEL_APPLY_MODES =
[live, reboot, reflash]` 单真值点，`RR3_PANEL_ROUTES` 升四元组 `(method, path, covers, applies)`，
今日唯一申报 = `POST /api/model/key` → `live`（secrets 落盘后 brain 每次现读；应答带
`applies` 字段＋前端文案"brain 下一轮对话生效"同义双面），其余 13 端点如实空；执法双门 =
`applies_use_declared_vocabulary_only` 单测（词表＋写口带 covers 必填＋申报面漂移 pin）＋
check.sh 步 ⑩ applies 段（词表打空/词表外/写口缺申报/行形制坏四路 fail-closed 实测全红过；
申报面硬红不进红账口径——covers 列整列丢失归 Rust 四元组类型门，bash 不重复执法）；可数
step 仍 17 行（⑩ 扩段非建步）；interact 册 I-20 行对表注记＋conventions §2 件四条目＋L-26
两判据行"已建/已修"注记＋architecture §14 门 d 行 applies 补记＋全案 §19.3 同批对表。

**2026-10-19 批N4 件五**（N-11①＋I-17＋I-18②③ 五组硬编码数据化，机制与策略分离收账）：①N-11①
connect 配网最短密码位出码——`min_pass_len` 进 `config.toml [rr3-connect.wifi]`（缺 8 位 legacy
语义回落）＋`atoms/wifi/assemble.toml` 第 3 槽，`rr3_wifi_connect_parse_args` 加参、文案模板化
（"密码至少 N 位（legacy 配网语义）"）。②I-17/I-18② interact 四组出码——`RR3_FACE_TICK` 节拍
（composite.toml 组合件级槽 face_tick_ms）、SelfTest 期望表情（selftest_face_expect）、face 兜底
"idle"（`rr3_face_with_default` 件级槽 default_expression，Default 真值点唯一）、面板五限额
（`Rr3PanelLimits` 结构 max_conns/read_timeout/header_limit/body_limit/call_timeout，Default=
legacy 8/5s/8KiB/4KiB/5s；`RR3_PANEL_ADDR` 按批I I-1 同口径保持 env+常量双真值不进 config）；
composite.toml 删 `no_injection`、ops 七槽首启用＝**组合件级申报面首次有内容**。③I-18③ 自检
逐腿（D15）——SelfTest 改逐腿 `[{leg, pass, reason?}]`＋`verdict` 聚合（全腿 pass 才 pass），
pass 词表 pass|fail|n-a；speaker 恒败腿（主循环缺席即 send 失败）**如实 fail 不折算通过**；
panel/www 逐腿渲染＋判定行。④门 d 扩面——check.sh ⑩ ops 导出改**两来源并集**（件级
assemble.toml ∪ 组合件 composite.toml），ops 27→36、分母 70→79 双行基线同批上调（79=上限=
下限）。⑤撞 pin 修复（门禁回执抓出）——rr-registry contract `composite_declaration_face_is_pinned`
原把六份组合件全 pin 零注入面，rr3-interact 七槽首启用后同批改 pin：rr3-interact 分立具名七槽
断言（句柄仍零＋no_injection 不写），其余五份零注入面断言不变。单测 interact 54/54（新 3 条：
逐腿形状、default 注入驱动回落、face/panel/selftest 键回落与拒值）＋connect 22/22；对表五文档
（interact 册 I-17/I-18、connect 册 N-11、计划 §3、全案 M-2/M-3/M-6/M-10/G-3）＋conventions §2
件五段。

**2026-10-19～20 批N4 件六～件九笔4（批次N4 收官，门 d 转绿终点达成）**（六笔分立提交，逐笔对表
见 conventions §2）：①件六一笔 `09df07e`（I-21①③＋B-13①＋B-16①②）——"成功"的假通过口两道
闭合（`rr3_tts_pcm_integrity` 零字节下界＋`played_ms==0` 折 `tts_play_reject`；面板 `BusCall` 载荷
判等 `rr3_bus_reply_status` 业务错误折 422 与 502 传输失败分码）＋brain 计账拆账
`rounds_replied`/`rounds_audible`（`chat_rounds` 退役）＋总线 RPC 超时数据化
`[rr3-brain.bus].call_timeout_s=30/connect_timeout_s=5`（legacy 两常量退役）＋云面异常收敛一个
处置点；②件六二笔（B-12 读面）——brain 新 RPC `rr3-brain/ops/list` 白名单六槽应答（密钥与
`api_key_ref` 不进应答，值零回显守脑面）＋面板 `GET /api/brain/ops` BusCall 透传＋covers 六槽 ⇒
门 d 红账 79→73 基线同批下调；③件七三笔（实-6 修法，rtos 档"只改数据"补课）——host 词形规则
下移平台表 `0082114`／桥上配置腿（数据面＋宿主腿 `b9cc482`）／板侧消费腿（`frame.rs` 分发
`rr3.set_bindings`＋进程级注入单点＋`board.rs` 三常量删除＋自愈 fail-closed 三观测）；
"改 physical 后板行为变化"的行为判据归真板批（敞口 21 行不结项）；④件八两笔（门 r 定形，二选一
裁定 **per-unit 日志文件**）——笔1 机制单点 `07b998d`（supervisor stderr 接 `<socket>.log/<unit>.log`
append、打不开回退 inherit 点名＋单测＋`e2e_unit_log.rs`）；笔2 清零＋执法位（五 Rust 模块 52 处
stdout 宏→`eprintln!`、brain 12 处 print→`file=sys.stderr`〔brain 扩面同批登记〕＋
`step "⑨-r"` ⇒ 可数 step 18 行、专属执法位 6 把、待建门 16 把三事实源同批）；⑤件九五笔——
笔1 openitems.toml 新立（裁定 6.3 开放项数据结构，门 d 差集 73 项五组归因）；笔2 观测面收官
（契约 32→37＋五模块 ops/list 快照读口＋面板五端点 covers 30 槽 ⇒ 红账 73→43）；笔3 配置面
（brain `rr3-brain/ops/set` 写口 `rr3_ops_write` 白名单 fail-closed＋live 生效不落盘
`persisted:false` 如实＋面板 `POST /api/brain/ops` pin 20→21＋applies 第二条申报，covers 六槽
∪ 不增）；笔4 接线图收官刀 `e0a7c1a`（rrd 第七 ctl 方法 `rr3.ctl.wiring`＋assembly.rs OnceLock
快照＋[pins]×wiring 全交叉三族三态判据单点＋profile boot 交出快照＋面板 `GET /api/wiring`
pin 21→22＋covers 四族全集 43 项 ⇒ **红账 43→0、差集空、门 d 转红（差集 > 0 即 FAIL）转绿终点
达成**；selftest 腿四 wiring 逐条摊开＋verdict 改判"有 fail 即 fail、无 fail 且至少一腿真判 pass
才 pass"；前端 tab-hw 接线图卡＋自检卡文案同批）。**批次N4 工单全件收官（件一至件九笔4）**；
decisions.md D6/D7 施工注记同批补登；openitems 四组（pin/role/domain/axis）status=done、
fields 清空；敞口表无增减（差集红账按章程不进敞口表，机器真值＝步 ⑩；15 行证据半边归验证窗、
14 行证据半边归真板批维持原状）。

**2026-10-20 批N5 首件（按形态分档可达终点表落档）**：判据原文＝[全案 §17](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) 批N5 行（三形态 × 十项"今天能做到｜不能｜缺什么"，每格指到一个 `文件:符号` 或已登记敞口号，"应该可以"即 FAIL）；改判口径＝**首版手写 ＋ 逐格标 `文件:符号` 或敞口号**（原句"由结构化开放项数据生成、禁手写"自相矛盾，证伪审查轮订正作废）。交付物 [批N5首件——按形态分档可达终点表.md](process/dev/0.0.5增补轮六——批N5首件——按形态分档可达终点表.md)：§1 三态判定口径（能做到＝机制在树且载体在位带 `文件:符号`／不能-无载体＝机制在树载体不在带触发条件／不能-缺码＝机制不在树带工单号）＋取证时点 2026-10-20 基线 `b6d5902`；§2 概览 30 格——**能做到 8 格**（ESP32 出字/转动/拾音＋观测〔仅守护输出档〕＋烧录〔手工档〕、树莓派配网/观测/烧录）、**不能 22 格**（无载体／缺码／语义未裁，逐格带触发条件或工单号——RK3566 行 10 格全"不能"，出厂路径装载门旁路一格点名基-23 仍欠且基-25 先行）；§4 维护纪律（改判时机＝敞口行结项/器件到货/批件收官触发，改判列入该批件交付清单并复核所引符号仍在——实-16 抄侧再跑；openitems.toml 生成器升级路径不随批N5 首件宣称）。零 `.rs` 零 `contracts/*.toml`；docs/README 主表、开发计划 §3 批N5 段、conventions §2 三处登记随批N5 首件同批（敞口表无增减）。

**2026-10-20 批N5 件二（运维留痕三笔收官＋波及订正，`low_power`"只升不降"敞口关闭）**：判据原文＝[全案 §17](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) 运维留痕行三半句（F-30 改判判据在内）。笔1 环容量键 `f8329f0`（**载体订正**：环容量进装配单 `[runtime]` 顶层段而非 HAL 描述符键集——`RR3_ASSEMBLE_DESC_KEYS` 有契约测试逐名钉死、环容量是 rrd 运维参数非外设绑定；F-30 改判判据落点随之改写"环容量∈`[runtime]` 段键集，装载期缺即红"；profile schema_version 4→5）；笔2 rrd 自身日志有界落盘 `8e02be8`（stdout/内存环/落盘三面同源＋启动回填尾 64KB——"一次重启后仍能取到崩溃前最后 N 行"另一半成立；模块侧批N4 件八已成立，rrd 自此同权）；笔3 解除路径 `8a336e5`（契约 append `rr3-power/battery/ok` event L1 主题 38→39＋schema 三必填；rr-mech `Rr3LowEdge` 补恢复沿〔先 recover 后 cross 的喂入次序是契约〕；rrd `rr3_reg_low_power_lower` 下降沿＋conn battery/ok 臂〔falling 才广播 `system/power(reason=battery-ok)`、重复 ok 不重复广播〕＋`e2e_power_ok` 全链线级证据；"low_power 旗补解除路径（今天只升不降）"敞口关闭——两个沿一个对：battery/low 上升沿置旗拒新 spawn／battery/ok 回阈下降沿清旗恢复新 spawn）；波及订正 `69b2b47`（rr3-act 订阅臂 reason 分流：battery-ok 解除广播不闭锁不解禁——低电段中 act 重启会被解除广播误闭锁至重启，解禁只认轨压样本复位语义④、喂入面挂账批N3 件七不变）。conventions §2 同批登记；敞口表无增减。

**2026-10-20 批N5 件三（装载消费真值集清单＋三处消费方判等——烧录与装机 host 半边收官）**：判据原文＝[全案 §17](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) 基-25 行＋实-12 修法；范围裁定＝烧录与装机 **host 半边**归基-25，rtos espflash 烧录半边等真板另登（余量注记落索引 §14.7 基-25 行，按敞口表章程缺载体不混装）。笔1 `280da7b`——rrd 立 `rr3_runtime_truth_set` 唯一清单（装载面 files 七条〔profiles/\<机型\>、contracts 四册、contracts/platform/\<chip\>、hardware/\<机型\>/wiring.toml〕＋dirs 一条〔axis 目录〕，单测以真仓双装配单逐成员钉死）；rr3-pack 照单扩包：装配单先按装载同判据预验（坏单拒出包）→ truth set 缺册检查（缺册拒出包）→ 按清单循环落位——打包面不自带第二份清单（清单真值只有 rrd 一份）。笔2 `497e1a6`——装机面消费方判等：distro-smoke ⑤ 补 `contracts/platform/rk3566.toml` 装机断言（块-15 修法 b：装机验证看见平台册缺席）；post-build.sh 新立第 5 件"装载消费真值集装机"，profiles/contracts/hardware 三目录整目录随装（块-15 修法 c＝基-23 第二半提前归基-25；整目录随装不抄第二份清单，S99rrd 传参启用消费仍归基-23）；OTA 判据占位落 [base/ota/partition-layout.md](../base/ota/partition-layout.md)——模块级更新两道判据（staging 真值集与 `rr3_runtime_truth_set` 判等＋manifest 契约 schema 版本列不等即拒装），载体随 M6（`rr3.ctl.ota` 当前恒拒、rr3-ota-ctl 已不在仓）。块-15 施工顺序约束"基-25 必须先于基-23"host 半边兑现；敞口表无增减；conventions §2、开发计划 §3、索引 §14.7、architecture §13 同批对表。

**2026-10-20 批N5 件四（基-23 出厂轨装载面齐装＋`--profile` 升必选——三笔收官）**：判据原文＝[索引 §14.7 基-23 行](design/模块设计索引.md)＋L-31 实-1（出厂轨上装载门不存在：`daemon.rs` None 臂＋S99rrd 只传 `--socket`/`--manifest`＋安装位 `/usr/lib/rr3` ≠ deb 轨 `/usr/lib/rockrobot3`）。笔1 `ff2f318`——post-build.sh 安装根全迁 `/usr/lib/rockrobot3`（与 deb 轨同名——modules-root 由装配单路径上两级推导是物理前件；`/var/lib/rr3/staging` 是分区挂载点不属安装位、不随根动）＋S99rrd 改传 `--socket`/`--contracts`/`--profile` 三参数（与 rrd.service ExecStart 逐字同源，`--manifest` 循环删除）＋机型档 distro.toml 随装＋真值集三目录落盘断言进场（装配冒烟 22 项断言，镜像级）；笔2 `8b1390c`——rrd cli 双缺拒门（装配面必选：`--profile` 与 `--manifest` 至少给其一，两者同给也拒〔Q3〕；拒门在 socket 空检查之后保住既有拒因序）＋单测改判 10 处＋新增三形状用例钉门＋监听面缺格跳过夹具适配（stub manifest 真实落盘＋夹具树 Drop 清理）＋`rr3_start_manifest_stub` 后门夹具（专供 ctl.wiring unavailable 用例）＋裸"拒绝"needle 收窄为"拒绝发布"＋daemon None 臂注释收窄＋[rrd-cli.md](reference/rrd-cli.md) 同批；笔3 对表（status.md 敞口 20 与本条、rrd.service:4、终点表 RK3566 观测格、索引 §14.7、architecture v0.16 段④与 §13 装载门行、conventions §2、开发计划 §3）。rrd.service:4"两轨同一组安装位与套接字"由假变真；敞口表无增减（敞口 20 修法半边已落、证据半边归真板批不结项）。

**2026-10-21 批N5 件五（出厂自检判据四缺口四笔收官）**：判据原文＝[全案 §17](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) 出厂自检行＋§20.1 Z-4 行（"包内四份契约文件齐全且 sha 对"）。笔0 取证定四缺口（F-31 预期表／speaker 恒 fail／wiring ⑦-c 额定加总族／契约 sha 链）；笔1 `6f32e94`——F-31 预期表进判：`[rr3-interact.selftest.expect]` 落装（固定腿 speaker/relay/face 逐名三态值域闭集＋组预期 zero-fail），verdict 纯函数化改判**逐腿结果集与预期集等值**（整体红绿是派生量非判据），段/键缺省回落全 pass＋组零 fail，未知腿名/在册外值 fail-closed 拒 boot；笔2 `dcdb28f`——speaker 内置测试音：beep 臂合成 440Hz/200ms 短正弦直喂播放面（同径 volume_pct 增益），波形源进程内自持不再依赖 tts_pcm 流（I-21① 由构造保证），speaker 恒败腿自此消失（D15"测试音内置策略件补回"兑现，落批N5 件五）；笔3 `b5016eb`——wiring ⑦-c1 额定加总族：wiring 带 feeds 后按 net 拓扑加总判（F-10 峰值直加 Σpeak≤rated_a×1000，三色 DFS 环/悬空即漂移 fail，菱形分叉只收账一次），判据真值单点仍在 `rr3_assembly_cross_checks`，装载期不预支（⑦-c2 件侧归批N2）；笔4 本笔——契约 sha 链 `rr3.ctl.contracts`（第八 ctl 方法，白名单七→八）：rrd 新 `contracts.rs` 手写 FIPS 180-4 sha256（零密码学依赖，NIST 五向量钉正确性）＋boot 装载链验收后契约四册（topics/slots/roles/devices）sha256 记账快照（OnceLock，定格点 fail-closed 拒 boot）＋自检腿五 contracts"齐全且 sha 对"（缺册＝齐全破、sha 漂＝sha 对破，判据同质无 n-a 豁免面；`--manifest` 调试路径 `available:false` 如实）＋面板 selftest 五腿 `contracts/*` 组 verdict 加 zero-fail 臂（与 wiring/* 同规则）＋预期表 contracts 键；门 d 分母 79 不动（无新 HTTP 端点，covers 零波及）。单测：rrd lib 250（＋sha256 向量＋判果渲染）、e2e_profile 5（＋契约问询四册点名）、interact 62（＋五腿/预期表/正弦确定性对拍）。**批N5 件五全件收官**；conventions §2、开发计划 §3、interact README、索引 D15、全案 §17/§20.1、ADR-0010、rrd-rpc.md ctl 表、architecture devices 行同批对表；敞口表无增减。

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
- **0.0.5 增补轮六 批N10 件二——"事件门拒收：事件体非 JSON"病灶定案（本批零代码，修不修归 0.0.7 另裁）（2026-10-22）**：
  打印点 [rr3-audio-rec :219](../tools/rr3-audio-rec/src/main.rs)（事件体 `serde_json::from_slice` 失败）←
  :746；排查面＝板侧事件帧组装发送（[mic.rs](../tools/esp32-periph-base/src/mic.rs) 组帧/audio_q →
  [state.rs](../tools/esp32-periph-base/src/state.rs) drain_audio 整帧写、写不全不计 sent →
  [link.rs](../tools/esp32-periph-base/src/link.rs) :85-100 写侧 Err/Ok(0) 即 break）vs 宿主
  [FrameAssembler](../base/runtime/rr-ipc/src/framing.rs) 读面；run6 同跑取证＋本机代码面，六条证据闭环：
  ①板面 INFO 对账 `seq=4688 sent=4688 drop=0 q=0`——板侧零丢弃全量发出（背压/组帧面洗清）；②宿主终账
  chunks=4486＋err=100，4688−4486=202≈gaps 200（1 拒收≈2 缺失）；③拒收错误全型 `trailing characters`
  100/100（EOF/invalid character 零条）；④干净窗口 seq 全连续（375/375）——丢片只在拒收窗口内；⑤trailing
  位置 C∈[10675,11021] vs 标准体≈11030（2B 前缀＋topic 19 字符＋b64 10924）——载荷＝**被拼短的合法 JSON＋
  帧内尾随残渣**；⑥err:gaps 恒 1:2（run4 46:92／run5 12:24／run6 100:200）。**定性＝块级字节替换/拼接**
  （USB 传输级 stale replay ≈2 帧字节量级嫌疑最合形状；"板侧静默写半帧"嫌疑与 EOF 零条不符）。
  **批N9 件四副证原判修正**：原记"板侧事件帧尾部截断"改判"JSON 值完整、帧内尾随残渣"。收口裁定：字节级
  机制取证归 0.0.7（原始流 tee 双录板面/宿主读面）；帧级 CRC 承载面增强候选随取证另裁；本批不修。
- **0.0.5 增补轮六 批N10 件一——判据② run6 600s 复测仍未过（如实敞口；三态判读＝恶化）（2026-10-22）**：
  命令形态补录（批N9 会话实录命令行未落档，本批以 [rr3-audio-rec main.rs](../tools/rr3-audio-rec/src/main.rs)
  :42/:584 为准）：`rr3-audio-rec /dev/cu.usbserial-A98KN77U --secs 600 --out rr3-audio-600s-run6.wav`
  （baud 缺省 921600）。实测 chunks=4486 **gaps=200** err=100 restarts=0 resyncs=0 reconnects=0，wav 落盘
  36.7MB 实测 16008Hz dur≈573.9s——600−573.9=26.1s ≈ 200×128ms=25.6s（口径自洽 ✓）。**口径 gaps=0 未过，
  维持敞口不冒领 PASS**；纵向对比 run4 92 → run5 24 → run6 200（比 run5 恶化 8×），结构与 run4/5 同族
  （散布丢片型，断流型病灶维持全零）；环境差异如实登记——run6 期间宿主并行 cargo build＋分析负载（run5 无）。
  同批注记**敞口 16 第二问（gaps/restarts 真实量级）取数**：restarts/resyncs/reconnects 三跑全零；gaps 量级
  0.5%–4.3%（run5 24/4686、run6 200/4686），随宿主负载波动，"可用但有损"定性不变。字节级病灶并案排查见
  批N10 件二（同会话取证）。
- **0.0.5 增补轮六 批N9 件三/件四上板会话——判据② run5 复测未过（如实敞口）＋判据③ DoA 冒烟过（2026-10-22）**：
  随件二 [flash-esp32.sh](../tools/deploy/flash-esp32.sh) 烧录全链首跑（串口 `/dev/cu.usbserial-1420` 自动
  探测→ESP32 rev v3.1→Flashing completed，板面重建为当前树固件）后同会话执行。**判据② run5**：600s 复测
  chunks=4662 **gaps=24** err=12 restarts=0 resyncs=0 reconnects=0，wav 落盘 `rr3-audio-600s-run5.wav`
  38.2MB 实测 16002Hz dur≈596.7s——口径 gaps=0 **未过，保持敞口不冒领 PASS**；纵向对比 run4：gaps 92→24、
  err 46→12（收窄 4×，散布丢片 2%→0.5%），断流型病灶维持全零——"可用但有损"定性不变、量级改善，散布丢片
  为长跑低频现象非回归（板面重建后与 run4 同族）。**判据③ run5 冒烟**：`--secs 40 --doa` 拍手（0–8s 左／
  10–18s 右，用户配合），active=11 窗汇总**左右符号分离 ✓（冒烟过）**——左拍窗 win4/5 `+11°/+13°`、右向
  强拍窗 win34/35 `-18°/-11°`；中段 win16/17/18 `+5°/+4°/-0°` 弱正角（ILD 0.6–0.7dB 近阈值，拍手余响
  衰减态）如实记；第二组左侧补拍三窗（+11°/+9°/+15°）同向吻合。副证发现：跑内 3 次"事件门拒收：事件体
  非 JSON"（板侧事件帧尾部截断，与散布丢片同族嫌疑），登记待查不属本批施工面。
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
- **板固件小批——板麦数字增益 ×8 落板生效 + 波特率提档实测否决回守 921600 + VAD 阈值重标定
  800 + 全链冒烟(三)过（2026-10-20，本提交）**：两常量级改动 + 一档重标定，真板验证全过。
  ①**增益 ×8**（[mic.rs](../tools/esp32-periph-base/src/mic.rs) `MIC_GAIN` 纯函数 + 热路径）：
  冒烟(一)标定板麦信噪余量 1.36 倍太薄，×8 后真板 wav RMS p50=**588 ≈ 73×8=584 期望值**
  （增益铁证）、说话 ~1304；饱和钳位不翻卷（翻卷产生假静音假爆音比削顶伤）；增益仅作用
  16bit 上行面，DoA 相关计算走 24bit 原始域线性增益不影响相关峰位置（B 批铺垫不受影响）。
  ②**波特率提档实测否决回守 921600**（[board.rs](../tools/esp32-periph-base/src/board.rs)
  注释全账落档）：2M 字节滑移稳定乱码（"ERR unknown cmd"）/ 1.5M 首包干净续包间歇乱码，
  双发方向同现——真凶**不是 FT232R 分频精度**（1.5M=n=2 精确档）而是**杜邦线信号完整性**，
  921600 是当前接线的物理可靠上限；回推：换短屏蔽线/PCB 走线后另批重评（1.5M 仍为首选）。
  ③**VAD 阈值重标定 rms_start 120→800**（config-esp32.toml）：增益后口径底噪 584 / 说话
  1304，阈值余量 1.63 倍；120 为增益前口径，增益后底噪即踩爆恒起音。④**冒烟(三)（真板全链
  两轮）**：run1（mac 系统静音期）brain 成轮 ×4 与用户 4 句**逐一对齐、5 分钟零误触发**——
  VAD 阈值 800 对真实语音干净、环境不误触；run2（解除静音后）用户 1 句 → **mac 出声用户亲耳
  确认** + 播放回灌瞬态（多 3 轮）**自限不发散**（30s 静默窗轮次零增长）——**播放期 VAD 门控
  （外放 → 板麦回捕抑制）登记为后续课题，B 批 DoA 立项前须处理**。⑤**环境课**：mac 系统输出
  静音（`osascript get volume settings` → `output muted:true`）使 interact"播放成功"日志 ≠
  用户可闻——排障先对拍系统静音再怀疑代码。⑥**audio-rec 工具两缺陷登记**（宿主面，不阻塞
  本批）：长时收流 mac FTDI 驱动偶发读挂起（楔死防护 10s 正确兜住退出）；残流下防御 OFF
  应答解析不跳帧（整帧音频 b64 混进应答行判"板面拒收"假象）。⑦**esp-idf 离线构建口径钉死**：
  本地伪仓 `~/.espressif/frameworks/esp-idf-v5.2.1-offline`（tarball 源树删 .gitmodules + 22
  个 gitlink 去 .git 化转普通文件 + tag v5.2.1）+ `ESP_IDF_REPOSITORY` 指向，构建 7m13s 过
  （github 被墙 + embuild `--recursive` clone 触网拉子模块 exit 1 两绕行）。
- **B批 听声识位全账——b1/b2 实装 + ILD→TDOA 升级 + 排障批 b3c 五项收口（2026-10-20/21，
  提交 `971bdb6`/`3febbb9`/`9dcad5c`/`6965484`/`f0678b7`/`c74afa9`/`e89312b`/`54ce893`/
  `31f18e1`/`4d13a12` + fmt `746866b`/`39ed826` + 本落档提交）**：
  ①**b1 命令旁路（`971bdb6`+收尾 `9dcad5c`、红账 `6965484`）**：flock 单消费者纪律下音频源
  持 conn 者即板外设命令下行唯一通路——`Rr3PeriphCmd` 语义命令入有界队列（cap=2 满丢最旧+
  溢丢计数）、读循环顶拍逐条弹出写出（io 失败回插队首转重连不丢命令）、板帧分类面 Response
  放行记账不污 err_streak；帧构造与舵机桥同单点（`rr3_bridge_call_frame`）逐字同源。
  ②**b2 反射环 + 播放期 VAD 门控（`3febbb9`）**：sense 订阅 `tts_pcm` 喂门（帧体 ms 累计外推
  +gap 轮分隔+tail 尾窗）抑制外放回捕，门开只弃 onset 发布（状态机照喂）；onset 带判向换算
  pan 目标（中位 90+clamp ±30）走 b1 旁路 fire-and-forget；策略量 `[rr3-sense.reflex]`
  缺省 disable 逐点等价、esp32 档 enable=true；上轮登记的「播放期 VAD 门控 B 批前课题」
  随 b2 结清。
  ③**判据 v2 + 批D 音频验收过（`bb9be1c`，前置 `14e7dcc` BCLK GPIO25→GPIO4 迁移）**：
  重建 rig 实录 L==R 0-5%、说话帧 ZCR 低至 0.135（浊音）、人耳听清「你好洛克」——真音频
  定案；**撤回「BCLK pad 损坏实锤」误判**——桥测判据对 BCLK 天生失明（I2S RX 上升沿采样
  SD 自采样恒捕捉跳变后高电平，BCLK 活也全 FF），根因改判 rig 接触面；判据 v2 = L==R 率 +
  ZCR 结构 + 取值种类按 MIC_GAIN 域重校，终判人耳听感。
  ④**b3a ILD 实证无判向力 → b3b ILD→TDOA 升级（`f0678b7`）**：b3a 板上实证裸麦近场 rms 比
  恒 ~1.0（方向信息只在相位差不在幅度差），判向机制重写 TDOA 时域 plain 互相关——整数 i64
  累加、物理搜索域 ±lag_max（按构造剔除离群峰）、Q7 抛物线亚样本、K_Q15 反演 sinθ 经 256
  格 asin LUT 出整度，零浮点零滤波器；判向从逐帧即时改 onset 时整窗回看（判向窗 ring +
  线性化快照交纯函数）；Rr3DoaCfg 整体注入（window_ms/window_rms_min/mic_baseline_mm
  三键，ops 七槽→十槽）；golden 向量 7 组延迟反演逐位钉死（`/tmp/rr3-b3-smoke/golden_doa.py`
  1:1 镜像），35 测试全绿。**b3b 遗留合同红修复（`e89312b`）**：doa 三键只给 esp32 档补了、
  漏默认档 config.toml——装配注入缺，补齐三键（window_rms_min 与默认 vad.rms_start 同口径
  1500）42 绿。
  ⑤**b3 回归通道映射定案（`c74afa9`）**：板上双侧交叉实测（站左判 +90 满偏右/站右判 −72）
  ——判向符号与物理方位系统性相反，定案 esp32 档 I2S 帧序 frame[0]=物理右麦（b3a「甲=右」
  口头确认不可靠，悬空实验只有相对稳定性无绝对方位锚点，装配事实以板上实测锚定）；互相关
  取样下标对调 + golden 通道摆放对调重钉，35 绿。
  ⑥**b3c 排障批五项**（触发 = b3 回归期「说完话 mac 无回应」间歇复发）：
  - **b3c-① 运维计数器天文数字（`54ce893`）**： pcm_frames ~40 亿根因 = 2026-10-05 批B
    落地即埋的**快照×增量双端语义分裂**——客户端发累计快照、rrd 按增量饱和累加，快增长键
    二次方膨胀（pcm_frames 25n²@12700 拍 ≈ 40 亿实测对上）、停止增长键线性虚胖；契约钉
    **增量语义**（心跳值 = 自上一拍以来的增量，conventions 双头各写各的两处口径同批归一），
    Rust/Python 双端镜像修（已发水位镜像 counters_sent），四种重启组合账目自洽，rrd 零改动
    两条既有 e2e 守恒；**在跑链路待重启生效**。
  - **b3c-② 0xE0FF glitch 词（零代码收口）**：mic 统计窗 0x00E0FF00/0x01E0FF00 固定位图
    （同族 bit16 翻转）= INMP441 **SD 三态残留**（固件 dump 注释早已在案），每 0.512s 窗
    ≥1 拍双通道同值，上行换算 ≈1799–3848 单拍 click（12% FS）；三消费面量化无害（VAD 单拍
    RMS≈152≪650 / ASR 抓流实测完美 / DoA 双通道同拍 lag=0 deadband 免转）；随 parked-5v
    独立供电复测预期消落。同批纠偏：console `build=12.3ms×4` 是窗内组帧块数（4×2048=8192=
    整窗）非重试，「组包超时」口径误读撤销。
  - **b3c-③ 断流重连卡死（`31f18e1`）**：sense 实录 24 次断流 23 次重连第 1 轮即愈
    （失步本身属宿主 USB 域，d4 系列在案）——**卡死真凶非回环逻辑**：重连第 2 轮握手扫到
    失步二进制乱行，诊断日志 `&line[..80]` 按字节切片切进 `from_utf8_lossy` 产出的 U+FFFD
    多字节字符内部 panic，**读线程当场毙命**，「重连永不放弃」被一行日志格式化打破；修
    `rr3_log_head` 按字符截断 + 回归测试复现实录形状（79 ASCII + U+FFFD 字节 80 恰在
    chars 79..82 内）。
  - **b3c-④ VAD 阈值余量（`4d13a12`，用户认可）**：交互实测半米嗓音峰值 RMS 830–945 vs
    起音阈 800 余量仅 1.04–1.18×（轻声/退半步即漏起音——正是「说完话无回应」主因之一），
    底噪 430（拔舵机后）比标定 584 干净；`rms_start`/`doa.window_rms_min` 同口径成对
    800→**650**（语音余量 1.28–1.45× / 底噪余量 1.51× 两侧均衡），默认档 1500 不动。
  - **b3c-⑤ 底噪/brownout 结案**：底噪 430 < 标定 584 可结案；**brownout 根因链回写实锤**
    ——舵机接板载 5V 激磁压垮轨（电压告警 tier=severe rail=142mV，BOD 复位循环与 0.0.5
    方案C 同形状），独立 5V 共地供电方案 parked-5v 待落。
  - **parked-5v 落成（2026-10-21 验收实录）**：可调稳压适配器（旋钮定 5.0V 锁死）独立喂
    云台——适配器红→两舵机红，黑→两舵机棕 + 板 GND 单点共地（只共地不共 V+），信号
    pan=GPIO32/tilt=GPIO33 不动。反射环激磁 ×2 实锤（doa=-47° → pan=60°，板应答
    id=110/111 ok + 帧面收令），激磁窗**零新欠压告警、零 BOD、零复位**（对比旧接线
    tier=severe 142mV + 复位循环），堆 free≈160KB 稳、OLED 照常——**舵机电流已不上板轨，
    brownout 根因链闭环**。boot 期 rail=205/343/466mV 各次不同 = 占位分压 1:1 未标定
    换算误报（方案A Q5 标定既有档案，非新问题）。0xE0FF 残留未全清（dump 位图转丰富、
    峰值仍偶踩同族值）但双通道同值特征消失（L==R 0%），b3c-② 无害论证继续成立。
  ⑦**真板验收实录**：反射环快环在板——说话即「doa=23° → pan=113°」判向换算 + 板面应答
  id=N ok（pre-execution ack）逐拍在案；ASR 抓流真云对拍完美（0xE0FF click 本在流中不碍
  识别）；计数器/断流/VAD 三修复后链路稳定性敞口随下次链路重启生效复测。
  ⑧**「说完话 mac 无回应」悬案定案 + brain 预滚环修复（2026-10-21，`5e161a3`/`64e111e`）**：
  b3c 收口重启后 E2E 连测两拦截项，逐一定案——
  - **主拦截项 = 舵机激磁 EMI（实锤，物理修复待用户动手）**：断流 3/3 紧跟 servo 命令
    应答出现；**拔掉舵机信号线后 7 连发激磁命令零断流**——触发器是电机激磁电流尖峰经
    共地路径打穿宿主 FTDI RX（非写帧本身，板子全程在推流：重连握手扫到 669KB 实时音频
    为证）。EMI 链：激磁尖峰 → FTDI RX 冻结 → 断流重连（1.6s 检测 + 1~3 轮握手对抗
    音频洪水）→ utterance 被斩/采集饥饿空帧 → ASR 空文本弃轮。parked-5v 治了 brownout
    （电流不上板轨）治不了 EMI（电机回流仍走共地路径）——独立 5V 后激磁仍断流即此因。
    **修复方案（用户动手）**：星形共地（电机回流汇接点挪适配器端，不经过板 GND 排针）+
    舵机供电两端并 470–1000µF 电解 + 0.1µF 陶瓷（贴近舵机）+ 舵机电源走线远离
    USB/UART → 重插信号线终验（舵机动 + mac 回应并存）。
  - **次拦截项 = brain 采集削头（`64e111e` 预滚环修复，健康流空文本成因）**：信号线拔除
    （EMI 隔离）后链路稳定，短句仍偶发「识别为空文本」——sense onset 要凑满
    `min_speech_ms=200` 连续有声才发事件，**这段判据语音本体在 onset 之前已流过总线**，
    brain 从 onset 起算采集必削头 200ms+，「你好」级短句（~0.5s）所剩过短，云端 ASR
    对过短音频回空文本。修 `dialog.py`（继任 = [atoms/dialog/mech.rs](../modules/rr3-brain/atoms/dialog/mech.rs) 的 `Rr3Dialog`）：
    `PREROLL_MS=400` 预滚环——未开轮期间滚动保留最近 400ms pcm（以帧自身 ms 计长，
    饥饿空帧 pcm_b64="" 不入环不计时），onset 时拼回采集头；3 条单测（拼头/窗界/空帧），
    全量 76 绿。**E2E 验收**：长句「请告诉我今天的上海的天气是晴天还是阴天？」完整识别
    → LLM → TTS 播放 5277ms；短句「你好。」带句号识别（头尾齐铁证）→ 回复 → TTS
    2557ms——原失败模式（短句必空文本）消除。
  - **登记两条**：ASR 云端 read timeout（api.siliconflow.cn）= 网络抖动环境性，不扩
    scope；拔信号线期间用户自由对话多轮全通（ASR/LLM/TTS/mac 出声全链无拦截）。
  - **⑧-终 验收过（2026-10-21，`2a801c0` 避让 + 用户物理整理）**：用户零采购三步——星形
    共地挪适配器黑线端（两舵机棕 + 板 GND 单线共汇接，电机回流不穿板地）+ 舵机红棕绞线
    远离 USB/UART + 地线加倍；代码侧反射环动作时机 onset→判停后（onset 判向存 pending，
    判停才经旁路下发——激磁尖峰不落采集窗，断流落 ASR/LLM/TTS 窗自愈；连发 onset 覆盖
    pending 取最新意图）。终验实录：**说完之后云台转头 + mac 回答并存，日志断流/重连
    零次**——EMI 拦截项闭环，「说完话无回应」从软件（panic/削头）到物理（brownout/EMI）
    四层根因全部清账。tilt（GPIO33）无宿主命令面，黄线暂不插（固件持续纠偏 = 额外噪声
    源）。**遗留小尾巴（不阻塞）**：弱嗓/短促语音偶发空文本轮（VAD 敏感度边际，非机制
    故障，重试即成）；电容加固（470–1000µF + 0.1µF 贴舵机）登记为可选采购项。
- **方向感实测通过——判向验收收口 + UART 开口载波依赖修复 + 旧棒烧毁定性修订（2026-10-24，
  `2c737ef` + 本提交）**：
  - **判向验收过（听声识位第③项）**：隔离 FT232R（A50285BI，iCoupler 数字隔离 + 隔离电源，
    TTL 3.3V 跳线；棒 TX→板 GPIO19 / 棒 RX→板 GPIO18 / GND 共地）换装后链路重起全绿
    （`音频源就绪 backend=esp32 rate=16000 ch=2`，主循环 CPU 增量实转），用户左/右两侧
    说话对拍——**左说 doa 负 → pan=60° 头朝左转 ✓；右说 doa 正 → pan=120° 头朝右转 ✓**，
    两次均朝发声者转，左右映射正确零翻符号；转幅维持 ±30°（既有裁定，不刷板不改档案）。
  - **UART 开口载波依赖修复（`2c737ef`）**：新隔离棒阻塞 open 于 tty.\*/cu.\* 双节点均永挂
    （线程 100% 卡内核 `__open`）——根因是 macOS 阻塞 open 以 DCD 载波为闸，隔离棒 DCD
    经隔离器悬空恒"无载波"；O_NONBLOCK 开口 + CLOCAL 即通。落码三步：数据 fd O_NONBLOCK
    进场（guard fd 同款理由）+ configure 置 CLOCAL|CREAD（TTL 链路无 modem，载波检测语义
    整体摘除）+ 开口收尾 fcntl 撤非阻塞（VMIN=1 阻塞读语义逐点等价）；`cargo test -p
    rr-ipc` 81 passed，uart.rs clippy 零告警。
  - **旧棒 A98KN77U 烧毁定性修订**：其四重隔离实验（悬空/关机/换口/裸 cat）全部以阻塞
    open 为判据——同机理可解释全部症状，"硬件烧毁"结论存疑（真死因需 O_NONBLOCK 方式
    复检，棒已收起不追验）；排障判据升级：**阻塞 open 永挂 ≠ 设备烧毁，先做
    O_NONBLOCK+CLOCAL 差分再定性**。
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
  → ①**云适配层**（继任 = [src/cloud.rs](../modules/rr3-brain/src/cloud.rs)）——
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
  `test_cloud_real.py` opt-in 冒烟（Python 侧，随批N32 退役；Rust 侧 real 路径形状由 cloud.rs 客户端单测经本地假端点钉住）
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
  MC-7 复核（2026-10-13）——**不勾，缺什么已写**：装配演练面在位（esp32-mini.toml + contract.rs pin + rr3-pack 正例），件承载三缺（件模块未进 xtensa 门 / 板侧仍专用固件 crate / `[hardware]` 进位未补），F-8 维持不可达。**批N9 件五（2026-10-22）：三缺之缺③ `[hardware]` 进位收**——`[hardware]` 三键→十键（uart2/ledc/i2s 七键进场）＋宿主 rr-ipc/rr-hal/rr-registry 三 crate 扩位＋板侧十键装配对账锚＋**真板三连实证**（正向注入✓对账✓五轮稳定／失配 ledc_pan=30 对账✗ fail-visible 点名／抽注入零注入态＋boot 解卡拒 fail-closed——实录见 [批N9 计划册件五行](process/dev/0.0.5增补轮六——批N9开发计划与真板收尾.md)）；缺①xtensa 门已随批N8 件三收，缺②固件 crate 收编归 0.0.7 余量，**MC-7 维持不勾整单**；**批N23（2026-10-23，四提交 `75532f1`/`9ef77d1`/`9158612`/`233707d`＋收口本提交）：三缺之缺② 板侧固件 crate 收编收码面半边**——`tools/esp32-periph-base` 立 `atoms/{servo,face,capture}/mech.rs` 三原子件目录形态（文件级机制/策略分离：机制纯函数＋行程生成物 `include!` 在件内，策略量 DUTY_MIN/MAX·BOOT_*·AXIS_SETTLE_MS·MIC_SR_HZ·MIC_GAIN·AUDIO_* 留 board.rs 按参传入，策略→机制单向依赖反向禁）；格位对齐头注申报 servo↔act.servo / face↔ui.face / capture↔audio.capture、不立 atom.toml 不冒领格主（计划册 §2 三不三立：不冒领格主／不扩契约词域／不 fork 校验器＋立件目录形态／立策略集中点显式化／立格位申报）；板侧纯函数单测 13 条宿主实跑（servo 4＋face 4＋capture 5）、xtensa 门两步绿×3、check.sh 87 PASS×3。**真跑半边不冒领**：件与装配单真跑未发生，0.0.7 余量如实归真板窗，**MC-7 维持不勾整单**；
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
- **LLM 模型面统一 + 模型行写面落库（2026-10-24，提交 `136289a`，ADR-0023）**：换厂商从"改代码"
  降为"改一行配置"——模型行 `base_url` 定为**完整 OpenAI 兼容 API 根**（GLM `/api/paas/v4`、
  Qwen `/compatible-mode/v1` 各写各的，客户端只追加 `/chat/completions`；`cloud.base_url` 仍是主机根，
  ASR/TTS 拼 `/v1/audio/*` 的语义不动）；新增 `config_store.rs`（`[[rr3-brain.model]]` 块重写，块外
  文本含表头前导注释逐字保留、块内行间注释丢弃，`.bak` + 原子写）；三 RPC `model/list|set|test`
  （完整 CRUD、恰一 active、拒删 active、probe 只判 2xx 不写历史）；面板端点 23→26
  （`GET /api/brain/models` / `POST /api/brain/model[|/test]`，写口申报 `live`），前端加"模型行"
  卡片（deepseek/glm/qwen/siliconflow 四预设 + 手填）；契约 39→42 条（request 19→22），
  `rr-registry` 两处 + `rrd` 三处 pin 同步。`bash tools/ci/check.sh` 全绿。
- **ESP32 协议共享 crate + 固件端口层（2026-10-24，提交 `b0fa690` 与本批，ADR-0024）**：帧协议真值从
  "宿主/固件各一份"收成一处——新 crate `base/runtime/rr-periph-proto`（依赖仅 `rr-ipc` + `serde_json`，
  零 `esp-idf-*`，故两端双向可编译）收拢 task 位形/主题/magic/请求解析/应答组帧/桥调用帧/板命令帧；
  `rr-hal` 两文件改 `pub use`（既有 `rr_hal::esp32_uart::*` 路径与测试零改动），固件 `frame.rs`
  收帧/回帧改吃 crate 单点，`board.rs` 只 re-export 并以编译期 `const` 断言钉住生成物主题串
  （⑨-d 管"生成物⇄契约"，本断言补"生成物⇄crate"这条边）。固件新增 `ports.rs` 五端口 trait
  （UART/I2C/PWM/ADC/Clock）——事务级接缝、策略留上层，I2C 总线自愈与 LEDC 回读不进端口；
  UART 帧面、舵机/轨压、OLED 写入三路迁移完成，I2S/麦克风本轮不抽象（登记后续 audio port）。
  门禁：`cargo test -p rr-periph-proto` 10 + `rr-hal` 63（1 ignored）；
  `tools/ci/esp32-periph-base-check.sh` xtensa 构建 + 单测编译 PASS；`bash tools/ci/check.sh` 全绿。
  **遗留**：树莓派板侧端口后端留待后续批（rr-hal 宿主后端已齐备）。
- **音频上行链路提档 2M（2026-10-30，本提交）**：音频上行是链路容量的天花板问题——
  16k×2ch×s16 = 64,000B/s 原始，b64 后 + JSON + 128B 帧头 ≈ 85–87KB/s；921600 8N1 有效
  92,160B/s，**占线 ~95%、余量仅 5%**，一次握手/日志/回包抖动即挤掉分片（表现为 recorder
  端 seq 断档/背压 drop）。处置选了"只扩链路容量、不动已验收的音频口径"：`UART2_BAUD_HZ`
  921600 → **2M**（有效 250KB/s，占线 ~34%），rr-ipc 档位表扩至 115200/9600/921600/1.5M/2M
  （macOS 字面值 + Linux 八进制常量各读各的），桥/audio-rec/探针/`RR3_HAL_PERIPH_BAUD` 同表。
  **历史否决的重评依据**：2026-10-20 的 2M/1.5M 否决真凶是**杜邦线信号完整性**（2M 字节滑移
  稳定乱码、1.5M 首包干净续包间歇乱码），本轮链路已换**树莓派 PL011 `/dev/ttyAMA0` 三线直连**
  （GPIO18/19 ↔ UART2，无 FT232、无线长杜邦），故重评。**真板实测（Pi 探针 termios
  `B2000000`）**：60s 音频 85.2KB/s / 469 帧 / 头失配 0 / 撕裂 0 / seq 断档 0；180s 长跑
  15,700,468B / 1406 帧全零误码；混跑 30s（每 5s 50×PAN + INFO）回包 300/300、ERR=0；
  U3 canonical digest 两连跑 10/10 自等（与 921600 基线 9/10 等，唯一差异 = HELP 行按
  `UART2_BAUD_HZ` 渲染的波特率字样）；行面 robustness 全套（超长行/半行超时/拼接/20 连发/
  写前记账 120→60）在 2M 下同样全过。classic 固件已烧（App 1,150,784/4,128,768 = 27.87%）；
  S3 目标构建同过。**回退预案**：1.5M，再退 921600（换线/换适配器后须重跑同套探针）。

- **2M 压实复测 → 根因闭环，定档 1.5M（2026-10-30 同日收口）**：15 分钟混跑（音频
  全速上行 + 每 5s 50×PAN+INFO）暴露**低频线面误码**——板侧账干净（`sent=seq=7037`、
  `drop=0 job_drop=0 reply_drop=0`），宿主侧却只收到 8982/9000 回包、`ERR` 5 条且全是
  **行首被啃**的碎片（`unknown cmd "N"/"0"/"AN"`）；120s 缩短版同形（1194/1200、ERR=3）。
  对照实验：纯命令 60s **600/600、ERR=0**；纯音频 120s（稀疏命令）**938 帧、断档 0、
  PCM 全 8192、ERR=0**。即**只有"连续音频 TX + 命令并发"双向同时跑才掉字节**。
  **根因排查（FT232RL 独立适配器到货后）**：Pi 上只插 FT232、PL011 三线全拔，先在
  921600 对照组跑混跑——**也炸**（150s ERR=66/1500，`"PAN"→"PAL"/"PAJ"/"@@N"` 全是
  单 bit 错，而音频方向零误码）→ 证明与波特率/PL011 无关。照片核对定位真凶：
  **模块 LEVEL 跳线出厂停在 5V 档**——3.3V 信号在 5V 门限边缘抖 bit（模块 VCC 由
  Pi USB 供电，不接板子 3V3；跳线订到 3.3V 档后 921600 混跑立即 1500/1500 ERR=0，
  根因闭环）。**1.5M 定档实测（同板 MAC 48:9d:31:01:16:d4，`/dev/ttyUSB0`）**：
  纯音频 60s 469 帧/零撕裂/零断档/120×PAN 全回包；混跑 300s×2 合计 **5996/6000、
  ERR=4（0.067%）**——`"PAN"→"NAN"`×2、`"90"→"990"`×1、丢 1，全被命令面双防线
  （unknown cmd 丢弃 / 角度范围检查）无害化；与 PL011@2M 的 5/9000（0.056%）同量级，
  而 921600 两路径均零误码 → **残余误码公共底是杜邦线物理层**（换适配器/换档位都
  不消除；串阻 100–330Ω + 缩短线长为可选优化）。**定档 1.5M**：占线 95%→46%；
  2M 不取（误码率不随档位下降，余量收益换不回物理层风险）。classic 固件已重烧
  1.5M（App 1,150,784/4,128,768 = 27.87%）；S3 构建过门。`board::UART2_BAUD_HZ`
  落 1,500,000，全宿主缺省档/注释口径同步；rr3-audio-rec 缺省 `--baud 1500000`。
  同批把六支探针收进 `tools/pi-periph-probe/`（缺省 1.5M，`RR3_PROBE_TTY`/
  `RR3_PROBE_BAUD` 可换口换档），首跑还修了探针自身在流尾截断片上 b64 padding
  的崩溃缺陷。**真实链路验收（同日补）**：Pi 上以 `rr3-audio-rec /dev/ttyUSB0 --secs 60`
  （缺省 1.5M）落 wav——程序自述 `chunks=468 gaps=0 restarts=0 err=0 resyncs=0
  reconnects=0 frames=958464`；wav 二进制独立解析 2ch/s16/16000Hz/59.904s、
  3,833,900B（=3,833,856+44 头）、RMS L=587.4/R=587.8（与标定底噪 ~584 对得上）、
  峰值 2656/2840、静音比 0、L/R 严格等长 → 判据①（音频上行落盘口径）在 1.5M
  真实链路成立。**遗留（并行）**：杜邦线串阻 100–330Ω 为可选物理层优化（0.067%
  残余误码已被命令面双防线无害化，非阻塞项）。

- **探针口径系统性修正（2026-10-30，六支收进公共层）**：此前六支探针各自复制
  `baud_of`/termios 开口/`pump`/帧扫描，同一判据在不同脚本里含义不同，已实测出
  三个假象——① 混跑把**音频帧间夹着的命令回包文本**逐字节计成「头失配」（2M 时代
  报 24012B，实为 30 轮回包 ≈24KB，纯属口径失真）；② 收尾固定 `pump(2s)` 在末轮
  回包还压在线上时提前返回，制造「回包 OK 少 3~9 条」的假丢包；③ 起流前
  `pump(0.8)` 白扔数据导致缓冲区**从一帧中间开始**，帧头保留区零字节被误判成
  「乱码 11124B」（抓到 dump 离线复核定位，`first magic off=11124` 即铁证）。
  处置不是逐个打补丁，而是抽 `probe_common.py` 作**判定口径的唯一实现**：帧面
  magic/头长/偏移、**段分类**（帧外字节按整段判「行面文本」vs「乱码」——只有含非
  可打印字节的段才算误码）、`drain_until_quiet` 收尾、`flush_input` 起流前清残留、
  以及「回包行**只从帧外文本段**切」的纪律（在整条解码文本上切行会把帧载荷 JSON
  和回包挤进同一行，制造假 mangled）。`bw_mixed` 同时补上**命令面四分类对账**
  （`sent = ok + err + mangled + missing`，`err`=板侧拒绝含被误码破坏的命令，
  `mangled`=回包文本本身被坏，`missing`=完全无回包）与**板侧↔宿主侧音频对账**
  （INFO 的 `seq/sent/drop` 增量 vs 宿主解出帧数，基线必须在起流前采否则窗口错位，
  实测差 6 帧即由此来）。回归：`bw_probe` 45s 乱码 0；`bw_mixed` 60s `发=600
  ok=600 err=0 mangled=0 missing=0`、板侧增量与宿主帧**差值 0**；150s `发=1500
  ok=1500 err=0 mangled=0 missing=0`、`seq/sent` 增量 +1178 = 宿主 1178 帧、
  乱码 0（**口径诚实后该轮零误码**，与早前 300s×2 的 4/6000 合并看 ≈0.05%，
  即误码是间歇而非恒定）；`p1_probe`
  四探全过；`probe2` 长行程 120→60 与 30/30 回包全过；`robust_probe` 六探全过；
  `u3_capture` 两连跑 **10/10 自等**。**探针从此不再允许各写一套口径。**

- **线格式启用载荷 CRC32（2026-10-30，协议硬切换）**：审查发现全仓**无任何完整性校验**——
  128B 头预留区 84 字节全空，帧内载荷被线面误码改坏后仍能通过 magic/长度/JSON 四道门，
  音频方向表现为**静默杂音**且无任何计数可查。处置：把预留区前 4 字节（offset 44..48）
  改作 `payload_crc32`（IEEE 反射多项式 0xEDB88320，逐位实现零查表），余 80 字节仍须全零。
  单点真值：`rr3_crc32`（唯一算法）、`rr3_header_bytes_with_crc`（唯一发送入口）、
  `rr3_verify_crc`（收侧校验，`rr3_framebuf_read` 与 `FrameAssembler::pull` 均已接，
  不符即**整帧丢弃**并计错，丢片由 seq 对账继续记账）。契约同步：`envelope.schema.json`
  偏移表 + `x-offset-sum`、ADR-0006 决策 1 追加项、contracts/README §3 均已写明这是
  **硬切换**（旧端按 `ReservedNotZero` 拒帧，全端须同批升级；跨生态互通再启用 flags 版本标记）。
  **验证**：rr-ipc 单测 80 通过（新增 `crc32_known_vectors`（含 `"123456789"`→0xCBF43926`）、
  `assembler_rejects_corrupted_payload_crc`（翻载荷 1 字节→整帧丢弃且下一真帧照常出）、
  `framebuf_read_rejects_crc_mismatch`）；rr-hal 63 通过；base/runtime workspace 685 通过。
  **实板**：classic 已重烧（App 1,151,952 / 27.90%，MAC 48:9d:31:01:16:d4）；1.5M 纯音频 45s
  358 帧 **CRC 不符=0**；混跑 90s 710 帧 **CRC 不符=0**、`发=900 ok=900 err=0 mangled=0
  missing=0`、板侧增量与宿主帧**差值 0**；Pi 端 `rr3-audio-rec` 重建后 30s 落 wav
  `chunks=233 gaps=0 err=0`、2ch/15996Hz/29.83s、RMS 593.6。
  探针同步升级：`probe_common.scan_frames` 现在逐帧校验 CRC 并输出 `CRC不符`（**音频方向
  误码率第一次有了直接判据**，不再只能靠"看不出来"来断言干净）。

- **平台限制与最终链路形态（2026-10-30 定档后的账面结论）**：
  - **宿主适配器**：板固件 1.5M 档在 macOS 上**不可用**——macOS 的 FTDI 驱动对
    `tcsetattr` 的 1000000/1500000/2000000/3000000 全部返回 `EINVAL`（实测 921600 是
    上限）。故 **Mac 侧开发若要与板直连，必须把板烧回 921600**（或换 Linux 宿主）。
    Linux（Pi，USB + FT232RL 或 PL011）接受 B1500000/B2000000，是唯一能做 1.5M 验证的宿主。
  - **适配器电平（仅备选路径适用）**：若回到"经 USB 适配器"形态，FT232RL 模块 `LEVEL`
    跳线**必须 3.3V 档**，模块 VCC 由宿主 USB 供电、
    不得接板子 3V3（板上有 3.3V LDO，双供电会倒灌）。这是 2026-10-30 误码的真凶，
    已写入 `board.rs` 常数账簿。
  - **最终链路形态：定版「Pi PL011 直连」**（2026-10-30 换线实测通过后改定）。接线只有三根
    杜邦线：Pi GPIO14(TXD0)→板 GPIO19、Pi GPIO15(RXD0)←板 GPIO18、GND↔GND（两端均 3.3V，
    不接 VCC、无电平转换）。**部署前提三条**（缺一即数据流被污染）：`config.txt` 加
    `dtoverlay=disable-bt`（否则 PL011 被蓝牙占、`/dev/serial0` 落到 mini UART）、
    `serial-getty@ttyAMA0` 停用（否则登录提示混进数据流）、`cmdline.txt` 不保留
    `console=serial0,115200`。改完后 `ls -l /dev/serial0` 必须指向 `ttyAMA0`。
    **判据实测（1.5M，`/dev/ttyAMA0`，板固件 1.5M + CRC）**：纯音频 45s 358 帧、撕裂 0、
    **CRC 不符 0**、seq 断档 0、PCM 全 8192；混跑 300s×2 合计 **5995/6000**（第二轮
    3000/3000 全绿，第一轮 5 处异常：ok=2995/err=1/missing=4），**CRC 不符 0**、
    板侧 `seq/sent` 增量 +2350 = 宿主 2350 帧、差值 0；`rr3-audio-rec` 在 `/dev/ttyAMA0`
    落 wav `chunks=233 gaps=0 err=0`、2ch/15995Hz/29.83s。合并误码 5/6000 ≈0.083%，
    与 FT232 路径的 4/6000 ≈0.067% **同量级**（两条路径的错误形态同为"宿主→板短命令偶发
    单字节坏"，音频方向均零 CRC 失败）。
    **FT232RL 退出部署链路**，但保留为两用调试器材：Mac 侧联调（macOS FTDI 驱动上不了
    1.5M，只能 921600）与 Pi 串口被占用时的备用接入。若未来要回到"经适配器"形态，其
    电平约束仍然有效（`LEVEL` 跳线必须 3.3V、模块 VCC 由宿主 USB 供电、**不得**接板子 3V3）。
  - **Pi 侧编译约束**：Pi 4 仅 1.8GB 内存，`cargo build --release` 单 crate 可过（rr3-audio-rec
    实测 11s 增量 / 40s 全量），但**不要并发多 crate 构建**，否则 OOM。

- **全功能冒烟 + 帧面真机首验 + 空闲前导根因处置（2026-10-30，V1/V2）**：
  **V1**：新增 `full_smoke.py`（探针第七支）一次覆盖行面 11 条命令——OLED、16 种 FACE
  白名单逐个、PAN/TILT/SERVO、两级拒绝（>180 解析期「角度超范围」/ 0-180 出轴行程
  「轴超出行程」）、WANDER、SCAN 五点、RAIL 异步回包、INFO 计数段、AUDIO 控制面，
  末行给判据集 digest（跨次对拍）。首跑 32/34 的两处失败均为探针自身文案/收流窗口
  写错，修后 **35/35**（digest `829477d49e20818d`）。顺带点名硬件项：`RAIL rail=284mV
  raw=0` = 5V 分压测点未接线（`rail_tier=severe` 由此来），非固件问题。
  **V2**：`frame_probe.py` 首次真机验证帧面（128B 头 + CRC + trace 回带 + dst 门 +
  `rr3.set_bindings` 注入）——首跑 7/10，失败请求**无痕消失**：板面控制台无
  `帧面 id=` 无 `帧面丢弃`，宿主收 `ERR line too long`。判定实验两步定位：
  ① 加 200ms 帧间隔仍失败（排除时序竞态）；② 每帧前插 1 个 `\n` 前导 → 三轮
  **30/30**。结论：**空闲后首突发的首字节失步**（电气层，只坏一个字节）——行面
  只损失一条命令（unknown cmd 拒，即此前记账的 0.067–0.083% 残余误码），帧面则
  整帧降级成行（`LINE_CAP`=128 < 帧长）无痕丢失；此前"两条路径残余误码同量级"
  与"音频方向零 CRC 失败"（连续流无空闲）都与此机理自洽。
  **处置（宿主单点）**：`rr-ipc Rr3UartConn::tx_idle_preamble`——空闲 >20ms 的
  宿主→板突发先发 1 个 `\n`（板面当空行跳过）+ 2ms 再发正式字节；`write` 是全部
  写路径的单点（帧经 std adapter、行经 write_all 都到达），桥的转发块被拆半的
  场景因间隔远小于阈值而不触发。板侧 `read_line` 超长行加前 8 字节 hex 取证日志
  （重烧待 devkit USB 恢复）。**验证**：rr-ipc 81（含 `tx_idle_preamble` 专测）、
  rr3-bridge 6（恒等测试按新线上契约吃前导）、check.sh 全绿；实机 frame_probe
  三轮 30/30、full_smoke 35/35、bw_probe 30s CRC 不符=0。
  **遗留**：devkit USB 恢复后重烧诊断版固件；`frame_probe` 上桥/上 rr-hal 的
  生产等价路径（`rr3_ensure` + rr3.call）后续按同判据跑。

- **V3 喊话回应闭环（2026-10-30，进行中——触发已验证，方向修复待复测）**：
  新增 `voice_respond.py`（探针第八支）：AUDIO ON → 流式解析 Event 帧 → 逐窗
  （512ms）双通道 RMS → 超自适应阈值（环境 ×1.25）→ 回应。**触发链已实机验证**：
  100s 内 3 次喊话 3 次触发、解析失败 0 帧。过程中修了三个探针自身缺陷：
  ① 帧跨读取块（11KB 帧 vs ~4KB/50ms 读）导致"每次清缓冲整帧解析"永远凑不齐
  → 改流式游标解析 + 残尾保留；② `pcm_b64` 是 str，拼 bytes padding 抛
  TypeError 被裸 except 吞掉 → encode 后拼接 + 失败计数 fail-visible；③ 退出
  路径漏发 AUDIO OFF 留下遗留流 → 补收尾。另据用户实测反馈修三个体验缺陷：
  舵机不动（az=−0.7° 映射回 PAN 90 原地踏步 → **ILD 只取符号**：死区 ±0.5°，
  超出即固定偏转 ±20°，与项目 DoA"符号分离不冒领精度"口径一致）；HELLO 文字
  盖掉表情且文本模式角度行常显（OLED 命令关 `expr_mode`，设计使然 → 回应不再
  发 OLED）；部分喊话漏触发（阈值 1.45× 实测漏 2 次 → 降 1.25×）。
  **诚实口径**：当前回应由声音事件触发，"你好洛克"关键词识别（ASR/KWS）未建。
  **DoA 判定实验（2026-10-30，新增 `doa_probe.py` 探针第九支——测量先行）**：
  v5 把判向从 ILD 换成 TDOA 后实机 3 次触发 lag 全负（-1/-3/-4），而 ~4.5cm 麦
  间距在 16kHz（1 采样点 = 2.14cm 声程）下物理上限只有 ±2.1 采样点——观测值
  超出物理上限，峰值必是噪声主导。判定实验（六轮带 OLED 提示：左×2/右×2/中×2，
  每轮 AUDIO OFF 后离线分析）拿到三份证据：
  ① **TDOA 判死**：唯一两次较强触发（2.1×/1.4×）的互相关峰值 lag=+5/-8 采样点
     = 10.6/-17.1cm 声程，远超物理上限，|c|≈0.04-0.07 近乎零相关；另发现 v5 用
     `abs(c)` 选峰是方法学错误（真峰应为正相关，负峰是反相关伪峰）。
  ② **通道增益偏差可忽略**：三轮标定 ±0.04dB——ILD 判读不需要通道校准。
  ③ **ILD 证据不足以下结论**：触发值普遍偏弱（1.2-2.1× 中位），右侧轮全窗 ILD
     为负（-1.67/-2.79dB）但左侧轮单独补测也是负（-1.29dB）——疑似用户喊话
     方位的参照系（"自己的左"vs"机器人的左"）混淆，需 AB 双相位实验一锤定音。
  **实验方法学教训**：六轮连跑靠用户盯屏幕对节奏，六轮只拿到 2-5 个有效触发
  （多数 onset 抓到的是走动声或压根没喊）——改单侧快跑模式（12s 窗 + 聊天里
  精确协调）与 AB 双相位（先用户左手边后右手边，符号直接给映射）。
  **未决**：AB 实验待用户配合跑完 → 依数据改 voice_respond 判向（ILD 符号 +
  PAN 方向映射，PAN 增角对应哪侧也要用同一次实验钉死）→ 再提交 voice_respond；
  阵列级定向（真 TDOA）需换麦间距或提采样率，记硬件约束（GCC-PH 类超本轮范围）。
  **系统体检同轮完成**：full_smoke 35/35、frame_probe 10/10、音频 240 帧
  CRC 不符=0——板侧全部模块正常；`RAIL raw=0` = 5V 分压测点未接线（硬件项）。
  **V4 舵机运动正确性（2026-10-30，新增 `servo_motion.py` 探针第十支，PASS）**：
  四相判据全绿——A 单轴到位回读（PAN/TILT 各四点扫行程，逐点轮询 INFO 至
  `pan=/tilt=` 等于目标角；`st` 只由运行面在作业成功后镜像硬件真值，INFO 等于
  目标 = 真到位而非控制面复述）；B 连续动作 30 条 PAN 75/105 @50ms 连发
  ack 30/30、最终角 = 最后一条（合批语义正确）；C SERVO 双轴串行到位；
  D WANDER 让位（外部点名后 wander 自动 off）。对账 `job_drop/reply_drop`
  增量全零。**诚实到位时间**：常规动作 ≤264ms、60° 大行程 727ms（含作业队列
  + OLED 重绘）；探针自身量了两版假读数（ack 等待 1.0s / INFO 等待 1.0s 把
  全部"到位时间"抬成 1016ms）——计时口径必须让等待窗口 < 被测量，教训入探针
  注释。
  **V7 源码审查补完（2026-10-30，部分收口）**：
  ① 六模块**非骨架**（源码行数盘点）：rr3-brain 4274 / rr3-act 1997 /
     rr3-sense 1406 / rr3-interact 1259 / rr3-power 1005 / rr3-connect 896，
     均有 dispatch/boot/main 完整结构。
  ② 音频通道契约**自洽**（批N28 记录的"缺省与实机相反"结构性闭合）：esp32
     HAL 自带 `RR3_ESP32_AUDIO_CHANNELS=2` 常量并显式校验（esp32audio.rs:80,
     :322），sense 对 channels≥2 分配判向窗 ring、双麦原始帧下混**前**入 ring、
     下混 mono 进总线（dispatch.rs:130-133, 232-239, 345-346）——总线事件恒
     channels=1，brain 消费面零改动；`RR3_AUDIO_CFG_16K_MONO16` 只是 legacy
     缺省常量，esp32 路径不用它。
  ③ rrd 面：conn.rs 写帧入口带 payload_crc32（与板侧 CRC32 定档一致），
     registry/supervisor 有注册上限、重启退避、失败上限。**深度审查（锁序/
     无界资源逐行）未完成**，归后续窗口。
  **V5 长跑稳定性（2026-10-30，新增 `longrun.py` 探针第十一支，板侧判据全绿）**：
  600s 并发（音频流全程 + INFO@5s + 全行程动作@10s）：59 动作 ack 30/30→
  全程零 ack 缺失、背压/作业/应答丢失**增量全零**、堆 min_free 零新低
  （100352B 稳住）——**板侧无生产阻碍**。探针自身三轮演进教训（全部入注释）：
  ① 忘发 AUDIO ON 且拿开机累计计数当本轮值（INFO 是累计口径，判据必须取增量，
     seq 基线 8378→13069 连续性可跨轮对账）；② `pkill -f longrun.py` 会自匹配
     ssh 自身 bash 命令行（kill/ pgrep 都要 `[l]ongrun` 方括号模式，且 kill 与
     启动拆两条 ssh——启动段命令行里也含同名字面量）；③ 探针 RX 消化差额
     7/4691=0.15%（<1% 记仪器限制）。**未决**：CRC 不符 3/4691=0.064% 出现在
     音频饱和线上，签名与"Pi 侧 RX 停顿撕裂帧"一致但归因未钉死——待 Rust 路径
     （rr3-audio-rec/bridge）同负载复验其 CRC-bad 计数再定责。
  `cmd_loop`（每轮先 `drain_outbox`/`drain_audio` 再窥视一字节、1-tick 空口回轮首）、
  `read_line`（字节间超时 + 半行预算 + 超长行先吞尾再回 ERR）、`drain_line_tail`、
  `frame_round` 的残尾吞净、`push_job`（同形状 Move 合批为最新目标 + 队满优先让位新 Move）、
  `drain_outbox`/`drain_audio`（写不全计入/点名，不静默）、`exec_job`（**失败也提交硬件真值**、
  上报角永不冒领）、`autonomous_tick`（10ms 拍，决策段持锁纯算、驱动段脱锁慢跑）、
  `mic` 组帧（16KB 线程栈 + 4KB 读缓冲，逐片减 DC/截 16bit/增益钳位）；`rr3-bridge` 双向转发
  （UART 死/UDS 死各自具名退出，UDS 未就绪按 4KiB 节流计数丢弃，下行 EINTR 重试）。
  三处**有界风险**（判断为权衡而非缺陷，故不改代码，写明触发条件供后来者复核）：
  1. `drain_audio` 无预算：`audio_q` 满 4 片时单轮可连写 ≈4×59ms = 236ms，其间不读命令；
     RX 环 4096B 在 1.5M 只抵 ≈22ms 入流，故理论上存在"宿主以线速连发 >4KB 命令 + Q 满"的
     丢字节窗口，且 RX 溢出在 IDF 简单读面无计数（既有盲区）。实测混跑 900 条命令零丢失，
     产品当前也无"线速灌命令"用法；加预算会牺牲音频平滑性，属权衡。
  2. 每片 ≈35–40KB 堆抖动（7.8Hz：`json!` + b64 + payload + 帧 Vec）；实测 `INFO` 里
     `free≈164KB / min_free≈100KB` 健康，缓冲复用属大改，无证据前不动。
  3. `selfheal_skip` 在一次 300s **裸 termios 会话**里涨到 34589——**不是缺陷**：绑定只由宿主
     `set_bindings` 注入，裸探针会话必然走 fail-closed 分支（`state.rs` 字段注释已写明
     "非零即绑定下发链未通"）；真实宿主（rr-hal）会推绑定、自愈照跑。**此为判据澄清**，
     防止后来者把计数当故障。

**已知偏差（有意保留，不是遗漏）：**

- 代码注释里的 `Task N`（如 `daemon.rs` 的"Task 11b 加了唯一的 supervisor 线程"、`registry.rs` 的  "Task 7 已落地"）指的是 **rrd 实现计划**的任务号，与本轮文档重构计划无关。两者同号不同事，
  读文档时别把它们对齐；这些注释不属于本任务订正范围。
- **门禁 ①b 偶发红的真凶（2026-10-30，先误判后取证）**：①b 报红两次，我最初归因
  "UART 时序测试在并发构建下被 CPU 争抢"——**那是没有取证的猜测**。随后连跑 8 次门禁
  把现场留住，抓到实际失败：`rr3mod::serve::tests::serve_dispatches_rpc_event_and_tick_in_order`
  的 `rpcs=[]`。根因是**夹具假设**：该用例把"第 2 拍止步"当终止条件，而预推的 4 帧由
  fake rrd 线程写 socket、再由客户端读线程投递，`poll_wait(0)` 首拍可能一帧未到，
  循环空转两拍即退出——负载只是放大了这个窗口，不是原因。
  修法（判据级，非加超时）：止步条件改为"两 Event + 两 RPC 都已回调 **且** 至少两圈"
  （保留"on_tick 每圈一次"的断言），另加 3s 墙钟兜底并返回**不同文案**，让"夹具没等到帧"
  与"被测语义错"分得开；测试侧 `poll_wait(5ms)` 仅为避免空转抢 CPU。
  验证：rr3mod 连跑 10 次全绿；6×`yes` 负载下 workspace 三连跑全绿；门禁三连跑全绿。
  **教训**：红灯先取证，再归因；"单跑绿"不构成结案。

**V3 判向因果闭环（2026-10-30 深夜轮：触发链根治收口、方向声明诚实撤除）**：
  用户转入"不配合实验、自主全量推进"模式，全部改为无人化验证（深夜静室实测
  + 离线 PCM 回放）。
  ① **转向映射钉死**（用户目视）：PAN 70 = 转向用户左、PAN 110 = 转向用户右
     （减角=用户左，增角=用户右）。
  ② **当日三次"定向采集"（L/R/严格R）全部作废**：onset 计量 1241-1340 全部
     **低于深夜静室纯噪声的 16ms 窗最大值 1740**——从未录到真实喊声；据此撤回
     "左锚 −2.52dB / 右锚 −0.89dB"（噪声涨落）。六轮实验 r3/r4（R 轮）16ms 窗
     峰值 939/948 = 底噪，同样无喊声。
  ③ **唯一可信喊声样本 = 六轮实验 C 轮**（r5/r6，正前 ~0.4m）：16ms 窗峰值
     2803-3541，事件 ILD −0.16~+0.06dB——双麦对正前完全对称（增益匹配锚）；
     但 r6 一次尾段触发 ILD=−2.31dB（ratio 1.53）证明**混响尾即可产生假左
     签名**，按 ILD 转左会误转。
  ④ **触发统计量定案（v7.2，「喊它没反应」根因根治）**：三代淘汰——512ms 窗
     中位（~1m 喊话只抬 1.1-1.2×，必漏）→ 16ms 窗峰值（静室噪声自身冲 1.7×，
     误触）→ **128ms 窗联合 RMS**（噪声窗波动 ±2%，近距喊话实测 2.1×）
     ×1.45 滚动底噪。验证：离线回放 6/6 真喊声触发、4 个无喊声文件 0 误触；
     实机静默 15s 零误触（底噪 587 vs 阈值 851）。DC 假设排除（实测 DC≈−4，
     板侧 chunk_to_pcm 已逐片去 DC）。
  ⑤ **板侧音频链定量体检（链路健康）**：深夜静室底噪与白天完全一致
     （rms L=590 R=594，|l|+|r| median 1024）→ 底噪电气主导，等效 ~41dB SPL
     （自噪 33dB + ~8dB 板级）；按增益链（24bit>>8 保满幅 + MIC_GAIN=8）反推
     r5 喊话 ≈73dB SPL@mic，与 INMP441 规格（−26dBFS@94dB SPL）对表自洽。
  ⑥ **新立案（白班物理检查项）**：0.4m 喊话对底噪能量比仅 ~5dB，自由场物理
     预期 ~40dB——疑阵列声学衰减 ~30dB（壳体遮挡/麦口朝向/装配腔体），需开壳
     目检两麦装配位与声孔。衰减若坐实，同时解释定向 ILD 幅度被异常压缩。
  ⑦ voice_respond v7.2（00882d2 / a1aaa03 已推三远端）：回应 = FACE happy +
     TILT 点头（80→100→93，俯仰示意动作）+ PAN 回中；**方向声明撤除**（待
     白天用本版触发器采带真值标注的定向喊话重建锚点后恢复），每声照记
     ratio/ILD 供账簿积累。工程面：main() guard 可导入化，window_stats/
     loudest_window/quietest_window 可离线回放复用；full_smoke 退场 FACE idle
     （OLED 文本模式残留根治，用户观测的"Hello 字样+舵机参数"即此）。

**V7 rrd 深度审查收口（2026-10-30 深夜轮：锁序 + 无界资源逐项）**：
  - **锁序**：全守护共用一把注册表锁 + 每连接 `Mutex<UnixStream>`（一帧一次
    write_all 整帧原子）+ counters gate 独立锁，三把锁**无嵌套获取点**——
    fanout 走 channel send 不在注册表锁内写慢 socket（registry.rs:927-934，
    这正是"慢连接不拖死全表"的关键构造）；match 守卫 `MutexGuard` 延寿陷阱
    三处立牌给硬规矩（conn.rs:302/:956、supervisor.rs:266——先 let 绑再
    match）；锁中毒单点口径（rr3_lock → into_inner，lib.rs:228）。
  - **无界资源逐项**：日志环有界（cap 512，push/回填双路裁剪，lib.rs:148-205）；
    落盘日志 append 不轮转（磁盘项，批N4 有意保留）；per-conn fanout channel
    `mpsc::channel` **无界但僵尸回收路径已通**（watchdog expire 置 `tx=None`
    → writer 终结 → fd 释放，conn.rs:1073-1081 注释把成立前提写清）；supervisor
    计划队列无界但速率=重启事件（可忽略）。
  - **carry 两条（不在本轮改）**：① fanout 通道有界化 + 丢弃最旧 + 断连
    （Task 10 carry 原样在——通道形状是行为改动，需独立判据与测试，风险窗口
    = writer 阻塞至心跳过期，量级 MB 级）；② 日志文件轮转。
  - **结论：rrd 无生产阻碍**；V7 全部子项（六模块非骨架、音频通道契约、rrd
    深审）至此收口。

**生产栈实机勘查与部署重启（2026-10-31 晨，U 轮）**：
  **形态口径订正（用户当日纠正，以 ADR-0021 裁定 1 为源）**：ESP32 不是"外设板"
  或 Pi 的末梢——**RK3566 / 树莓派 / ESP32 是三个并列独立载体，都是从 Rock3 底座
  裁剪出的不同产品**（各自独立整机、零关系；ESP32 微型形态自带 OLED 表情面 +
  双轴云台 + 双麦 + 配网，认知经桥到某台 host 形态机器，桥只是它的可选能力升级，
  "桥的一端是一台 host 形态机器，不是一台机器的两半"）。本机只是把两台独立机器
  用 UART 桥接运行（`pi4b-host-esp32-classic` 是该组合的装配单）——下文所有
  "板侧/末梢"措辞仅指这台组合里的物理接线位置，不构成形态定性。
  背景：用户报告「喊它没反应/舵机不转/OLED 出现 Hello 字样与舵机参数」。实机
  勘查（pi@192.168.124.52）推翻此前"探针问题"的假设——**生产栈本身在跑**
  （rrd + 五模块 + python brain），但部署停留在 **2026-10-14**，整 17 天未更新。
  逐条取证（全部可复现）：

  ① **部署链腐化（已修，721c12f）**：`pack-arm64.sh` 仍按 python 树打包 brain
     （`cp modules/rr3-brain/rr3_brain`），而 dc9d6f4（批N32件四）已把 brain 迁为
     Rust 二进制并撤除 rr3-pack 的 lang=python 分支——cp 失败整链断。**部署脚本
     无任何门禁覆盖，形态变更无人同批扫描 tools/deploy/**。同日发现 brain 的
     ureq+rustls(ring) 需 C 工具链，musl 宿主交叉结构性不覆盖（aarch64-linux-musl-gcc
     不存在），与 alsa 同类 → 归板上原生构建。已重组：① musl 五件 → ② payload
     （brain 只落 manifest）→ ③ build-onboard（brain+sense+interact 三件）→
     ④ staging → ⑤ 断言。**当日已按新链成功出包并安装**（含旧树备份
     `/home/pi/rockrobot3-old-20261031-0703`，可回滚）。

  ② **装配单与实机不符（生产级根因，未修）**：实机是 Pi4B 宿主 × ESP32 classic
     头（舵机/OLED/双麦全在板上），但 rrd.service 指向 `profiles/distro.toml`
      ——其 [[pin]] 把 `act.axis.pwm` 指向 **Pi PWM0 引脚12**、`ui.face.spi` 指向
     **Pi MAX7219**，两者实机都不存在。`profiles/pi4b-host-esp32-classic.toml`
     是更接近的档案（板侧十一键在 [hardware]），但其 [[pin]] 仍留 ui.face.spi
     （注释写明"rr3-interact 开机必探显示面 fail-closed，无显示件时退出"）。

  ③ **装载门拒启导致音频链整体缺席（「喊它没反应」的直接根因）**：新 rrd 启动后
     `rr3-ctl status` 只有 connect/act running；日志明示
     `拒绝启动 rr3-sense：缺格 sensor.env / sensor.imu`、
     `拒绝启动 rr3-power：缺格 power.charger`——distro 档声明 SHTC3(0x70)/
     ICM-42688(0x68)/INA219(0x40) 三个 I2C 器件，实机未接 → 探测不到 →
     **fail-closed 整个模块不启动，连同 audio.capture/vad 一起没了**。
     这是"配置声明了不存在的硬件"造成的整模块连坐，不是音频代码缺陷。

  ④ **brain/interact 注册后即死（未定案，有精确证据与下一步）**：两模块在
     rrd 日志里都能看到 `注册 rr3-xxx task=N`（分配了递增 task 号），随即
     `连接断开（Connection reset by peer）`；模块侧 per-unit 日志
     （`/var/run/rock3-ipc.sock.log/<unit>.log`）只留下
     `退出: 注册失败: 总线已断开`（rr-bus-api client.rs:621 的 reader-EOF 路径）。
     手工直跑（注入 RR3_BUS_PATH/RR3_MANIFEST）同样在该点退出——**rrd 不接受
     非受管子进程的注册**。
     **strace 加壳取证（当日，决定性）**：以 shell 壳包住 interact 二进制跑
     `strace -xx -e trace=network`，时间线为——19.300 发出 904B 注册帧（一次
     write）→ 19.301657 **只收到 128B（恰为帧头）**，该帧头解析出 plen≈51 →
     19.310 读线程 close(3) 并报「注册失败: 总线已断开」→ 进程退出；**期间没有
     第二次 recvfrom**（即 51B 载荷从未到达）。rrd 侧同刻反复报
     `写帧失败 dst=0: Io("Broken pipe")`。结论：**回复帧在 128B 头之后断流**
     ——要么 rrd 的应答写在中途断（对端已关/写面竞态），要么客户端在头解析后
     立即放弃。因帧头 magic 合法，重点转向 rrd 应答写面（`rr3_reply_ok` 与
     writer/fanout 两条写路径共用 `Mutex<UnixStream>` 的整帧原子性）。
     取证壳已撤除、二进制还原（生产不留调试壳）。
     下一步实验：在 rrd 侧给 `rr3_reply_ok` 加写后长度校验（跨进程可观测），
     或复现"注册瞬间恰好有广播帧并发写同一 socket"的竞态。

  ⑤ **单 UART 多消费方的结构性冲突（未修，ADR 级）**：rr-hal 的 esp32 后端
     （`RR3_HAL_BACKEND=esp32` + `RR3_HAL_PERIPH_UART` + `RR3_HAL_PERIPH_BAUD`）
     让 interact(显示)/act(舵机)/sense(音频) 各自**直开同一串口**，而 rr-ipc
     开口是 **flock 排他**（第二个 opener 当场被拒）。三模块共存于一台 Pi 时
     必然互斥。桥（rr3-bridge）名义上是解法，但板固件不发 register 帧
     （全仓 grep 无 register 臂；它是纯 UART 帧对端 `RR3_PERIPH_TASK`），
     桥接上 rrd 会在握手上失败——**桥链对当前固件不可用**。正确修法需
     UART broker（单进程持口 + 本地代理）或 fd 传递，属架构决议。

  ⑥ **硬件事实补充**：Pi 只有一块声卡（sndrpigooglevoi，Google voiceHAT，
     capture 可用；`arecord -D hw:2,0` 当时被生产进程占用报 busy），**无 USB
     回放卡**——`profiles/pi4b-host-esp32-classic.toml` 的
     `alsa_device="plughw:CARD=Device"` 在本机不成立。`/dev/ttyAMA0` 存在、
     无 ttyUSB（板走 GPIO14/15 直连而非 USB 转串）。装配手册记载 DoA 真件档
     设计是"USB ≥2 麦阵列"（BOM #1），实机未接，与 ESP32 双麦口径并存需裁定。

  ⑦ **brain 无唤醒词/KWS**：全模块 grep `你好洛克|wake|唤醒|kws` 零命中；
     ASR 走云端（ureq，需 env 端点与密钥）。「喊它要有回应」的本地无条件
     可达路径仍是探针侧 voice_respond v7.2（见 V3 收口）。

  结论：**当前实机生产栈处于"部分模块缺席"状态**——connect/act 起、
  sense/power 被装载门拒、brain/interact 注册后死。这不是单点 bug，而是
  "装配单描述的是另一台机器 + 部署 17 天未更新 + 单 UART 多消费方结构冲突"
  三者叠加。修复顺序（依赖序）：先定机型档案（②⑥，含移除不存在的 IIC 器件、
  显示面走 esp32 后端）→ 再解单 UART 冲突（⑤，ADR 级）→ 再定 brain/interact
  注册死因（④）→ 最后才谈「你好洛克」闭环（⑦）。

**遗留队列（白班，按优先序）**：
  1. 机型档案定案（U 轮②⑥）：新增/修正本机档案——移除 SHTC3/ICM-42688/INA219
     三个不存在的 IIC 器件声明（避免 sense/power 整模块连坐）、alsa 指向
     voiceHAT、显示/舵机/音频走 esp32 后端；须与 `[slots] present`、
     `enabled_slots` 三处一致。
  2. 单 UART 多消费方架构决议（U 轮⑤）：UART broker 或 fd 传递；桥链需板固件
     补 register 臂才可用。
  3. brain/interact 注册后死因定案（U 轮④）：最小装配集 + strace。
  4. 阵列声学衰减 ~30dB 物理检查（V3 深夜轮⑥）：开壳目检两麦装配位/声孔
     朝向——它同时压制定向 ILD 幅度与喊话信噪比，是拾音定向恢复转向的前提。
  5. 定向锚点重建：白天用 voice_respond v7.2（触发器已根治）采带真值标注的
     定向喊话（左右各 ≥3 次有效触发），ILD 锚点成形后恢复两态转向。
  6. V5 CRC 归因（3/4691）：先给 rr3-audio-rec FrameAssembler 加 crc_bad
     计数（现无校验，grep crc 零命中）→ build-onboard → 10min 同负载复验。
  7. 「你好洛克」ASR/KWS 未建（喊任何响声都会回应的诚实口径不变）。
  8. RAIL 分压测点未接线（full_smoke RAIL raw=0，硬件项）。
  9. 表情胶囊眼观感打磨（用户反馈"表情符号做得并不好"）。
  10. ESP32 独立形态动作面未建（face/pan/tilt/audio 端点）——配网面已闭环，
     动作仍走 UART 帧；要让板子单机可喊可动，需在板侧补动作端点。

**U 轮续：ESP32 独立形态配网面落地（2026-10-31 午后）**：
  ① 生产栈恢复：按新链（musl 五件 → payload → build-onboard → staging → 断言）
  出包安装后 **五模块 running**（`rr3-ctl status`：sense/connect/act/interact/
  brain 全 running）；rrd 装配单切 `pi4b-host-esp32-classic.toml`（drop-in
  `rrd.service.d/profile.conf`），配套 `hardware/pi4b-host-esp32-classic/wiring.toml`
  与 `contracts/platform/raspberry-pi-4.toml` 已归位。

  ② 板侧配网面（ADR-0021 裁定 1 独立形态要求；用户口径"WiFi 会换，机器人要能
  自己配网"）：APSTA 常开 SoftAP `RR3-BOARD`（WPA2，口令 rock3robot）＋ `GET /`
  手机配网页＋`POST /api/board/wifi` 事务写口（读旧→写 NVS→重连→有界等 DHCP
  →败回退旧值）＋`POST /api/board/reboot`。**真板验收（本日，固件 b15b496）**：
  - 无凭据 boot → `凭据源=ap-only`，AP 广播（Pi `nmcli` 扫到 RR3-BOARD，ch1，
    信号 87）；
  - Mac 连 AP（192.168.4.2）→ `GET /`、`GET /api/board/state`、
    `GET /api/board/bindings` 全 **200**；
  - `POST /api/board/wifi`（真实家用凭据）→ 应答
    `{"ok":true,"connected":true,"reverted":false,"stage":"apply"}`；UART0 实录
    `wifi:connected with ziroom602` ＋ `sta ip: 192.168.124.10`；AP 同时仍广播
    （ch6，信号 97）；
  - 经 `POST /api/board/reboot` 重启后 boot 横幅 `凭据源=nvs`，STA 自动重连同
    IP，局域网 `GET /api/board/state` 200——NVS 持久化闭环成立。
  - 现网状态：板 STA 在 `ziroom602`，IP `192.168.124.10`；SoftAP 192.168.4.1
    常开（换网时手机连它重配即可）。

  ③ 取证方法订正（当日最贵的假象）：Mac 侧旧脚本只拉低 DTR/RTS、从不释放 EN，
  **从未复位板子**，故"读不到 UART0／AP 未广播"是取证假象。正确序列：DTR 高
  （IO0 正常启动）＋ RTS 先低（EN 复位）后高（释放）。UART0 可达后一次 boot 即以
  `wifi:mode : sta + softAP` 与 `DHCP server ... 192.168.4.1` 证伪"AP 未起"。
  另：`nmcli dev wifi connect` 依赖扫描缓存，缓存过期时报 "No network with SSID
  found"（与 AP 广播无关）；脚本须先 `nmcli dev wifi rescan` 并等列表出现。

  ④ 事故与禁令：用 Pi 的 wlan0 切到板子 AP 做端到端测试，期间 **Pi 卡死约
  1h45m 后自行重启**（/tmp 日志被 systemd 清空、journal 无持久化、根因未定；
  与 brcmfmac 切网强相关嫌疑）。**结论：Pi 的 wlan0 是生产链路（ssh + 栈观测），
  禁止再把生产机 WiFi 切去测试面**；板侧配网验收改由 Mac 客户端在 AP 上直打
  （可恢复——Mac 会自动回连首选网，实测两次均回 `ziroom602_5G`）。

**U 轮续2：板侧音频上行打通（2026-10-31 傍晚，方案①）**：
  背景：组合形态的宿主音频源此前是 voiceHAT（实机无麦＝数字静默，VAD 0 事件），
  真实双麦在 ESP32 上而板侧 `audio_on=false`、上行未开；「喊它没反应」的音频
  半边由此断开。

  改法（代码 + 数据两层）：
  - **能力面后端覆盖键 `RR3_HAL_AUDIO_BACKEND`**（`rr-hal/src/backend.rs`）：只被
    音频**采集面**读取，未设/空白回落全局 `RR3_HAL_BACKEND`；两键共用白名单裁决
    `rr3_hal_backend_parse`（单测钉死）。理由：全局设 esp32 会让同机的舵机/显示面
    也去开同一 UART（跨进程 flock 单消费者，U 轮⑤）——采集面单独选档，其余能力
    面维持平台缺省（act/interact 不受影响）。
  - **`config-esp32.toml` 随包**（rr3-pack 可选件复制 + pack ⑤ 断言）：板麦档
    channels=2 / VAD 650 / 反射环开启，装机位
    `/usr/lib/rockrobot3/modules/rr3-sense/config-esp32.toml`。
  - 部署 drop-in（`/etc/systemd/system/rrd.service.d/profile.conf`）：
    `RR3_HAL_AUDIO_BACKEND=esp32`、`RR3_HAL_PERIPH_UART=/dev/ttyAMA0`、
    `RR3_HAL_PERIPH_BAUD=1500000`、`RR3_SENSE_CONFIG=<装机 config-esp32.toml>`。

  验收（真板，固件 b15b496）：
  - 物面先证：独立收流工具 1.5M 8s，`chunks=61 gaps=0 err=0`，16001Hz 双声道，
    实测 L rms=633 / R rms=598（环境底噪，非静默）；
  - 生产 sense：`音频源就绪: backend=esp32 rate=16000 ch=2 frame_ms=20`；板侧
    `/api/board/state` `audio_on=true` 且 `audio_sent` 持续增长；
  - VAD 边沿已进总线（rrd `转发 rr3-sense/audio/vad_start`）；反射环实测下发
    `pan=120°/114°/107°` + 点头 `tilt 90→98`，板侧 pan/tilt 同步变化；
  - brain 收到 VAD 后起对话轮（日志 `轮次 1..9: ASR 失败…缺少环境变量
    RR3_CLOUD_API_KEY`）——**音频通路已到 brain，只差云凭据**；
  - 面板 `POST /api/beep` 实测 `played_ms=200`（回放面持有 ALSA 设备、可出声）。

  遗留（按优先序）：
  1. 云凭据 `RR3_CLOUD_API_KEY` 未注入（ASR/LLM/TTS 全链因此不可用）——「语音
     对话」的最后一格；面板 secrets 面（`POST /api/model/key`）可落值。
  2. VAD 阈 650 与当前底噪 633 太近：15 分钟内 9 轮起音（含 beep），反射环在
     环境噪声下会误转头；需用真实嗓音重标（config-esp32 的 650 来自底噪 430 的
     旧场地）。
  3. DoA 锚点/双麦基线仍是旧标定（遗留 4/5 不变）；当前方向只作粗档（±30/±90）。
  4. `rr3.set_bindings` 未随音频链下发（板 `/api/board/bindings` 仍 `null`）——
     音频引脚走固件编译缺省、功能不受影响；绑定对账锚待舵机/显示接入时一并接。

**U 轮续3：云凭据接入与真语音闭环（2026-10-31 晚）**：
  凭据（值不落账，只记来源与落位）：
  - ASR/TTS：`RR3_CLOUD_API_KEY` ← `~/.rr3/cloud_key`（51B）。**订正**：Mac
    `.zshrc` 里同名的旧键已失效（SiliconFlow 返回 401 / code 30014），重接必须
    用 cloud_key 这把。
  - LLM：模型行 `api_key_ref = "DeepSeek官方"` ← secrets 文件同名键（`model/test`
    返回 200 / 743ms）。
  - 板端落位：`/etc/rockrobot3/cloud.env`（0600）＋
    `/etc/rockrobot3/secrets.json`（0600）；rrd drop-in 加
    `EnvironmentFile=-/etc/rockrobot3/cloud.env` 与
    `RR3_SECRETS_FILE=/etc/rockrobot3/secrets.json`（root 无 HOME，缺省 secrets
    路径不可靠）。
  三腿直测（不经模块）：合成语音 WAV「你好，今天天气怎么样」→ 云端 ASR 正确回文；
  TTS 200 / 78KB PCM；LLM `model/test` 200 / 743ms。
  真闭环（实机、用户对麦说话）：
  - brain：`对话轮完成: trace=10 user='没有啊，今天天气怎么样？经。'
    reply='我这边看不到实时天气，你告诉我城市名，我帮你查查。'
    tts_pcm_ms=6877 played_ms=6877`
  - interact：`tts: 25 字符播了 6877ms（PCM 220078 字节，gain 70%）`——扬声器
    出声，**ASR→LLM→TTS→回放全链闭合**。
  - 噪声轮：空文本如实弃轮（`识别为空文本，弃轮`），不产生假回复；但反射环仍会
    因底噪转头（续2 遗留 2）。
  遗留：VAD 阈/DoA 标定（噪声轮与误转头）；「你好洛克」唤醒词仍无（任何响声都会
  进 ASR，空文本弃轮）。
