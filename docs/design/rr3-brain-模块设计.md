# rr3-brain 模块设计（框架 v1.1）

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程，`type = brain`、`lang = python`（装机 shim 语义，ADR-0014）；`start_priority = 80`、`mem_limit_mb = 256`、`restart_policy = "on-failure"` | `module.toml` |
| 格 | `cognition.dialog` · `cognition.asr` · `cognition.llm` · `cognition.tts` | `contracts/slots.toml` |
| 件 | 四件为**两件式 manifest**（`atom.toml` + `assemble.toml`），机制本体 `.py` 原位；**`intent.py` / `bus.py` 不是件**，属组合件机制壳 | `atoms/*/`、`rr3_brain/` |
| 主题 | 发布 `intent/result`；**申报订阅** `interact/asr/text` + `connect/net/link`；**实际订阅** `sense/audio/pcm` + `audio/vad_start`（未申报）；RPC `llm/completion` | `module.toml` 与 `composite.py`（`TOPIC_PCM`/`TOPIC_VAD`） |
| 句柄 | 无能力面句柄（云 HTTP；**配置读自 `config.toml` 的 `[rr3-brain.cloud]`，只有 API key 走 env**） | `cloud/*.py`、`composite.py` |
| 参数 | `base_url`、`asr_model`、`llm_model`、`tts_model`、`timeout_s=20`、`max_utterance_ms=8000` **有读者**；`silence_tail_ms=700`、`max_turn_chars=800` **在册无读者**（B-2/B-8）；另有一个不在 config 里的硬编码上界 `RR3_CALL_TIMEOUT_S = 30`（点名 interact 的等待） | `config.toml`、`composite.py` |
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
| `handles` | 无能力面句柄（HTTP + 总线） |
| `ops`（策略，落位正确） | 模型名与 `base_url`（**读自 `config.toml` 的 `[rr3-brain.cloud]`**，不是 env；只有 `SILICONFLOW_API_KEY` 走 env）、`SYSTEM_PROMPT`、`LLM_MAX_TOKENS=150`、`LLM_HISTORY_ROUNDS=4`、`timeout_s`、`max_utterance_ms`、舵机角 45/90/135、关键词表；⚠️ `max_turn_chars` 在 `config.toml` 里在册但**无读者**（B-8：实装或删除）；⚠️ `silence_tail_ms` 同为无读者的死参数（B-2） |
| 契约面（本册要改的） | `subscribes` 必须**只增**补齐为实然集（`sense/audio/pcm`、`sense/audio/vad_start`），`interact/asr/text` 走 retired |

**为什么策略在这里是对的**：ADR-0019 裁定 5 的 L4 就是 brain；把这些外移反而违反分层。本单元唯一
不合规的是**申报面**，不是策略内容。

## §4 主循环与时序预算

单循环：`poll` 收帧 → pcm/vad 喂 dialog → 起轮后**串行**跑 asr→llm→tts → 调 `interact/tts/text`（点名）
→ 发 `intent/result`。真实量：云 `timeout_s = 20`/段，三段 = 60 s，**再加一段同轮内的点名等待**
`RR3_CALL_TIMEOUT_S = 30`（调 `interact/tts/text`）→ 一轮最坏 **≈ 90 s**（旧版写 60 s 漏了这段），
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
| `audio/pcm` | stream | 消费 | `seq, pcm_b64, ms` | **未申报**（实然消费） |
| `audio/vad_start` | event | 消费 | `seq, edge, doa?` | **未申报** |
| `net/link` | event | **申报消费、代码无处理**（`SUBSCRIBES` 里有，事件分支里没有） | up/ssid/rssi_dbm/mode | **申报超前**（B-7） |
| `asr/text` | event | 申报消费 | 在册 | 生产侧不存在 → retired |
| `intent/result` | event | 发布 | `actions[]: {topic,args}` | 一致 |
| `llm/completion` | RPC | 点名 | 文本 → 回复 | 一致 |
| `interact/tts/text` | RPC（调用方） | 点名 | `{text}` | 不占订阅面（点名形状） |

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
| dialog 轮次 | vad/pcm 时间戳序列（起音→静音→超时） | 裁定语义（**判停归 sense**、8000 ms 封轮、一轮一流水） | 封轮 / 尾长差 1 拍 / 连续两轮 / 云 RTT 与封轮的关系 | ⚠️ 实况打折：`tests/test_dialog.py` 有 7 条，但封轮那条把 `max_utterance_ms` **设成 0** 来触发（没有测 8000 这个名义边界）；无尾长差 1 拍用例；"连续两轮"只测了缓冲重置；云 RTT 与封轮的关系**零覆盖**（B-3）。且这些用例**在九步门外**——`check.sh` 不含 Python，`.gitcode/workflows/ci.yml` 虽有 `brain-unittest` 双矩阵 job，atomgit 的 Actions 从不执行 |
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
| B-8 | `max_turn_chars` 无读者：实装截断或删键（与 power P-6、connect N-6、interact I-1 同批同纪律） | 批I | — |
| B-3 | 云总预算与 `max_utterance_ms` 的关系定案（轮次超时是否覆盖云 RTT） | **待裁** | — |
| B-4 | Python 测试接进门禁（现在只登记 conventions，不进 `check.sh`；本机 Python 3.9 不过 3.11 门） | 批K + 宿主 | C11 |
| B-5 | 起轮/云失败 counters（健康面可见性） | 批K | C9 |
| B-6 | Rust 化随 ADR-0016（届时两腿对拍与闭包门才适用） | ADR-0016 | — |

## §11 待证与敞口

1. 真云从未联调；端到端时延无实测；`SYSTEM_PROMPT`/`LLM_MAX_TOKENS` 是 legacy 移植值（来源在案）。
2. 本册 §8 的用例存在性依据是 conventions 登记的 brain-unittest 双矩阵 job —— **该 job 在远端从未执行过**，
   所以"有测试"与"测试跑过"是两件事（与全仓敞口第 1 条同源）。
3. `intent.py` / `bus.py` 未登件（本册 §0 已写明它们是壳）——避免被误读为"漏登记的五件套"。
