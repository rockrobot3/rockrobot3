# rr3-act

执行模块：舵机、LED、反射环宿主进程。结构由 `rr3-mkmod` 生成
（[`conventions.md`](../../docs/process/conventions.md) §3）。

## 目录结构

```
modules/rr3-act/
├── module.toml
├── config.toml
├── composite.toml   ← 组合件装配清单（⊗ 链：servo ⊗ led ⊗ relay）
├── Cargo.toml
├── atoms/           ← 原子件（每格一件，五件套：atom.toml + 入口 + mech + assemble.toml + private.rs）
│   ├── servo/       ← act.servo：舵机指令与状态摘要（经 rr-hal 同步 trait，限速流）
│   ├── led/         ← act.led：灯效（on/off/blink；组合件节拍驱动的 rr3_led_tick 状态机——批I 去线程）
│   └── relay/       ← act.relay：继电器通断（Q1 立格 act.relay）与通断状态摘要
└── src/
    ├── main.rs      ← 入口：只装配与启动（≤80 行）
    ├── composite.rs ← 组合件：config 解析 + HAL 三面探活 + 点名分发 + intent 接入编排
    ├── intent.rs    ← intent/result 订阅执行面（0.0.3 T3：两道归属门 + relay 执行）
    └── reflex.rs    ← 1k–10kHz 反射环宿主，**进程内闭环**
```

## 总线面

| 方向 | 主题 | kind |
|---|---|---|
| rpc | `rr3-act/servo/command` | request |
| rpc | `rr3-act/led/effect` | request |
| rpc | `rr3-act/relay/set` | request |
| publish | `rr3-act/servo/state` | stream |
| publish | `rr3-act/relay/state` | event（变化即发，RPC 与 intent 两条驱动路共用） |
| publish | `rr3-act/motor/reflex` | notification |
| subscribe | `rr3-brain/intent/result` | event（actions 两道归属门执行，0.0.3 T3） |

**反射环永不进总线**（ADR-0007 裁定 1）：`reflex.rs` 只允许在异常/饱和时发通告，
闭环数据本身不过 IPC。舵机指令走 <100Hz 事务面，不走事件面。

## 心跳自报计数（0.0.4 批B，Q1 裁定）

`bus.rr3_counter_bump` 记账，随 `rr3.heartbeat` 的可选 `counters` 字段上行，rrd 按
`rr3-act/<键>` 归并（`rr3.ctl.counters` 可查）。本模块的键：

| 键 | 记账点 |
|---|---|
| `relay_on` / `relay_off` | `relay.rs 的 rr3_relay_counter_key`（字面量唯一落点）；两条驱动路（RPC / intent）在电平**真变**（`rr3_relay_state_due` 判定，0.0.5 批I A-7）且 publish 之前各记一次——同态命令不发布也不计数，键值 = 实际翻转数 |
| `errors` | 通用可选键（各模块共用词表），异常路径按需自报 |

## 本地自检

```bash
cargo run --manifest-path modules/rr3-act/Cargo.toml --bin rr3-act
```

**交付到哪一批不在本文件登记**：GPIO/PWM 驱动（rr-hal linux 后端 + 设备树）与总线接入
今天有没有实现，唯一落点是 [`docs/status.md`](../../docs/status.md)；
门禁与用例条数见 [`docs/process/conventions.md`](../../docs/process/conventions.md) §2。
