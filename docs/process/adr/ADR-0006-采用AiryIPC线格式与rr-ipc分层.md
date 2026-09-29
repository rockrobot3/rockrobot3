# ADR-0006：采用 Airy IPC 线格式与 rr-ipc 协议/传输分层

- **状态**：已接受（修订 [ADR-0003](ADR-0003-IPC选型-uds-jsonrpc-事件总线.md) 的线格式部分）
- **日期**：2026-09-28
- **关联**：[architecture.md §6](../../architecture/architecture.md) · [ADR-0004](ADR-0004-Airymax接入路径-运行时层对接.md) · [ADR-0007](ADR-0007-控制面分层与模块级升级.md)

## 背景

通用化评估发现两处结构性欠账：① rr-ipc 的协议层与传输层同层耦合，未来换共享内存/跨机需改协议；② 自造 32B 消息头与 Airymax 实际开放标准不一致。调研确认 Airymax 存在正式开放标准 **Airy IPC（A-IPC）**（airymaxhub/docs/openstandards/02-airy-ipc-protocol-standard.md，v1.0，AGPL-3.0-or-later OR Apache-2.0 双许可）。

## 决策

1. **线格式：直接采用 Airy IPC 128B 定长消息头（Layout C）**，不再维护自造 32B 头
   - 布局 L1 永久稳定：magic 'ARE1'（**字段值** 0x41524531；消息头所有多字节字段小端序落盘，
     故 magic 在线上字节为 `31 45 52 41`，与 `contracts/envelope.schema.json`、
     `contracts/golden/event-header-128.hex` 一致；标准注释按 ASCII 序读会得 `41 52 45 31`，
     本项目采规范条目 LE 为准，歧义登记见 ADR-0004 追加复查项与 `contracts/README.md`）、
     opcode、flags、trace_id、timestamp_ns、src_task、dst_task、payload_len、reserved[84]；扩展只允许写 reserved 区
   - 5 种 payload 类型全采用：REQUEST/RESPONSE（RPC）、EVENT（事件总线）、STREAM（音频/运动数据流，有序投递）、NOTIFICATION（心跳/系统通知）
   - 事务面 payload 默认 JSON-RPC 2.0（标准默认，零改动）
2. **语义字段放置**：rock3 的 topic 字符串与 phase（normal/degraded/recovery）置于 payload 内，**不占用消息头 reserved 区**（保持与标准逐字节兼容）
3. **错误码**：RESPONSE.status 采用 AIRY_E* 子集（EOK/EINVAL/EPERM/EBUSY/ETIMEDOUT）
4. **运行时自实现**：src_task 由 rrd（受信用户态运行时）在模块注册时分配并校验；capability 令牌、内核强制、防重放窗口等 AirymaxRT 运行时机制**不引入**（单机可信环境，见 ADR-0007 后续演进）（v0.0.1 现状：rrd 仅打印启动横幅，src_task 分配/校验与握手回显随 M2 落地）
5. **rr-ipc 分层**：
   - `protocol/`：128B 头与 payload 纯编解码，与传输无关；Rust/Python 双侧对齐同一 schema
   - `transport/`：`trait Transport`，uds 为 M2 唯一实现；shm（控制面，ADR-0007）、tcp（多机）预留
6. **UDS 路径**：`/var/run/rock3-ipc.sock`（自有路径，避免与将来同机部署的真实 Airymax 运行时冲突）

## 备选方案否决理由

- **保留自造 32B 头**：体积省 96B/条，但失去"永不破坏"承诺与 A-IPC 生态（dissector、测试规约 IPC-T-001~010），且与 ADR-0004 对接目标需要双向翻译
- **完整引入 AirymaxRT 运行时**：35 万行 C 依赖过重，与 Buildroot 精简路线冲突
- **capability 令牌同步引入**：当前单机可信，收益低复杂度高；留待具身化（电机物理危险性）时启用

## 后果

- 正面：协议层永久稳定承诺；与 Airymax 字节级互通（ADR-0004 落地路径简化为"同格式即同协议"）；STREAM 类型统一承载音频/运动流
- 负面：每消息头开销 128B（本地 UDS 可忽略）；A-IPC 兼容性测试（IPC-T-001~010）需自行选做，M2 至少覆盖 001/002/003
- 维护项：Airymax 标准升级时人工核对（并入 ADR-0004 半年复查）
