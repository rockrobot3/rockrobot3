/* bt —— 经典蓝牙 SPP 串口控制台（ESP-IDF 组件 core/bt）
 * 不依赖 WiFi 的应急/近场通道：手机装蓝牙串口 App（如 Serial Bluetooth Terminal），
 * 配对连接设备名 RockRobot3，即可实时收日志、发命令控制设备。
 * 命令：help / status / ver / logs / relay on|off / mic on|off / selftest / reboot
 * 复用方法见本目录 README.md
 */
#ifndef BT_H
#define BT_H

#include <stdbool.h>

void bt_start(void);        /* 开机调用（不依赖 WiFi，NVS 初始化之后） */
bool bt_connected(void);    /* 是否有蓝牙串口客户端在线 */

#endif
