/* llm —— 独立云端 AI 能力模块（ESP-IDF 组件）
 * 提供：流式语音识别(ASR) / 大模型对话(LLM，带历史记忆) / 语音合成(TTS)
 * 只依赖 ESP-IDF 自身，不依赖任何具体固件；音频播放通过回调交给使用方。
 * 模块参数（API 密钥/模型/音色）在 llm_config.h，复用方法见本目录 README.md
 */
#ifndef LLM_H
#define LLM_H

#include <stdint.h>
#include <stddef.h>
#include "esp_err.h"

/* ---- 流式语音识别（输入：16位 单声道 PCM） ----
 * 用法：begin 先握手（约1~2秒，期间不丢音）→ 边录边 chunk 上传 →
 *       finish 补零收尾并取识别结果；中途放弃用 abort。
 */
typedef struct llm_asr_s llm_asr_t;

llm_asr_t *llm_asr_begin(int sample_rate, int max_seconds);      /* 成功返回句柄，失败返回 NULL */
int        llm_asr_chunk(llm_asr_t *s, const void *data, int len); /* 流式上传一段音频 */
int        llm_asr_sent(llm_asr_t *s);                             /* 已上传的 PCM 字节数（防发超用） */
int        llm_asr_finish(llm_asr_t *s, char *text, int text_sz);  /* 成功返回 0，text=识别结果 */
void       llm_asr_abort(llm_asr_t *s);                            /* 放弃本次请求并断开 */

/* ---- 大模型对话（带历史记忆，服务端多轮上下文） ---- */
int llm_chat_init(void);                                        /* 开机调用一次 */
int llm_chat(const char *user_text, char *answer, int ans_sz);  /* 成功返回 0，answer=回答 */

/* ---- 语音合成（服务端返回 PCM 流，经回调逐段交付给使用方播放） ----
 * sink 会在 HTTP 200 后被反复调用：pcm=16位单声道采样，samples=本次采样数；
 * sink 返回非 ESP_OK 即中止播放。 */
typedef esp_err_t (*llm_pcm_sink_t)(const int16_t *pcm, int samples, void *ctx);
int llm_tts_play(const char *text, int sample_rate, llm_pcm_sink_t sink, void *ctx);

#endif
