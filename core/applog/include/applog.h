/* log —— 独立日志模块（ESP-IDF 组件 core/log）
 * 作用：设备出问题时能回答"刚才发生了什么"。
 * - 接管 ESP-IDF 全部 ESP_LOGx 日志输出（串口照常打印，同时存入 RAM 环形缓冲）
 * - 应用层事件用 log_writef 记录（对话流程/失败/控制动作）
 * - 面板 /api/logs、蓝牙 logs 命令随时回放最近日志，复现问题不再靠猜
 * 复用方法见本目录 README.md
 */
#ifndef LOG_H
#define LOG_H

/* 开机尽早调用：接管 ESP-IDF 日志输出 */
void log_init(void);

/* 应用事件写日志（自动加运行秒数前缀，并同步打印到串口） */
void log_writef(const char *fmt, ...);

/* 导出最近 max_lines 条日志（新的在前），JSON 字符串数组；返回写入长度 */
int  log_dump_json(char *out, int out_sz, int max_lines);

/* 缓冲中现有条数 */
int  log_count(void);

/* 注册镜像回调：每条新日志回调一次 fn(line)，如蓝牙转发（可传 NULL 取消） */
void log_set_mirror(void (*fn)(const char *line));

#endif
