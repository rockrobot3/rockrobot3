# rr3-brain 模块设计（框架 v1.3）

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程，`type = brain`、`lang = python`（装机 shim 语义，ADR-0014）；`start_priority = 80`、`mem_limit_mb = 256`、`restart_policy = "on-failure"` | `module.toml` |
| 格 | `cognition.dialog` · `cognition.asr` · `cognition.llm` · `cognition.tts` | `contracts/slots.toml` |
| 件 | 四件为**两件式 manifest**（`atom.toml` + `assemble.toml`），机制本体 `.py` 原位；**`intent.py` / `bus.py` 不是件**，属组合件机制壳 | `atoms/*/`、`rr3_brain/` |
| 主题 | 发布 `intent/result` + `rr3-brain/audio/tts_pcm`（MC-1 下行载荷流，2026-10-13 申报随实现补齐）；订阅 `sense/audio/pcm` + `sense/audio/vad_start`（B-1 已申报）；RPC `llm/completion` | `module.toml` 与 `composite.py`（`TOPIC_PCM`/`TOPIC_VAD`/`TOPIC_TTS_PCM`） |
| 句柄 | 无能力面句柄（云 HTTP；**配置读自 `config.toml` 的 `[rr3-brain.cloud]`，只有 API key 走 env**） | `cloud/*.py`、`composite.py` |
| 参数 | `base_url`、`asr_model`、`llm_model`、`tts_model`、`timeout_s=20`、`max_utterance_ms=8000` **有读者**；`silence_tail_ms=700`、`max_turn_chars=800` **在册无读者**（B-2/B-8）；另有一个不在 config 里的硬编码上界 `RR3_CALL_TIMEOUT_S = 30`（点名 interact 的等待）**2026-10-20 对表（批N4 件六一笔 B-13①）：该常量已退役**——总线 RPC 超时数据化为 `[rr3-brain.bus].call_timeout_s/connect_timeout_s`（缺键回落 `RR3_BUS_CALL_TIMEOUT_DEFAULT_S`；`connect_timeout_s` 后随批N32 迁移退役删除，见 §4 B-13① 注记），见 §13 `composite.py` 行与 B-13 对表 | `config.toml`、`composite.py` |
| 健康 | 心跳 2000 / 看门狗 10000 ms；`[depends].required = [bus.subscribe, bus.rpc, net.online]`，`optional = [power.battery]` | `module.toml` |

## §1 件契约表（Python 面按模块边界写，不虚构 Rust 签名）

| 件 / 成分 | 公开函数（现状） | 输入 → 输出 | 状态 | 失败语义 |
|---|---|---|---|---|
| dialog | `Rr3Dialog.rr3_feed_pcm(body)` / `rr3_feed_vad(body)`（旧版写的 `on_pcm`/`on_vad` 不存在） | pcm 样点与 vad 边沿 → 轮次边界（起轮 / 封轮） | `_active`、`_started_ms`（`time.monotonic()*1000`） | 无异常上抛（吞帧继续） |
| asr | `cloud/asr.py` HTTP `v1/audio/transcriptions`（multipart，`model` 字段） | 整段 WAV → 文本 | 无 | 超时/非 2xx → 记错、本轮降级 |
| llm | `cloud/llm.py` HTTP `v1/chat/completions`（`SYSTEM_PROMPT` + 历史 `LLM_HISTORY_ROUNDS` + `LLM_MAX_TOKENS=150`） | 文本 → 回复 | 多轮历史 | 同上 |
| tts | `cloud/tts.py` HTTP `v1/audio/speech` → PCM | 文本 → PCM | 无 | 同上 |
| intent（壳） | `rr3_extract_actions(...)` + `_SERVO_ANGLE_LEFT/CENTER/RIGHT = 45/90/135` + 关键词表 → `actions[]` | **用户原话（user_text）**→ 意图载荷；`composite.py` 调的是 `rr3_extract_actions(user_text)`，且 `contracts/events/brain-intent-result.schema.json` 与 `intent.py` 的 docstring 都明写**不得拿回复文本匹配**（旧版"回复文本 →"是反的） | 无 | 不匹配 → 无动作 |
| bus（壳） | 注册 / 心跳 / poll / call | — | task | 传输性失败 → 进程退出交监督 |

