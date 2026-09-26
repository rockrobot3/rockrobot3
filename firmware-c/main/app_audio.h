/* 音频硬件模块：I2S 麦克风 + 功放 + 板载继电器 */
#ifndef APP_AUDIO_H
#define APP_AUDIO_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

void audio_init(void);
/* 读麦克风原始数据（32bit 整型），返回 ESP_OK/失败 */
int  audio_read(int32_t *buf, int max_bytes, size_t *bytes_read, int timeout_ms);
/* 往喇叭写播放数据（32bit 整型） */
int  audio_write(const void *data, size_t bytes, int timeout_ms);
/* 播放提示音 */
void audio_play_tone(int freq, int ms);
/* 麦克风单采样处理：去直流 + 加增益，返回原始峰值（VAD 判断用） */
int  audio_mic_process(int32_t raw, int16_t *out);

void relay_init(void);            /* 初始化板载继电器引脚 */
void relay_set(bool on);          /* 继电器吸合/断开 */

#endif
