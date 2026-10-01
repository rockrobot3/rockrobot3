# ADR-0011：shm 传输 —— 单写者多读者环形缓冲与 POSIX 附着

- **状态**：已接受
- **日期**：2026-10-01
- **关联**：[ADR-0007](ADR-0007-控制面分层与模块级升级.md)（决策一实时控制面 / §裁定 3）· [ADR-0008](ADR-0008-传输层trait定型与uds实现.md)（三 trait 面 / 裁定 4"shm 不写半成品"）· [ADR-0006](ADR-0006-采用AiryIPC线格式与rr-ipc分层.md)（§5 传输分层）· base/runtime/README.md 依赖预算

## 背景

实时控制面（100Hz–1kHz 关节状态 / 力矩 / PCM 流，ADR-0007 决策一）规划由 A-IPC over
共享内存承载，格式不变、仅换 transport（ADR-0008 裁定 2/3 已把接缝定型：三 trait +
所有权移交的 `Rr3FrameBuf`，"M5 的 shm 在同一签名下把拷贝降为环形缓冲 grant"）。
uds 的形状（一连接一线程、accept 产连接、可靠字节流）与"一个写者、多个读者、永远
追最新"的实时流语义不匹配，这是 shm 存在的理由；本 ADR 定 shm 初版的语义、区域布局、
附着方式与依赖面（F6 交付范围经评审定为**含跨进程附着**）。

## 决策

### 裁定 1：单写者多读者、丢最旧保最新（lossy newest-wins），写者永不阻塞

环形缓冲**没有反压**：区满（写者将追上最慢读者的游标）时直接覆盖最旧未读数据，
`rr3_send_buf` 除"帧长超环半容量"外**永不失败、永不等待**；读者游标落后超过半圈时
**重同步到最新帧边界**并累加丢失计数（`rr3_lost()`），丢失不报错、不重放。

理由：实时控制面订阅的是"最新状态"——1kHz 关节状态流被最慢读者反压是错误形状，
宁可丢旧不可堵新。可靠交付与反压是事务面（uds）的职责，两层语义相反正是分层的意义。

- 否决 **区满 Busy 拒写**：写者被最慢读者反压，实时路径不可接受（且 AIRY_EBUSY 在
  shm 面会变成常态错误而非异常）。
- 否决 **读者注册表 + min 游标空满判断**：需要读者 attach/detach 协议与死读者回收
  （崩溃读者把整环钉死），初版复杂度不成比例；丢最旧语义下读者无状态，死读者零影响。
- 否决 **多写者**：单写者是环内无需写者间互斥的前提；指令流（力矩）与状态流（关节）
  分属不同区域，一区域一写者。

### 裁定 2：区域布局与无锁协议（seqlock 式有效性复核）

区域 = 固定 64B 头 + 字节环，环内记录 = **一整帧**（128B 头 + payload，即
`Rr3FrameBuf::rr3_as_sealed_bytes()` 的原样字节）：

| 偏移 | 字段 | 说明 |
|---|---|---|
| 0 | magic `RR3S`（LE 字节 52 52 33 53） | 附着校验 |
| 4 | version = 1 | 布局演进号（经 ADR 升） |
| 8 | cap（环字节数） | 建区时定，附着时读回 |
| 16 | head（已发布写游标，u64 单调） | 唯一共享变量，Release 发布 / Acquire 读取 |
| 24..64 | reserved 全 0 | 与 128B 头同一条 reserved 纪律 |

- **写者**（`rr3_send_buf`）：整帧 wrap 拷入环（先 `head.load` 取写点）→
  `head.store(t + size, Release)` 一步发布。无进行中写游标字段——发布即完整帧。
- **读者**（try-recv）：`h1 = head.load(Acquire)` → 空则 None；落后 > cap/2 则重同步；
  读帧头得 size，未发布完（`t + size > h1`）则 None；wrap 拷出后**复核
  `h2 == h1`**，不等则丢弃重试（写者在拷贝期间又发布了新帧）。
