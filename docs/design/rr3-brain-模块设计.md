# rr3-brain 模块设计（Rust 实现）

> **真值声明（2026-11-18 重写）**：本篇按 ADR-0016 迁移后的 Rust 实现重写，代码是唯一
> 真值（`modules/rr3-brain/`，`module.toml` 的 `lang = "rust"`，entry 与其余模块同形）。
> 迁移前的 Python 工单与对表原文保留在 §11 附录（编号沿用旧文，**不是**本文 §0–§10 的续号），
> 只作追溯：已结/未结状态以该附录最后一次对表为准。

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程，`type = brain`、`lang = rust`、`entry = rr3-brain`（与其余模块同形） | `module.toml` |
| 格 | `cognition.dialog` · `cognition.asr` · `cognition.llm` · `cognition.tts` | `[provides].capabilities` |
| 件 | 四原子件 `dialog` / `asr` / `llm` / `tts`，每件五件套（`atom.toml` + `assemble.toml` + `mech.rs` + 件入口 + `private.rs`）；`bus` 是组合件机制壳，不是件 | `atoms/*/`、`composite.toml` |
| 主题 | 发布 `rr3-brain/intent/result`、`rr3-brain/audio/tts_pcm`；订阅 `rr3-sense/audio/pcm`、`rr3-sense/audio/vad_start` | `[events]` |
| RPC | exposes 六条：`llm/completion`、`ops/list`、`ops/set`、`model/list`、`model/set`、`model/test`（全部带 `rr3-brain/` 前缀） | `[rpc].exposes` |
| 句柄 | 无能力面句柄（`handles = []`、`ops = []`、`no_injection = true`） | `composite.toml` |
| 依赖 | required `bus.subscribe` / `bus.rpc` / `net.online`；optional `power.battery` | `[depends]` |
| 资源 | `restart_policy = "on-failure"`、`mem_limit_mb = 256`、`start_priority = 80` | `[resources]` |
| 健康 | 心跳 2000 ms / 看门狗 10000 ms | `[health]` |

## §1 件与装配

四件在 `src/main.rs` 以 `#[path]` 原位挂载（执行单元进程内函数调用，`rr3-sense` 同形；
`composite.toml` 的 `[assemble].atoms` 与依赖闭包门 ⑧-b 同源）：

| 件 | 机制面 | 职责 |
|---|---|---|
| `dialog` | `atoms/dialog/mech.rs` | 轮次状态机：pcm 预滚、onset 起轮、offset 判停、`max_utterance_ms` 封轮 |
| `asr` | `atoms/asr/mech.rs` | 整段 WAV → 文本（云 HTTP，OpenAI 兼容 multipart） |
| `llm` | `atoms/llm/mech.rs` | system 恒守首位 + 最近 4 轮历史 → 回复；对外只暴露历史预算常量 |
| `tts` | `atoms/tts/mech.rs` | 文本 → s16le/16k/mono PCM（云 HTTP 或宿主兜底后端） |

装配面不注入句柄与参数槽（`no_injection = true`）：模型行、超时与唤醒参数全部走
`config.toml` 与 RPC 面，装配层只负责把四件串成一条流水。

## §2 状态机（dialog 轮次）

- **起轮**：`vad_start` 的 onset 边沿；onset 前的 pcm 进预滚缓冲（起轮时回放，避免吞字）。
- **判停**：`vad_start` 的 offset 边沿（判停归 sense，本单元不自判静音尾）。
- **封轮兜底**：`on_tick` 的 `utterance_expired` 按 `max_utterance_ms = 8000` 强制封轮
  （两路封轮共用同一个 `rr3_finish_turn`，ADR-0016 增补裁定 2）。
- **一轮至多一次流水**：ASR → LLM → TTS 串行；云面失败即弃轮，不排队重放。
- **唤醒门**（`[rr3-brain.wake]`，可选段，缺段 = 关闭且行为与旧版逐点等价）：ASR 文本先过
  `rr3_wake_decide`——短语表精确剥前缀；未中再走"你好"兜底（"你好"后 ≤2 字视为名字/误识
  尾巴 → 回 Ack，更长 → 去前缀进 LLM）；`followup_ms` 窗内免唤醒，窗外未命中静默丢弃
  （不 LLM、不 TTS）。Ack 的 PCM 在装配段预热缓存，命中即直发不重复合成。

## §3 注入面

