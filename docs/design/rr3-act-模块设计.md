# rr3-act 模块设计

> 证据：`modules/rr3-act/module.toml`、`atoms/{servo,led,relay}/`、`src/{composite,intent,reflex}.rs`、
> `config.toml`、`rr-core/src/control.rs`、`rr-hal` 的 `rr3_hal_servo` / `rr3_hal_gpio_out(role)`。

## 1 职责

把角度 / 电平 / 通断三类**执行指令**落成动作，并把动作结果摘要上报；同时是**反射环的进程内宿主**
（1 kHz 闭环永不进总线，ADR-0007 裁定 1）。格账：`act.servo` + `act.led` + `act.relay`。

## 2 事实面

| 件 / 成分 | 格 / 面 | 机制公开面 | 现状平台耦合 |
|---|---|---|---|
| `servo` | `act.servo` | `rr3_servo_parse_args(args, cfg)` · `rr3_servo_state_body(seq, angle)` · 限速门真值在 `rr-core::control`（re-export） | `serde_json` |
| `led` | `act.led` | `rr3_led_parse_args`（白名单 on/off/blink）· `Rr3Led::rr3_led_new/apply` | **自持 `thread` + `mpsc` + `recv_timeout(3600s)`** + `rr3_block_on` + `Arc<dyn Rr3GpioOut>` |
| `relay` | `act.relay` | 通断裁决 + `relay/state` 变化即发（不做限速门） | `rr3_block_on` + `Arc<dyn Rr3GpioOut>` |
| `intent`（组合件成分） | — | 订阅 `rr3-brain/intent/result`，**两道归属门**（exposes 归属 + 意图面开放），越权拒执行折日志不断链 | — |
| `reflex`（组合件成分） | — | 1 kHz 环宿主 + 饱和窗（窗方案与检测器真值已迁 `rr-core::control`），发 `motor/reflex` 通告 | `Instant` |

参数面（`config.toml`）：`angle_limit_deg = [0, 180]`、`move_timeout_ms = 800`、
`state_min_interval_ms = 50`、`blink_period_ms = 500`、`loop_hz = 1000`、`saturate_notice_ms = 500`。
健康：心跳 1000 / 看门狗 5000 ms；`start_priority = 40`，`mem_limit_mb = 48`。
安全核接入：RPC 与 intent **两路落角前都过 `rr-core::control::rr3_travel_clamp`**。

**必须记账的三条实况**：① 限速门的时基取 `SystemTime` 墙钟（`composite.rs` 的 `rr3_now_ms`，
与 `rr-bus` 同口径），墙钟回拨会让 `servo/state` 在回拨时长内不再发布；② `loop_hz` 从 `u64`
截断成 `u32` 用裸 `as`，越界不报错；③ LED 的 blink 由件内线程翻转，不是被组合件节拍驱动。

## 3 件级切分与闭包（设计目标）

- `servo` 已近 L0（纯函数 + cfg 形参）：行程/时限/限速三参数就是 ops 槽的第一个真实样本。
- `led` 改造为**被节拍的纯状态机**：件出 `rr3_led_tick(now_ms, current) → Option<level>`，
  线程与 sleep 归组合件（rtos 档归任务、bare 档归定时器）。这是 ADR-0018 修订 1 ③ 接缝一的样板件。
- `relay` 去 `rr3_block_on`：句柄换 `rr-hal-api` 同步签名后即可下沉。
- 灯效白名单（on/off/blink）与"哪个效果对应哪种情绪"是**策略**：白名单是件的输入契约（可留），
  情绪映射不得进本单元（现状在 interact/brain，正确）。

## 4 职责四面

| 面 | 落点 |
|---|---|
| 接入 | 三 RPC（`servo/command`、`led/effect`、`relay/set`）+ `intent/result` 订阅 |
| 运行 | 落角 / 落电平 / 通断；反射环（本地，永不 `⊙`） |
| 状态 | `servo/state`（限速流）、`relay/state`（变化即发，RPC 与 intent 两路共用 `relay_seq` 序） |
| 守护 | **本单元是全项目安全核最密处**：限位钳位 + 时限（"超时即停，不给堵转留机会"）+ 饱和通告 |

## 5 时序设计（本单元是全项目唯一有硬实时诉求的）

| 量 | 设计值 / 状态 |
|---|---|
| 反射环名义周期 | `1000 Hz`（`loop_hz`），窗粒度 250 ms、饱和通告 500 ms（真值在 `rr-core::control`） |
| 心跳 / 看门狗 | 1000 / 5000 ms（申报面），与环**不同线程**——环饿死不影响心跳，故"活着但不动"可被漏检 |
| 待给而未给的量 | 环最坏执行时间、连续饱和判定的抖动预算、**安全动作端到端延迟上界**（从感知边沿到落角）。这三项是矩阵第⑦列的主要对象，本机无实测 |
| 时基 | 设计定为单调时基（心跳与限速都不该看墙上钟），现状为墙钟——列改造项 |

## 6 降级与缺格语义

- `act.servo` 缺 → 本单元仍可跑（led/relay 独立），但"能动的形态"不成立：装配单应把该形态的
  `present` 写清，而不是起一个动不了的 act。
- 句柄给了但值错（描述符键名拼错）→ 现状静默落占位后端、首次指令才回 `Unsupported`，
  设计要求改为 boot 期点名（批J）。
- `motor/reflex` 通告是**观察者面**，不参与仲裁：订阅者缺席也必须能发（notification 语义）。

## 7 红线

三 RPC 名与参数裁决文案（点名缺失字段是契约行为）、`servo/state` 的 seq 连贯（e2e 断言 RPC 路 seq=1 →
intent 路 seq=2）、`relay/state` 变化即发不做限速、SURFACE 自述、反射环永不进总线。
限位/时限/饱和三件的**语义**不可被策略改写（参数可注入，判据不可）。

## 8 待证与敞口

1. 真舵机 / 真继电器的动作证据未取（外设未采购）；sysfs PWM 与 chardev GPIO 只有代码 + 交叉编译证据。
2. 环的实时性从未在目标上量过——现在写"1 kHz"只是 config 里的一个数。
3. 墙钟时基与 `as u32` 两处修法属代码改动，本册只定方向与时机（批I）。
