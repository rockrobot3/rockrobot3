/* audio —— 独立音频模块（ESP-IDF 组件 core/audio）
 * I2S 全双工：INMP441 麦克风录音 + MAX98357A 功放播放
 * 公开契约：16kHz 采样、32位整型、AUDIO_CHUNK 帧 */
#ifndef AUDIO_H
#define AUDIO_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#define AUDIO_SAMPLE_RATE 16000   /* 采样率（模块固定契约） */
#define AUDIO_CHUNK       256     /* 单次缓冲帧数（采样数） */

void audio_init(void);            /* 初始化 I2S（麦克风+功放） */
/* 读麦克风原始数据（32bit 整型），返回 ESP_OK/失败 */
int  audio_read(int32_t *buf, int max_bytes, size_t *bytes_read, int timeout_ms);
/* 往喇叭写播放数据（32bit 整型） */
int  audio_write(const void *data, size_t bytes, int timeout_ms);
/* 播放提示音 */
void audio_play_tone(int freq, int ms);
/* 麦克风单采样处理：去直流 + 加增益，返回原始峰值（VAD 判断用） */
int  audio_mic_process(int32_t raw, int16_t *out);

/* 麦克风静音开关：关=读麦克风一律得 0 字节（默认开）。
 * 面板/蓝牙可远程控制，真正的"闭嘴"——上层监听循环读到 0 就不会走识别。 */
void audio_mic_set_enable(bool en);
bool audio_mic_enabled(void);

#endif