| 面 | 内容 |
|---|---|
| `handles` | 无（云 HTTP 与总线都不是能力面句柄） |
| `ops` | 空（`no_injection = true`）；模型行与槽值经 `model/*`、`ops/*` RPC 面读写 |
| 凭据 | `api_key_ref` 双源：`env:<名>` 读环境变量，裸名读面板维护的仓外 secrets 文件（0600）；密钥永不进 `config.toml`、不进应答（`ops/list` 白名单组体） |

## §4 主循环与时序预算

`rr3mod` 的 `Dispatcher` 回调即主循环（单线程）：

- `on_event` 两臂：`pcm` → dialog 积累/预滚；`vad_start` → 起轮/判停触发整轮；
- `on_tick` 一臂：`utterance_expired` 时长判停（与 offset 臂共用 finish）；
- `on_rpc` 六臂：`llm/completion` 真调；`ops/list` 快照；`ops/set` 写槽（live 生效不落盘，
  应答 `persisted:false`）；`model/list|set|test` 模型行 CRUD（set 落盘 `.bak` + 原子写并重建
  客户端，test 按名探测一次）。

一轮的固定次序：ASR → LLM → TTS → 按 20 ms/640 B 分帧发布 `rr3-brain/audio/tts_pcm`
（`seq` 模块级单调、`ms` 由样点数导出）→ 点名 `rr3-interact/tts/text`（args 携 `text` 与
`pcm_bytes` 作收流完整性唯一判据）→ 发布 `rr3-brain/intent/result`。

预算：`[rr3-brain.cloud].timeout_s = 8`（429/5xx/传输层瞬时失败最多重试一次，其余不重试）；
点名等待 `[rr3-brain.bus].call_timeout_s = 30`（缺键回落 30）。一轮最坏 ≈ 3×8 + 30 ≈ 54 s；
三段各命中一次瞬时重试再加 ≤24 s（≈78 s）。`max_utterance_ms` 只管采集段、管不到云段——两者的关系仍是未决工单
（见附录 B-3）。心跳 2000 ms 由骨架独立线程续命，故"卡在云上"期间健康面看不出异常；
轮次与失败计数（§8）是这类"活着但不动"的唯一可读证据。

## §5 主题面契约

| 主题 | kind | 方向 | 载荷 / 语义 |
|---|---|---|---|
| `rr3-sense/audio/pcm` | stream | 消费 | `seq, pcm_b64, ms`（s16le/16k/mono） |
| `rr3-sense/audio/vad_start` | event | 消费 | `seq, edge, doa?`（onset / offset 判停） |
| `rr3-brain/intent/result` | event | 发布 | `actions[]: {topic, args}`；topic 只许 `rr3-act/relay/set`、`rr3-act/servo/command` |
| `rr3-brain/audio/tts_pcm` | stream | 发布 | `{seq, format:"s16le", rate:16000, channels:1, ms, pcm_b64}`，20 ms/640 B 分帧 |
| `rr3-brain/llm/completion` | RPC | 暴露 | 文本 → 回复（mock 档回显，real 档带历史真调） |
| `rr3-brain/ops/list` · `ops/set` | RPC | 暴露 | 槽值快照 / 写槽（白名单六槽，密钥不进应答） |
| `rr3-brain/model/list` · `set` · `test` | RPC | 暴露 | 模型行 CRUD 与单次探测（ADR-0023） |

`net/link` 与 `asr/text` 的申报已在 C14 评审撤下（历史见附录 B-7/B-2）。

## §6 降级矩阵

| 场景 | 设计行为 |
|---|---|
| 云不可达 / 非 2xx | 计 `asr_fail`/`llm_fail`/`tts_fail` 并弃轮；不排队、不发假 intent |
| TTS 失败 | PCM 无产出，但 `intent/result` 照发（意图与声音解耦） |
| 唤醒门关闭 | 与迁移前逐点等价：每轮都进对话 |
| 唤醒窗外语义 | 未命中静默丢弃（用户听不到回声） |
| `RR3_BRAIN_CLOUD=mock` | 测试注入位，**不是在位证据**；契约面不得据此声明认知可用 |
| 无网形态（四格全缺） | 其余单元本地闭环照常（认知层是"有 OS/云才叠加"的那一层） |

## §7 安全核接入

无，且不得有：兜底链不能依赖云端在场。唤醒门只做节流（决定"这一轮进不进对话"），
不是安全核；任何安全动作都不得以 ASR/LLM 结果为前提。

## §8 可观测面

