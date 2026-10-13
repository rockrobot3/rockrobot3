# golden —— 同机字节基准向量

| 文件 | 内容 |
|---|---|
| `event-header-128.hex` | 一条真实 EVENT 消息头的 hex 编码，256 个 hex 字符 = 128 字节，以单个 LF 结尾（读入方须 `.trim()` 再解码） |
| `tts-pcm-frames.json` | MC-1 跨语言一致性基准：一段 s16le/16k/mono PCM（1600 字节 = 20ms×2 + 10ms 尾块，字节模式 `i%251` 确定性可重算）与其按 `topics.toml#rr3-brain/audio/tts_pcm` 分帧后的帧列（seq 从水位 0 起 1/2/3，ms 20/20/10），JSON（UTF-8，单个 LF 结尾） |

作用：IPC-T-001 的**同机**基准。消费方是 `base/runtime/rr-ipc/src/protocol/header.rs`
的 `header_matches_golden_vector` 测试——把 hex 解码后与 Rust 常量逐字节比对，并要求
decode→encode 往返无损；`tests/envelope_meta.rs` 另断言其解码字节数等于
`envelope.schema.json` 的 `x-byte-size`（128）与 Rust `RR3_IPC_HDR_SIZE` 三处一致。

`tts-pcm-frames.json` 是语音下行两条腿的**同机**对拍基准（MC-1③）：Python 腿
`modules/rr3-brain/tests/test_composite.py` 断言 `rr3_tts_pcm_frames` 的分帧输出与
golden 帧列逐字段一致；Rust 腿 `modules/rr3-interact/src/composite.rs` 测试把帧列
逐帧 `rr3_base64_decode` 入收流缓冲，拼接流必须逐字节还原 `pcm_hex`（seq/ms 同拍
断言）。生成即冻结：由 Python 腿分帧函数一次性产出，重算验证按上述字节模式与
640B 切分规则。

**效力边界**（与 [`../README.md`](../README.md) 一致）：它只钉住"本机 Rust 编解码与契约
字节逐位一致"。跨架构等值（x86 与 aarch64 布局一致，即 IPC-T-003）由**契约外**的对拍脚本
`tools/ci/ipct003.sh` 承载（多路对拍含本 golden 为第三方）——2026-10-11 CI ipct003 job
`--strict` 四路全绿回执在案，IPC-T-003 已移入 `envelope.schema.json` 的
`x-rock3-v0.0.1-coverage`；实况见 [`../../docs/status.md`](../../docs/status.md)。

演进规则：magic 与 golden 向量**不适用"只增不改"**的冻结约束，属评审裁定的许可项；
改动须与 `envelope.schema.json` 一起升 major（见 ../README.md 演进规则）。
