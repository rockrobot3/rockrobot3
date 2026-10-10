# tools/sim —— `rr3-sim` x86 全链路联调入口

规划（[`architecture.md`](../../docs/architecture/architecture.md) §3 与 §5 的 `sim` 后端）：
mock HAL 设备源 + 假模块，让整条链路在没有 RK3566 板子的机器上跑起来。
F4 起本入口是**脚本化全链路演练**：全部真进程、真套接字、真契约，一条命令走完——

    cargo build -p rrd（base/runtime/ 下，产出 rrd + sim_peer）
    cargo build --manifest-path tools/rrctl/Cargo.toml
    cargo build --manifest-path tools/sim/Cargo.toml
    ./tools/sim/target/debug/rr3-sim            # 仓内任意目录可跑；--keep 保留现场

演练脚本五步：

1. **起 rrd**：`--manifest` 带 `rr3-interact`（boot spawn，`resident` 常驻喂狗）；
2. **等就绪**：轮询 `rr3-ctl status`，直到 rr3-interact 到 `running`；
3. **事件流转**：外部起第二个真模块 `rr3-sense`（sim_peer + `publish_once`），
   发布契约主题 `rr3-sense/audio/vad_start` 后退场（C14①：asr/text 走 retired 后
   演练主题换 vad_start，发布方身份随之换位——owner 前缀归属要求发布方叫 rr3-sense）；
4. **rr3-ctl 四命令巡检**：status（断言 `denied_publish = 0`——发布被拒在此现形）、
   health、logs（rrd 日志环）、ota（恒拒随 M6，预期非 0 退出）；
5. **报告 + 收尾**：杀 rrd（resident 喂狗失败自退）；`--keep` 保留现场人工排查。

两个真模块的身份用的是**契约 owner 的真名**（rr3-sense / rr3-interact），所以发布
主题的前缀归属校验、契约主题集加载都走生产路径；有一处**不作死断言**：publish_once
退场不排空接收队列，未读广播帧会把 FIN 变 RST（`sim_peer.rs` abort 支的注释记了同一
形状），故 B 的终相在 `unregistered` / `failed` 两可，演练只断言它不再 `running`。

参数：`--socket <路径>`、`--repo <仓根>`、`--rrd/--sim-peer/--rrctl <二进制>`、`--keep`。
缺省值按仓内 `target/debug` 布局推；二进制缺失时报错点名对应的 `cargo build` 命令。

另有两个与两面演练互斥的模式（同给即拒）：

- **`--brain`**（M4，ADR-0014 裁定 4）：rrd + rr3-sense(sim) + rr3-brain + rr3-interact
  四真进程全语音对话闭环——云端走 mock（`RR3_BRAIN_CLOUD=mock`），闭环里只有云端
  socket 一跳是假的。判据：brain 日志出现「对话轮完成」+ interact 播放应答 +
  `denied_publish = 0`。
- **`--profile <装配单>`**（D9 装配演练，`assemble.rs`）：装配单驱动 rrd 装配链正门
  （`--profile` + `--contracts`，与 rrd.service 的 ExecStart 同形）boot 全部 face 模块。
  演练 modules-root 是**装机布局**：仓源 manifest 原文只把 entry 改写为产物绝对路径
  （rr3-pack 装机改写同语义同三道门，"entry 在仓相对、装机绝对"的第三面）+ config
  同目录就位（Rust 模块用仓源 config，brain 用 mock 演练 config）。判据：全部 face
  模块真进程 running + `denied_publish = 0` + 四命令巡检。python 模块产物 = shim
  （`python3 -m`），运行门槛 stdlib tomllib（Python ≥3.11）——不满足的 python3 会被
  `which_python3` 能力验证当场点名（fail-fast，不空等超时）。macOS 本机 3.9 起不了
  brain，六模块判据归 CI `sim-assemble` job 的远端回执；本机可用无 brain 子集装配单
  验证同一机制。CI 入口 `bash tools/ci/sim-assemble.sh`（构建 + 运行同一脚本）。

sim 设备源（`devices.rs`，`rr-hal` 的 sim 后端消费方）保留：报告里带一段静音流的时长
换算（`Rr3PcmStream::rr3_pcm_duration_ms()`——sim 不算第二遍），钉住"sim 与 linux
后端交出同一种流"这条不变量。波形注入（正弦/噪声/静音）在 `rr-hal/src/sim.rs`。

独立工程：自带空 `[workspace]`，路径依赖 `rr-hal` + `rr-ipc`。
单测（参数解析 / 仓根搜索 / manifest 文本 / status 表解析 / 注入式轮询）全部平台无关，
与编排（Unix-only）分居 `drill.rs` / `main.rs`；测试
`cargo test --manifest-path tools/sim/Cargo.toml`，编译由 check.sh 第 ④ 步覆盖。
本文件不记进度，欠账见 [`docs/status.md`](../../docs/status.md)。
