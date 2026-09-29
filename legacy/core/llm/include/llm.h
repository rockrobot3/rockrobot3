/* llm —— 独立大模型对话模块（ESP-IDF 组件 core/llm）
 * 带历史记忆的多轮对话（服务端上下文由本地历史数组维护）。
 * 语音识别在 core/asr、语音合成在 core/tts（本模块只管文字↔文字）。
 * 模块参数（模型/人设/地址）在 llm_config.h；密钥不在仓库里，构建期注入
 * SILICONFLOW_API_KEY（见 legacy/core/README.md「密钥注入」）。复用方法见本目录 README.md
 */
#ifndef LLM_H
#define LLM_H

int llm_chat_init(void);                                        /* 开机调用一次 */
int llm_chat(const char *user_text, char *answer, int ans_sz);  /* 成功返回 0，answer=回答 */

#endif
