# core/audio —— 独立音频模块（ESP-IDF 组件）

I2S 全双工音频：INMP441 麦克风录音 + MAX98357A 功放播放，供任何 ESP32 工程复用。

## API（include/audio.h）

| 函数 | 说明 |
|------|------|
| `audio_init` | 初始化 I2S 全双工 |
| `audio_read` | 读麦克风原始数据（32 位整型） |
| `audio_write` | 写喇叭播放数据（32 位整型） |
| `audio_play_tone` | 播放提示音（正弦波） |
| `audio_mic_process` | 单采样处理：IIR 去直流 + 增益，返回原始峰值（VAD 用） |

## 契约

- 采样率 `AUDIO_SAMPLE_RATE` = 16kHz，数据 `AUDIO_CHUNK` = 256 帧
- 麦克风与功放共享 BCLK/WS 时钟

## 配置（audio_config.h）

I2S 引脚（BCLK/WS/DOUT/DIN）与 `REC_GAIN` 录音增益，都在模块根目录的 `audio_config.h`。

## 复用方法

工程 `CMakeLists.txt`（`project()` 前）：

```cmake
list(APPEND EXTRA_COMPONENT_DIRS "路径/core/audio")
```

使用方组件 `REQUIRES` 加 `audio`，代码 `#include "audio.h"`。
