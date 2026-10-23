# ESP32 板卡笔记 —— rock3 仓 ESP32 载体实操台账

- **定位**：ESP32 板卡实操笔记（硬件/接线/串口/证据台账）。0.0.7 批A"随批D1
  新建 ESP32 笔记"欠账的载体落地（status.md 2026-10-11 结项注记）。
- **权威落点声明**（只引不抄）：串口排障四实锤唯一落点 =
  [0.0.7 方案 §9.4 ⑥](../process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md)；
  esp-idf 版本组合唯一落点 = 0.0.7 方案批W1 登记；板上 runbook 唯一落点 =
  0.0.7 方案 §9 / [tools/espidf-sentinel/README.md](../../tools/espidf-sentinel/README.md)。
  本笔记只做实操聚合与证据台账。

## 1 硬件与接线

- **板卡**：ESP32 经典款 devkit——chip revision v3.1、4MB flash、factory app
  分区 @0x10000（boot banner 实录见 §3）。
- **UART2 接线规范**（0.0.7 批C④⑤ 实测同款）：GPIO17→适配器 RXD /
  GPIO16←TXD / GND 共地，**3.3V TTL（严禁 5V）**。
- **USB 串口双节点**：板载串口 `cu.usbserial-1410`（烧录 + 控制台观察）；
  CP2102 适配器 `cu.usbserial-0001`（对端数据链 + 注入测试）。macOS 侧一律用
  `cu.*` callout 节点（`tty.*` callin open 报 EBUSY 与占用无关）。
- **供电铁律**（后续外设批适用）：舵机由 UBEC 独立 5V 轨供电（堵转 ~700mA/只，
  禁接板载 5V），UBEC/板/电池单点共地。

## 2 串口实操速查

物理层通断判别：`cat` 直听，开头见到 magic `31 45 52 41`（ASCII "1ERA"）即
物理层与波特率正确。其余（stty 参数与读写同 fd、残留旧桥 ps 清场、DTR/RTS
复位时序——rrd socket 先于任何复位点就绪）见 0.0.7 方案 §9.4 ⑥，不在此重抄。

## 3 证据台账

### 3.1 批C④⑤ 板级 round-trip（2026-10-10，两板收口批 `6d3a1ee`+`3c467c2`）

原物证存 `/tmp/esp32-c4/`（易失区，不进仓）；本节即持久摘要——
**0.0.7 状态行 R2（"串口日志摘要未留档，boards 回填时补"）的回填兑现
（2026-10-11，0.0.5 增补轮五批A）**。

- **烧录**：espflash 4.x → `cu.usbserial-1410`，2026-10-10T13:03:55Z（UTC）。
  boot banner 关键行（console.log 原文摘录）：
  `rst:0x1 (POWERON_RESET),boot:0x17 (SPI_FAST_FLASH_BOOT)`；
  `I (27) boot: ESP-IDF v6.1-beta1-497-g14f663f003e 2nd stage bootloader`；
  `I (28) boot: chip revision: v3.1`；`SPI Flash Size : 4MB`；
  `I (132) boot: Loaded app from partition at offset 0x10000`。
  固件横幅：`esp32-uart-echo ready (uart2 gpio16/17 115200)`。
- **运行期 dump（批B"xtensa 运行期 dump 归批C UART 夹具作第二重证据"的兑现）**：
  console.log 含 request / response / event / stream / notification 五帧 hex
  （128B 头 + payload，头 12 字节 `31 45 52 41 01 00 00 00 78 56 34 12` =
  magic + version 1 + `0x12345678`）。
- **④ round-trip**：`frame.bin`（160B = 128B 头 + 32B payload）→ 板回显
  `frame.echo` / `frame.echo2`——**当日 cmp 逐字节一致；2026-10-11 落档时
  cmp 复验两轮一致（本笔记归档时重跑）**。
- **⑤ 注入拒收**：console.log 旁证两条——
  `reject: short frame (got 64/128 header bytes)`、
  `reject: short frame (got 3/128 header bytes)`，注入后
  `frame received: len=160 payload_len=32`（正常帧恢复收发）。坏 magic 为
  静默丢弃路径（固件仅对短帧打点），"无回发"判据由测试脚本侧 PASS 现场确认
  ——`3c467c2` 口述确认口径维持，**脚本 stdout 未留档，如实注记**。
- **测试脚本**：`roundtrip.py`（macOS termios 会话内配置，115200，
  `cu.usbserial-0001`，静态 rrd 之外零仓内代码）——用例序：正常帧往返 →
  半帧截断（应拒收无回发）→ 坏 magic（应拒收无回发）→ 注入后正常帧仍通；
  拒收日志旁证在 1410 控制台抓取（espflash monitor 常驻）。
- **物证文件清单**：`console.log`（3.9KB，烧录+boot+运行期 dump+拒收旁证）、
  `frame.bin` / `frame.echo` / `frame.echo2`（各 160B）、`frame.echo.hex`、
  `header.dump`（128B 头 hex）、`roundtrip.py`。

### 3.2 批D1 层③ 哨兵（2026-10-10，`f72c0d4`+`c6e3a9c`）

HARNESS_EXIT=0（14 PASS：注册 / 8 拍心跳 195–204ms / round-trip id=9001 回显）
+ 三点 RAM（boot 295672B → roundtrip min_free 250620B ≈ 启动堆 85%）。权威
落点 [tools/espidf-sentinel/README.md](../../tools/espidf-sentinel/README.md)
（实测报告回填件），本笔记不重复。

### 3.3 批E 层④ 桥 e2e（2026-10-11，`870cfe6`+`567c832`）

E6 本机彩排 PASS=18 + E7 真板 e2e PASS=16 FAIL=0（断链自愈真拔实锤）。权威
落点 0.0.7 方案状态行与 §9.4，本笔记不重复。

### 3.4 批N20 产线真板实录（2026-10-23，敞口 13 结项批）

权威落点＝[批N20 计划册 §2](../process/dev/0.0.5增补轮六——批N20开发计划与产线真板实录.md)
逐件证据行原文，本节只记板上机器事实摘要：

- **首烧（merged 直烧）**：`espflash write-bin 0` 直烧 image-esp32.sh --release
  产的 merged 4MB 全量 98s；板上分区表＝espflash 默认模板（factory 0x3F0000）；
  boot 三达：OLED 4 行出字 / LEDC pan=60 tilt=98 / 双麦 16kHz rms 流；RAM
  point 2-periph-ready 250660B。
- **升级（重烧即升级）**：改横幅（n20-live）＋去编译期注入重烧 → `凭据源=nvs`
  保持＋sta ip 192.168.0.167 自动重连——NVS 跨重烧保留实证（写口 POST 写入）。
- **恢复（回得来）**：`espflash erase-flash` 3s 全片擦 → 带注入重烧 →
  `凭据源=编译期` 回落＋重新在线。
- **写口断链**：POST /api/board/wifi 应答 1.99s recv failure＝批N16 裁定物理
  事实（STA 重配置杀 socket）；6s 后 GET state 200 旁证。
- **机器事实订正**：release app image 实测 1,086,848 B（批N18 记录 444,576 B
  系测量错误，批N20 件一定案）；分区表双表事实见 partition-layout.md 批N18 段
  批N20 订正（IDF 表 0x100000 vs espflash 模板表 0x3F0000）。
