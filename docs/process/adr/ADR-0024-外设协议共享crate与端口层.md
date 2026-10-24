# ADR-0024：外设协议共享 crate 与固件端口层

- **状态**：已接受（用户 2026-10-24 口径：*"对于在 ESP32 上完成的所有任务，要进行足够的抽象化……
  作为一个标准化的通用底座，对已经踩过的坑进行优化，并且在模块当中进行抽象化的一个实现，
  要保证能复用，在包括树莓派等其他平台上也同样可用"*）。
  本 ADR 把"板/宿主各写一份帧协议"和"硬件动作散落各模块"两个复用障碍拆掉：
  协议真值收到一个**零平台依赖**的共享 crate，硬件动作收到**端口 trait**。
- **日期**：2026-10-24
- **关联**：[ADR-0017](ADR-0017-模块内部分层-机制与策略分离.md)（机制/策略分离）·
  [ADR-0018](ADR-0018-跨操作系统移植-机制核与主承载层分离.md)（机制核与承载层分离——本 ADR 是
  同一思想在**板侧**的落点）· [ADR-0019](ADR-0019-原子功能模块模型-格件面与组合代数.md)（原子件）·
  [ADR-0023](ADR-0023-LLM连接语义与模型行写面.md)（同批 LLM 连接面）

## 背景

三类事实，都是"同一件事存在两份实现"：

1. **帧协议真值双份**：宿主的 `rr-hal::esp32_uart`/`esp32audio` 与 xtensa 固件的 `frame.rs`/`board.rs`
   各自维护 task 位形、magic、主题串、请求/应答组帧与板帧分类。两边靠"注释同值 + 各自测试"
   维持一致——一次主题改名就可能只改一边，线上表现为"帧到了但被拒"。
2. **硬件动作无接缝**：UART 收帧、LEDC 设占空、ADC 采样、I2C 页写直接吃 `esp-idf-hal` 驱动类型，
   机制函数签名与板绑死。换平台（树莓派 GPIO/PWM、S3、仿真）必须逐函数改签名，
   真板踩过的坑（重试/自愈/降亮/缓速步进）也无法整块搬走。
3. **踩过的坑没有落成"底座能力"**：I2C 丢拍自愈、整页重写、降亮阶梯、舵机缓速、串行降级、
   TX 环背压等，都是**机制**（与板无关），却长在板专属文件里，复用时只能复制。

## 决策

### 裁定 1：新建 `base/runtime/rr-periph-proto`（协议机制唯一实现）

- 路径 `base/runtime/rr-periph-proto/`，lib 名 `rr_periph_proto`；依赖**仅** `rr-ipc` + `serde_json`，
  **禁止任何 `esp-idf-*`**——宿主（rr-hal）与 xtensa 固件都要引用它，依赖方向必须双向可编译。
- 模块与职责：

  | 模块 | 内容 |
  |---|---|
  | `consts.rs` | task 位形、`RR3_SERVO_TOPIC`/`RR3_FACE_TOPIC`、magic 首字节、桥波特率、宿主 env 名 |
  | `request.rs` | `Rr3PeriphRequest` + 帧→请求解析（dst 门→payload 前缀门→JSON 门→id 门） |
  | `response.rs` | 应答组帧 `rr3_periph_response_frame` + 宿主解析 `rr3_bridge_parse_response` |
  | `host_call.rs` | `rr3_bridge_request_body_params/request_frame/call_frame`（自 rr-hal 平迁，函数名不变） |
  | `cmd.rs` | `Rr3PeriphCmd`、`rr3_esp32_cmd_frame`、板帧分类 `rr3_board_frame_classify` |

- **两端只 `pub use`，不写第二份**：`rr_hal::esp32_uart::*`、`rr_hal::esp32audio::Rr3PeriphCmd`
  等既有路径与测试零改动；固件 `board.rs` 只 re-export 并加**编译期同值断言**钉住生成物
  `topics_gen.rs` ⇄ crate 常量（契约改串而未同步常量即编译红）。
- **策略不外溢**：音频载荷四道门与解码、行程/步进/栈/分片大小等策略量仍留各自消费面
  （rr-hal / 固件 `board.rs`）；crate 只收"宿主与固件必须逐字节一致"的协议机制。

### 裁定 2：固件端口层五 trait（`tools/esp32-periph-base/src/ports.rs`）

