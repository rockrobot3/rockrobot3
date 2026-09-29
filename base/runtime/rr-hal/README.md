# rr-hal —— 硬件抽象层（仅 trait）

模块只见 trait、不见 Linux 细节（[`architecture.md`](../../../docs/architecture/architecture.md) §5）。
命名规约：trait 带 `Rr3` 前缀、方法不带，错误统一 `Rr3HalError`。

| 内容 | 位置 |
|---|---|
| 5 个上行/控制 trait：`Rr3AudioSource` / `Rr3GpioOut` / `Rr3PwmServo` / `Rr3Power` / `Rr3Link`（全部 `async` + `Send + Sync`） | `src/lib.rs` |
| 数据类型：`Rr3AudioCfg`（样板常量 `RR3_AUDIO_CFG_16K_MONO16` = 16k/mono/16bit）、`Rr3PcmStream`（时长换算入口 `rr3_pcm_duration_ms`）、`Rr3BatteryState`、`Rr3LinkState`、错误枚举 `Rr3HalError` | `src/lib.rs` |
| 占位后端 `Rr3UnavailableSource`（所有方法返回 `Unsupported`），object-safety 由 `src/lib.rs` 的单元测试在 `cargo test` 时机器校验 | `src/lib.rs` |
| 后端名常量 `RR3_BACKEND_LINUX` / `RR3_BACKEND_SIM` / `RR3_BACKEND_PLACEHOLDER` | `src/backend.rs`（只有常量，无实现文件） |

具名后端（linux 的 ALSA / gpiod / NM-D-Bus 与 sim）以及音频**输出** trait（rr3-interact 的消费面，
当前只有上行）属 §5 规划的形状；它们今天有没有实现、缺哪一手，唯一落点是
[`docs/status.md`](../../../docs/status.md)。测试条数只登记在
[`docs/process/conventions.md`](../../../docs/process/conventions.md) §2。

反射环（1k–10kHz 电机 FOC/PID）**不进这些 trait**，留在 rr3-act 进程内（ADR-0007 裁定 1）。
