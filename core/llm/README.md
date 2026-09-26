# core/llm —— 独立云端 AI 能力模块（ESP-IDF 组件）

语音识别(ASR) + 大模型对话(LLM) + 语音合成(TTS)，一个组件全包含。不依赖任何具体固件——任何 ESP32/ESP32-S3 的 ESP-IDF 工程都能直接复用（比如 MakeRobot 的 firmware-c，或将来的新固件）。

## 目录结构

```
core/llm/
├── include/llm.h      ← 公开 API（使用方只 include 这一个头文件）
├── src/llm.c          ← 实现：流式 ASR / LLM 对话(带历史记忆) / TTS
├── src/llm_config.h   ← 模块参数：API 密钥、模型、音色、超时
├── CMakeLists.txt     ← 组件注册
└── idf_component.yml  ← 依赖 cJSON（构建清单，与 CMakeLists 同类）
```

## 能力清单

| 能力 | API | 说明 |
|------|-----|------|
| 流式语音识别 | `llm_asr_begin` → `llm_asr_chunk` → `llm_asr_finish` | 先握手再说话（不丢音），边录边传，说完补零收尾 |
| 放弃识别 | `llm_asr_abort` | 断开连接，释放资源 |
| 大模型对话 | `llm_chat_init` / `llm_chat` | 服务端多轮上下文（默认记 4 轮），回答缓存 |
| 语音合成 | `llm_tts_play` | 拉取 PCM 流，经回调逐段交付，**播放由使用方实现** |

## 复用方法（以 firmware-c 为例）

1. 在工程 `CMakeLists.txt`（`project()` 之前）加入组件路径：

```cmake
list(APPEND EXTRA_COMPONENT_DIRS "${CMAKE_CURRENT_LIST_DIR}/../core/llm")
```

2. 使用方组件的 `REQUIRES` 里加 `llm`，代码里 `#include "llm.h"`。
3. 改 API 密钥/模型/音色：编辑 `src/llm_config.h`。

## ASR 调用约定

- 输入 16 位单声道 PCM，`begin(sample_rate, max_seconds)` 声明最长秒数；
- 服务端不支持 chunked，模块内部用固定 Content-Length + 事前声明总长 + 说完提前结束补零（踩坑验证过的方案）；
- `llm_asr_sent()` 返回已传字节数，使用方到达上限应主动停止录音；
- 各函数会释放资源：finish/abort 之后句柄不可再用，重新 begin 即新一轮。

## TTS 回调约定

```c
static esp_err_t tts_sink(const int16_t *pcm, int samples, void *ctx) {
    return audio_write(pcm, samples * 2, 2000);   /* 使用方自行做增益/格式转换 */
}
llm_tts_play("你好", 16000, tts_sink, NULL);
```

模块只负责把服务端返回的 16 位单声道 PCM 逐段送到回调，音量、I2S 输出、静音收尾都由使用方决定。
