/* bt 模块配置区 */
#ifndef BT_CONFIG_H
#define BT_CONFIG_H

#define BT_DEVICE_NAME  "RockRobot3"   /* 蓝牙设备名（手机搜索看到的） */

/* 蓝牙开关：0=不初始化蓝牙（默认，语音对话优先——经典 ESP32 内存装不下
 * 蓝牙常驻 + HTTPS/TLS 峰值，两者互斥）；1=启用蓝牙控制台，此时语音不可用。
 * 使用场景：设备无法联网时改 1，用蓝牙串口 App 免网排查/控制 */
#define BT_ENABLE  0

#endif
