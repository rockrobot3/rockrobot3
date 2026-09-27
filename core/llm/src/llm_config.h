/* llm 模块配置区：对话服务的密钥、模型、人设 */
#ifndef LLM_CONFIG_H
#define LLM_CONFIG_H

#define API_KEY     "sk-tgvtipnqvzonqjxltidszzitsnfraecqjlgpxuoyhkuxsmqj"
#define API_BASE    "https://api.siliconflow.cn"

#define LLM_MODEL   "Qwen/Qwen2.5-7B-Instruct"  /* 想用 DeepSeek 改 "deepseek-ai/DeepSeek-V3"（收费） */

#define SYSTEM_PROMPT  "你是一个中文语音助手，回答要口语化、简短，一般不超过50个字，" \
                       "不要用列表、符号和表情。"
#define LLM_MAX_TOKENS     150
#define LLM_HISTORY_ROUNDS 4    /* 记住最近几轮对话 */

#define LLM_TIMEOUT_MS   60000

#endif