- **无撕裂的证明**（head 单变量 seqlock）：帧长 ≤ cap/2（发送侧强制，见裁定 4）时，
  head 停在 `h1` 的整个窗口内写者写点 `< h1 + cap/2 ≤ t + cap`（读者窗口满足
  `h1 − t ≤ cap/2`），物理上**不可能**重写读者正拷的字节；`h2 == h1` 复核排除拷贝
  期间 head 前进（写点随之前进的情形）。两条合起来，接受帧必然完整。
- 读者游标是**读者本地的**（`Rr3ShmReaderConn` 内部），不进区域——多读者 = 多个
  独立游标，互不知道对方。
- 通知原语（Linux futex / macOS semaphore）**不做**：`rr3_recv_buf`（trait 阻塞语义）
  以 200µs 退避轮询实现；控制面本就以环率轮询读最新，阻塞唤醒是 uds 面的形状。

### 裁定 3：与 uds 同 trait 面；角色分型，方向不匹配的方法恒报 Io

`transport/shm.rs` 落地四个类型，全部走 ADR-0008 的三 trait，**接口零新增**：

| 类型 | 实现 | 语义 |
|---|---|---|
| `Rr3ShmListener`（写者侧，`rr3_bind` 建区） | `Rr3TransportEndpoint` + `Rr3TransportListener` | cleanup = `shm_unlink` 残留（不存在不报错，同 uds 删 socket 文件）；`rr3_accept` = 在既有区域上开一个**新读者视图**（独立游标） |
| `Rr3ShmWriterConn`（`rr3_writer_conn()` 取得） | `Rr3TransportConn` | send = 裁定 1/2 的写路径；recv 恒报 `Io`（点名"shm 写者连接不支持接收"） |
| `Rr3ShmReaderConn`（accept / `rr3_attach(name)` 取得） | `Rr3TransportConn` | recv = 裁定 2 的读路径；send 恒报 `Io`（点名"shm 读者连接不可发送"） |

方向不匹配的方法恒报 `Io` 是"同 trait 面"的代价：shm 连接天生单向，备选方案
（为 shm 拆第二组 trait / 改 `Rr3TransportConn` 签名）等于推翻 ADR-0008，不为初版做。
跨进程读者附着 `rr3_attach` 与 uds 的 `Rr3UdsListener::rr3_connect` 对位。

零拷贝 grant（写者直接在环内填 payload、省去先建 `Rr3FrameBuf` 的一次拷贝）作为
**F_ZEROCOPY 的行为落点，本版不做**——它是 trait 之外的扩展方法（trait 签名消费
整个缓冲，grant 省不掉调用方那半），初版 send 路径一次拷入，与 uds 的 `write_all`
等价；`flags` 沿既有纪律原样透传（F_URGENT 暂无行为差异，随流控裁定一并考虑）。

### 裁定 4：半容量守卫

发送侧拒绝 `size > cap/2`（`Err(Io)`，size = 128 + payload_len；默认 cap 1 MiB →
最大帧 512 KiB）。它是裁定 2 无撕裂证明的前提，也是语义边界：大于环半容量的数据
不属实时控制面（uds 承载）。默认容量常量 `RR3_SHM_RING_CAP_DEFAULT = 1 MiB`，
建大区走 `rr3_bind_with_cap`（inherent 方法，不进 trait——`rr3_bind` 签名只有 path，
与 uds 对齐）。shm 名默认 `/rock3-ipc-shm`，环境键 `RR3_SHM_NAME`（naming.md 同源）。

### 裁定 5：POSIX 附着 —— 手写最小 extern 面，不引 libc crate