**目标**：Rust 化随 ADR-0016；在那之前本单元**不适用**件级两腿对拍门与依赖闭包门（登记豁免，不是达标）。

## §2 状态机（dialog 轮次）

不变式（按代码实况，含两条"设计有、代码无"的缺口）：`Onset` 边沿起轮；**判停不靠本单元**——`dialog.py`
只认 sense 发来的 `Offset` 边沿，它自己的 `silence_tail_ms` 键**没有任何读者**（B-2 因此从"归一重复声明"
改判为"删除死参数"）；超过 `max_utterance_ms = 8000` 由 `rr3_utterance_expired` 强制封轮；
一轮至多一次 asr→llm→tts 流水；**"链路 down 不起新轮"今天只是设计主张**——`SUBSCRIBES` 列了
`rr3-connect/net/link`，但 `composite.py` 的事件分支只处理 pcm/vad，链路状态既没被记录也没被问过（B-7）。
非法输入收敛方向：云面失败 → 本轮回"无意图"（不发假 intent），而不是重试排队。

## §3 注入面

| 面 | 内容 |
|---|---|
| `handles` | 无能力面句柄（HTTP + 总线）；四件 `assemble.toml` 只报 ops（批M MC-10 L-5，2026-10-13 已结） |
| `ops`（策略，落位正确） | 模型名与 `base_url`（**读自 `config.toml` 的 `[rr3-brain.cloud]`**，不是 env；只有 `RR3_CLOUD_API_KEY` 走 env——通用键名，MC-8② 已结）、`SYSTEM_PROMPT`、`LLM_MAX_TOKENS=150`、`LLM_HISTORY_ROUNDS=4`、`timeout_s`、`max_utterance_ms`、舵机角 45/90/135、关键词表；⚠️ `max_turn_chars` 在 `config.toml` 里在册但**无读者**（B-8：实装或删除）；⚠️ `silence_tail_ms` 同为无读者的死参数（B-2） |
| 契约面（本册要改的） | `subscribes` 必须**只增**补齐为实然集（`sense/audio/pcm`、`sense/audio/vad_start`），`interact/asr/text` 走 retired |

**为什么策略在这里是对的**：ADR-0019 裁定 5 的 L4 就是 brain；把这些外移反而违反分层。本单元唯一
不合规的是**申报面**，不是策略内容。

## §4 主循环与时序预算

单循环：`poll` 收帧 → pcm/vad 喂 dialog → 起轮后**串行**跑 asr→llm→tts → `rr3_finish_turn` 把 TTS 产出的 PCM 按 20ms/640B 分帧发布 `rr3-brain/audio/tts_pcm`（seq 模块级单调不随轮次复位、`ms` 由样点数导出，MC-1）→ 调 `interact/tts/text`（点名，args 携 `text` + `pcm_bytes` 字节总数作收流完整性判据）→ 发 `intent/result`。真实量：云 `timeout_s = 20`/段，三段 = 60 s，**再加一段同轮内的点名等待**
`RR3_CALL_TIMEOUT_S = 30`（调 `interact/tts/text`）→ 一轮最坏 **≈ 90 s**（旧版写 60 s 漏了这段）
**2026-10-20 对表（批N4 件六一笔 B-13①）：该等待数据化为 `[rr3-brain.bus].call_timeout_s`
（缺省 30，回落常量改名 `RR3_BUS_CALL_TIMEOUT_DEFAULT_S`；`connect_timeout_s=5` 新增）——
一轮最坏 ≈ 90 s 的预算形状不变，读数从 config 取**；
**2026-10-24 对表（批N32 件二，增补评审裁定 4）：`connect_timeout_s` 随迁移退役删除**——
Rust 迁移件遵 rr3mod bootstrap 固定序，注册帧等待上界由 rr-bus-api 内部承载，键无消费点
即死键（批I B-2 先例）；`call_timeout_s` 保留（`BusHandle::call` timeout 显式实参消费点），
而 `max_utterance_ms = 8000` 只管"起轮到封轮"的采集段，管不到云段——两者关系仍无人定义（工单 B-3）；
心跳 2000 ms 独立线程，故"卡在云上 90 s"期间心跳仍续 →
面板/健康看不出异常（同全系统"活着但不动"类别）。

