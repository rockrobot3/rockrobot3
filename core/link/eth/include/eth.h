/* eth —— 以太网链路（接口预留）
 * 经典 ESP32 支持 EMAC + 外接 PHY（LAN8720/TLK110/RTL8201）实现有线网，
 * 但本学习板未接 PHY：接口占位，接上 PHY 后实现真实驱动即可。
 * 复用方法见本目录 README.md
 */
#ifndef ETH_H
#define ETH_H

#include <stdbool.h>

void eth_start(void);    /* 本板未接 PHY：只打日志说明 */
bool eth_up(void);       /* 恒为 false（无 PHY 硬件） */

#endif
