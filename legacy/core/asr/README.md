# core/asr — 云端语音识别模块

16位单声道 PCM 流式上传到云端（硅基流动 Qwen3-ASR），返回识别文字。
从原 core/llm 拆出，可单独复用到任何需要"说话变文字"的设备。

## 公开 API（include/asr.h）

```c
asr_t *s = asr_begin(16000, 6);        // 先握手再录音，握手期的安静不丢音
asr_chunk(s, pcm, bytes);              // 边录边传
asr_sent(s);                           // 已传字节数（防发超）
asr_finish(s, text, sizeof(text));     // 说完收尾，返回 0 则 text=识别结果
asr_abort(s);                          // 中途放弃
```

## 关键实现约束（踩过坑）

- 服务端**不支持 chunked 分块上传**（HTTP 500），用固定 Content-Length 声明
  `max_seconds` 总长 + WAV 头同长，说完提前结束、剩余补静音零。
- 晚高峰响应慢，`ASR_TIMEOUT_MS` 保持 ≥90 秒。
- 模块参数在 `src/asr_config.h`（模型/地址/超时）。密钥**不在**该文件里：构建期由环境变量
  `SILICONFLOW_API_KEY` 注入成编译宏，注入方式与缺失后果见 [`legacy/core/README.md`](../README.md)
  的「密钥注入」一节。
