# rr3-sense 模块设计（框架 v1）

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程（host）；`restart_policy = "always"`、`mem_limit_mb = 64` | `module.toml` |
| 格 | `audio.capture` · `audio.vad` · `sensor.env`（占位）· `sensor.imu`（占位） | `contracts/slots.toml`、`atoms/*/atom.toml` |
| 件 | `capture` · `vad` · `env` · `imu`；组合件成分：采集主循环 + 配置解析（`src/composite.rs`） | `atoms/` |
| 主题 | 发布 `audio/pcm`(stream)、`audio/vad_start`(event)、`env/temperature`、`imu/state`（后两条**无生产者**）；RPC `capture/dump`；订阅面为空 | `module.toml` |
| 句柄 | `rr3_hal_audio_source()` → `dyn Rr3AudioSource`（`open(cfg)` / `read(frames)`） | `src/composite.rs` |
| 参数 | `sample_rate=16000`、`channels=1`、`frame_ms=20`、`rms_start=1500`、`silence_tail_ms=700`、`min_speech_ms=200` | `config.toml`，**仅当 `RR3_SENSE_CONFIG` 已设时才读** |
| 健康 | 心跳 1000 / 看门狗 5000 ms | `[health]` |

## §1 件契约表

| 件 | 公开符号（现状） | 输入 → 输出 | 状态 | 失败语义 |
|---|---|---|---|---|
| capture | `rr3_frames_per_tick(sample_rate_hz: u32, frame_ms: u64) -> usize` | 率 + 拍长 → 每拍帧数 | 无 | `rate=0` 或 `frame_ms=0` → 0 帧（**语义未定**，§6） |
| capture | `rr3_pcm_downmix_mono(samples: &[i16], channels: u8) -> Vec<i16>` | 多声道样点 → 单声道 | 无 | 非整除交错 → 现状截断（历史缺陷已修，见 mono golden） |
| capture | `rr3_pcm_downmix_stream(pcm: Rr3PcmStream) -> Rr3PcmStream` | 同上（含 cfg） | 无 | — |
| capture | `rr3_pcm_event_body(seq: u64, pcm: &Rr3PcmStream) -> Value` | 序号 + 流 → 载荷（`pcm_b64` + `ms`） | 无 | — |
| capture | `rr3_doa_fix(samples, channels) -> Option<Rr3DoaFix>` · `rr3_doa_field(&Rr3DoaFix) -> Value` · `Rr3DoaBin` · `RR3_DOA_BIN_THRESHOLD_DEG: i32 = 30` | 双麦样点 → 方位与三档 | 无 | 单声道/能量不足 → `None`（不发 doa 字段） |
| vad | `rr3_frame_rms(samples: &[i16]) -> i64` | 一帧 → 能量 | 无 | — |
| vad | `Rr3VadState::rr3_vad_new(cfg: Rr3VadConfig, frame_ms: u64) -> Self` | 阈值组 + 拍长 → 状态机 | 见 §2 | — |
| vad | `rr3_vad_feed(&mut self, samples: &[i16]) -> Option<Rr3VadEdge>` | 每拍一样点 → 边沿（只发边沿） | `Rr3VadEdge{Start, End}` | — |
| vad | `rr3_vad_event_body(seq: u64, edge: Rr3VadEdge, doa: Option<Value>) -> Value` | 序号 + 边沿 + 可选 doa → 载荷 | 无 | — |
| env / imu | **无公开机制面**（`mech.rs` 各 2 行注释；入口只导出 `SURFACE`） | — | — | — |
| 目标签名 | 全部 `Value` → `Vec<u8>`（走 `rr3_body_encode`）；`Vec<i16>` → `&[i16]` 借用；`Rr3PcmStream`/`Rr3AudioCfg` → `rr-hal-api` | | | |

## §2 状态机（VAD，`Rr3VadState`）

不变式：**只发边沿**（`Start`/`End`），持续高电平期间不续发；起音需连续满足 `rms_start` 达
`min_speech_ms`；判停需连续静音 `silence_tail_ms`；所有窗长以 `frame_ms` 为粒度换算成拍数。
非法输入收敛方向：`rms` 读数异常（帧空）→ 不改变状态、不发边沿。
`DoaBin` 三档：`θ < -30` = 左、`-30 ≤ θ ≤ 30` = 中、`θ > 30` = 右（阈值是**策略**，见 §3）。
`env`/`imu` 无状态机——**无机制可写**，此事实必须显式留在表里而不是省掉。

## §3 注入面

| 件 | `handles` | `ops` 槽（名 / 类型 / 来源 / 默认） | 非法收敛（目标） |
|---|---|---|---|
| capture | `dyn Rr3AudioSource` ← `[hardware].alsa_device` | `sample_rate: u32`=16000；`channels: u8`=1；`frame_ms: u64`=20 | `frame_ms < 1` → 收敛为 20 并留痕（**现状：允许 0**） |
| vad | 无（吃 capture 输出） | `rms_start: i64`=1500；`silence_tail_ms: u64`=700；`min_speech_ms: u64`=200 | 阈值 ≤0 → 拒 boot |
| capture（DoA） | 阵列几何（`channels ≥ 2`） | `doa_bin_threshold_deg: i32` **=30 现状硬编码在件内** → 迁 ops | 几何不足 → 不发 doa |
| env / imu | 真件句柄（`[hardware]` 尚无对应键） | 待定（随真件批） | — |

## §4 主循环与时序预算

`let tick = Duration::from_millis(cfg.audio.frame_ms)`；每拍以 `Instant::now()` 计时（**单调，正确**）。
两种后端同一形状：sim 的 `read` 即时返回、ALSA 自耗一拍 → **pacing 由本单元自持**。

