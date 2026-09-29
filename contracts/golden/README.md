# golden —— 同机字节基准向量

| 文件 | 内容 |
|---|---|
| `event-header-128.hex` | 一条真实 EVENT 消息头的 hex 编码，256 个 hex 字符 = 128 字节，以单个 LF 结尾（读入方须 `.trim()` 再解码） |

作用：IPC-T-001 的**同机**基准。消费方是 `base/runtime/rr-ipc/src/protocol/header.rs`
的 `header_matches_golden_vector` 测试——把 hex 解码后与 Rust 常量逐字节比对，并要求
decode→encode 往返无损；`tests/envelope_meta.rs` 另断言其解码字节数等于
`envelope.schema.json` 的 `x-byte-size`（128）与 Rust `RR3_IPC_HDR_SIZE` 三处一致。

**效力边界**（与 [`../README.md`](../README.md) 一致）：它只钉住"本机 Rust 编解码与契约
字节逐位一致"。跨架构等值（x86 与 aarch64 布局一致，即 IPC-T-003）需要交叉编译两端对拍，
那属**契约外**的验证环节、本契约未覆盖它，故记在 `envelope.schema.json` 的 `x-rock3-v0.0.1-uncovered`；
何时补上属实况，见 [`../../docs/status.md`](../../docs/status.md)。

演进规则：magic 与 golden 向量**不适用"只增不改"**的冻结约束，属评审裁定的许可项；
改动须与 `envelope.schema.json` 一起升 major（见 ../README.md 演进规则）。
