/* tts 模块配置区：合成服务的密钥、地址、模型、音色 */
#ifndef TTS_CONFIG_H
#define TTS_CONFIG_H

/* 密钥不入库：构建时由 -DSILICONFLOW_API_KEY="…" 注入，来源见 core/README.md */
#ifndef SILICONFLOW_API_KEY
#error "SILICONFLOW_API_KEY 未定义：先 export 该环境变量再 idf.py build"
#endif
#define API_KEY     SILICONFLOW_API_KEY
#define API_BASE    "https://api.siliconflow.cn"

#define TTS_MODEL   "FunAudioLLM/CosyVoice2-0.5B"
#define TTS_VOICE   TTS_MODEL ":bella"          /* 女声 bella；男声改 alex */

#define TTS_TIMEOUT_MS   60000

#endif
