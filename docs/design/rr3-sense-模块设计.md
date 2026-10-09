# rr3-sense 模块设计

> 证据：`modules/rr3-sense/module.toml`、`atoms/{capture,vad,env,imu}/`、`src/composite.rs`、
> `config.toml`、`contracts/{slots,topics}.toml`、`rr-hal` 的 `rr3_hal_audio_source`。
> 规矩：**§1–§2 是现状事实，§3–§6 是设计意图，§8 是未证项**——三段不得互相引用为"已实现"。

## 1 职责（一句话）

把麦克风字节变成**时间有序的音频流与语音活动边沿**，交给认知层；本单元不含对话策略、不含 VAD 之外的
判停逻辑、不含任何"听懂了什么"的语义。格账：`audio.capture` + `audio.vad`（body 层）、
`sensor.env` + `sensor.imu`（body 层占位）。

## 2 事实面

| 件 | 格 | 发布 | 机制公开面 | 平台耦合（现状） |
|---|---|---|---|---|
| `capture` | `audio.capture` | `rr3-sense/audio/pcm` | `rr3_frames_per_tick` · `rr3_pcm_downmix_mono/stream` · `rr3_pcm_event_body` · DoA 三件（`Rr3DoaFix`、`rr3_doa_fix`、`RR3_DOA_BIN_THRESHOLD_DEG`） | `rr_hal::{Rr3AudioCfg, Rr3PcmStream}` + `rr_ipc::rr3_base64_encode` + `serde_json` |
| `vad` | `audio.vad` | `rr3-sense/audio/vad_start` | 边沿状态机（起音阈值 / 判停尾静音 / 最短语音），只发边沿不发续音 | `serde_json` |
| `env` | `sensor.env` | `rr3-sense/env/temperature`（申报） | **无机制**（`mech.rs` 仅 2 行注释） | 无 |
| `imu` | `sensor.imu` | `rr3-sense/imu/state`（申报） | 同上 | 无 |

组合件 `src/composite.rs`：开机打印 SURFACE（四件的自述逐字冻结）→ 读 manifest（`RR3_MANIFEST`）→
`rr3_hal_audio_source()` 开拾音 → 自 pacing 主循环 → `rr3_publish` 两路 + `capture/dump` 点名应答。
参数面：`sample_rate = 16000`、`channels = 1`、`frame_ms = 20`、`rms_start = 1500`、
`silence_tail_ms = 700`、`min_speech_ms = 200`；健康：心跳 1000 ms / 看门狗 5000 ms；
`[depends].required = ["bus.publish"]`。

**两条实况必须先记账**：① 配置只在 `RR3_SENSE_CONFIG` 被设置时才读（未设即静默用代码默认），
所以仓内 `config.toml` 的六个值在裸跑路径上不是真值来源；② `frame_ms` 解析无下界钳制
（同文件其他参数有），`0` 是合法值——拍长 0 会让主循环按 `Duration::from_millis(0)` 全速空转。

## 3 件级切分与依赖闭包（设计目标）

- `capture`/`vad` 已是**函数型件**（输入切片 → 输出摘要/边沿），是最接近 L0 可达的两件。改造点：
  值类型换成能力面契约 `rr-hal-api` 的定长/借用形状（`Rr3PcmStream.samples: Vec<i16>` → `&[i16]` 视图），
  事件体构造从 `serde_json::Value` 改产 `rr-ipc` body 字节（L1→L0 迁移清单，批I/后续批）。
- `env`/`imu` 的取向二选一（**不可长期停在"有主题无机制"**）：真件到位即按 §2 的机制形状填实；
  若某形态确实不需要这两格，按 ADR-0019 修订 2 ② 走 retired（号位保留、义务解除、随件处理）。
  格标记位今天应先落"占位"。
- DoA 的 30° 分档阈值是**策略**（与机型麦克风几何绑定），设计定为 ops 槽：
  `rr3_doa_fix` 保留为机制，阈值与"几麦 / 何种阵列"由注入给。

## 4 职责四面

| 面 | 本单元落点 |
|---|---|
| 接入 | 拾音句柄（唯一硬件入口）；`capture/dump` 点名 |
| 运行 | 采集拍（`frames_per_tick` × `frame_ms`）+ 下混 + DoA + VAD 边沿 |
| 状态 | `audio/pcm`(stream) 与 `audio/vad_start`(event) 两流；启动 SURFACE 自述 |
| 守护 | 音频源打开失败即 `exit(1)`（能力缺失不装活着，交监督复位）；**无本地安全核** |

## 5 时序设计

- 名义节拍 `frame_ms = 20 ms`（16 kHz / 单声道 = 每拍 320 帧）；VAD 的判停尾与最短语音都以 20 ms 为粒度。
- 两种后端同一形状：sim 源的 `read` 即时返回（不等一拍），ALSA 自耗一拍——**所以 pacing 必须由本单元
  自己负责**，这一点在 rtos/bare 档同样成立（拍长由 `[supplies]` 时基决定）。
- 待给而未给的量（矩阵⑦列）：单拍最坏耗时、丢拍判定线、`frame_ms` 合法域（设计定为 `≥ 1`，
  与"畸形输入往不触发动作方向收敛"同一条安全纪律）。

## 6 降级与缺格语义

| 情形 | 设计行为 |
|---|---|
| 无 `audio.capture` 句柄（板上无麦） | 该格 `Degraded`；`audio/pcm` 与 `vad_start` 不再产生，下游（brain）按依赖环降级 |
| `audio.vad` 在而 capture 缺 | 不可能成立（vad 的输入就是 capture 的产出）——**设计约束：两格同源，缺一即两缺**，注入面校验时应一起报 |
| env/imu 缺（真件未到） | 记"占位/不适用"，**不得**让 `contracts/topics.toml` 的在册主题显得"有生产者" |
| 畸形 `frame_ms` | 收敛到最小合法拍长并留痕（现状未做，列入 §8） |

## 7 红线（不得动的东西）

对外四主题名与载荷形状（pcm 的 `pcm_b64` + `ms`、vad_start 的可选 `doa` 字段）；SURFACE 自述字符串；
`capture/dump` 的应答形状；128B 线上字节；`module.toml` 的 provides/exposes/subscribes。
下混、DoA、VAD 三条都是**机制**：它们的参数可被注入，语义不可被策略改写。

## 8 待证与敞口

1. 真麦采集与回放从未发生（USB 麦阵列未采购，见 status 敞口）；板上 ALSA 路径只有构建证据。
2. 申报拓扑缺口：本单元真实消费者是 brain，但 **brain 的 `subscribes` 未列 pcm/vad_start**
   （索引 §2）——契约面与代码面不是同一张图，属批J 前要结清的债。
3. `frame_ms` 下界、config 必达性（不依赖 env 也能读到仓内 config）两处设计缺陷的修法待批I/J 落。
