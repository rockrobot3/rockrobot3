# rr3-sense 模块设计（框架 v1.3）

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程（host）；`restart_policy = "always"`、`mem_limit_mb = 64` | `module.toml` |
| 格 | `audio.capture` · `audio.vad` · `sensor.env`（占位）· `sensor.imu`（占位） | `contracts/slots.toml`、`atoms/*/atom.toml` |
| 件 | `capture` · `vad` · `env` · `imu`；组合件成分：采集主循环 + 配置解析（`src/composite.rs`） | `atoms/` |
| 主题 | 发布 `audio/pcm`(stream)、`audio/vad_start`(event)、`env/temperature`、`imu/state`（后两条**无生产者**）；RPC `capture/dump`；订阅面为空 | `module.toml` |
| 句柄 | `rr3_hal_audio_source() -> (Box<dyn Rr3AudioSource>, &'static str)`（第二元是**后端名**，是诚实面，载荷/日志都要带） | `src/composite.rs`、`rr-hal/src/backend.rs` |
| 参数 | `sample_rate=16000`、`channels=1`、`frame_ms=20`、`rms_start=1500`、`silence_tail_ms=700`、`min_speech_ms=200` | `config.toml`，**仅当 `RR3_SENSE_CONFIG` 已设时才读** |
| 健康 | 心跳 1000 / 看门狗 5000 ms | `[health]` |

## §1 件契约表

| 件 | 公开符号（现状） | 输入 → 输出 | 状态 | 失败语义 |
|---|---|---|---|---|
| capture | `rr3_frames_per_tick(sample_rate_hz: u32, frame_ms: u64) -> usize` | 率 + 拍长 → 每拍帧数 | 无 | `rate=0` 或 `frame_ms=0` → 0 帧（**语义未定**，§6） |
| capture | `rr3_pcm_downmix_mono(samples: &[i16], channels: u8) -> Vec<i16>`〔**未导出**：只在 `mech` 内 `pub`，件入口 `capture.rs` 的 re-export 白名单里没有它——见 §10 S-6〕 | 多声道样点 → 单声道 | 无 | 非整除交错 → 现状截断（历史缺陷已修，见 mono golden） |
| capture | `rr3_pcm_downmix_stream(pcm: Rr3PcmStream) -> Rr3PcmStream` | 同上（含 cfg） | 无 | — |
| capture | `rr3_pcm_event_body(seq: u64, pcm: &Rr3PcmStream) -> Value` | 序号 + 流 → 载荷（**六键**：`seq, format, rate, channels, ms, pcm_b64`） | 无 | — |
| capture | `rr3_doa_fix(samples: &[i16], channels: u8, threshold_deg: i32) -> Option<Rr3DoaFix>` · `rr3_doa_field(d: &Rr3DoaFix) -> Value`（两者**已导出**） · `Rr3DoaBin` · `rr3_doa_bin_as_str()`（两者**未导出**） | 双麦样点 + 分档边界 → 方位与三档 | 档界经 `Threshold` 注入（S-5） | 单声道/能量不足 → `None`（不发 doa 字段） |
| vad | `rr3_frame_rms(samples: &[i16]) -> i64`〔**未导出**〕 | 一帧 → 能量 | 无 | — |
| vad | `Rr3VadState::rr3_vad_new(cfg: Rr3VadConfig, frame_ms: u64) -> Self` | 阈值组 + 拍长 → 状态机 | 见 §2 | — |
| vad | `rr3_vad_feed(&mut self, samples: &[i16]) -> Option<Rr3VadEdge>` | 每拍一样点 → 边沿（只发边沿） | `Rr3VadEdge::Onset{rms_peak: i64, speech_ms: u64}` / `Offset{speech_ms: u64}`〔枚举本身未导出，件面只出 `Rr3VadState` + `rr3_vad_event_body`〕 | — |
| vad | `rr3_vad_event_body(seq: u64, edge: Rr3VadEdge, doa: Option<Value>) -> Value` | 序号 + 边沿 + 可选 doa → 载荷 | 无 | — |
| **件面导出总账** | `capture.rs` 导出 5 个：`rr3_frames_per_tick`、`rr3_pcm_downmix_stream`、`rr3_pcm_event_body`、`rr3_doa_fix`、`rr3_doa_field`；`vad.rs` 导出 2 个：`Rr3VadState`、`rr3_vad_event_body` | — | — | 框架 §1 的"缺任一 `pub` 即未完成"按**件面导出**算；上表标注的 6 个 `mech` 内 `pub` 符号要进导出面，须先定它们是不是契约面（§10 S-6） |
| env / imu | **无公开机制面**（`mech.rs` 各 2 行注释；入口只导出 `SURFACE`） | — | — | — |
| 目标签名 | 全部 `Value` → `Vec<u8>`（走 `rr3_body_encode`）；`Vec<i16>` → `&[i16]` 借用；`Rr3PcmStream`/`Rr3AudioCfg` → `rr-hal-api` | | | |

