# ADR-0003：IPC 选型 —— UDS + JSON-RPC 2.0 + 定长消息头事件总线

- **状态**：已接受；**线格式部分已被 [ADR-0006](ADR-0006-采用AiryIPC线格式与rr-ipc分层.md) 修订**（自造 32B 头 → Airy IPC 128B 头，UDS 路径定为 `/var/run/rock3-ipc.sock`）
- **日期**：2026-09-28
- **关联**：[architecture.md](../architecture.md) · contracts/envelope.schema.json

## 背景

模块为多进程（ADR-0002），需要单机 IPC：事件广播 + 点对点 RPC + 全链路追踪。消息需在 Rust 基座与 Python brain 间字节级一致。

## 决策

1. **传输**：Unix Domain Socket（UDS），单机场景，无网络栈开销
2. **协议**：JSON-RPC 2.0 body + **定长 32B 消息头**（trace_id / ts_ns / source / phase / topic）——头部格式对齐 AirymaxRT gateway 规约，为将来对接预留字节级兼容
3. **事件主题**：`rr3-<模块>/<域>/<事件>`，全量登记于 `contracts/topics.toml`，rrd 启动校验前缀归属（= manifest.name），越权发布拒绝（v0.0.1 现状：前缀归属已由 CI 契约测试强制，见 `tools/ci/check.sh` ③；rrd 运行时校验待 M1/M2 落地）
4. **phase 字段**：`normal | degraded | recovery` 贯穿全链路，供 brain 与 base 做降级决策
5. **契约管理**：`contracts/` 为顶层目录，envelope 与事件负载 schema 为 [SC] 字节级共享契约，Rust（rr-ipc）与 Python（rr3_brain.ipc）双方唯一事实源

## 备选方案否决理由

- **gRPC/Protobuf**：多语言代码生成链重，单机 UDS 场景收益为负；若未来跨机协作（具身多机）再评估
- **DBus 全量使用**：系统服务交互（NetworkManager/BlueZ）走 D-Bus，但**模块间**不用——D-Bus 的服务模型与"能力依赖仲裁"不匹配，且 Python/Rust 双侧调试成本高
- **纯 JSON 行协议（无定长头）**：无法字节级对齐 Airymax gateway，追踪字段易漂移

## 后果

- 正面：协议轻、可调试（rr3-ctl 可直接订阅总线）、与 AirymaxRT 头格式兼容
- 负面：JSON 序列化性能低于二进制——音频 PCM 大流量走总线需评估；**决策：PCM 流不经事件总线**，sense/interact 直接经 HAL 层句柄传递（后续细化，M3 验证）
