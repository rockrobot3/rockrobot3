# core/llm — 大模型对话模块

带历史记忆的多轮对话（Qwen2.5-7B），只管文字进文字出。
语音识别已拆到 core/asr，语音合成已拆到 core/tts——三个模块可独立复用。

## 公开 API（include/llm.h）

```c
llm_chat_init();                              // 开机一次
llm_chat("今天天气怎么样", ans, sizeof(ans));  // 返回 0 = ans 有回答
```

## 说明

- 历史记忆：本地保留最近 `LLM_HISTORY_ROUNDS`（4）轮，人设与轮数在 `src/llm_config.h`。
- 换 DeepSeek：改 `LLM_MODEL` 为 `"deepseek-ai/DeepSeek-V3"`（收费）。
- `LLM_TIMEOUT_MS` 保持 ≥60 秒（晚高峰慢）。
