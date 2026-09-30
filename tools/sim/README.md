# tools/sim —— `rr3-sim` x86 全链路联调入口

规划（[`architecture.md`](../../docs/architecture/architecture.md) §3 与 §5 的 `sim` 后端）：
mock HAL 设备源 + 假模块，让整条链路在没有 RK3566 板子的机器上跑起来。
**当前只做一件事**：用 `rr-hal` 的真实数据类型
产出一段模拟拾音流并报告时长，证明"sim 与 linux 后端交出同一种流"这条不变量有落点。

    RR3_SIM_FRAMES=1600 cargo run -q --manifest-path tools/sim/Cargo.toml --bin rr3-sim
    # rr3-sim: backend=sim, 1600 帧 -> 100ms, 目标总线 /var/run/rock3-ipc.sock

`backend=` 打印的是 `rr-hal` 的 `RR3_BACKEND_SIM` 常量、时长换算调的是
`Rr3PcmStream::rr3_pcm_duration_ms()`——sim 不算第二遍，也不自己写一遍后端名。
**M3 之后波形注入不再是本工具的欠账**：正弦/噪声/静音三模式由 `rr-hal` 的 sim
后端（`rr-hal/src/sim.rs`）承担，`rr3-sense` 经 `RR3_HAL_BACKEND` 直接用；
模块与总线之间的真注册/心跳/发布由 `rr3-sense` / `rr3-interact` 真入口 +
rrd 的 boot spawn 承担（见 modules/ 两份 README）。本工具今天还差什么，见
[`docs/status.md`](../../docs/status.md)，本文件不记进度。

独立工程：自带空 `[workspace]`，路径依赖 `rr-hal` + `rr-ipc`。
测试：`cargo test --manifest-path tools/sim/Cargo.toml`；编译由 check.sh 第 ④ 步覆盖。