- 计数（心跳上报，rrd 聚合后 `ctl.status`/`ctl.health` 可读）：`rounds_replied`（完成回复的
  轮数）、`rounds_audible`（真正点名播放成功的轮数）、`asr_ok`、`asr_fail`、`llm_fail`、
  `tts_fail`——"完成一轮"与"用户听见"分两本账（附录 B-16 ①）。
- 轮次日志一行：`对话轮完成: trace=… user='…' reply='…' asr_ms=… llm_ms=… tts_cost_ms=…
  ready_ms=… tts_pcm_ms=… actions=…`——卡住时先看这一行缺哪一段。

## §9 不变量与红线断言

- `intent/result` 的 `actions[]` 只含本单元自限的两个主题；执行侧 exposes 门是第二道归属门。
- 云凭据不落盘、不进仓、不进应答；secrets 文件在仓外且 0600。
- 一轮至多一次流水；offset 与 `utterance_expired` 两路封轮共用同一 finish（不重入）。
- 唤醒门关闭时行为与迁移前逐点等价（配置缺段不得改变现有语义）。

## §10 跨档动作（host / rtos / bare）

单元级判定先行：brain 属主承载层（ADR-0018 裁定 3/修订 1）。三档里 brain 的位置：

| 档 | brain 在哪 | 跨档动作 | 理由与代价 |
|---|---|---|---|
| host 全承载 | 本机进程（现状） | 不适用 | 认知四格跑 std Rust |
| rtos 轻承载（ESP32 类经桥） | **仍在 host 侧**，板只经桥上灌 pcm/vad、下领 intent | 板侧无 brain 件；动作全在桥语义面 | 云 RTT 进闭环（§4 的一轮预算就是这个代价） |
| bare 无承载 | 不存在 | 无；不起对话轮是设计事实，不是降级 | 描述符里不得挂 `cognition.*` 格 |

`dialog` 是唯一可能跨档的认知件（只吃时间戳与边沿，纯 `core` 可达）；`asr`/`llm`/`tts`
绑云，依赖闭包里的 HTTP/TLS 是主承载层专属，rtos/bare 档按发布权判据**不启用**而非
"改造后可用"。

## §11 附录：Python 时代工单与对表（历史，迁移前口径）

> 以下小节编号（§10–§14）沿用迁移前原文，**不是**本文 §0–§10 的续号；内容逐字保留，
> 只作追溯。代码已迁移到 Rust，凡与本文正文冲突处一律以正文与代码为准。

### §10 改造工单

