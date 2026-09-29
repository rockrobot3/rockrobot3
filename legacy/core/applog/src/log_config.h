/* log 模块配置区 */
#ifndef LOG_CONFIG_H
#define LOG_CONFIG_H

#define LOG_MAX_LINES  100   /* 环形缓冲条数（100×160B≈16KB；BT+TLS 共存时内存紧张，别调大） */
#define LOG_LINE_MAX   160   /* 单条日志最长字符数（超出截断） */

#endif