| trait | 方法 | 语义 |
|---|---|---|
| `Rr3UartPort` | 继承 `Rr3ByteStream`（阻塞读 / 整段写）+ 别名 `rr3_uart_write_all` | 帧面 io；`Send` 是 rr-ipc 传输契约前置 |
| `Rr3I2cPort` | `rr3_i2c_write(addr, bytes)` | **单笔**事务（START-addr-bytes-STOP） |
| `Rr3PwmPort` | `rr3_pwm_duty(channel, duty)` | 一路占空（0 = 释放） |
| `Rr3AdcPort` | `rr3_adc_read_mv(pin)` | 一次测点读数；`pin` 与装配锚不符即点名拒 |
| `Rr3ClockPort` | `rr3_now_ns()` | 帧头 timestamp（单调、epoch 无关） |

**边界纪律**：

- 端口只露**事务级**动作；重试、总线自愈（GPIO 位打）、整页重写、降亮阶梯、缓速步进、
  串行降级全部留在上层机制件——**端口不做策略**。
- I2C 总线自愈操作的是**引脚**而非事务，故不进 `Rr3I2cPort`，仍由 `oled::i2c_bus_recover` 承担。
- LEDC **回读**（`get_duty`）是"硬件侧证据"诊断面，不抽象成端口方法（各平台回读语义不一，
  强行统一会失真）；执行面走端口，观测面走具名驱动。

### 裁定 3：迁移顺序与范围

1. **UART**：`frame_recv`/`frame_send_response` 泛型化到 `impl Rr3UartPort`，时钟由
   `impl Rr3ClockPort` 注入；
2. **PWM+ADC**：`RtPeriph::servo_set`/`servo_release`/`rail_sample` 走端口；
3. **I2C**：OLED 全部写入收拢到 `i2c_write_retry`/`i2c_scan` 一处端口调用；
4. **I2S/麦克风本轮不抽象**：采集面与音频策略强耦合，登记为后续 `Rr3AudioPort`。

行面（`reply`/`read_line`/`drain_audio`）继续直用 `UartDriver` 固有方法：它是**调试面**，
不经帧语义；其中整段写出已改用端口别名，帧面/音频面同一写语义。

### 裁定 4：树莓派后端留待后续批

`rr-hal` 已是宿主实现层（linux/macos/sim 后端齐备），本批**不做**树莓派板侧端口实现。
端口 trait 定义即接缝契约：后续 Pi 后端按同一五 trait 写一份实现，上层机制件零改动。

## 登记（写进代码注释与测试）

1. **波特率 cfg 双定义**：`rr_ipc::RR3_UART_BAUD_DEFAULT`/`rr3_uart_baud_speed` 受
   `cfg(any(linux, macos))` 门限制（espidf 目标不可见），故 `RR3_BRIDGE_BAUD` 在 crate 内按
   `cfg` 双定义（宿主档取 rr-ipc 真值，板档 115_200）。消费面不得据此推断板档运行波特率
   （固件自有策略常量 `UART2_BAUD_HZ`）。
2. **生成物 ⇄ crate 常量编译期断言**：`board.rs` 的 `rr3_str_eq` + `const _: () = assert!(...)`
   把主题串两处副本钉死；⑨-d 只管"生成物⇄契约"，本断言补另一边。
3. **平迁不改函数名/文案**：`cmds`/`host_call` 的公开函数名、协议门错误文案逐字保留，
   宿主 `rr3_bridge_parse_response` 解析契约与既有测试零改动。
4. **`Rr3PwmChannel` 是端口级轴名**，不绑引脚号；引脚真值仍在装配面与注入对账面。
5. **`Rr3AdcPort::pin` 是对账参数**：实现拒收与已装配测点不符的引脚（不冒领读数）；
   换测点须改具名装配，而不是传一个数字。

## 后果

- 协议改名/组帧调整只改 crate 一处，宿主与固件同时生效；漂移从"线上故障"变成"编译红"。
- 换平台 = 另写一份五 trait 实现；帧面、舵机机制、OLED 渲染、轨压判定等机制件零改动。
- 真板踩过的坑以**机制形态**留在层内（重试/自愈/降亮/缓速），不随平台重写。
- 遗留：I2S 未抽象；LEDC 回读仍走具名驱动；树莓派后端未实现。

## 回写清单

- `base/runtime/rr-periph-proto/`（新 crate：`consts/request/response/host_call/cmd`）+
  `base/runtime/Cargo.toml` members；
- `base/runtime/rr-hal`：`Cargo.toml`、`src/esp32_uart.rs`、`src/esp32audio.rs`（改 `pub use`）；
- `tools/esp32-periph-base`：`Cargo.toml`、`src/board.rs`、`src/frame.rs`、`src/ports.rs`（新）、
  `src/periph.rs`、`src/oled.rs`、`src/state.rs`、`src/commands.rs`、`src/main.rs`。
