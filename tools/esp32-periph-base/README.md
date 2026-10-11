# esp32-periph-base —— 外设底座档（0.0.5 增补轮五 批B）

espidf std 固件：SSD1306 OLED（I2C）+ 云台双轴舵机（LEDC PWM）+ UART2 行指令面。
**批B 不进城**（不依赖 rr-ipc/rr-core）——批C 在本 crate 上加哨兵面换能力帧，批D 加 I2S（全案 §4/§5）。

## 构建（专用门）

```bash
bash tools/ci/esp32-periph-base-check.sh
```

SKIP 纪律同 espidf-sentinel-check.sh（esp 工具链/ldproxy/ESP-IDF v5.2.2/esp-clang 缺位 = SKIP 明说）。

## 接线（与 0.0.7 实测接线零冲突）

| 外设 | 引脚 | 备注 |
|---|---|---|
| OLED SSD1306 0x96" | VCC→3V3 · GND→GND · SCL→GPIO22 · SDA→GPIO21 | I2C0 @400kHz，地址 0x3C |
| 舵机 pan（水平） | 信号→GPIO32 · 电源→UBEC 5V · GND→共地 | **禁接板载 5V**（堵转 ~700mA/只） |
| 舵机 tilt（俯仰） | 信号→GPIO33 · 电源→UBEC 5V · GND→共地 | UBEC/板/舵机单点共地（铁律） |
| UART2 指令面 | GPIO17→CP2102 RXD · GPIO16←CP2102 TXD · GND→共地 | 115200，3.3V TTL（严禁 5V） |
| 板载 USB（cu.usbserial-1410） | 烧录 + monitor | `cargo +esp run`，Ctrl+] 退出 |

## 烧录与验收 runbook

1. **烧录**（板载口 1410）：`cd tools/esp32-periph-base && cargo +esp run`——monitor 见
   `[periph] surface=...` SURFACE 行 + RAM 采样两行 + `I2C 扫描应答: [... 3C ...] 在列 ✓`。
2. **指令面**（CP2102 口 0001，stty 参数须与读写同 fd 生效——0.0.7 §9.4 ⑥ 实锤）：

   ```bash
   python3 -c "
   import serial, time
   s = serial.Serial('/dev/cu.usbserial-0001', 115200, timeout=1)
   s.reset_input_buffer()
   for cmd in ['HELP', 'INFO', 'OLED HELLO ROCK3', 'PAN 0', 'PAN 180', 'PAN 90', 'TILT 45', 'SCAN']:
       s.write((cmd + '\n').encode()); time.sleep(1.2)
       print(cmd, '=>', s.read(2048).decode(errors='replace').strip())
   "
   ```

   先烧录后开指令面：烧录的 DTR/RTS 翻转会复位板子（0.0.7 实锤），指令面时序在烧录之后。

3. **验收判据**（全案 §4 批B 四件套）：
   - ① I2C 扫描 0x3C 应答：monitor 扫描表 0x3C 在列；
   - ② OLED 出字：`OLED <text>` 后目视内容与指令一致（另 `FONT` 指令打印 "ABC123"
     字模 hex 供人工比对护栏）；
   - ③ 舵机五点：`SCAN` 后两轴各 0/45/90/135/180 逐点目视一致、串口回读角度一致、
     无堵转抖振（失败处置：堵转时 UBEC 输出须 ≥4.7V，仍异常单轴排除法）；
   - ④ 供电核验：舵机由 UBEC 5V 供电、UBEC/板/舵机单点共地（装配步目视 + 万用表）。

## 指令表（UART2 行协议，\n 结尾；批C 换能力帧时本面整体退役）

| 指令 | 效果 |
|---|---|
| `OLED <text>` | 三行显示（≤63 字符，21 列/行硬切 3 行），第 4 行为实时角度 |
| `PAN <deg>` / `TILT <deg>` | 单轴 0–180（超范围 ERR，fail-closed） |
| `SERVO <pan> <tilt>` | 双轴同设 |
| `SCAN` | 两轴各 0/45/90/135/180 五点扫描（每点 600ms，逐点回读） |
| `INFO` | 回读当前角度 / OLED 最近文本 / 堆水位 |
| `FONT` | "ABC123" 字模 hex 自检护栏 |
| `HELP` | 指令表 |

未知指令一律 `ERR`（不猜不拖）。

## Mac 实时数据演示（scripts/mac_monitor.py，批B 增量批）

OLED 4 行扩行（2→4）的指令面真件验证配套：Mac 侧 1Hz 采集 CPU 温度
（`osx-cpu-temp`）/内存占用（`memory_pressure -Q`）/IP（`ipconfig getifaddr`）/
系统时间，拼 63 字符一条 `OLED` 命令上屏（row3 角度行固件自管）：

```bash
brew install osx-cpu-temp && pip3 install pyserial
python3 scripts/mac_monitor.py /dev/cu.usbserial-0001   # CP2102 设备号以实际为准
```

开口帧同步已内置（open 后先发空行冲掉 boot 期 RX 毛刺残行——真板探针实锤：
首行指令会被噪声前缀污染回 `ERR unknown cmd`）。脚本为演示件，不进十步门禁
扫面（crate 目录整体在 ④/⑥ 排除清单内）。
