/* 语音管家配置区：改 WiFi/密钥/模型/阈值都在这里 */
#ifndef APP_CONFIG_H
#define APP_CONFIG_H

/* ---------- 网络 ---------- */
#define WIFI_SSID   "iPhone"
#define WIFI_PASS   "12345678"

/* ---------- 云端（硅基流动） ---------- */
#define API_KEY     "sk-tgvtipnqvzonqjxltidszzitsnfraecqjlgpxuoyhkuxsmqj"
#define API_BASE    "https://api.siliconflow.cn"

#define ASR_MODEL   "Qwen/Qwen3-ASR-1.7B"          /* 语音识别（实测0.4秒出结果，比SenseVoice快100倍） */
#define LLM_MODEL   "Qwen/Qwen2.5-7B-Instruct"     /* 对话大模型；想用 DeepSeek 改 "deepseek-ai/DeepSeek-V3"（收费） */
#define TTS_MODEL   "FunAudioLLM/CosyVoice2-0.5B"  /* 语音合成 */
#define TTS_VOICE   TTS_MODEL ":bella"             /* 女声 bella；男声改 alex */

/* ---------- 录音 ---------- */
#define REC_GAIN    1      /* 录音增益：1=放大2倍，0=不放大；识别不准时可试 2(=4倍) */
#define MAX_REC_S   6      /* 一次说话最长秒数（说完自动提前结束+补零，服务端可解析） */
#define VAD_START   3200   /* 确认在说话的阈值（太小环境噪音会误录） */
#define VAD_END     2200   /* 静音阈值：低于它算说完了 */
#define VAD_END_MS  1200   /* 安静多久（毫秒）算说完 */
#define IDLE_RESTART_MS 45000  /* 一直没人说话就重连一次服务器（防连接老化） */

/* ---------- 播放 ---------- */
#define PLAY_GAIN   3      /* 播放音量：4=满音量，3=75%，2=50% */

/* ---------- 硬件 ---------- */
#define RELAY_GPIO  GPIO_NUM_21  /* 板载继电器（语音开灯/关灯） */

/* ---------- 音频常量（一般不用改） ---------- */
#define SAMPLE_RATE 16000
#define CHUNK       256

#endif
