# core/tts — 云端语音合成模块

文字 → 硅基流动 CosyVoice2 合成 → PCM 流回调交付。模块不碰喇叭，
播放由使用方在回调里完成（放大、增益、写 I2S 都自己定），可复用到任何设备。

## 公开 API（include/tts.h）

```c
static esp_err_t sink(const int16_t *pcm, int samples, void *ctx) {
    return audio_write(...);   // 自己决定怎么播
}
tts_play("你好，世界", 16000, sink, NULL);   // 返回 0 = 播完
```

## 注意

- 音色在 `src/tts_config.h`：`bella` 女声 / `alex` 男声。
- `TTS_TIMEOUT_MS` 保持 ≥60 秒（晚高峰慢）。
- 回调里阻塞太久会拖慢网络读取；需要慢速播放就先攒到自己的队列。
