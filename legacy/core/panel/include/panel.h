/* panel —— 独立 Web 操作面板模块（ESP-IDF 组件 core/panel）
 * 手机连 RockRobot3 热点访问 http://192.168.4.1，或连同一 WiFi 访问设备 IP
 * 功能：状态查看 / WiFi 配网 / 硬件自检与控制 / 重启与恢复出厂 */
#ifndef PANEL_H
#define PANEL_H

void panel_start(void);   /* 启动 HTTP 面板服务（wifi_start 之后调用） */

#endif
