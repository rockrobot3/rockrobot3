/* asr —— 独立云端语音识别模块（ESP-IDF 组件 core/asr）
 * 流式上传 PCM 到云端识别服务，返回文字。
 * 用法：begin 先握手（约1~2秒，期间不丢音）→ 边录边 chunk 上传 →
 *       finish 补零收尾并取识别结果；中途放弃用 abort。
 * 模块参数（密钥/模型）在 asr_config.h，复用方法见本目录 README.md
 */
#ifndef ASR_H
#define ASR_H

typedef struct asr_s asr_t;

asr_t *asr_begin(int sample_rate, int max_seconds);       /* 成功返回句柄，失败 NULL */
int    asr_chunk(asr_t *s, const void *data, int len);    /* 流式上传一段音频（16位PCM） */
int    asr_sent(asr_t *s);                                /* 已上传字节数（防发超用） */
int    asr_finish(asr_t *s, char *text, int text_sz);     /* 成功返回 0，text=识别结果 */
void   asr_abort(asr_t *s);                               /* 放弃本次请求并断开 */

#endif
