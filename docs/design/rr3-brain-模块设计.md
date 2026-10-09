# rr3-brain 模块设计

> 证据：`modules/rr3-brain/module.toml`、`rr3_brain/{composite,dialog,intent,bus}.py`、
> `rr3_brain/cloud/{__init__,asr,llm,tts}.py`、`atoms/{dialog,asr,llm,tts}/atom.toml`、`config.toml`。

## 1 职责

**`⊙` 编排的唯一承担者**（ADR-0012 裁定 4）：把音频边沿与文本变成意图，并把意图投给执行单元。
它是 L4 策略层的家（人格、阈值、对话规则都在此，**这是正确落位，不是债**）。
格账：`cognition.dialog` + `cognition.asr` + `cognition.llm` + `cognition.tts`。

## 2 事实面

| 件（两件式 manifest） | 格 | 机制本体 | 云面 |
|---|---|---|---|
| `dialog` | `cognition.dialog` | `rr3_brain/dialog.py`：utterance 边界状态机（起轮 / 判停 / `max_utterance_ms = 8000` 上限），吃 `audio/pcm` 与 `audio/vad_start` | — |
| `asr` | `cognition.asr` | `cloud/asr.py`：整段 WAV 一次上传（OpenAI 兼容 `v1/audio/transcriptions`） | SenseVoice 适配器；mock 云（R4） |
| `llm` | `cognition.llm` | `cloud/llm.py`：多轮历史（`LLM_HISTORY_ROUNDS`）、`LLM_MAX_TOKENS = 150` | `SYSTEM_PROMPT` 在 `cloud/__init__.py` |
| `tts` | `cognition.tts` | `cloud/tts.py`：合成 PCM，验证性消费 | CosyVoice2 |

`composite.py` 是组合件机制主体（`__main__.py` 17 行瘦入口）；`bus.py` 是总线壳（ADR-0014 shim）；
`intent.py` 是**意图裁决**（关键词 → `servo_angle`，`_SERVO_ANGLE_LEFT/CENTER/RIGHT = 45/90/135`）。
申报面：`publishes = ["rr3-brain/intent/result"]`、`subscribes = ["rr3-interact/asr/text",
"rr3-connect/net/link"]`、`exposes = ["rr3-brain/llm/completion"]`、
`[depends].required = [bus.subscribe, bus.rpc, net.online]`、`optional = [power.battery]`、
`restart_policy = "on-failure"`、`mem_limit_mb = 256`、`start_priority = 80`、心跳 2000 / 看门狗 10000。

**本册最重要的一条事实**：真实运行拓扑是 **`sense → brain` 直连**——`composite.py` 里有
`TOPIC_PCM = "rr3-sense/audio/pcm"` 与 `TOPIC_VAD = "rr3-sense/audio/vad_start"`，
而这两个主题**不在申报的 `subscribes` 里**；被申报的 `interact/asr/text` 反而在生产侧无人发布。
即：**契约面上的语音入口是断的，代码里的语音入口是通的**（索引 §2 首行）。

## 3 件级切分与闭包（设计目标）

- brain 今天**不适用**件级对拍门与依赖闭包门：Python 面按 ADR-0014 是宿主档专属承载，
  Rust 化随 ADR-0016 线（这条在 ADR-0019 修订 1 的敞口里已登记，不写作已达标）。
- 但**契约面申报必须补齐**，与语言无关：`subscribes` 应等于真实订阅集（pcm + vad_start + net/link，
  asr/text 若不存在生产者则走 retired）。这不是设计洁癖——裁剪一个形态时，"谁依赖谁"只能从申报面读，
  申报错则降级推演全错。
- `intent.py` 的角度常量是**策略**且落位正确（在 brain）；它与 act 的行程钳位是"策略提案 + 安全否决"
  的关系，act 侧 `rr3_travel_clamp` 必须在任何情况下都能否掉 45/90/135 之外的值——这条要在两册里
  互相指认（已指认）。

## 4 职责四面

| 面 | 落点 |
|---|---|
| 接入 | 订阅 pcm / vad_start / net/link；`llm/completion` 点名 |
| 运行 | dialog 状态机 → asr → llm → tts 的四段流水（云 HTTP 阻塞式） |
| 状态 | `intent/result` 发布；本轮无本地可观测面（面板/日志属 rrd 侧） |
| 守护 | 无本地兜底（**设计如此**：兜底不得依赖云端在场）；`net/link` down 时应停止上行而不是排队重放 |

## 5 时序设计

云面 `timeout_s = 20`、`max_utterance_ms = 8000`、`silence_tail_ms = 700`、`min_speech_ms = 200`
（与 sense 的 `silence_tail_ms` 是**同一个物理量的两处声明**——这是设计缺陷：一份事实两个真值点，
待 ops 统一）。整轮对话的端到端时延今天没有任何实测数（mock 云不代表时延），列为敞口。

## 6 降级与缺格语义

| 情形 | 设计行为 |
|---|---|
| `net.online` / `net.wifi` 缺 | 四格全 `Degraded`（云件不可用），但**不得**阻塞 sense / act / power 的本地链 |
| 只有 asr 不可用 | 允许退化为"文本输入"（`llm/completion` 点名面），须显式标注而非静默 |
| 无音频回放（无声形态） | tts 产出可丢弃，但 intent 仍要发（意图与声音解耦） |
| mock 云（R4） | 是**测试注入点**，不是降级态；契约面上不得把 mock 当在位证据 |

## 7 红线

`intent/result` 载荷形状（`actions[]` 的 `{topic, args}`，act 侧按它执行且要过两道归属门）、
`llm/completion` 应答、凭据不落盘、`type = brain` 与 `lang = python`（装机 shim 语义，ADR-0014）。

## 8 待证与敞口

1. 真云联调未做（凭据与云侧不在范围）；时延无实测。
2. 申报订阅与真实订阅不一致（§2）——**这是本轮新查出的契约面缺陷，属批J 前要结清的债**。
3. `silence_tail_ms` 双点声明的归一方式待裁（建议：sense 为事实源，brain 只读 vad_start 的载荷，
   不再自带判停尾参数）。
4. `intent.py` / `bus.py` 不是件（无 `atom.toml`），它们属组合件机制壳——本册把它写进结构，
   免得读者以为漏登记。
