/* 语音管家配置区：对话策略参数（VAD 阈值/录音时长/播放音量）
 * 硬件与网络参数在各模块自己的配置里（路径为迁入 legacy/ 后的真实位置）：
 *   WiFi   → legacy/core/link/wifi/src/wifi_config.h
 *   录音   → legacy/core/audio/src/audio_config.h（增益/引脚）
 *   继电器 → legacy/core/relay/src/relay_config.h（GPIO）
 *   云端AI → legacy/core/llm/src/llm_config.h（模型/人设/地址）
 *            云端密钥不在任何 config.h 里：构建期注入 SILICONFLOW_API_KEY，
 *            注入方式与缺失后果见 legacy/core/README.md 的「密钥注入」一节 */
#ifndef APP_CONFIG_H
#define APP_CONFIG_H

/* ---------- 对话策略 ---------- */
#define MAX_REC_S   15     /* 一次说话最长秒数：云端接口要求预声明总长（不支持 chunked），
                              上限必须容得下完整一句话，6 秒会拦腰截断长句。
                              说完会自动提前结束+补零，短句不亏速度 */
#define VAD_START   3200   /* 确认在说话的阈值（太小环境噪音会误录） */
#define VAD_END     2200   /* 静音阈值：低于它算说完了 */
#define VAD_END_MS  1500   /* 安静多久（毫秒）算说完：太短会把说话中途的思考停顿当断句 */
#define IDLE_RESTART_MS 25000  /* 一直没人说话就主动重连一次：云端实测 30~75s 就掐空闲连接，必须抢在它前面 */

/* ---------- 播放 ---------- */
#define PLAY_GAIN   3      /* 播放音量：4=满音量，3=75%，2=50% */

#endif