| 量 | 名义 | 实测 |
|---|---|---|
| 起轮 → 判停 | 700 ms 静音尾（+20 ms 拍） | 无 |
| 判停 → intent 落执行 | asr RTT + llm RTT + tts RTT + 点名 | 无（mock 云不代表时延） |
| 饿死可见性 | 需要 counters（本轮起轮数、云失败数） | 未做 |

## §5 主题面契约

| 主题 | kind | 触发 | 载荷 | 申报与实现 |
|---|---|---|---|---|
| `audio/pcm` | stream | 消费 | `seq, pcm_b64, ms` | **已申报**（B-1 只增补齐） |
| `audio/vad_start` | event | 消费 | `seq, edge, doa?` | **已申报**（B-1） |
| `net/link` | event | **申报消费、代码无处理**（`SUBSCRIBES` 里有，事件分支里没有） | up/ssid/rssi_dbm/mode | **申报超前**（B-7） |
| `asr/text` | event | 申报消费 | 在册 | 生产侧不存在 → retired |
| `intent/result` | event | 发布 | `actions[]: {topic,args}` | 一致 |
| `rr3-brain/audio/tts_pcm`（发） | event stream | 发布（MC-1，每轮 TTS 完成后） | `{seq, format:"s16le", rate:16000, channels:1, ms, pcm_b64}`，20ms/640B 分帧，seq 模块级单调 | 申报随实现落地（2026-10-13）；逐字段复用上行 `rr3-sense/audio/pcm` 形状（MCP-1） |
| `llm/completion` | RPC | 点名 | 文本 → 回复 | 一致 |
| `interact/tts/text` | RPC（调用方） | 点名 | `{text, pcm_bytes}`（MC-1：字节总数 = 收流完整性唯一判据） | 不占订阅面（点名形状） |

## §6 降级矩阵

| 格 | 缺格 / 不可用 | 设计行为 |
|---|---|---|
| `cognition.asr` | 云不可达 | 退化为"文本输入"（`llm/completion` 点名面仍在），**须显式标注**不得静默 |
| `cognition.llm` | 云不可达 | 本轮无回复、无动作；本地链（sense/act/power）不受影响 |
| `cognition.tts` | 无音频回放形态 | PCM 产出丢弃，**intent 仍要发**（意图与声音解耦） |
| `cognition.dialog` | 缺 pcm/vad 上游 | 不起轮；不得排队重放旧轮 |
| 四格全缺（无网形态） | 常见裁剪 | 期望：**其余单元全本地闭环照常**（这是 §7.3 兜底设计的验收点） |
| mock 云（R4） | 测试注入点 | **不是在位证据**，契约面不得据此声明 cognition 可用 |

## §7 安全核接入

无（且**不得有**）：兜底链不能依赖云端在场。本单元原本主张的"链路 down 时不起新轮"今天**没有实现**
（B-7），所以它连节流都算不上——现状是无链路状态判断，照样起轮、照样把三段云请求挂到超时。
补上之后它的定位仍是节流，不是安全核。

## §8 两腿对拍用例设计（Python 面的等价形式）

| 面 | 输入序列 | 期望值来源 | 边界集 | 现状 |
|---|---|---|---|---|
| dialog 轮次 | vad/pcm 时间戳序列（起音→静音→超时） | 裁定语义（**判停归 sense**、8000 ms 封轮、一轮一流水） | 封轮 / 尾长差 1 拍 / 连续两轮 / 云 RTT 与封轮的关系 | ⚠️ 实况打折：`tests/test_dialog.py` 有 7 条，但封轮那条把 `max_utterance_ms` **设成 0** 来触发（没有测 8000 这个名义边界）；无尾长差 1 拍用例；"连续两轮"只测了缓冲重置；云 RTT 与封轮的关系**零覆盖**（B-3）。且这些用例**在门禁步序外**——`check.sh` 不含 Python，`.github/workflows/ci.yml` 虽有 `brain-unittest` 双矩阵 job，远端首跑回执待取（R30：atomgit 无流水线功能，CI 改走 GitHub Actions，GitHub 仓库待建） |
| intent 映射 | 关键词集 + 空文本 | 舵机角常量表（45/90/135） | 不匹配 / 多关键词 | 有 |
| 载荷两腿 | `intent/result` 构造 | `contracts/topics.toml` + act 侧执行器对 `actions[]` 的接受形状 | 越权 topic（act 必须拒） | ⚠️ 跨单元断言缺 |

