# rr3-brain

认知模块（Python）：ASR / LLM / TTS 编排 + 对话策略。逐能力继承自 `legacy/core/{asr,llm,tts}`，
按 R1「移植式消解」搬到 RK3566 侧，最终删掉 legacy 对应组件。

## 目录结构

```
modules/rr3-brain/
├── module.toml          ← manifest（type = "brain", lang = "python"）
├── config.toml          ← 运行参数；密钥不落盘
├── pyproject.toml       ← 打包：console script `rr3-brain` → rr3_brain.__main__:main
└── rr3_brain/
    ├── __init__.py      ← 包级 API：rr3_brain_submit_text()
    ├── __main__.py      ← python -m rr3_brain 入口（自述，不联网）
    ├── dialog.py        ← Rr3Dialog / Rr3DialogTurn 对话状态机
    └── cloud/
        ├── __init__.py  ← 密钥只从 SILICONFLOW_API_KEY 读
        ├── asr.py       ← Rr3AsrClient（M4）
        ├── llm.py       ← Rr3LlmClient（M4）
        └── tts.py       ← Rr3TtsClient（M4）
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
python -m compileall rr3_brain && python -m rr3_brain
```

交付面是**结构**：接口签名、状态机数据形状、密钥来源。云端链路（ASR / LLM / TTS 三处
`NotImplementedError`）今天接到哪一段属实况，唯一落点
[`docs/status.md`](../../docs/status.md)，本文件不记进度。
