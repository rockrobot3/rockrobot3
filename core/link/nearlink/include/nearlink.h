/* nearlink —— 星闪 NearLink 链路（接口预留）
 * 经典 ESP32 无星闪射频硬件：本模块只占位，保持 link 体系完整。
 * 迁移到瑞芯微/海思等带 NearLink 的板卡时，实现同名 API 即可无缝接入。
 * 复用方法见本目录 README.md
 */
#ifndef NEARLINK_H
#define NEARLINK_H

#include <stdbool.h>

void nearlink_start(void);   /* 本平台：只打一条日志说明硬件不支持 */
bool nearlink_up(void);      /* 恒为 false（无硬件） */

#endif
