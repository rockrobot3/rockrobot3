# rr-hal —— 硬件抽象层（仅 trait）

模块只见 trait、不见 Linux 细节（architecture.md §5）。命名规约：trait 带 `Rr3` 前缀、
方法不带，错误统一 `Rr3HalError`。

| 内容 | 位置 | 状态 |
|---|---|---|
| 5 个上行/控制 trait：`Rr3AudioSource`/`Rr3GpioOut`/`Rr3PwmServo`/`Rr3Power`/`Rr3Link` + 数据类型（`Rr3AudioCfg` 默认 16k/mono/16bit、`Rr3PcmStream`、`Rr3BatteryState`、`Rr3LinkState`） | `src/lib.rs` | 已在 HEAD |
| 占位后端 `Rr3UnavailableSource`（所有方法返回 `Unsupported`），object-safety 由 `lib.rs` 测试在 `cargo test` 时机器校验 | `src/lib.rs` | 已在 HEAD |
| 后端名常量与落地排期（`RR3_BACKEND_{LINUX,SIM,PLACEHOLDER}`） | `src/backend.rs` | 仅常量，**无实现文件** |
| linux 后端（ALSA/gpiod/NM-D-Bus）、sim 后端 | — | 规划 M2+/M3/M5 |
| 音频**输出** trait（rr3-interact 消费面） | — | 规划 M2+（当前只有上行） |

反射环（1k–10kHz 电机 FOC/PID）**不进这些 trait**，留在 rr3-act 进程内（ADR-0007 裁定 1）。
