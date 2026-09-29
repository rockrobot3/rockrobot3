# rr3-act

执行模块：舵机、LED、反射环宿主进程。骨架由 `rr3-mkmod` 生成（conventions §3）。

## 目录结构

```
modules/rr3-act/
├── module.toml
├── config.toml
├── Cargo.toml
└── src/
    ├── main.rs      ← 入口：打印自述与发布面，未接总线
    ├── servo.rs     ← 舵机指令与状态摘要（经 rr-hal trait）
    ├── led.rs       ← 灯效
    └── reflex.rs    ← 1k–10kHz 反射环，**进程内闭环**
```

## 总线面

| 方向 | 主题 | kind |
|---|---|---|
| rpc | `rr3-act/servo/command` | request |
| rpc | `rr3-act/led/effect` | request |
| publish | `rr3-act/servo/state` | stream |
| publish | `rr3-act/motor/reflex` | notification |
| subscribe | `rr3-brain/intent/result` | event |

**反射环永不进总线**（ADR-0007 裁定 1）：`reflex.rs` 只允许在异常/饱和时发通告，
闭环数据本身不过 IPC。舵机指令走 <100Hz 事务面，不走事件面。

## 实现进度

v0.0.1 交付结构 + manifest。GPIO/PWM 驱动随 M5（rr-hal linux 后端 + 设备树），
总线接入随 M2。本地自检：

```bash
cargo run --manifest-path modules/rr3-act/Cargo.toml --bin rr3-act
```
