/* tts 模块配置区：合成服务的密钥、地址、模型、音色 */
#ifndef TTS_CONFIG_H
#define TTS_CONFIG_H

#define API_KEY     "sk-tgvtipnqvzonqjxltidszzitsnfraecqjlgpxuoyhkuxsmqj"
#define API_BASE    "https://api.siliconflow.cn"

#define TTS_MODEL   "FunAudioLLM/CosyVoice2-0.5B"
#define TTS_VOICE   TTS_MODEL ":bella"          /* 女声 bella；男声改 alex */

#define TTS_TIMEOUT_MS   60000

#endif
