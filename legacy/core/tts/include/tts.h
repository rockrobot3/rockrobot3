/* tts —— 独立云端语音合成模块（ESP-IDF 组件 core/tts）
 * 文字 → 云端合成 → PCM 流经回调逐段交付给使用方播放（模块不管喇叭）。
 * 模块参数（模型/音色/地址）在 tts_config.h；密钥不在仓库里，构建期注入
 * SILICONFLOW_API_KEY（见 legacy/core/README.md「密钥注入」）。复用方法见本目录 README.md
 */
#ifndef TTS_H
#define TTS_H

#include <stdint.h>
#include "esp_err.h"

/* sink 在 HTTP 200 后被反复调用：pcm=16位单声道，samples=本次采样数；
 * sink 返回非 ESP_OK 即中止播放 */
typedef esp_err_t (*tts_pcm_sink_t)(const int16_t *pcm, int samples, void *ctx);

int tts_play(const char *text, int sample_rate, tts_pcm_sink_t sink, void *ctx);

#endif
