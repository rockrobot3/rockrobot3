# rr3-act

执行模块：舵机、LED、反射环宿主进程。结构由 `rr3-mkmod` 生成
（[`conventions.md`](../../docs/process/conventions.md) §3）。

## 目录结构

```
modules/rr3-act/
├── module.toml
├── config.toml
├── Cargo.toml
└── src/
    ├── main.rs      ← 入口：总线接入、点名分发、config 解析（0.0.2 T4a 实装）
    ├── servo.rs     ← 舵机指令与状态摘要（经 rr-hal trait，限速流）
    ├── led.rs       ← 灯效（on/off/blink，worker 线程驱动翻转）
    ├── relay.rs     ← 继电器通断（Q1 立格 act.relay）
    └── reflex.rs    ← 1k–10kHz 反射环宿主，**进程内闭环**
```

## 总线面

| 方向 | 主题 | kind |
|---|---|---|
| rpc | `rr3-act/servo/command` | request |
| rpc | `rr3-act/led/effect` | request |
| rpc | `rr3-act/relay/set` | request |
| publish | `rr3-act/servo/state` | stream |
| publish | `rr3-act/motor/reflex` | notification |
| subscribe | `rr3-brain/intent/result` | event |

**反射环永不进总线**（ADR-0007 裁定 1）：`reflex.rs` 只允许在异常/饱和时发通告，
闭环数据本身不过 IPC。舵机指令走 <100Hz 事务面，不走事件面。

## 本地自检

```bash
cargo run --manifest-path modules/rr3-act/Cargo.toml --bin rr3-act
```

**交付到哪一批不在本文件登记**：GPIO/PWM 驱动（rr-hal linux 后端 + 设备树）与总线接入
今天有没有实现，唯一落点是 [`docs/status.md`](../../docs/status.md)；
门禁与用例条数见 [`docs/process/conventions.md`](../../docs/process/conventions.md) §2。