## §9 不变量与红线断言

- 断言：`intent/result` 的 `actions[]` 只含**已申报 exposes** 的主题；越权由 act 拒（两道归属门），本单元不得"猜"可执行面。
- 断言：云凭据不落盘、不进仓。
- 断言：一轮至多一次流水，且 `max_utterance_ms` 到点必封轮（防挂死轮次）。
- 断言：`type = brain` / `lang = python` 的装机 shim 语义零改动。

## §10 改造工单

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
| B-13 | **超时两种落点 + 孤儿键 + 悬空单重挂**（主线自查，实测）：① 云 HTTP 超时**已数据化**（`config.toml` 的 `[rr3-brain.cloud] timeout_s = 20`），而总线 RPC 调用超时**仍住码内**（`rr3_brain/composite.py` 的 `RR3_CALL_TIMEOUT_S = 30.0`，另 `bus.py` 的 `RR3_CONNECT_TIMEOUT_S = 5.0`）——同一类参数一半在配置一半在码，"参数已数据化"这句因此只能算半真；本单为各超时立 ops 槽，并把封轮上限 `max_utterance_ms` 与云预算**解耦**（D7 裁定：不写 `timeout_s×3` 这类派生凑法）**2026-10-20 对表（批N4 件六一笔 B-13①）：总线 RPC 超时已数据化**——`[rr3-brain.bus].call_timeout_s=30/connect_timeout_s=5`（config.toml 新段＋回落常量，legacy 两常量退役；"解耦"仍留敞口待裁）**。② `[rr3-brain] log_level = "info"` 经 grep 全仓 `.py` **零读者**——其余五册同类孤儿键各有单号（P-6/N-6/I-1/A-11/S-1），本册独缺，被读或删除。③ **B-9 挂在已收口的 MC-10 上属悬空归属**，本单接手并改挂 0.0.8 W-B。细则见 [全案](../process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §15.1 M-4/M-5 与 §12 D7 | ① **批N4**（面板可见可写）② **0.0.8 W-B** ③ 随 ② | I-15；门 k |
| B-14 | **模型连接表与 ops 申报分裂 + 档位角第二真值点 + 件面时钟未注入**（2026-10-15 复盘核实三条）：① `atoms/{asr,llm,tts}/assemble.toml` 申报的 `base_url`/`llm_model` 槽指向 `[rr3-brain.cloud]`，但**有 active 行时 LLM 实读的是行内 `base_url`/`model_id`**（`rr3_brain_model_row` 一路），`cloud.llm_model` 只在无表 fallback 路径被读——**同一个参数两个来源、装配申报只覆盖其中一个**（换云改哪一处？今天的正确答案是"看有没有 model 表"，这就是不可运维的形状）；② `rr3_brain/intent.py` 的 `_SERVO_ANGLE_LEFT/CENTER/RIGHT = 70/90/110` 是 act `angle_limit_deg` 的**第二处真值**，ADR-0021 已把行程收敛到执行件档案，本单负责让 brain 的档位角**由档案派生**（意图带语义位"左/中/右"，档案给角度）；③ `dialog.py` 的 `rr3_feed_vad`/`rr3_utterance_expired` 用 `time.monotonic()` 自取时基，测试只能把 `max_utterance_ms` 设成 0 变通——**Python 件面因此写不出逐拍对拍用例**（Rust 五册已按 `Rr3TimeBase` 收口，本条是 ADR-0016 迁移窗的尾巴）。细则与裁定见 [ADR-0022](../process/adr/ADR-0022-代码事实复盘-机制核纯净度与申报面逃逸与安全语义身体.md) 裁定 1/2/5 | ① **批N1**（申报面扩到组合件层后才有判据，见门 l/门 j）② **批N3**（随 A-13 档案）③ **0.0.8 W-C**（brain Python→Rust 同窗）。**2026-10-19 对表：批N3 评估改判批N4**——档案侧已落（A-13 ①② 件一/件二与件五行程收敛），但"档案给角度"的通道未立：语义位进 `TOPIC_SERVO_COMMAND` payload 属契约级变更须走评审（payload schema/golden 冻结门），语义位→档案键的映射（left→lo / right→hi / center→center）本身待裁定，且与 A-13③ `rr3_goto` 执行面（挂 A-16②/A-19① 独立小件）同链——批N3 件清单无此授权，硬塞即跨批依赖；留批N4 与 B-12/B-13 面板线同批（敞口表无增减，`intent.py` 三常量维持原状待通道落） | A-13；门 j；B-13 |
| B-15 | **证据号命名空间不存在，于是 D11 的界面门不可执行**（2026-10-15 证伪审查，全案 §18 F-24/§12 D11）：D11 裁"`armed=true` 不许面板单步置位——必须引用一条**真板对拍证据的编号**"，这条把"无真板对拍不接闸"从口头规矩做成界面门，方向是对的；但实测 `docs/evidence/` 与 `evidence/` **都不存在**，仓内也没有任何证据编号规则（`conventions.md` 只有"每行带证据路径"的散文，`contracts/golden/` 是 IPC 字节向量、不是板上对拍记录）。**没有可引用的编号，这条门就只能靠人肉判**——这正是本册反复批评的"承诺有名字、没有身体"在流程面的复现。本单三件事：① 立命名空间 `docs/evidence/<形态>/<编号>.md`（编号规则、必填字段：形态 / 格或角色 / 观测方法 / 结论 / 日期 / 取证人；**一次对拍一份文件，编号不复用**）；② 面板写面与装载期都**校验所引编号存在且形态匹配**，引用不存在的号即拒（fail-closed，与"缺键即红"同一列语义）；③ `armed=true` 之外的两个安全写（`severe_mv`、`current_ceil_ma`）同样要求引用证据号，**不得只带二次确认**。**归 brain 册的理由**：本册 §13 已立"mock 不得作为在位证据"（G-4）——"什么叫一条证据"是这个问题的正面答案，六册共引这一处命名空间。**注意不做的事**：本单不代替验证窗产出任何板上结论，它只规定"结论长什么样、住哪儿、怎么被引用" | **批N4 的前置**（命名空间与校验规则先行；实际证据随 0.0.6/0.0.7 与批N2 对拍产） | D11；G-4；B-13；全案 §12 D11/§18 F-24 |
| B-16 | **一轮对话"完成"与"用户听见"解耦，且云失败有两种结局**（2026-10-15 第六轮追流核实，全案 §20.6，索引 **L-27**）：① `composite.py:265-277` 里播放点名失败被 `except RuntimeError` 接住**只 print 一行**，随后照常 `publish intent` → `rr3_record` → **`chat_rounds + 1`**（`:288-291`）⇒ 计数器意义上的"完成一轮"与是否出声**完全无关**，而这本是最该被产线信任的一格数。裁定：**不许用一个词盖两件事**——要么点名失败即弃轮（不计 `chat_rounds`），要么把计数拆成 `rounds_replied` / `rounds_audible` 两本账，面板与 `/api/status` 同时展示。② 云调用异常**分两类两种结局**：`RuntimeError`（HTTP 非 200、缺 `text`）→ 计 `asr_fail`/`llm_fail`/`tts_fail` + 弃轮（`:238-241`）；而 `URLError`／超时／`asr.py:56` 那个**在 try 之外**的 `json.loads`（`JSONDecodeError`）不是 `RuntimeError` ⇒ **逃逸到进程退出**。同一类故障（云不可用）一半静默弃轮、一半掀进程，不可接受。修法：云面异常收敛到一个类型 + 一个处置点，`json.loads` 进护栏；结局只有两种且**都要计数**。③ 密钥缺失今天"当场可见但只折日志"（`cloud/__init__.py:48-53` → `asr_fail` + 一行 print，**不回错给对话方、无告警面**），且 `RR3_BRAIN_CLOUD=mock` 使 `*_fail` 与缺 key 分支**永不可达**（3 处代码分支 + 3 处 spawn）⇒ **sim 绿的语音链没有覆盖过任何一条云失败路径**，须写进可达终点表而不是当成就。**同族既有项**：D7/B-13 的 `RR3_CALL_TIMEOUT_S` 仍住码内（30 s），封轮上限与云预算解耦那条不变 **2026-10-20 对表（批N4 件六一笔，①② 结清）**：① 计数拆账兑现——`chat_rounds` 退役、改 `rounds_replied`/`rounds_audible` 两本账；② 云面异常收敛一个处置点＋`json.loads` 进护栏；③ 密钥缺失可见性随 mock 档拆分推进（`RR3_BRAIN_CLOUD=real` 已上线）。同族项：`RR3_CALL_TIMEOUT_S` 已随 B-13① 数据化退役 | **①② 已结（批N4 件六一笔）**；③ 留敞口 | B-9；B-13；B-15；I-21；全案 §20.6/§20.7；索引 L-27 |

## §11 待证与敞口

1. 真云从未联调；端到端时延无实测；`SYSTEM_PROMPT`/`LLM_MAX_TOKENS` 是 legacy 移植值（来源在案）。
2. 本册 §8 的用例存在性依据是 conventions 登记的 brain-unittest 双矩阵 job —— **该 job 在远端从未执行过**，
   所以"有测试"与"测试跑过"是两件事（与全仓敞口第 1 条同源）。
3. `intent.py` / `bus.py` 未登件（本册 §0 已写明它们是壳）——避免被误读为"漏登记的五件套"。

## §12 跨档动作（host / rtos / bare 逐件）

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

## §13 策略字面量清单

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

## §14 基线对表（代码 `22783ea`，2026-10-10）

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

### §14.1 增量对表（2026-10-12，代码基线 `78625ae`；全仓总表见 [索引 §13.4](模块设计索引.md)）

| 项 | 复验实况（符号） | 判定 / 新落点 |
|---|---|---|
| B-4 Python 测试进门禁 | `.github/workflows/ci.yml` 的 `brain-unittest` job（`setup-python 3.12` + `python3 -m unittest discover -s tests`），八 job 首跑全绿已随 0.0.7 批H 落档 | **结**（本机仍无载体：Windows `python` 坏——按"远端执行 / 本地无载体"分档，不折算成本机绿） |
| B-5 起轮/云失败 counters | `composite.py` 的 `rr3_counter_bump("asr_ok"/"asr_fail"/"llm_fail"/"tts_fail")` | **结** |
| B-8 `max_turn_chars` | 键已从 `config.toml` 删除（文件注记自陈"在册无读者；实装截断随真用例回归"） | **结（走"删"分支）**；截断行为将来要回须连同回归用例立单 |
| B-9 `RR3_CALL_TIMEOUT_S = 30.0` | 仍是 `composite.py` 常量（注释里它是"一轮最坏 ≈90 s"的组成项） | **未结 → 0.0.5 MC-10**（ops/config 槽，与 A-9/I-11/S-5 同族"策略出件"）——**2026-10-20 对表（批N4 件六一笔）**：已结清，数据化为 `[rr3-brain.bus]` 两键、legacy 常量退役（§10 B-9 行） |
| B-10 `SILICONFLOW_API_KEY` | **已结（批M MC-8②）**：`cloud/__init__.py : API_KEY_ENV` 改 `RR3_CLOUD_API_KEY`，供应商归 config（换云只改配置不改码） | **结** |
| B-11（本册新立）TTS PCM 上线 | **已结（批M MC-1，2026-10-13）**：`rr3_finish_turn` 已把 `reply_pcm` 按 20ms/640B 分帧发布 `rr3-brain/audio/tts_pcm`（seq 模块级单调、`ms` 由样点数导出），`tts/text` 点名 args 增 `pcm_bytes` 作收流完整性唯一判据；跨语言 golden（`contracts/golden/tts-pcm-frames.json`）两腿对拍 | **MC-1 已结**（通路对**引擎中立**；对话闭环施工仍在**增补轮五批E**） |
| R31 **Q3** 对本册的改判 | ASR/TTS 首发为**本地 vosk + Piper**，brain 侧立 adapter 缝，**云后端实现位留空不写半成品**（ADR-0008 裁定 4 口径） | 本册 §0/§5 的"三家云客户端"读作**现状**（存量实现），不再读作目标形态；"云真 key 联调"敞口**不再是 F-3/F-4 的阻塞项**——这条订正同时收回本册与 0.0.5 §17 里"欠真 key"的写法 |
