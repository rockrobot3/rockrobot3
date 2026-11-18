# ADR-0008：传输层 trait 定型与 uds 实现

- **状态**：已接受
- **日期**：2026-09-30
- **关联**：[ADR-0006](ADR-0006-采用AiryIPC线格式与rr-ipc分层.md)（§5 传输层规划）· [ADR-0007](ADR-0007-控制面分层与模块级升级.md)（§裁定 3 零拷贝签名）· [architecture.md §6](../../architecture/architecture.md)

## 背景

`rr-ipc/src/transport/mod.rs` 的头注曾把 `trait Transport` 的签名推给 M2"一并定"，理由是
"以免现在造一个 ADR-0007 §裁定 3 要推翻的接口"。M2 收口（rrd supervisor 已进树）时该定型
不再可拖：M3 起的模块（sense/interact/brain）、`tools/sim` 的假设备、M5 的 shm 全都以
"UDS 客户端"为先验，若不先定接缝，各端会各自手搓 socket 代码。

## 决策

### 裁定 1：传输层保持同步阻塞线程模型，不引 async

rrd 的并发形状是"每连接 2 线程 + 阻塞 IO + 唯一 watchdog/supervisor 线程"，28+ 条线级
用例（register_publish / e2e_sim_peer / e2e_spawn_restart / e2e_watchdog）钉在这个形状上。
把它换成 async 等于推翻整个 conn/daemon 测试面，而 ADR-0007 的零拷贝诉求在同步所有权移交
语义下完全成立。**async 运行时只进模块侧**（M3 定 executor；届时 HAL trait 若需 async
驱动，用 trait 级别隔离，不倒灌进传输层）。

### 裁定 2：接缝拆成三个 trait，零拷贝语义落在缓冲类型上

| trait | 职责 | 关键语义 |
|---|---|---|
| `Rr3TransportConn` | 连接侧帧收发 | `rr3_send_buf` **消费**整个 `Rr3FrameBuf`（幂等 seal 后单次 `write_all`，读侧不可能看到两半）；`rr3_recv_buf` 返回新分配的**所有权**缓冲。错误语义与帧层同一枚举 |
| `Rr3TransportListener` | 监听侧 | `rr3_accept` 产出连接；一次 accept 的 `Err` 只属于那一次（rrd 口径：记日志不带走守护进程） |
| `Rr3TransportEndpoint` | 端点建立 | `rr3_cleanup`（uds：删残留 socket 文件，不存在不报错）→ `rr3_bind` |

- 零拷贝不落在方法数上，落在 [`Rr3FrameBuf`](../../../base/runtime/rr-ipc/src/transport/buf.rs)：
  单块连续内存 `[0..128)` 头预留区 + `[128..)` payload；payload 直接填进缓冲（不另配
  payload Vec 再拷贝），`rr3_seal` 原地编头（幂等）。uds 整块读写；**M5 的 shm 在同一签名
  下把"拷贝"降为环形缓冲 grant，接口不改**——这正是裁定 3 要的"造一次就不再推翻"。
- 连接侧对一切 `Read + Write + Send` 提供统一 blanket 实现（framed 收发即帧层自由函数），
  `UnixStream` 因此自动满足，uds 的监听器 `type Conn = UnixStream` 无需包装类型。
- `Rr3Frame`（头+payload 的值视图）保留不动：测试与既有代码零扰动，与缓冲低成本互转。

### 裁定 3：rrd 服务侧只把 bind/cleanup/accept 三处换成 trait 工厂

conn.rs 的读写循环继续持有 `UnixStream` 具体类型，走 socket 专有门（注册前读超时、
shutdown）。全量泛化推迟到 M5 shm 落地时评估——避免为抽象重写已绿的线级用例。

### 裁定 4：shm / tcp 不写半成品

沿 rr-bus "不放半成品"先例：M2 只留 mod.rs 头注的分层规划（shm M5、tcp 预留、标准端口
6060），不写任何半成品方法或恒错类型。

## 备选方案否决理由

- **全栈 async（tokio 传输层）**：见裁定 1，测试面成本不成比例，且 shm 场景本就以
  "固定内存布局 + 轮询/事件"为主，async 无增益
- **单一胖 `Transport` trait（监听+连接合一）**：shm 的"监听"语义（创建共享内存区 +
  多客户端附着）与 uds 的 accept 差异大，合一接口必留恒错方法
- **recv 返回借用 / 环形复用缓冲**：M2 的 uds 下无收益（反正要拷进用户缓冲），跨线程
  移交所有权是最简单的正确形状；shm 的 grant 语义在所有权签名下仍可实现

## 后果

- 正面：M3 起模块与 sim 有唯一客户端入口（`Rr3UdsListener::rr3_connect` + trait 收发）；
  M5 shm 落地不动接口；rrd 服务侧改动最小化
- 负面：`Rr3FrameBuf` 与 `Rr3Frame` 两套形状并存（互转成本低，但读者需知道各管什么）；
  conn.rs 的具体类型依赖到 M5 才收
- 校验项：`cargo test -p rr-ipc`（含 buf/uds 新用例）；rrd 既有线级用例一条不改全绿
