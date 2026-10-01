# rr3-brain

认知模块（Python）：ASR / LLM / TTS 编排 + 对话策略。逐能力继承自 `legacy/core/{asr,llm,tts}`，
按 R1「移植式消解」搬到 RK3566 侧，最终删掉 legacy 对应组件。

## 目录结构

```
modules/rr3-brain/
├── module.toml          ← manifest（type = "brain", lang = "python"）
├── config.toml          ← 运行参数；密钥不落盘
├── pyproject.toml       ← 零第三方依赖（stdlib-only）；console script `rr3-brain` → rr3_brain.__main__:main
├── tests/               ← stdlib unittest（test_bus / test_cloud / test_dialog）
└── rr3_brain/
    ├── __init__.py      ← 包级 API：rr3_brain_submit_text()
    ├── __main__.py      ← python -m rr3_brain 入口：连总线 → 事件循环 → 整轮对话
    ├── bus.py           ← 模块侧同步总线客户端（128B 头逐语义镜像 rr-bus client.rs，ADR-0012 裁定 1）
    ├── dialog.py        ← Rr3Dialog / Rr3DialogTurn 对话状态机（onset 起轮 / offset+时长双判停）
    └── cloud/
        ├── __init__.py  ← 密钥只从 SILICONFLOW_API_KEY 读；RR3_BRAIN_CLOUD 选 real / mock
        ├── asr.py       ← Rr3AsrClient（SiliconFlow /v1/audio/transcriptions）
        ├── llm.py       ← Rr3LlmClient（SiliconFlow /v1/chat/completions）
        └── tts.py       ← Rr3TtsClient（SiliconFlow /v1/audio/speech，PCM 回包）
```

## 与其它模块的边界

| 方向 | 主题 | 说明 |
|---|---|---|
| publish | `rr3-brain/intent/result` | 意图结论，供 act / interact 消费 |
| subscribe | `rr3-interact/asr/text` | 识别文本上行后的呈现事件 |
| subscribe | `rr3-connect/net/link` | 链路状态；离线时降级为本地提示 |
| rpc | `rr3-brain/llm/completion` | 事务面补全请求 |

主题均已在 [`contracts/topics.toml`](../../contracts/topics.toml) 登记；新增主题要先改契约。

## 本地自检

```bash
# 单测（stdlib unittest，26 条）
python3 -m unittest discover -s tests
# 四真进程全语音对话闭环（rrd + rr3-sense(sim) + rr3-brain + rr3-interact，云端 mock）
cargo run --manifest-path tools/sim/Cargo.toml --bin rr3-sim -- --brain
```

M4 交付面（ADR-0012）：云端链路 ASR / LLM / TTS 已按 legacy 移植实装（urllib，零第三方），
`RR3_BRAIN_CLOUD=mock` 是测试注入点不是产品路径；缺 `SILICONFLOW_API_KEY` 的 real 调用
照报缺环境变量，不静默降级。失败语义：业务失败折日志弃轮，传输失败 exit(1) 交 supervisor。
