/* asr 模块配置区：识别服务的密钥、地址、模型 */
#ifndef ASR_CONFIG_H
#define ASR_CONFIG_H

/* 密钥不入库：构建时由 -DSILICONFLOW_API_KEY="…" 注入，来源见 core/README.md */
#ifndef SILICONFLOW_API_KEY
#error "SILICONFLOW_API_KEY 未定义：先 export 该环境变量再 idf.py build"
#endif
#define API_KEY     SILICONFLOW_API_KEY
#define API_BASE    "https://api.siliconflow.cn"

#define ASR_MODEL   "Qwen/Qwen3-ASR-1.7B"     /* 实测 0.4 秒出结果 */

#define ASR_TIMEOUT_MS   90000                /* 晚高峰响应慢，别调太小 */

#endif
