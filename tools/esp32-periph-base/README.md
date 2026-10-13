# esp32-periph-base —— 外设底座档（0.0.5 增补轮五 批B）

espidf std 固件：SSD1306 OLED（I2C）+ 云台双轴舵机（LEDC PWM）+ UART2 行指令面。
**批B 不进城**（不依赖 rr-ipc/rr-core）——批C 在本 crate 上加哨兵面换能力帧，批D 加 I2S（全案 §4/§5）。

**控制面/运行面分离**（0.0.5 现场处置续批）：控制面线程独占 UART2（解析/校验/改状态/
推作业/回包，不碰慢外设），运行面线程独占 I2C0+LEDC（执行硬件作业 + 自主节拍）——
慢 I2C/舵机整定不阻塞命令吞吐（消"卡顿"）。**舵机串行降级**：复合动作先 pan 到位 →
整定 200ms → 再 tilt（任一时刻只驱动一轴），削板载 5V 直供峰值电流。
**舵机缓速软启动**：每轴按 2°/20ms（=100°/s）逐档逼近目标，把整段跳变的电流尖峰
摊成涓流。**供电分时仲裁（板载 5V 直供档的根因修复）**：见下节。

## 构建（专用门）

```bash
bash tools/ci/esp32-periph-base-check.sh
```

SKIP 纪律同 espidf-sentinel-check.sh（esp 工具链/ldproxy/ESP-IDF v5.2.2/esp-clang 缺位 = SKIP 明说）。

## 供电分时仲裁（板载 5V 直供档，2026-10-12 真板实锤的根因修复）

现场已改为**舵机走板载 5V 直供**（无 UBEC、无额外硬件）。真板实测出一条硬机理：

> **SSD1306 的电荷泵电流与舵机的保持电流在同一 5V 轨上不能共存。**
> 两轴一激磁，OLED 的页写/命令笔即恒 `ESP_FAIL`（NACK）；**把 duty 掉到 0，同一笔
> 立刻 Ok**。且激磁越久越顽固——一度到"掉 duty 也不复"，需重走 `oled_init`
> 才救回。附注：地址单笔探测在总线已死时仍"通过"（弱上拉下的**假 ACK**），
> 单用它判活会被骗。

固件据此做**供电分时仲裁**（运行面单线程，渲染与移动本就串行）：

| 机制 | 落点 | 作用 |
|---|---|---|
| 舵机让位 | [`RtPeriph::servo_release()`](src/periph.rs)，在 [`runtime_loop`](src/runtime.rs) 每次动作收尾调用 | 除"正在移动/扫描"外**一律 duty=0**，把 5V 轨整段让给 OLED |
| I2C 强制重同步 | `i2c_bus_recover(true)`，紧随让位 | 无条件打 9 SCL 时钟 + STOP——舵机电机换向噪声会在**空闲期**把从机 I2C 状态机顶进半途态（此时 SDA 仍高，"只在 SDA 低时打时钟"的条件版救不回来） |
| 亮度降级阶梯 | `OLED_CONTRAST_LEVELS` = 0x4F→0x3F→0x2F→0x1F | 整屏渲染失败即降一档重来（boot 首帧通常会自降一档到 0x3F，之后稳定） |
| 整页重写 + 分块 | `oled_page`：≤32B/笔（硬件 FIFO），整页重来 2 次 | 丢拍后从机列指针会错位，必须从确定性起点重写整页 |

**代价（如实登记）**：舵机在两次命令之间**不保持激磁**，位姿靠齿轮摩擦维持，
长时间静置可能缓慢下沉；`WANDER` 期间每步只激磁 ~20ms、随即让位，故云台是"碎步"
逼近而非连续平滑扫摆。**想要"持续保持位姿 + OLED 同时稳定"必须上硬件**：舵机独立
UBEC 5V 轨，或至少在 OLED 模块 VCC/GND 就近并一颗 ≥100µF 电解 + 0.1µF 陶瓷。
判据见 0.0.5 §4 批B 四件套 ④（堵转时 UBEC ≥4.7V）。

## 5V 轨电压监测与欠压判定（方案A Q1+Q2，批M 步2/步3）

BOD 根因链的**观测 + 响应手段**：这条 5V 轨此前无任何采样，欠压既看不见也响应不了。本档加
ADC1_CH6 @ GPIO34 采样轨压，按「**机制与策略分离**」落两处：

| 面 | 落点 | 内容 |
|---|---|---|
| 策略 | [`board.rs`](src/board.rs) `ADC_*` / `RAIL_*` 常量区 | 引脚 / 分压比（**占位 1:1 待标定**）/ 采样节拍 / ADC 衰减·分辨率·标定 / 双阈值（`RAIL_MILD_MV`/`RAIL_SEVERE_MV`，**占位待标定**）/ 接闸（`RAIL_GATE_ARMED`） |
| 机制 | [`power.rs`](src/power.rs) | `RailMonitor` 读 ADC 一次 + 按分压比换算回轨压 mV；`tier_of` 双阈值电平分类；`RailJudge` 一核两条线（rr-mech `Rr3LowEdge` ×2）产越阈事件 |

