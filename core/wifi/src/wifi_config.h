/* wifi 模块配置
 * STA：编译默认账号（NVS 里存过面板配置的账号则优先用它）
 * AP ：操作面板热点（手机连它访问 http://192.168.4.1），密码至少 8 位
 * 注意：名字区分大小写；ESP32 只支持 2.4GHz */
#ifndef WIFI_CONFIG_H
#define WIFI_CONFIG_H

#define WIFI_SSID     "716"
#define WIFI_PASS     "71688888"

#define WIFI_AP_SSID  "MakeRobot"
#define WIFI_AP_PASS  "12345678"

#endif
