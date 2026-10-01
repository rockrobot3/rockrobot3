# tools/sim —— `rr3-sim` x86 全链路联调入口

规划（[`architecture.md`](../../docs/architecture/architecture.md) §3 与 §5 的 `sim` 后端）：
mock HAL 设备源 + 假模块，让整条链路在没有 RK3566 板子的机器上跑起来。
F4 起本入口是**脚本化全链路演练**：全部真进程、真套接字、真契约，一条命令走完——

    cargo build -p rrd（base/runtime/ 下，产出 rrd + sim_peer）
    cargo build --manifest-path tools/rrctl/Cargo.toml
    cargo build --manifest-path tools/sim/Cargo.toml
    ./tools/sim/target/debug/rr3-sim            # 仓内任意目录可跑；--keep 保留现场

演练脚本五步：

1. **起 rrd**：`--manifest` 带 `rr3-sense`（boot spawn，`resident` 常驻喂狗）；
2. **等就绪**：轮询 `rr3-ctl status`，直到 rr3-sense 到 `running`；
3. **事件流转**：外部起第二个真模块 `rr3-interact`（sim_peer + `publish_once`），
   发布契约主题 `rr3-interact/asr/text` 后退场；
4. **rr3-ctl 四命令巡检**：status（断言 `denied_publish = 0`——发布被拒在此现形）、
   health、logs（rrd 日志环）、ota（恒拒随 M6，预期非 0 退出）；
5. **报告 + 收尾**：杀 rrd（resident 喂狗失败自退）；`--keep` 保留现场人工排查。

两个真模块的身份用的是**契约 owner 的真名**（rr3-sense / rr3-interact），所以发布
主题的前缀归属校验、契约主题集加载都走生产路径；有一处**不作死断言**：publish_once
退场不排空接收队列，未读广播帧会把 FIN 变 RST（`sim_peer.rs` abort 支的注释记了同一
形状），故 B 的终相在 `unregistered` / `failed` 两可，演练只断言它不再 `running`。

参数：`--socket <路径>`、`--repo <仓根>`、`--rrd/--sim-peer/--rrctl <二进制>`、`--keep`。
缺省值按仓内 `target/debug` 布局推；二进制缺失时报错点名对应的 `cargo build` 命令。

sim 设备源（`devices.rs`，`rr-hal` 的 sim 后端消费方）保留：报告里带一段静音流的时长
换算（`Rr3PcmStream::rr3_pcm_duration_ms()`——sim 不算第二遍），钉住"sim 与 linux
后端交出同一种流"这条不变量。波形注入（正弦/噪声/静音）在 `rr-hal/src/sim.rs`。

独立工程：自带空 `[workspace]`，路径依赖 `rr-hal` + `rr-ipc`。
单测（参数解析 / 仓根搜索 / manifest 文本 / status 表解析 / 注入式轮询）全部平台无关，
与编排（Unix-only）分居 `drill.rs` / `main.rs`；测试
`cargo test --manifest-path tools/sim/Cargo.toml`，编译由 check.sh 第 ④ 步覆盖。
本文件不记进度，欠账见 [`docs/status.md`](../../docs/status.md)。