运行面自主节拍每 `RAIL_SAMPLE_PERIOD_MS` 周期采一次：喂判定核 → 写
`PeriphState.rail_mv/rail_raw/rail_tier`（`INFO` 观测），越阈（**向下档变**）打站并累加
`rail_low_events`；`RAIL` 命令另可点名**即刻**采一次。**装配失败不 panic**（照 OLED init
纪律）——`RtPeriph.rail = None` 时采样路径回 Err，不冒领读数。

**判定核（步3）**：双阈值 `RAIL_SEVERE_MV < RAIL_MILD_MV`。**重度欠压 → 执行面拒动**
（MC-5 语义：`servo_set` 点名拒收、**妆面 OLED 不禁**、**回阈解禁不自动重放**）；**轻度
欠压 → 维持既有串行降级**（pan→整定→tilt，本就常开）；**正常 → 直通**。越阈事件用
rr-mech 安全核 `Rr3LowEdge` **两实例**（一核两条线）产——"越阈一次一段、严格回阈才重新
武装"的语义钉死在安全核内，调用方只能喂电压不能改语义。

**接闸纪律（`RAIL_GATE_ARMED=false` 默认）**：分压比与阈值仍是**占位待标定**，且 GPIO34
测点**非必接线**——浮空/未标定下 ADC 可能读到近 0 的**伪欠压**，若直接接闸会把正常运动
误判打死（把能跑的板变砖）。故**未标定时只观测、不接闸**（档位照算照看、越阈照打站）；
真板接分压测点、与万用表对拍（≤2%）回填分压比与阈值后，置 `true` 接闸。**无真板证据前
不接闸、不 claim**。真板欠压注入验收（判据④堵转 UBEC≥4.7V）**待硬件**。

## 接线（与 0.0.7 实测接线零冲突）