| # | 动作 | 归属 | 前置 |
|---|---|---|---|
| B-1 | **申报订阅面补齐**（pcm + vad_start，只增）；`asr/text` 走 retired | 契约评审（批J 前） | C14 |
| B-2 | **改判**：`silence_tail_ms` 在 brain 侧不是"重复声明"而是**死参数**（`dialog.py` 从不读它，判停完全依赖 sense 的 `Offset` 边沿）。处置 = 从 brain 的 `config.toml` 删除，或把它接成真的兜底尾长；不许留着让人以为它在生效 | 批I | — |
| B-7 | `net/link` 申报消费但无处理：要么补"链路 down 不起新轮"的实现（含一条用例），要么把这条订阅从 `subscribes` 撤下（撤 = 契约面变更，走 C14 的 retired 评审） | 契约评审（批J 前） | C14 |
| B-8 | `max_turn_chars` 无读者：实装截断或删键（与 power P-6、connect N-6、interact I-1 同批同纪律）——**2026-10-12 对表结案**：键已随批I 从 `config.toml` **删除**（文件第 18 行注记自陈"在册无读者；实装截断随真用例回归"），本条按"删除"分支结清；截断行为若将来要回，须连同真回归用例一起立单，不保留静默键 | **已结（批I）** | — |
| B-3 | 云总预算与 `max_utterance_ms` 的关系定案（轮次超时是否覆盖云 RTT） | **待裁** | — |
| B-4 | Python 测试接进门禁（现在只登记 conventions，不进 `check.sh`；本机 Python 3.9 不过 3.11 门） | 批K + 宿主 | C11 |
| B-5 | 起轮/云失败 counters（健康面可见性） | 批K | C9 |
| B-6 | Rust 化随 ADR-0016（届时两腿对拍与闭包门才适用；`dialog` 件按 §12 是唯一可能跨档的认知件，`asr`/`llm`/`tts` 按发布权判据在 rtos/bare 档**不启用**而非"改造后可用"） | ADR-0016 | — |
| B-9 | `RR3_CALL_TIMEOUT_S = 30.0` 从 `composite.py` 常量改 ops/config 槽——它是"一轮最坏 ≈90 s"的组成项，调它却要改码违背数据面原则；与 B-3 同批裁 **2026-10-20 对表（批N4 件六一笔随 B-13① 结清）**：已数据化为 `[rr3-brain.bus].call_timeout_s`（config 槽，legacy 常量退役）——"调它不改码"诉求兑现，本条按数据化分支结清 | **已结（批N4 件六一笔）** | — |
| B-10 | 凭据键名把云供应商焊进代码——**已结（0.0.5 批M MC-8②，2026-10-12）**：改通用键 `RR3_CLOUD_API_KEY`（原 `SILICONFLOW_API_KEY`），供应商由 config `[rr3-brain.cloud]` 的 `base_url`/模型名承载，换云只改配置不改码；测试随 `API_KEY_ENV` 常量注入 | 已结 | — |
| B-11 | **把已合成的 PCM 送上总线**（2026-10-12 立单，版本边界改判产物 = 索引 R-14 / ADR-0020 背景表 #1）：`composite.py` 的 `rr3_finish_turn` 今天取到 `reply_pcm = tts.synthesize(reply_text)` 后只算 `tts_ms` 入日志，再 `rr3_call_topic(TOPIC_TTS, {"text": reply_text})`——**云侧真人声从未有可能出声**。本单 = 按 MCP-1 的分帧把 `reply_pcm` 发布到新主题 `rr3-brain/audio/tts_pcm`，`tts/text` 点名保留作"该不该播"的事务判据；`seq` 单调、`ms` 由样点数导出 | **0.0.5 批M MC-1**（前置 MCP-1 契约 append 已落 `8d05590`；消费侧对号 interact I-14） | MCP-1 |
| B-12 | **模型连接表与 ops 槽的面板可见性**（[ADR-0021](../process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md) 裁定 6）：`[[rr3-brain.model]]` 连接表与密钥双源解析已落，但模型面参数（超时、重试、失败是否降级到本地引擎）住在 ops 槽里，**面板既看不见也改不了**——这是增补轮六批N4 覆盖率门上线即红的第一批项。本单负责把 brain 的 ops 槽与密钥端点对该门逐面转绿，并守住"值零回显"（写面只报"已更新/长度"，不回显值）**2026-10-19 对表：读面收官（批N4 件六第二笔）**——新 RPC `rr3-brain/ops/list`（contracts/topics.toml 契约注册 request/L2 ＋ module.toml `[rpc].exposes` 双写同源），应答体 `rr3_ops_snapshot` 白名单组体（asr/llm/tts/dialog 四原子分组六槽名单键，llm 组取模型行实效值；密钥与 `api_key_ref` 不进应答＝值零回显守脑面）；面板 `GET /api/brain/ops` BusCall 透传＋covers 申报六槽 ⇒ 门 d 红账 79→73（基线同批下调）；密钥面两端点（`GET /api/model/keys`/`POST /api/model/key`）既有零回显如实核对不申报 covers（secrets 不住五路分母）。**面板写 ops 槽（改值）归件九配置面扩端点，不在本单** | **增补轮六 批N4** | I-15；D11 |
| B-13 | **超时两种落点 + 孤儿键 + 悬空单重挂**（主线自查，实测）：① 云 HTTP 超时**已数据化**（`config.toml` 的 `[rr3-brain.cloud] timeout_s = 20`），而总线 RPC 调用超时**仍住码内**（`rr3_brain/composite.py` 的 `RR3_CALL_TIMEOUT_S = 30.0`，另 `bus.py` 的 `RR3_CONNECT_TIMEOUT_S = 5.0`）——同一类参数一半在配置一半在码，"参数已数据化"这句因此只能算半真；本单为各超时立 ops 槽，并把封轮上限 `max_utterance_ms` 与云预算**解耦**（D7 裁定：不写 `timeout_s×3` 这类派生凑法）**2026-10-20 对表（批N4 件六一笔 B-13①）：总线 RPC 超时已数据化**——`[rr3-brain.bus].call_timeout_s=30/connect_timeout_s=5`（config.toml 新段＋回落常量，legacy 两常量退役；"解耦"仍留敞口待裁）**。② `[rr3-brain] log_level = "info"` 经 grep 全仓 `.py` **零读者**——其余五册同类孤儿键各有单号（P-6/N-6/I-1/A-11/S-1），本册独缺，被读或删除。③ **B-9 挂在已收口的 MC-10 上属悬空归属**，本单接手并改挂 0.0.8 W-B。细则见 [全案](../process/dev/0.0.5/增补轮六/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §15.1 M-4/M-5 与 §12 D7 | ① **批N4**（面板可见可写）② **0.0.8 W-B** ③ 随 ② | I-15；门 k |
| B-14 | **模型连接表与 ops 申报分裂 + 档位角第二真值点 + 件面时钟未注入**（2026-10-15 复盘核实三条）：① `atoms/{asr,llm,tts}/assemble.toml` 申报的 `base_url`/`llm_model` 槽指向 `[rr3-brain.cloud]`，但**有 active 行时 LLM 实读的是行内 `base_url`/`model_id`**（`rr3_brain_model_row` 一路），`cloud.llm_model` 只在无表 fallback 路径被读——**同一个参数两个来源、装配申报只覆盖其中一个**（换云改哪一处？今天的正确答案是"看有没有 model 表"，这就是不可运维的形状）；② `rr3_brain/intent.py` 的 `_SERVO_ANGLE_LEFT/CENTER/RIGHT = 70/90/110` 是 act `angle_limit_deg` 的**第二处真值**，ADR-0021 已把行程收敛到执行件档案，本单负责让 brain 的档位角**由档案派生**（意图带语义位"左/中/右"，档案给角度）；③ `dialog.py` 的 `rr3_feed_vad`/`rr3_utt_expired` 用 `time.monotonic()` 自取时基，测试只能把 `max_utterance_ms` 设成 0 变通——**Python 件面因此写不出逐拍对拍用例**（Rust 五册已按 `Rr3TimeBase` 收口，本条是 ADR-0016 迁移窗的尾巴）。细则与裁定见 [ADR-0022](../process/adr/ADR-0022-代码事实复盘-机制核纯净度与申报面逃逸与安全语义身体.md) 裁定 1/2/5 | ① **批N1**（申报面扩到组合件层后才有判据，见门 l/门 j）② **批N3**（随 A-13 档案）③ **0.0.8 W-C**（brain Python→Rust 同窗）。**2026-10-19 对表：批N3 评估改判批N4**——档案侧已落（A-13 ①② 件一/件二与件五行程收敛），但"档案给角度"的通道未立：语义位进 `TOPIC_SERVO_COMMAND` payload 属契约级变更须走评审（payload schema/golden 冻结门），语义位→档案键的映射（left→lo / right→hi / center→center）本身待裁定，且与 A-13③ `rr3_goto` 执行面（挂 A-16②/A-19① 独立小件）同链——批N3 件清单无此授权，硬塞即跨批依赖；留批N4 与 B-12/B-13 面板线同批（敞口表无增减，`intent.py` 三常量维持原状待通道落） | A-13；门 j；B-13 |
| B-15 | **证据号命名空间不存在，于是 D11 的界面门不可执行**（2026-10-15 证伪审查，全案 §18 F-24/§12 D11）：D11 裁"`armed=true` 不许面板单步置位——必须引用一条**真板对拍证据的编号**"，这条把"无真板对拍不接闸"从口头规矩做成界面门，方向是对的；但实测 `docs/evidence/` 与 `evidence/` **都不存在**，仓内也没有任何证据编号规则（`conventions.md` 只有"每行带证据路径"的散文，`contracts/golden/` 是 IPC 字节向量、不是板上对拍记录）。**没有可引用的编号，这条门就只能靠人肉判**——这正是本册反复批评的"承诺有名字、没有身体"在流程面的复现。本单三件事：① 立命名空间 `docs/evidence/<形态>/<编号>.md`（编号规则、必填字段：形态 / 格或角色 / 观测方法 / 结论 / 日期 / 取证人；**一次对拍一份文件，编号不复用**）；② 面板写面与装载期都**校验所引编号存在且形态匹配**，引用不存在的号即拒（fail-closed，与"缺键即红"同一列语义）；③ `armed=true` 之外的两个安全写（`severe_mv`、`current_ceil_ma`）同样要求引用证据号，**不得只带二次确认**。**归 brain 册的理由**：本册 §13 已立"mock 不得作为在位证据"（G-4）——"什么叫一条证据"是这个问题的正面答案，六册共引这一处命名空间。**注意不做的事**：本单不代替验证窗产出任何板上结论，它只规定"结论长什么样、住哪儿、怎么被引用" | **批N4 的前置**（命名空间与校验规则先行；实际证据随 0.0.6/0.0.7 与批N2 对拍产） | D11；G-4；B-13；全案 §12 D11/§18 F-24 |
| B-16 | **一轮对话"完成"与"用户听见"解耦，且云失败有两种结局**（2026-10-15 第六轮追流核实，全案 §20.6，索引 **L-27**）：① `composite.py:265-277` 里播放点名失败被 `except RuntimeError` 接住**只 print 一行**，随后照常 `publish intent` → `rr3_record` → **`chat_rounds + 1`**（`:288-291`）⇒ 计数器意义上的"完成一轮"与是否出声**完全无关**，而这本是最该被产线信任的一格数。裁定：**不许用一个词盖两件事**——要么点名失败即弃轮（不计 `chat_rounds`），要么把计数拆成 `rounds_replied` / `rounds_audible` 两本账，面板与 `/api/status` 同时展示。② 云调用异常**分两类两种结局**：`RuntimeError`（HTTP 非 200、缺 `text`）→ 计 `asr_fail`/`llm_fail`/`tts_fail` + 弃轮（`:238-241`）；而 `URLError`／超时／`asr.py:56` 那个**在 try 之外**的 `json.loads`（`JSONDecodeError`）不是 `RuntimeError` ⇒ **逃逸到进程退出**。同一类故障（云不可用）一半静默弃轮、一半掀进程，不可接受。修法：云面异常收敛到一个类型 + 一个处置点，`json.loads` 进护栏；结局只有两种且**都要计数**。③ 密钥缺失今天"当场可见但只折日志"（`cloud/__init__.py:48-53` → `asr_fail` + 一行 print，**不回错给对话方、无告警面**），且 `RR3_BRAIN_CLOUD=mock` 使 `*_fail` 与缺 key 分支**永不可达**（3 处代码分支 + 3 处 spawn）⇒ **sim 绿的语音链没有覆盖过任何一条云失败路径**，须写进可达终点表而不是当成就。**同族既有项**：D7/B-13 的 `RR3_CALL_TIMEOUT_S` 仍住码内（30 s），封轮上限与云预算解耦那条不变 **2026-10-20 对表（批N4 件六一笔，①② 结清）**：① 计数拆账兑现——`chat_rounds` 退役、改 `rounds_replied`/`rounds_audible` 两本账；② 云面异常收敛一个处置点＋`json.loads` 进护栏；③ 密钥缺失可见性随 mock 档拆分推进（`RR3_BRAIN_CLOUD=real` 已上线）。同族项：`RR3_CALL_TIMEOUT_S` 已随 B-13① 数据化退役 | **①② 已结（批N4 件六一笔）**；③ 留敞口 | B-9；B-13；B-15；I-21；全案 §20.6/§20.7；索引 L-27 |

### §11 待证与敞口

1. 真云从未联调；端到端时延无实测；`SYSTEM_PROMPT`/`LLM_MAX_TOKENS` 是 legacy 移植值（来源在案）。
2. 本册 §8 的用例存在性依据是 conventions 登记的 brain-unittest 双矩阵 job —— **该 job 在远端从未执行过**，
   所以"有测试"与"测试跑过"是两件事（与全仓敞口第 1 条同源）。
3. `intent.py` / `bus.py` 未登件（本册 §0 已写明它们是壳）——避免被误读为"漏登记的五件套"。

### §12 跨档动作（host / rtos / bare 逐件）

单元级判定先行：**brain 属于主承载层**（ADR-0018 裁定 3/修订 1——认知与云是"有 OS 才叠加"的那一层）。
所以本册的跨档表不写"怎么把 Python 塞进 MCU"，而写**三档里 brain 的位置怎么摆**：

| 档 | brain 在哪 | 本单元件跨档动作 | 理由与代价 |
|---|---|---|---|
| host 全承载 | 本机进程（今天） | 不适用（豁免登记，见 §1） | 认知四格跑 std Python，两腿对拍门与闭包门本就不适用 |
| rtos 轻承载（ESP32 类微型形态经桥） | **仍在 host 侧**，板只经桥把 pcm/vad 上灌、把 intent 下领 | 板侧**没有 brain 件**；跨档动作全在桥的语义面（`⊙` 经桥拓扑，architecture §12.7） | 代价 = 云 RTT 进对话闭环（本册 §4 的一轮最坏 ≈90 s 就是这个代价的形状）；收益 = 一枚 MCU 不必装下认知层 |
| bare 无承载（自足微型） | **不存在** | 无。该形态"不起对话轮"是设计事实，不是降级 | 索引 §10 的 F-C 形态据此成立；别在描述符里给它挂 `cognition.*` 格——挂了就是要求一个不可能满足的依赖（与 D3 同类错误） |

**Rust 化后（ADR-0016）本节的改写预告**：`dialog` 是唯一可能要跨档的认知件（它只吃时间戳与边沿，
纯 `core` 可达），`asr`/`llm`/`tts` 三件永远绑云 ⇒ 它们的依赖闭包里 `reqwest`/TLS 是**主承载层专属**，
按 ADR-0019 修订 4 的发布权判据，这三件在 rtos/bare 档直接不启用，而不是"改造后可用"。

### §13 策略字面量清单

| 文件 : 符号 | 字面量 | 语义 | 读者 | 去处 |
|---|---|---|---|---|
| `rr3_brain/intent.py` | `_SERVO_ANGLE_LEFT/CENTER/RIGHT = 45/90/135` + 关键词表 | 意图映射=**策略** | 有 | ✅ 落位正确（L4 就该在 brain）；但三个角值与 act 的行程钳位是**同一物理事实的两处表达** → 角值应由描述符/ops 供，或至少在 §9 断言"三者必落在 `[angle_min, angle_max]` 内"（现在靠人记） |
| `rr3_brain/cloud/llm.py` | `SYSTEM_PROMPT`、`LLM_MAX_TOKENS=150`、`LLM_HISTORY_ROUNDS=4` | 人格与预算=**策略** | 有 | ✅ 落位正确；`SYSTEM_PROMPT` 的出处已在 §11 登记为 legacy 移植值 |
| `config.toml [rr3-brain.cloud]` | `base_url`、`asr_model`、`llm_model`、`tts_model`、`timeout_s=20` | 云面 | 有（`composite.py` 读 config，**不是 env**） | ✅ 合规；§3 旧版"env 注入"的说法已订正 |
| `composite.py : RR3_CALL_TIMEOUT_S` | `30.0` | 点名 interact 的等待上界 | 有 | **硬编码在代码里、不在 config** → ops 槽（B-9）：它是"一轮最坏时长"的组成部分，调它要改码就违背数据面原则。**2026-10-20 对表（批N4 件六一笔 B-13①）**：已数据化为 `[rr3-brain.bus].call_timeout_s=30/connect_timeout_s=5`，该常量退役，本行取证时点写法保留 |
| `config.toml` | `max_utterance_ms=8000` 有读者；`silence_tail_ms=700`、`max_turn_chars=800` **无读者** | 轮次面 | 部分 | 两个死键分别走 B-2（删或接）与 B-8；**"假活配置面"在 brain 有两处**，与 interact 五键、connect 三项同批清 |
| `composite.py : SUBSCRIBES` / `TOPIC_PCM` / `TOPIC_VAD` | `"rr3-interact/asr/text"`、`"rr3-connect/net/link"`、`"rr3-sense/audio/pcm"`、`"…/audio/vad_start"` | 订阅面与实然面**两套字面量** | 有（实然那份真在用，申报那份只 `net/link` 被读、且读了不处理） | 契约面修正走 C14/B-1；**双写债在此最典型**：申报面与实现面各写一遍主题串，改一处就漂 |
| `rr3_brain/cloud/*.py` | 路径串 `v1/audio/transcriptions`、`v1/chat/completions`、`v1/audio/speech` | 云 API 形状 | 有 | 契约面（OpenAI 兼容形状）；换云供应商时**必须改代码的地方不止这里**——见下一行 |
| env 面 | `RR3_CLOUD_API_KEY`（**通用键名**，MC-8② 已结） | 凭据键名 | 有 | 凭据不落盘 ✅；键名不再焊厂商：供应商由 config `[rr3-brain.cloud]` 承载，换云只改配置不改码（原 `SILICONFLOW_API_KEY` 已退役） |
| `session/测试面` | `RR3_BRAIN_CLOUD=mock`、`tests/` 内假响应 | 注入点 | 有 | ✅ 正确（mock 在测试注入位）；§6 已钉"mock 不是在位证据" |
| 硬编码在件里的形态参数 | 心跳 2000 ms 拍点、`poll` 超时等 | 机制参数 | 有 | 与五册同纪律：进 ops/config，别停在 `.py` 常量 |

### §14 基线对表（代码 `22783ea`，2026-10-10）

- **契约面（B-1 / R-3）已结**：`rr3-brain/module.toml` 现 `subscribes = ["rr3-sense/audio/pcm",
  "rr3-sense/audio/vad_start"]`、`publishes = ["rr3-brain/intent/result"]`——申报面第一次等于实现面。
  （**MC-1 增补（2026-10-13）**：`publishes` 增 `"rr3-brain/audio/tts_pcm"`——走"补实现"合法形式，
  主题本体在 MCP-1 评审时已在 `topics.toml` 在册，计数 30 未动；见 0.0.5 方案 §20 第 5 条。）
- **B-2 已结**（`silence_tail_ms` 退役，判停在 sense 的 `Offset`）；**B-8 未结**（`max_turn_chars` 仍零读者）。
- **B-7 已结（以一种我没写过的合法方式）**：`net/link` 订阅申报被**撤下**而不是补实现——
  按 R-3 的"补实现或撤申报二选一"，实现侧选了后者。本册 §2 那句"链路 down 不起新轮"从此**不再是待实现项**，
  而是被撤销的主张；若将来要恢复，须重走申报面变更 + C14 类评审。
- **门禁口径**：brain 的 26 条 stdlib unittest 仍**不在 `check.sh` 十步内**，但 `.github/workflows/ci.yml`
  的 `brain-unittest` 双矩阵 job 是真的（远端首跑回执待取——R30 起押 GitHub Actions → 只算"清单在册未执行"，不折算达标）。
- **§12 的三档表不受影响**：brain 属主承载层，rtos/bare 档不启用认知格——`esp32-mini.toml` 的
  `[enabled_slots]` 不列 `cognition.*`，正是本册 §12 那行的实现证据。

#### §14.1 增量对表（2026-10-12，代码基线 `78625ae`；全仓总表见 [索引 §13.4](模块设计索引.md)）

| 项 | 复验实况（符号） | 判定 / 新落点 |
|---|---|---|
| B-4 Python 测试进门禁 | `.github/workflows/ci.yml` 的 `brain-unittest` job（`setup-python 3.12` + `python3 -m unittest discover -s tests`），八 job 首跑全绿已随 0.0.7 批H 落档 | **结**（本机仍无载体：Windows `python` 坏——按"远端执行 / 本地无载体"分档，不折算成本机绿） |
| B-5 起轮/云失败 counters | `composite.py` 的 `rr3_counter_bump("asr_ok"/"asr_fail"/"llm_fail"/"tts_fail")` | **结** |
| B-8 `max_turn_chars` | 键已从 `config.toml` 删除（文件注记自陈"在册无读者；实装截断随真用例回归"） | **结（走"删"分支）**；截断行为将来要回须连同回归用例立单 |
| B-9 `RR3_CALL_TIMEOUT_S = 30.0` | 仍是 `composite.py` 常量（注释里它是"一轮最坏 ≈90 s"的组成项） | **未结 → 0.0.5 MC-10**（ops/config 槽，与 A-9/I-11/S-5 同族"策略出件"）——**2026-10-20 对表（批N4 件六一笔）**：已结清，数据化为 `[rr3-brain.bus]` 两键、legacy 常量退役（§10 B-9 行） |
| B-10 `SILICONFLOW_API_KEY` | **已结（批M MC-8②）**：`cloud/__init__.py : API_KEY_ENV` 改 `RR3_CLOUD_API_KEY`，供应商归 config（换云只改配置不改码） | **结** |
| B-11（本册新立）TTS PCM 上线 | **已结（批M MC-1，2026-10-13）**：`rr3_finish_turn` 已把 `reply_pcm` 按 20ms/640B 分帧发布 `rr3-brain/audio/tts_pcm`（seq 模块级单调、`ms` 由样点数导出），`tts/text` 点名 args 增 `pcm_bytes` 作收流完整性唯一判据；跨语言 golden（`contracts/golden/tts-pcm-frames.json`）两腿对拍 | **MC-1 已结**（通路对**引擎中立**；对话闭环施工仍在**增补轮五批E**） |
| R31 **Q3** 对本册的改判 | ASR/TTS 首发为**本地 vosk + Piper**，brain 侧立 adapter 缝，**云后端实现位留空不写半成品**（ADR-0008 裁定 4 口径） | 本册 §0/§5 的"三家云客户端"读作**现状**（存量实现），不再读作目标形态；"云真 key 联调"敞口**不再是 F-3/F-4 的阻塞项**——这条订正同时收回本册与 0.0.5 §17 里"欠真 key"的写法 |
