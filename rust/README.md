# RockRobot3 Rust 模块工程（esp-hal）

新模块用 Rust 开发：屏幕（表情）、身体（舵机）、外接传感器。每个模块是 `src/bin/` 下一个独立程序，单独编译烧录，互不影响主系统固件（`../firmware-c/`）。

## 硬件规划

| 模块 | 硬件 | 引脚 | 状态 |
|------|------|------|------|
| 工具链验证 | 板载 LED | GPIO2（blink） | 待开发 |
| screen 屏幕 | 0.96" I2C OLED (SSD1306) | GPIO32=SCL, GPIO33=SDA | 待买屏 |
| body 身体 | SG90 舵机 ×1~2 | GPIO15（或 22/23） | 待买舵机 |
| sensor 传感器 | DHT11 温湿度 / 光敏 | DHT11→GPIO27, 光敏→GPIO36(ADC) | 待买 |

引脚避让：GPIO4/5/18/25 被主系统音频占用，GPIO21 被继电器占用，GPIO6/7/8 闪存禁用。

## 开发流程（每个模块）

```bash
# 一次性：安装 Rust ESP32 工具链
cargo install espup && espup install

cd rust
cargo build --release --bin blink          # 编译
espflash flash --speed 460800 target/xtensa-esp32-none-elf/release/blink -p /dev/tty.usbserial-1410   # 烧录
espflash monitor -p /dev/tty.usbserial-1410   # 串口（115200）
```

## 模块清单（按顺序实现）

- [ ] `blink.rs` — 工具链验证：LED 闪烁 + 串口 hello
- [ ] `screen_oled.rs` — OLED 画点/画线/文字，静态表情
- [ ] `screen_face.rs` — 表情系统：待机眨眼 / 说话 / 思考 / 开心
- [ ] `body_servo.rs` — 舵机 0/90/180° 扫动，点头动作
- [ ] `sensor_dht11.rs` — 温湿度读取，串口打印