| 外设 | 引脚 | 备注 |
|---|---|---|
| OLED SSD1306 0.96" | VCC→3V3 · GND→GND · SCL→GPIO22 · SDA→GPIO21 | I2C0 @100kHz，地址 0x3C（400k 在长跳线/弱上拉下时序边际，真板实录页写 ESP_FAIL，降 100k；50k 更差——老 I2C 驱动的低速档是反直觉的坏档） |
| 舵机 pan（水平） | 信号→GPIO32 · 电源→**板载 5V（现场档）** · GND→共地 | 正式装机应改 UBEC 独立 5V 轨（堵转 ~700mA/只），见上节代价 |
| 舵机 tilt（俯仰） | 信号→GPIO33 · 电源→**板载 5V（现场档）** · GND→共地 | UBEC/板/舵机单点共地（铁律） |
| UART2 指令面 | GPIO18→CP2102 RXD · GPIO19←CP2102 TXD · GND→共地 | 115200，3.3V TTL（严禁 5V）。**引脚定案（2026-10-12 真板实锤）：GPIO16/17 一装 UART2 驱动即把 I2C0 打死（换序/前后装均复现），故改 18/19**；19 与未来点阵 VSPI 预留同脚，已记为技术债 |
| 5V 轨分压测点（方案A Q1/Q2） | GPIO34 ← 5V 轨经分压网络（5V→≤3.3V） · GND→单点共地 | ADC1_CH6（input-only 档）；分压比/阈值在 `board.rs` 策略面（**占位待标定**，接线后对拍回填）。属观测面、**非必接线**——未接线时浮空读数**不接闸**（`RAIL_GATE_ARMED=false`），只观测不拒动 |
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
   for cmd in ['HELP', 'INFO', 'OLED HELLO ROCK3', 'FACE happy', 'PAN 60', 'PAN 120', 'PAN 90', 'TILT 98', 'SCAN']:
       s.write((cmd + '\n').encode()); time.sleep(1.2)
       print(cmd, '=>', s.read(2048).decode(errors='replace').strip())
   "
   ```

   先烧录后开指令面：烧录的 DTR/RTS 翻转会复位板子（0.0.7 实锤），指令面时序在烧录之后。

3. **验收判据**（全案 §4 批B 四件套）：
   - ① I2C 扫描 0x3C 应答：monitor 扫描表 0x3C 在列；
   - ② OLED 出字：`OLED <text>` 后目视内容与指令一致（另 `FONT` 指令打印 "ABC123"
     字模 hex 供人工比对护栏）；
   - ③ 舵机五点：`SCAN` 后两轴各自按**本轴行程**五等分点**串行**逐点目视一致、串口回读角度一致、
     无堵转抖振（pan 60/75/90/105/120、tilt 80/85/90/95/100；每点先 pan 到位驻留 600ms
     再 tilt 到位驻留 600ms——任一时刻只驱动一轴，削峰值电流；失败处置：堵转时供电轨输出须 ≥4.7V，仍异常单轴排除法）；
   - ④ 供电核验：正式档为舵机由 UBEC 5V 供电、UBEC/板/舵机单点共地（装配步目视 + 万用表）；
     **现场档**为板载 5V 直供，此时判据改为"表情与舵机动作期间 `resets=0 bod=0`"，
     并接受"舵机不保持激磁"的代价（见"供电分时仲裁"节）。

## 指令表（UART2 行协议，\n 结尾；批C 换能力帧时本面整体退役）

| 指令 | 效果 |
|---|---|
| `OLED <text>` | 三行显示（≤63 字符，21 列/行硬切 3 行），第 4 行为实时角度 |
| `FACE <id>` | 切表情胶囊眼（`idle`/`blink`/`talk`/`think`/`happy`/`listen`/`turn`），进入表情模式并停帧模式 |
| `PAN <deg>` / `TILT <deg>` | 单轴，**分轴行程内**（pan 60–120 / tilt 80–100，中位 90；越程 ERR 点名，fail-closed） |
| `SERVO <pan> <tilt>` | 双轴设定，**串行降级**：先 pan 到位 → 整定 200ms → 再 tilt（削峰值电流）；两轴均按 2°/20ms 缓速逼近 |
| `WANDER` / `WANDER STOP` | 开/停自主扫摆；开时两轴**错拍单动**（同拍只驱动一轴），端点=本轴行程边界 |
| `SCAN` | 两轴各自按本轴行程五等分点**串行**扫描（每点 pan→600ms→tilt→600ms，逐点回读）；控制面即刻 ack，逐点在运行面异步回包 |
| `RAIL` | 采一次 5V 轨压（方案A Q1 观测面）：控制面即刻 ack，运行面采一次经 outbox 异步回包 `RAIL rail=<mV> raw=<raw>`（真板分压测点接线后对拍标定用） |
| `INFO` | 回读当前角度 / 限幅区间 / **5V 轨最近采样（rail/raw/tier/low_events）** / OLED 最近文本 / 堆水位 |
| `FONT` | "ABC123" 字模 hex 自检护栏 |
| `HELP` | 指令表 |

未知指令一律 `ERR`（不猜不拖）。

**开机默认**：直接进 `idle` 胶囊眼表情 + 走一遍指定姿态 **pan=60（中位左转 30°）/
tilt=98（中位上抬 8°）**，队列序为"先表情上屏、后舵机动作"（可见性优先：板载 5V
下舵机启动是全板最大电流事件，万一触发 BOD 也要保住已出的表情）。UART2 链路现场
劣化时这是"看得见"的兜底。

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

## 现场注记（0.0.5 现场处置续批，2026-10-12 真板实测）

- **控制/运行分离已成**：boot SURFACE 行含 `planes=ctrl+runtime serial-servo`；`INFO`
  在运行面执行 `SCAN`（约 6s）期间仍即时应答（旧版在命令线程跑扫描是"卡顿"来源）。
- **OLED 表情不出的根因与修复（本批主交付）**：根因 = **舵机保持电流与 SSD1306 电荷泵
  抢同一 5V 轨**（见"供电分时仲裁"节）。修复 = 舵机让位 + I2C 强制重同步 + 亮度
  降级阶梯 + 整页重写。**真板复验（3 次 35–40s 采集）：`anim_fail=0 capsule_fail=0
  resets=0 bod=0`，boot 姿态 `姿态到位 pan=60 tilt=98（LEDC 回读 pan=60° tilt=98°）`**
  ——即"胶囊眼上屏"与"舵机转到指定角"两件事同时达成且板子不复位。
- **排查方法论（可复用）**：分辨"面板侧 vs 外设侧"要看三问——① 失败当下地址还 ACK
  吗；② 短命令笔与整页笔是否同命；③ 隔 1s 重走 `init` 能否救回。本批正是靠
  "逐轴掉 duty 试探"一步定死根因（掉 duty 后同一笔立刻 Ok）。
- **UART2 链路（0001）现场劣化**：Mac→板干净（命令被正确解析），但**板→Mac 回包前
  ~10 字节固定乱码**，且板子把自己的回包收进 RX（回 `ERR unknown cmd`，控制台见
  `收行失败: line too long`）——属**物理接线/线缆**问题（非固件）：需核查
  CP2102 TXD↔GPIO18 / RXD↔GPIO19 是否交叉、有无与 18 短接、共地是否可靠。
  判读回包时按需过滤前导乱码。以板载 1410 控制台日志为固件行为证据。
- **BOD 敞口**：板载 5V 直供下，舵机启动/整定偶发 `Brownout detector was triggered`
  （真板实录一次）。boot 已用"先表情后舵机"排序把可见性保住；根治仍需 UBEC 独立轨。
