# rr-hal —— 硬件抽象层

模块只见 trait、不见 Linux 细节（[`architecture.md`](../../../docs/architecture/architecture.md) §5）。
命名规约：trait 带 `Rr3` 前缀、方法不带，错误统一 `Rr3HalError`。

| 内容 | 位置 |
|---|---|
| 5 个上行/控制 trait：`Rr3AudioSource` / `Rr3AudioSink` / `Rr3GpioOut` / `Rr3PwmServo` / `Rr3Power` / `Rr3Link`（全部 `async` + `Send + Sync`） | `src/lib.rs` |
| 数据类型：`Rr3AudioCfg`（样板常量 `RR3_AUDIO_CFG_16K_MONO16` = 16k/mono/16bit）、`Rr3PcmStream`（时长换算入口 `rr3_pcm_duration_ms`）、`Rr3BatteryState`、`Rr3LinkState`、错误枚举 `Rr3HalError` | `src/lib.rs` |
| 同步执行入口 `rr3_block_on`（noop-waker，v0.0.1 不引 async 运行时的裁定，见 `src/lib.rs` 头注） | `src/lib.rs` |
| 占位后端 `Rr3UnavailableSource`（所有方法返回 `Unsupported`），object-safety 由 `src/lib.rs` 的单元测试在 `cargo test` 时机器校验 | `src/lib.rs` |
| sim 音频后端（`Rr3SimWave` 三模式：440Hz 正弦 / 确定性噪声 / 静音；虚拟麦克风 `Rr3SimAudioSource` + 尾窗虚拟扬声器 `Rr3SimAudioSink`） | `src/sim.rs` |
| linux（ALSA）音频后端（`Rr3AlsaAudioSource` / `Rr3AlsaAudioSink`，`alsa` feature + `target_os = "linux"` 双门，依赖 `alsa = "0.9"`） | `src/linux.rs` |
| 后端名常量与环境变量选择入口（`RR3_HAL_BACKEND` / `RR3_HAL_SIM_WAVE` / `RR3_HAL_ALSA_DEVICE`）：`rr3_hal_backend_resolve`、`rr3_hal_audio_source`、`rr3_hal_audio_sink` | `src/backend.rs` |

具名后端有哪几路、各自由什么承载，逐条见 `src/backend.rs` 顶部那张后端表（本文件不复述用词）。
TTS 文本到 PCM 的合成属模块侧（rr3-interact），不进 HAL。各后端今天能不能过真机验证、缺哪一手，
唯一落点是
[`docs/status.md`](../../../docs/status.md)。测试条数只登记在
[`docs/process/conventions.md`](../../../docs/process/conventions.md) §2。

反射环（1k–10kHz 电机 FOC/PID）**不进这些 trait**，留在 rr3-act 进程内（ADR-0007 裁定 1）。
