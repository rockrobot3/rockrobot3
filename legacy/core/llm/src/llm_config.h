/* llm 模块配置区：对话服务的模型、人设、地址（密钥不在本文件：构建期注入 SILICONFLOW_API_KEY，
 * 注入方式与缺失后果见 legacy/core/README.md 的「密钥注入」一节） */
#ifndef LLM_CONFIG_H
#define LLM_CONFIG_H

/* 密钥不入库：构建时由 -DSILICONFLOW_API_KEY="…" 注入，来源见 core/README.md */
#ifndef SILICONFLOW_API_KEY
#error "SILICONFLOW_API_KEY 未定义：先 export 该环境变量再 idf.py build"
#endif
#define API_KEY     SILICONFLOW_API_KEY
#define API_BASE    "https://api.siliconflow.cn"

#define LLM_MODEL   "deepseek-ai/DeepSeek-V3"  /* DeepSeek-V3（按量计费）；识别/合成分别见 legacy/core/asr、legacy/core/tts */

#define SYSTEM_PROMPT  "你是一个中文语音助手，回答要口语化、简短，一般不超过50个字，" \
                       "不要用列表、符号和表情。"
#define LLM_MAX_TOKENS     150
#define LLM_HISTORY_ROUNDS 4    /* 记住最近几轮对话 */

#define LLM_TIMEOUT_MS   60000

#endif
