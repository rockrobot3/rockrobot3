/* 云端 AI 模块：语音识别(ASR) + 大模型对话(LLM) + 语音合成(TTS)，走硅基流动 API */
#ifndef APP_CLOUD_H
#define APP_CLOUD_H

#include <stdint.h>

/* ---- 流式语音识别 ---- */
typedef struct {
    void *c;                   /* esp_http_client 句柄（内部用） */
    int sent;                  /* 已发送的 WAV 数据字节数 */
} cloud_asr_t;

int cloud_asr_begin(cloud_asr_t *s);                              /* 提前握手（约1~2秒） */
int cloud_asr_chunk(cloud_asr_t *s, const void *data, int len);   /* 流式上传一段音频 */
int cloud_asr_finish(cloud_asr_t *s, char *text, int text_sz);    /* 补零收尾并取识别结果 */
void cloud_asr_abort(cloud_asr_t *s);                             /* 放弃本次请求并断开 */

/* ---- 大模型对话（带历史记忆） ---- */
void cloud_history_init(void);
int  cloud_llm_chat(const char *user_text, char *answer, int ans_sz);

/* ---- 语音合成并流式播放 ---- */
int  cloud_tts_play(const char *text);

#endif