| 输出 | 名义周期 | 最坏延迟 | 备注 |
|---|---|---|---|
| `audio/pcm` | 每拍（20 ms） | 一拍 + 一次 publish | 与 brain 的消费速率耦合（brain 判停也用 20 ms 粒度） |
| `audio/vad_start` | 边沿 | 起音判定 = `min_speech_ms`；判停 = `silence_tail_ms` | 端到端"说完 → brain 知道"≈ 700 ms + 一拍（推算，未实测） |
| `capture/dump` | 点名 | 同步 | — |
| 饿死可见性 | 采集阻塞（ALSA 卡）则 pcm 停而心跳仍续 → **同 act 的"活着但不动"问题**；s-3 工单补 counters | | |

## §5 主题面契约

| 主题 | kind | seq | 触发 | 载荷必填 | 现状与申报差 |
|---|---|---|---|---|---|
| `audio/pcm` | stream | 自增 | 每拍 | `seq, pcm_b64, ms` | 一致（brain 真实消费但未申报） |
| `audio/vad_start` | event | 自增 | 边沿 | `seq, edge`（+ 可选 `doa`） | 一致 |
| `env/temperature` | event | — | 应为周期 | schema 在册 | **无生产者**（占位件） |
| `imu/state` | stream | — | 应为周期 | 在册 | **无生产者**（占位件） |
| `capture/dump` | RPC | — | 点名 | 帧缓冲摘要 | 一致 |

## §6 降级矩阵

| 格 | 缺格 | 句柄缺 | 句柄值错 | 后端未实装 | 参数非法 |
|---|---|---|---|---|---|
| `audio.capture` | 语音链整体不成立，下游（brain 的 asr）应 `Degraded` | placeholder → 使用时 `Unsupported` 显式回错（不静默落 sim） | 描述符作废且静默（批J 修） | 板上 ALSA 路径未实测 | `frame_ms=0` → 全速空转（缺陷，工单 S-1） |
| `audio.vad` | pcm 有而边沿无 → brain 只能靠时长上限判停（`max_utterance_ms` 兜底仍在） | 无独立句柄 | — | — | 阈值 ≤0 |
| `sensor.env` / `sensor.imu` | 只停本流 | — | — | 真件未到 | — |
| 同源约束 | capture 缺则 vad 必缺（vad 的输入就是 capture 的产出）——**注入校验要一起报，不得分开** | | | | |

## §7 安全核接入

**无安全核**：本单元的失效不会造成机械/电气危险（与 act/power 不同）。唯一可主张的守护语义是
"读不到就不发"（不发假帧、不发假边沿），它由 §2 的收敛方向承担，**不新立 trait、不改机制核**。

## §8 两腿对拍用例设计

| 件 | 输入序列 | 期望值来源 | 边界集 | 现状 |
|---|---|---|---|---|
| capture 帧数 | `(16000,20) (48000,10) (16000,0) (0,20)` | 公式 `rate×ms/1000` + "零值不得产 0 帧循环"的新裁定 | 0 拍长、0 率 | ✅ 有正例；⚠️ 零值无断言 |
| capture 下混 | 交错样点 `[L,R,L,R…]` 长度非整除 | mono golden 向量（`contracts/golden/`）+ 逐字节 pcm 事件体 | 奇数样点、单声道直通 | ✅（含历史缺陷回归） |
| vad | rms 序列 `[0,1600,1600,1600,0,0,0,0]`，frame_ms=20 | §2 裁定语义（起音需 `min_speech_ms`、判停需 `silence_tail_ms`、只发边沿） | 恰达阈 / 差 1 / 连续高电平 100 拍只发 1 边沿 | ✅ 件内有边沿用例；⚠️ 需补"持续高电平只发一次"的长序列断言 |
| DoA | 左/中/右三组样点 + 单声道 | 三档边界 `-30/30` 等号归属 | 等号、None | ⚠️ 需补等号边界（30° 恰好落哪档） |
| env/imu | — | — | — | **不适用**（格标记位 = 占位，ADR-0019 修订 2 ③） |

## §9 不变量与红线断言

- 断言：`vad_start` 只发边沿——任意持续语音段内事件数 = 1。
- 断言：pcm 与 vad 共用同一 seq 空间且各自连贯（跨拍不重号）。
- 断言：mono golden 逐字节不变（下混与事件体是契约面）。
- 断言：不向总线发"上一拍的重复帧"（读不到 → 不发）。
- 断言：DoA 字段是**可选 append**，不得成为必填（schema 只增纪律）。

## §10 改造工单

| # | 动作 | 归属 | 前置 |
|---|---|---|---|
| S-1 | `frame_ms` 下界钳制 + 合法域声明；配置读取去 env 依赖（默认读 manifest 同目录 `config.toml`） | 批I | — |
| S-2 | capture/vad 句柄与值类型换 `rr-hal-api`（`&[i16]` 借用、去 `Vec`/`String`） | 批I | C5 |
| S-3 | 采集新鲜度 counters（治"活着但不动"） | 批K | C9 |
| S-4 | `env`/`imu` 取向定案：填实 or retired（连带两条在册主题的处置） | **待裁** | — |
| S-5 | DoA 阈值从件内迁 ops | 批J | C5 |

## §11 待证与敞口

1. 真麦采集从未发生；sim 与 ALSA 两后端的"同一形状"只在 sim 上验证过。
2. "说完 → brain 判停"的端到端时延是**推算**（700 ms + 一拍），无实测。
3. 板上 ALSA 打开失败的确切行为（探活退不退）本册不下结论，属待证。
4. 契约申报与实现不一致（brain 未申报 pcm/vad 订阅）——见索引 §2，需契约评审。
