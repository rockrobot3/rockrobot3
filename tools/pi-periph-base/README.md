# pi-periph-base —— 树莓派 4B 板侧五端口后端

树莓派档的**板侧适配层**：std/Linux 单线程，把五端口（UART / I2C / PWM / ADS1115 / 单调时钟）
接成同一份帧契约的板侧半边。与 [`esp32-periph-base`](../esp32-periph-base/README.md) **同构**：
同一条字节契约（`rr-periph-proto`）、同一份机制件（`periph-mech` 的帧收发、OLED 编码、
舵机缓速与轨压判定），连舵机行程表与面目几何都用 `#[path]` 直接复用 ESP32 底座的 `atoms/`
源码——**同件复用，不抄第二份**；本 crate 只写端口实现与策略量。

## 两个控制面

| 面 | 形状 | 内容 |
|---|---|---|
| 帧面（产品面） | UART 上的 AiryIPC 帧，宿主桥同一条字节流 | `rr3.call` × 主题 `rr3-act/servo/command` / `rr3-interact/face/expression`；`rr3.set_bindings`（method 面）注入 `i2c_sda`/`i2c_scl`/`uart2_tx`/`uart2_rx` 四键，逐键对账**只计数不拒收**（`INFO` 的 `binding_mismatch`） |
| 行面（现场面） | 换行结尾的文本行 | `HELP` · `INFO` · `RAIL` · `OLED <text>` · `FACE <expr>` · `PAN <deg>` · `TILT <deg>` · `SERVO <pan> <tilt>` |

`WANDER` / `SCAN` / `AUDIO` 在 T3 档**明确回"不支持"**（本档无自主扫摆、无语音/DoA），
未知指令一律 `ERR`——不猜、不静默丢弃。

## 五端口

| 端口 | 设备 / 参数 | 策略量住哪 |
|---|---|---|
| UART | `/dev/serial0`（PL011）@ 1.5 Mbps；首字节超时 10 ms、行内字节间 50 ms、行长上限 128B、帧内无进展预算 2 拍 | `board.rs` `UART_*` / `*_TIMEOUT_MS` |
| I2C | `/dev/i2c-1`：SSD1306 0x3C 与 ADS1115 0x48 同总线；单笔重试 3 次、整页最多 3 轮、32B 分块 | `board.rs` `I2C_*` / `OLED_*` |
| PWM | `/sys/class/pwm/pwmchip0/{pwm0,pwm1}`（`dtoverlay=pwm-2chan`）：50 Hz / 20 ms 周期，14 位刻度 16384，行程对应 410–2048 tick（0.5–2.5 ms） | `board.rs` `PWM_*` / `DUTY_*` |
| ADC | ADS1115 A0 轨压：单端 ±4.096 V、128 SPS、单次转换（配置字 0x8383），等待 12 ms | `board.rs` `ADS1115_*` |
| 时基 | `CLOCK_MONOTONIC` 单调时钟（`Rr3ClockPort`） | `ports` |

舵机缓速（2°/20 ms）与"串行降级"由机制件给，本 crate 只提供 tick→纳秒换算
（`board::pwm_ticks_to_ns`）与 sysfs 写面——**两板的角→tick 表因此逐值相同**。

## 接线

| 信号 | Pi 40-pin | 备注 |
|---|---|---|
| OLED SDA / SCL | GPIO2 / GPIO3（I2C1） | 3V3 供电，与 ADS1115 同总线 |
| 板↔宿主 UART | GPIO14 / GPIO15（`/dev/serial0`） | 3.3V TTL，与宿主同为 1.5M 档 |
| 舵机 pan / tilt | PWM `pwm0` / `pwm1` | 信号 3.3V；舵机电源**走独立轨、单点共地**（与 ESP32 档同一供电纪律） |
| 5V 轨测点 | ADS1115 A0 ← 5V 轨经 2:1 分压（2×10 kΩ） | 观测面、非必接线；未标定不接闸（见下节） |

## 构建与试跑

```bash
cargo run --release --manifest-path tools/pi-periph-base/Cargo.toml
```

独立 workspace（`Cargo.toml` 自带空 `[workspace]`）：宿主即可编，Linux 设备面与纯逻辑
单测分离（macOS 侧可跑单测），门禁 ④/⑥ 照常扫本目录。启动先打
`READY periph-base(pi) v0.1.0 五端口（UART/I2C/PWM/ADS1115/CLOCK_MONOTONIC）`，
随后 `boot_surfaces()` 走一遍上电面：OLED 初始化 + `idle` 表情上屏 + 轨压首采
（器件缺席只点名，不 panic、不拦启动）。

设备与波特率可用环境变量应急覆盖：`RR3_PI_UART` / `RR3_PI_BAUD` / `RR3_PI_I2C_DEV` /
`RR3_PI_PWM_ROOT`；缺省即 `board.rs` 常数（**换板只改策略面，机制与协议字节零改动**）。

## 未收口（如实登记）

- **轨压标定未回填**：2026-11-05 实测所用 ADS1115 模块输入端不跟踪中点（RAIL 读数与万用表
  不符），故 `RAIL_GATE_ARMED = false`——**只观测不拒动**；换直入型模块后按 2:1 分压重测、
  回填分压比与双阈值，才武装闸门。
- **单线程的代价**：控制面与动作在同一线程串行，舵机缓速/整屏重写期间不读新帧（T3 档无
  语音、无并行音频，按此档位成立）；要并行需先把 I2C/PWM 让到独立工作线程。
- **行面不是契约**：它只是现场面（无宿主时点名、调试用），产品面是帧面；别在行面上开发依赖。
