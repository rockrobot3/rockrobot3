/* audio 模块配置：硬件引脚与录音增益（改这里，不碰源码） */
#ifndef AUDIO_CONFIG_H
#define AUDIO_CONFIG_H

/* ---------- I2S 引脚（麦克风与功放共享时钟） ---------- */
#define I2S_BCLK  GPIO_NUM_4    /* 位时钟：麦克风 SCK / 功放 BCLK */
#define I2S_WS    GPIO_NUM_5    /* 帧时钟：麦克风 WS   / 功放 LRC */
#define I2S_DOUT  GPIO_NUM_25   /* 功放 DIN */
#define I2S_DIN   GPIO_NUM_18   /* 麦克风 SD（左声道） */

/* ---------- 录音处理 ---------- */
#define REC_GAIN  1             /* 录音增益：1=放大2倍，0=不放大；识别不准可试 2(=4倍) */

#endif
