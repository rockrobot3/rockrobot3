/* asr 模块配置区：识别服务的密钥、地址、模型 */
#ifndef ASR_CONFIG_H
#define ASR_CONFIG_H

#define API_KEY     "sk-tgvtipnqvzonqjxltidszzitsnfraecqjlgpxuoyhkuxsmqj"
#define API_BASE    "https://api.siliconflow.cn"

#define ASR_MODEL   "Qwen/Qwen3-ASR-1.7B"     /* 实测 0.4 秒出结果 */

#define ASR_TIMEOUT_MS   90000                /* 晚高峰响应慢，别调太小 */

#endif