跨进程附着用 POSIX shm（`shm_open`/`ftruncate`/`mmap`/`munmap`/`close`/`shm_unlink`，
共 6 个函数），**手写 `extern "C"` 声明**（约 60 行 unsafe，集中单文件、cfg 门控
`any(target_os = "linux", target_os = "macos")`），不引 libc crate、
`[workspace.dependencies]` 不动——依赖预算（base/runtime/README.md）的"libc 受控
引入"边界因此不被推开：本仓对 libc 的姿态是"能不引就不引"，6 个稳定 POSIX 函数
的手写声明面比整个 libc crate（几百个 bindgen 生成项）小两个数量级，audit 面同理。
写路径 unsafe 的安全论据（页对齐 / 区域生存期 / 无 Rust 引用别名）写进代码注释。

名称规约：POSIX shm 名以 `/` 开头（如 `/rr3-ipc-shm-<用途>`）；`O_CREAT|O_EXCL` 对位
uds"不允许 bind 到已存在路径"（先 `rr3_cleanup` 后 bind，同一纪律）。附着侧先以
64B 探针映射校验 magic/version，再按头内 cap 全量映射（PROT_READ——读者只读区域，
原子 Acquire 读在只读页上成立）。

## 明确不做（初版裁定边界，任何"shm 已完成"的措辞不许盖过）

1. **tcp**（多机协作，预留位不变）。
2. **rrd 接线**：conn.rs 仍持 `UnixStream`（ADR-0008 裁定 3）；shm 面进总线扇出
   （哪些主题走 shm、rrd 如何双面发布）是独立的总线侧工作，随 M5 板上集成裁定。
3. **F_ZEROCOPY 的 grant 路径 / F_URGENT 行为差异**（裁定 3）。
4. **通知原语**（futex / mach semaphore）：退避轮询够用，唤醒延迟不是初版指标。
5. **可靠交付 / 反压 / 读者存活管理**：shm 面永远是尽力投递（裁定 1）。
6. **跨进程 e2e 用例**：附着层用真 `shm_open`/`mmap` 跨**线程**验证（同一内核对象、
   同一系统调用面）；跨**进程**的线上证据随 rr3 接线（第 2 条）一并落。

## 备选方案否决理由

- **引 libc crate / memmap2 / shared_memory crate**：见裁定 5——依赖预算与 audit 面
  不为一个 6 函数 FFI 破例；`shared_memory` 类 crate 还引入构建脚本与平台兜底逻辑。
- **双变量协议（head + 进行中写游标 w）**：能省掉"帧长 ≤ cap/2"限制，但读者对 w 的
  Acquire 读与写者的 Release 写之间没有 happens-before 边（写者发布 head 时 w 可能
  是旧值），证明复杂且易错；半容量守卫把证明压成三行不等式（裁定 2），一个共享
  变量比两个变量的协议少一整类并发错。
- **区满覆盖 + 无丢失计数**：丢失不可观测 = "尽力投递"退化成"静默丢"（ipct003 的
  SKIP 语义同一原则：不静默当作没发生）；一个 u64 计数器的成本换可诊断性，值得。
- **crc / 帧内校验**：seqlock 复核已保证完整性；校验和是对"区域被第三方改写"的防御，
  初版的威胁模型里不存在该对手。

## 后果

- 正面：实时控制面的通道就位（建区 / 附着 / 收发全通，macOS 与 Linux 板同一代码）；
  三 trait 面零改动验证了 ADR-0008"造一次就不再推翻"；依赖预算零增量。
- 负面：`Rr3ShmWriterConn::rr3_recv_buf` / `Rr3ShmReaderConn::rr3_send_buf` 恒报
  `Io`（同 trait 面的已知代价，调用方按角色取连接即可不触雷）；慢读者静默丢帧只有
  计数器可观测（订阅 shm = 接受丢帧，文档措辞冻结）；约 60 行 unsafe FFI 集中在
  `shm.rs` 的 POSIX 段。
- 校验项：`cargo test -p rr-ipc` 新增 shm 用例（核心环平台无关 + POSIX 附着段
  cfg 门控，条数登记 conventions §2——Windows 侧计入核心段、POSIX 段不参与编译，
  与 uds 的 cfg 边界同口径）；rrd 既有用例一条不改全绿（裁定"不做"第 2 条）。