## §2 状态机（VAD，`Rr3VadState`）

不变式：**只发边沿**（`Onset` / `Offset`），持续高电平期间不续发；起音需连续满足 `rms_start` 达
`min_speech_ms`；判停需连续静音 `silence_tail_ms`；所有窗长以 `frame_ms` 为粒度换算成拍数。
非法输入收敛方向：`rms` 读数异常（帧空）→ 不改变状态、不发边沿。
`DoaBin` 三档：`θ < -t` = 左、`-t ≤ θ ≤ t` = 中、`θ > t` = 右（阈值 `t` 是**策略**，S-5 已迁 ops 槽 `doa_bin_threshold_deg`，见 §3）。
`env`/`imu` 无状态机——**无机制可写**，此事实必须显式留在表里而不是省掉。

## §3 注入面

| 件 | `handles` | `ops` 槽（名 / 类型 / 来源 / 默认） | 非法收敛（目标） |
|---|---|---|---|
| capture | `dyn Rr3AudioSource` ← `[hardware].alsa_device` | `sample_rate: u32`=16000；`channels: u8`=1；`frame_ms: u64`=20 | `frame_ms < 1` → 收敛为 20 并留痕（**现状：允许 0**） |
| vad | 无（吃 capture 输出） | `rms_start: i64`=1500；`silence_tail_ms: u64`=700；`min_speech_ms: u64`=200 | 阈值 ≤0 → 拒 boot |
| capture（DoA） | 阵列几何（`channels ≥ 2`） | `doa_bin_threshold_deg: i32` =30（S-5：已从件内常量迁此槽，来源 config `[rr3-sense.doa].bin_threshold_deg`） | 几何不足 → 不发 doa |
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
| `audio/pcm` | stream | 自增 | 每拍 | `seq, format, rate, channels, ms, pcm_b64`（六键恒发） | 一致（brain 真实消费但未申报） |
| `audio/vad_start` | event | 自增 | 边沿 | `seq, edge`；`speech_ms` **每次必发**，`Onset` 另带 `rms_peak`；`doa` 可选 append | 一致 |
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
| S-4 | `env`/`imu` 取向定案：填实 or retired（连带两条在册主题的处置）——**归属写明（2026-10-12，索引 §14.5）**：待裁期间挂"用户裁定窗"，**不塞进 0.0.5 批M**（批M 只收 F 清单内的功能码，温湿度/IMU 不在其列）；若裁"填实"，则新开一条 MC 并先解决前置——**器件不在 0.0.6 §5.4 BOM 任何一行**，两格今天已如实记 `placeholder`（承诺 ≤ 事实已守，不是欠账伪装成计划） | **待裁**（不卡批M 其余工单） | D9 |
| S-5 | DoA 阈值从件内迁 ops —— **已结（0.0.5 MC-10 S-5）**：`RR3_DOA_BIN_THRESHOLD_DEG` 件内常量删除，档界作 `rr3_doa_fix(samples, channels, threshold_deg)` 第三参注入；ops 槽 `doa_bin_threshold_deg` ← `[rr3-sense.doa].bin_threshold_deg`（缺键回落默认 30，组合件解析 `Rr3DoaConfig`） | 批J→**已结** | C5 |
| S-6 | **件面导出面收口**：`rr3_pcm_downmix_mono`、`rr3_frame_rms`、`Rr3DoaBin`(+`rr3_doa_bin_as_str`)、`Rr3VadEdge` 四个 `mech` 内 `pub` 符号今天不出件入口——要么进 `capture.rs`/`vad.rs` 的 re-export（成为契约面），要么降 `pub(crate)`。不许停在"内松外紧"的中间态。**已结**：`rr3_pcm_downmix_mono` 降 `pub(crate)`；`Rr3DoaBin` 转 `#[cfg(test)]` 测试面专用导出（原 `RR3_DOA_BIN_THRESHOLD_DEG` 随 S-5 件内删除） | 批I→**已结** | C5 |
| S-7 | **载荷组装上移承载层**：`rr3_pcm_event_body` / `rr3_vad_event_body` 的 `json!` 生产移到 `src/composite.rs`，件面只交 `Rr3PcmStream`/`Rr3VadEdge` 结构（依据 = ADR-0019 修订 4）。验收 = `audio/pcm`、`audio/vad_start` 字节逐字不变（mono golden 与 schema 用例继续绿）；做完本单元两件去 `serde_json` | 批I | — |
| S-8 | DoA 数学面整型化：`rr3_doa_fix` 去 `f64::{sqrt,asin,to_degrees,round}`，改整数时延 + 定点 `asin` 查表（表进 sim 后端/描述符数据面），误差预算 ±1°；复用 `vad` 的 `rr3_isqrt` 整型开方先例 | 批I | — |
| S-9 | **传感能力面立 trait**（[ADR-0021](../process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md)：能力面半边残缺——`rr-hal-api` 七个 trait 里**零个传感器读取**）：`Rr3Sensor { rr3_read(..) -> Rr3Sample{kind, value, unit, quality} }`，按**一张格 + 角色表 + 器件档案**建模，**不逐器件扩格**（DHT11 一格、BMP280 一格那种扩法会炸掉坐标系，也违反"格 = 能力"的定义）。`sensor.env`/`sensor.imu` 从 placeholder 升 normal 的时机 = trait 落 + 一个真后端到（与 D9 一并裁） | **增补轮六 批N3**（与执行件同批走契约级评审） | 批N1；器件到货（D9） |
| S-10 | **数字输入面**（`Rr3DigitalIn`，与 `Rr3GpioOut` 对称）：底座今天只有输出没有输入，于是"碰壁检测 / 限位开关 / 触摸"这类部件在契约面无对象，`[pins].direction` 与"输入专用脚不得作输出"那条校验也失去一半用武之地。定长形状 + 消抖 / 极性 / 内部上拉走绑定（`role` + `active_low` + `debounce_ms` + `internal_pull`） | **增补轮六 批N3** | 批N1 |
| S-11 | **两条**：① 缺省值三处且**已漂**——`config.toml` `channels = 1` vs `tools/sim/src/chain.rs` 内联串 `channels = 2` vs 件面 `Rr3SenseConfig` 缺省，S-1 原文**不提 sim 那一份**（一余两主），本单把覆盖面补全为"三处引同一真值"；② 阵列事实住件内（`channels ≥ 2`、`frame[0]=L/[1]=R`）与 DoA `30°` **等号归属无断言**——后者是出厂自检证不了"头转不到位"的直接原因。另承接 **L-14 的器件级探测需求面**（本册的 `env`/`imu` 两格今天只要 I2C 总线在就恒判在位，细则见 [全案](../process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §12.1） | ① **0.0.8 W-B**；② **批N3**（与 `Rr3Sensor` 同批）；探测需求随**批N1** | 全案 §15.1 M-7/M-8；门 a（四表一致） |
| S-12 | **三条申报面与实现面不齐**（2026-10-15 复盘核实）：① `module.toml` 与 `atoms/env`/`atoms/imu` 的 `publishes` 登记了 `sensor/env/temperature`、`sensor/imu/state`，而主循环 `rr3_sense_run` 只发 pcm/vad_start——**两个主题全仓零发布点**（格是 placeholder，但申报面没说）；② `atoms/env/env.rs` 的 `SURFACE` 自述与 body 字段无机械对账（同族事实：power 的 `SURFACE` 称"温度摘要"而 `Rr3BatteryState` 根本没有 `temperature` 字段），须由拟增门 l 钉；③ 拾音格式：契约层 `RR3_AUDIO_CFG_16K_MONO16` 把 `channels: 1` 写进跨形态类型（注释自述"与 legacy ESP32 链路一致"），而**现役板固件 `mic.rs` 跑双麦 2 通道**，`audio` 上行的 `seq` 还两侧宽度不同（宿主 `u64` / 板侧 `u32`）——一条链线上三个第二真值点。本单负责：publishes 随占位降级为不申报（或运行期按 `status` 拦）、`bit_depth`/声道数进描述符键、`seq` 宽度由 `contracts/` 生成器统一。**裁定原文见 [ADR-0022](../process/adr/ADR-0022-代码事实复盘-机制核纯净度与申报面逃逸与安全语义身体.md) 裁定 1/2** | ①② **批N1**（随门 l 与生成器）③ **批N1** | 全案 §4.5 生成物；门 l |

## §11 待证与敞口

1. 真麦采集从未发生；sim 与 ALSA 两后端的"同一形状"只在 sim 上验证过。
2. "说完 → brain 判停"的端到端时延是**推算**（700 ms + 一拍），无实测。
3. 板上 ALSA 打开失败的确切行为（探活退不退）本册不下结论，属待证。
4. 契约申报与实现不一致（brain 未申报 pcm/vad 订阅）——见索引 §2，需契约评审。

## §12 跨档动作（host / rtos / bare 逐件）

发布权判定先行：`capture` 发 `audio/pcm`、`vad` 发 `audio/vad_start` —— **两件的载荷只在"自己发主题"的档里才需要**；
自足 bare 形态不宣称发布主题（ADR-0018 修订 2 ①），故那档里两件只提供机制函数，不碰序列化面。

| 件 | host（今天） | rtos（esp-rs std + FreeRTOS，0.0.7 Q2 档） | bare（no_std 自足） | 卡住它的**具体那一行依赖** → 动作 |
|---|---|---|---|---|
| capture | ✅ 跑（sim 与 ALSA 两后端） | ⚠️ 不能，等 `rr-hal-api` | ⚠️ 不能，另加数学面 | ① `rr_hal::Rr3PcmStream{ cfg, samples: Vec<i16> }` 把 alloc 焊进件签名 → 批I F1 改借用形状（`&[i16]` + 调用方缓冲）；② `Rr3AudioSource::open/read` 是 `#[async_trait] + Send + Sync`，件侧经 `rr3_block_on`（`std::thread::yield_now` 自旋）→ 同上；③ **`rr3_doa_fix` 用 `f64::{sqrt, asin, to_degrees, round}`** → 改整型：交叉相关取整数时延 + 定点 `asin` 256 项查表（表随 sim 后端/描述符数据面走），误差预算 ±1°（三档分类只需 30° 边界不糊）；`vad` 已有 `rr3_isqrt` 二分整型开方可直接复用为先例 |
| vad | ✅ 跑 | ✅ **只差载荷面**（std 在，HAL 不进件签名？不：`Rr3VadConfig` 是件内类型） | ✅ 机制本体已是 core-only | 无 `use std::`、无 `rr_hal::`、手搓整型开方——**13 件里最接近 L0 的一件**。唯一动作：`rr3_vad_event_body` 的 `json!` + `Option<Value>` 参数按 §12 通则上移到承载层，件面只留 `rr3_vad_feed -> Option<Rr3VadEdge>` |
| env / imu | ⚠️ 格在位但**无机制本体**（`mech.rs` 各 2 行注释，入口只导出 `SURFACE`） | 同左 | 同左 | 跨档表这一行不写"改造动作"，写"**待填件**"。纪律：这两格在 rtos/bare 档只能靠 `contracts/slots.toml` 的三态标记记"占位"（C9），**不许**用 manifest 假装有件来过"每格一件"门——那会把分母做成假的 |
| 载荷面通则 | `rr3_pcm_event_body` / `rr3_vad_event_body` 今天返回 `Value`，全仓每一处 body 都由 `serde_json` 造（`rr-bus::rr3_publish` 收 `Value`） | 同 host | 不需要 | **裁定落 ADR-0019 修订 4**：`*_body` 组装上移到承载层（本单元 `src/composite.rs`），件只交结构体。于是 11 件的 `serde_json` 依赖是**落位错误**，不是跨档障碍；bare 档连"无堆编码器"都不需要 |

**动作序**：批I F1（`rr-hal-api` 同步形状 + 去 alloc 签名）→ 本册 S-2 → 载荷上移（新工单 S-7）→ DoA 整型化（S-8）。
前两步不做，件在 rtos 档一行都编译不过；后两步不做，件在 bare 档编译不过。

## §13 策略字面量清单

| 文件 : 符号 | 字面量 | 语义 | 今天有没有读者 | 去处 |
|---|---|---|---|---|
| ~~`atoms/capture/mech.rs : RR3_DOA_BIN_THRESHOLD_DEG`~~（S-5 已删） | ~~`30`（±30°）~~ | DoA 三档边界=**业务策略** | **已结**：件内无此常量；档界由 `rr3_doa_fix` 第三参 `threshold_deg` 注入，读者 = 组合件（从 config 取） | ~~ops 槽 `doa_bin_threshold_deg`（S-5）~~ → 已落：ops 槽 `doa_bin_threshold_deg` ← `[rr3-sense.doa].bin_threshold_deg` |
| `atoms/capture/mech.rs : rr3_doa_fix` | `channels < 2`、`frame[0]`=L / `frame[1]`=R | 阵列几何与通道序 | 无声明 | 描述符 `[hardware]`（几何是板事实，不是件常识） |
| `src/composite.rs : open_cfg` | `bit_depth: 16` | s16 位深 | config 无该键 | 契约字面量（线格式 `s16le`）——**标豁免**，但要与 `rr3_pcm_event_body` 的 `"format":"s16le"` 同处登记，避免两处真值 |
| `src/composite.rs : Rr3SenseConfig::default()` | `16000 / 1 / 20`（audio）+ `30`（doa.bin_threshold_deg） | 采集与 DoA 缺省 | 有读者，但**与 `config.toml` 逐字重复**，`tools/sim/src/chain.rs` 还有第三份 | **双写即债**：缺省值单点归 `config.toml`（或归描述符 `[supplies]` 旁的音频面），代码侧不保留副本；sim 那一份改读同一真值（doa 阈值同此债，S-5 落点即此模式） |
| `atoms/vad/vad.rs : Rr3VadConfig::default()` | `1500 / 700 / 200` | 对话阈值=**策略** | 同上，三份重复 | 同上（且 brain 侧 `silence_tail_ms` 已判死参数，B-2） |
| `atoms/capture/mech.rs : rr3_doa_bin_as_str` | `"left"/"center"/"right"` | 事件词汇 | 在册：`contracts/events/sense-audio-vad_start.schema.json` 的 enum | **契约字面量—不许动** |
| `atoms/capture/mech.rs : rr3_pcm_event_body` | `"format": "s16le"` 及键名 `seq/rate/channels/ms/pcm_b64` | 线格式与载荷键 | 在册 schema | **契约字面量—不许动** |
| `src/composite.rs : RR3_DUMP_TOPIC` | `"rr3-sense/capture/dump"` | RPC 面 | 与 `module.toml [rpc].exposes` 双写 | 契约字面量；双写记为债（与 act 的 `EXPOSED` 内联同类） |
| `capture.rs/vad.rs/env.rs/imu.rs : SURFACE` | `"rr3-sense 0.0.1 (M3 实装)"` 等逐行自述 | 面板逐行取用 | 有读者 | **契约字面量—不许动** |
| `src/composite.rs : RR3_SENSE_CONFIG` / `RR3_MANIFEST` | env 键名 | 配置入口 / 接入契约 | `RR3_MANIFEST` 未设 → `Err` → `exit(1)`（fail-closed，**正面样板**）；`RR3_SENSE_CONFIG` 未设 → **静默用代码缺省，config.toml 整份不生效** | 静默回落点，S-1 处置：改为默认读 manifest 同目录 `config.toml`（与 power/connect 同口径），并在日志里写明配置来源 |
| `config.toml [rr3-sense] log_level` | `"info"` | 日志级 | **全仓无读者** | 被读或删除（S-1 同批，与 P-6/N-6/I-1/B-8 同纪律） |
| `atoms/*/assemble.toml` | `handles = []` / `ops = []`（四份皆空） | 注入面申报 | **零申报**：上面每一条策略都没有申报位 | 批J G4 之前，本册 §3 的 ops 槽全是纸面——这正是索引 §11 审计项 L-3 的实证 |

## §14 基线对表（代码 `22783ea`，2026-10-10；本节是本册唯一"现在时"）

本册 §0–§13 写于批I–K 开工前，其中"待做/现状卡点"类断言已随批I/批J/批K 收口而**部分作废**；
按存档纪律不回改正文，作废关系只在本节点名（承 [模块设计索引 §13](模块设计索引.md)）。

| 本册旧断言 | 现在的事实（符号为证） | 判定 |
|---|---|---|
| §1 目标签名"`rr3_pcm_*`/`rr3_vad_*` 的 `Value` → 待上移"；工单 S-7 | 件面 `serde_json`/`json!` **零命中**，`rr3_pcm_event_body`、`rr3_vad_event_body`、`rr3_doa_field`、`rr3_pcm_downmix_stream` 全在 `src/composite.rs` | **S-7 已结** |
| §1 "内松外紧"五个符号未导出（S-6） | `rr3_pcm_downmix_mono`、`rr3_frame_rms` 改 **`pub(crate)`**；载荷类符号随 S-7 离件，导出面自然收窄 | **S-6 已结** |
| §12 DoA 卡在 `f64::{sqrt,asin,to_degrees,round}`（S-8） | 改 **`RR3_ASIN_DEG_LUT: [i32; 257]` + isqrt/Q15** 整型；`f64`/`sin` 只在 `#[cfg(test)]` 内 | **S-8 已结**；`f64::sqrt` 仍在测试夹具里（不影响 bare 编译面） |
| §1 `Rr3AudioSource::read` 是 `#[async_trait]` + `Send + Sync`，件侧经 `rr3_block_on` | 契约面 `rr-hal-api::Rr3AudioSource` 为同步且改**借用缓冲**：`read(&mut self, buf: &mut [i16]) -> Result<usize, Rr3HalError>`（返回 0 = 无新数据）；`rr3_block_on` 全仓不存在 | §12 该行作废 |
| §12 "件签名里 `Vec<i16>` 待改借用" | `rr3_pcm_downmix_mono(&[i16], u8) -> Vec<i16>` **仍产堆**（现为 `pub(crate)`，件内部使用） | **未结** → 批L L-3（改调用方缓冲或降 `pub(crate)` 后豁免） |
| §3/§13 "`sample_rate/channels/frame_ms` 待进 ops，DoA 阈值待进 ops（S-5）" | `atoms/{capture,vad}/assemble.toml` **已填**（批M MC-10 L-5，2026-10-13）：capture 报 `Rr3AudioSource`←`alsa_device` 句柄 + `sample_rate`/`channels`/`frame_ms` ops；vad 报 `rms_start`/`silence_tail_ms`/`min_speech_ms` ops；门有内容可校验 | **L-5 已结**；DoA 阈值迁 ops 续做（S-5，见下） |
| §13 `bit_depth: 16` 无 config 键 | 实况未变，但**本轮判定归位**：s16le 是线格式契约，不是策略 → 从"孤儿参数"改记**契约字面量豁免**（0.0.5 §17 同批订正） | 判定改，动作撤 |
| §13 DoA 查表：`RR3_ASIN_DEG_LUT` 257 项常量在件内 | **本轮裁定**：数学常量表属**机制**（跨板同值、不随硬件变），与 `RR3_FRAMES_*`（板面渲染数据）不同类 → 允许留在 `mech.rs`，不必外提到描述符 | 新裁定 R-12（见索引 §13.3 同源） |

**本册新净余数**：批L **L-3**（`downmix_mono` 去堆）。**L-5 已结（批M MC-10 L-5）**：capture/vad 的 `handles`/`ops` 申报补齐。

### §14.1 增量对表（2026-10-12，代码基线 `78625ae`；全仓总表见 [索引 §13.4](模块设计索引.md)）

| 项 | 复验实况（符号） | 判定 / 新落点 |
|---|---|---|
| S-6 件面导出面收口 | `atoms/capture/capture.rs` 已把 `rr3_pcm_downmix_mono` 收为 `pub(crate)`，`Rr3DoaBin` 注明"仅测试按名构造" | **结**（"内松外紧"消除；阈值注入的余数转 S-5） |
| S-5 DoA 阈值迁 ops | **已结（0.0.5 MC-10 S-5）**：`RR3_DOA_BIN_THRESHOLD_DEG` 件内常量删；`rr3_doa_fix` 增第三参 `threshold_deg`；ops 槽 `doa_bin_threshold_deg` ← `[rr3-sense.doa].bin_threshold_deg`（缺键回落默认 30）；组合件 `Rr3DoaConfig` 解析 + 透传 | 已结 |
| S-3 采集新鲜度 counters | **已结（批M M-β 3bc30f1，2026-10-12）**：`composite.rs` 主循环 `pcm_frames`（发布成功一帧 +1）/ `pcm_empty_frames`（空帧另账）两键落账，e2e_audio_chain ⑤ 经 `rr3.ctl.counters` 有界轮询点验非零（先红后绿） | 已结 |
| S-1 缺项：`bit_depth` 不是配置键 | 本册曾把 `[rr3-sense.audio].bit_depth` 记为"在册无读者"——**实况不存在**：`config.toml` 只有 `sample_rate`/`channels`/`frame_ms`，`bit_depth: 16` 是 `composite.rs` 的代码常量（`linux.rs` 用它做 ALSA 校验） | **收回该宣称**；位深若将来要可配，须连同读者与用例一起立单，不留静默常量当配置 |
| F-3 采集承载路径 | R31 **Q1/Q2** 增第二条路径：INMP441 ×2 → I2S → UART 921600 过桥，DoA/ASR 在宿主算（施工 = 增补轮五批D）；Pi 侧 ALSA 保留 | 本册 §12 跨档表按两条路径读；"缺麦不降级"的清零判据留 **MC-4** |
