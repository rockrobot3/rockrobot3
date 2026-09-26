/* llm 模块配置区：API 密钥、服务地址、模型、音色都在这里 */
#ifndef LLM_CONFIG_H
#define LLM_CONFIG_H

/* ---------- 服务（硅基流动，一个密钥通吃 ASR/LLM/TTS） ---------- */
#define API_KEY     "sk-tgvtipnqvzonqjxltidszzitsnfraecqjlgpxuoyhkuxsmqj"
#define API_BASE    "https://api.siliconflow.cn"

/* ---------- 模型与音色 ---------- */
#define ASR_MODEL   "Qwen/Qwen3-ASR-1.7B"          /* 语音识别（实测0.4秒出结果） */
#define LLM_MODEL   "Qwen/Qwen2.5-7B-Instruct"     /* 对话大模型；想用 DeepSeek 改 "deepseek-ai/DeepSeek-V3"（收费） */
#define TTS_MODEL   "FunAudioLLM/CosyVoice2-0.5B"  /* 语音合成 */
#define TTS_VOICE   TTS_MODEL ":bella"             /* 女声 bella；男声改 alex */

/* ---------- 对话风格 ---------- */
#define SYSTEM_PROMPT  "你是一个中文语音助手，回答要口语化、简短，一般不超过50个字，" \
                       "不要用列表、符号和表情。"
#define LLM_MAX_TOKENS     150
#define LLM_HISTORY_ROUNDS 4    /* 记住最近几轮对话 */

/* ---------- 接口超时（晚高峰响应慢，别调太小） ---------- */
#define ASR_TIMEOUT_MS   90000
#define LLM_TIMEOUT_MS   60000
#define TTS_TIMEOUT_MS   60000

#endif
