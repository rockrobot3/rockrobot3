/* 语音管家配置区：对话策略参数（VAD 阈值/录音时长/播放音量）
 * 硬件与网络参数在各模块自己的配置里：
 *   WiFi   → core/wifi/wifi_config.h
 *   录音   → core/audio/audio_config.h（增益/引脚）
 *   继电器 → core/relay/relay_config.h（GPIO）
 *   云端AI → core/llm/llm_config.h（密钥/模型/音色） */
#ifndef APP_CONFIG_H
#define APP_CONFIG_H

/* ---------- 对话策略 ---------- */
#define MAX_REC_S   6      /* 一次说话最长秒数（说完自动提前结束+补零，服务端可解析） */
#define VAD_START   3200   /* 确认在说话的阈值（太小环境噪音会误录） */
#define VAD_END     2200   /* 静音阈值：低于它算说完了 */
#define VAD_END_MS  1200   /* 安静多久（毫秒）算说完 */
#define IDLE_RESTART_MS 45000  /* 一直没人说话就重连一次服务器（防连接老化） */

/* ---------- 播放 ---------- */
#define PLAY_GAIN   3      /* 播放音量：4=满音量，3=75%，2=50% */

#endif
