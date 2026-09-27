/* data —— 运行时数据统计模块（ESP-IDF 组件 core/data）
 * 全系统结构化运行数据：开机次数、对话轮数、识别/回答/合成失败、继电器动作、错误。
 * 存 NVS 断电不丢，供面板/蓝牙查询——定位"机器慢/不稳定"先看这些数字。
 * 复用方法见本目录 README.md
 */
#ifndef DATA_H
#define DATA_H

#include <stdint.h>

typedef enum {
    DATA_BOOTS = 0,    /* 开机次数 */
    DATA_CHAT_ROUNDS,  /* 对话轮数 */
    DATA_ASR_OK,       /* 识别成功次数 */
    DATA_ASR_FAIL,     /* 识别失败次数 */
    DATA_LLM_FAIL,     /* 大模型回答失败次数 */
    DATA_TTS_FAIL,     /* 语音合成失败次数 */
    DATA_RELAY_ON,     /* 继电器吸合次数 */
    DATA_RELAY_OFF,    /* 继电器断开次数 */
    DATA_ERRORS,       /* 其他错误次数 */
    DATA_COUNT
} data_key_t;

void     data_init(void);                   /* 开机调用：从 NVS 载入，开机次数 +1 */
void     data_inc(data_key_t k);            /* 计数 +1（立即写 NVS） */
uint32_t data_get(data_key_t k);            /* 读当前值 */
int      data_get_json(char *out, int sz);  /* 全部计数导出 JSON 对象 */

#endif
