/* relay —— 独立继电器模块（ESP-IDF 组件 core/relay）
 * 板载继电器开关（语音"开灯/关灯"等场景复用） */
#ifndef RELAY_H
#define RELAY_H

#include <stdbool.h>

void relay_init(void);      /* 初始化继电器引脚（默认断开） */
void relay_set(bool on);    /* true=吸合(通电) false=断开 */
bool relay_get(void);       /* 当前是否吸合 */

#endif
