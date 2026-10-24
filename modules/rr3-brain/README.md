# rr3-brain

认知模块（Rust）：ASR / LLM / TTS 编排 + 对话状态机 + 意图规则面。逐能力继承自
`legacy/core/{asr,llm,tts}` 与 Python 侧 `rr3_brain`（ADR-0014 移植、ADR-0016
收口——批N32 件四起 Python 承载侧整体退役，`lang = "rust"`，与其余模块同形）。

## 目录结构

```
modules/rr3-brain/
├── module.toml          ← manifest（type = "brain", lang = "rust"；entry = target/debug 同名二进制）
├── composite.toml       ← 组合件装配清单（dialog ⊗ asr ⊗ llm ⊗ tts）
├── config.toml          ← 运行参数；密钥不落盘（env: 前缀 → 环境变量；裸名 → 面板 secrets）
├── Cargo.toml           ← 模块 crate（workspace 成员，六模块同形）
├── atoms/               ← 原子件五件套（atom.toml + assemble.toml + <名>.rs + mech.rs + private.rs）
│   ├── dialog/          ← utterance 边界状态机（onset 起轮 / offset+时长双判停；预滚环防短句削头）
│   ├── asr/             ← Rr3AsrClient（SiliconFlow /v1/audio/transcriptions，multipart + WAV 头仓内自建）
│   ├── llm/             ← Rr3LlmClient（SiliconFlow /v1/chat/completions，多轮历史窗口 + 失败轮回滚）
│   └── tts/             ← Rr3TtsClient（SiliconFlow /v1/audio/speech，PCM 回包）
└── src/
    ├── main.rs          ← 装配入口（SURFACE 自述 → bootstrap → serve 主循环；#[path] 原位挂载四原子件）
    ├── boot.rs          ← 接入层：config schema 解析（TOML → JSON 文档树 + boot 投影，缺键 fail-closed）
    ├── cloud.rs         ← 云端共享原语（云档/密钥双源/secrets/HTTP/WAV 头/multipart boundary）
    ├── dispatch.rs      ← 机制层注册面（pcm/vad 订阅消费 + ASR→LLM→TTS→播放→intent 整轮编排）
    ├── intent.rs        ← 意图规则面（0.0.3 T3：user_text → actions[]，形状见 brain-intent-result.schema.json）
    └── rpc.rs           ← ops/list 快照 + ops/set 写臂（白名单/类型域校验 → live 生效不落盘 → 按组重建）
```

## 与其它模块的边界

| 方向 | 主题 | 说明 |
|---|---|---|
| publish | `rr3-brain/intent/result` | 意图结论，供 act / interact 消费 |
| publish | `rr3-brain/audio/tts_pcm` | 分帧 PCM，供 interact 播放 |
| subscribe | `rr3-sense/audio/pcm` | 语音主链音频流（dispatch on_event 实际消费） |
| subscribe | `rr3-sense/audio/vad_start` | 语音起点事件（onset 起轮 / offset 判停） |
| rpc | `rr3-brain/llm/completion` | 事务面补全请求 |

主题均已在 [`contracts/topics.toml`](../../contracts/topics.toml) 登记；新增主题要先改契约。
C14（ADR-0019 修订 2 ②，2026-10-10 评审裁定）：原申报的 `rr3-interact/asr/text` 走
**retired**（号位保留、义务解除）、`rr3-connect/net/link` 订阅撤申报（不补实现），
订阅面对齐实然面（B-1）。

## 心跳自报计数（0.0.4 批B，Q1 裁定）

`dispatch.rs` 的 `counter_bump` 记账，心跳线程每拍随 `rr3.heartbeat` 的可选
`counters` 字段上行，rrd 按 `rr3-brain/<键>` 归并（`rr3.ctl.counters` 可查）。本模块的键：

| 键 | 记账点 |
|---|---|
| `rounds_audible` / `rounds_replied` | `rr3_finish_turn`：B-16① 拆账——听见一整轮 / 出应答各一（旧 `chat_rounds` 键退役） |
| `asr_ok` / `asr_fail` | 同上：ASR 成功 / 异常各一 |
| `llm_fail` / `tts_fail` | 同上：LLM / TTS 失败各一 |
| `errors` | 通用可选键（各模块共用词表），异常路径按需自报 |

## 本地自检

```bash
# 单测（cargo test，77 条）
cargo test --manifest-path modules/rr3-brain/Cargo.toml
# 四真进程全语音对话闭环（rrd + rr3-sense + rr3-brain + rr3-interact 真二进制，云端 mock）
cargo run --manifest-path tools/sim/Cargo.toml --bin rr3-sim -- --brain
```

M4 交付面（ADR-0014，批N32 件二起 ureq+rustls 承载）：云端链路 ASR / LLM / TTS 已按
legacy 移植实装，`RR3_BRAIN_CLOUD=mock` 是测试注入点不是产品路径；缺 key 的 real 调用
照报缺环境变量，不静默降级。失败语义：业务失败折日志弃轮，传输失败 exit(1) 交 supervisor。
