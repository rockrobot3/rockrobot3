# ADR-0012：Python 模块接入（py 总线客户端）与 M4 对话闭环

- **状态**：已接受
- **日期**：2026-10-01
- **关联**：[ADR-0006](ADR-0006-采用AiryIPC线格式与rr-ipc分层.md)（线格式与传输分层）· [ADR-0007](ADR-0007-控制面分层与模块级升级.md)（决策一：对话属事务面）· [ADR-0010](ADR-0010-rr3ctl控制面.md)（ctl 旁支不复用）· architecture.md §2（brain 是唯一编排者）· legacy/core/{asr,llm,tts}（API 形状的移植来源）

## 背景

M4 的交付：`rr3-brain`（Python）从占位变实装——接 rrd 总线 + 移植 legacy 三家云客户端，
在本机（macOS，sim 音频后端）跑通四真进程全语音对话闭环：
**rrd + rr3-sense（sim 源）+ rr3-brain + rr3-interact**。

动工前有三个没有现成答案的裁定点：Python 侧没有任何总线客户端（rr-bus 是 Rust crate，
Python 不能依赖）；云端三家的 key 自 legacy 泄露轮换后本机不可用（e2e 需要无 key 也能跑）；
四真进程的编排落点需要一个家。

## 决策

### 裁定 1：py 总线客户端 stdlib-only，落在 `rr3_brain/bus.py`，逐语义镜像 rr-bus 客户端

`rr3_brain/bus.py` 用 **标准库**（`socket` / `struct` / `json` / `threading` / `queue` /
`tomllib`）手写 128B 头编解码 + UDS 客户端。**不引任何第三方依赖**（pyproject 的
`dependencies = []` 不动）。并发形状与语义逐条对齐 `base/runtime/rr-bus/src/client.rs`：

- reader 线程独占读；应答按 JSON-RPC id 投 pending 表，其余帧进 inbox 队列，`poll` 排空；
- 心跳线程按 manifest 的 `heartbeat_ms` 喂 `rr3.heartbeat`，失败 = stderr 一行 +
  `os._exit(1)`（模块不装"活着"，死交 supervisor 按 restart_policy 裁定）；
- 注册：首帧 `Request` + `dst_task = RR3_TASK_RRD` + 方法 `rr3.register`，params 带
  manifest **全文**，`entry` 改写为本进程入口的绝对路径（对位 Rust 侧
  `rr3_manifest_entry_absolutize` 用 `current_exe`，Python 侧用 `sys.argv[0]` 的
  realpath——`python -m rr3_brain` 时它就是 `__main__.py` 的绝对路径，过 rrd 的绝对 entry 门）；
- `publish` / `rr3.call` / `reply_call` / 死亡探针（`rr3_connected`）同名同语义。

理由：brain 是仓里**唯一**的 Python 模块，`dependencies = []` 是 M2 以来的明文纪律
（骨架必须零依赖可导入）；HTTP 三家（裁定 2）同样用 stdlib `urllib`，整包零第三方。
128B 头的偏移/枚举与 `contracts/envelope.schema.json` 对齐，测试用
`contracts/golden/event-header-128.hex` 逐字节钉住（跨语言对拍不做第二份 golden）。

- 否决 **引 httpx / websockets / pydantic**：一个模块拉进一串依赖树，装机（Buildroot
  rootfs）体积与 audit 面都翻倍；`urllib` + `json` 足以覆盖本模块全部线上形状。
- 否决 **提取共享 py 包（如 base/runtime/rr-bus-py）**：第二个 Python 模块出现之前
  是过早抽象；真到那天再从 `rr3_brain/bus.py` 平移（函数已按包级 `rr3_brain_*` 命名规约
  落位，平移成本低）。

### 裁定 2：云客户端按 legacy 三家的 API 形状移植（SiliconFlow），密钥只走环境变量；mock 后端经 `RR3_BRAIN_CLOUD=mock` 注入

`cloud/{asr,llm,tts}.py` 移植 legacy 三家的**线上契约**（端点 / 请求体 / 应答解析逐项对齐，
流式上传等 ESP32 传输细节不复刻）：

| 家 | 端点 | 请求 | 应答解析 |
|---|---|---|---|
| ASR | `POST {base}/v1/audio/transcriptions` | multipart（model + WAV 文件，WAV 头仓内自建） | `{"text": …}` |
| LLM | `POST {base}/v1/chat/completions` | `{model, messages, max_tokens}`，历史 = system + 最近 4 轮 | `choices[0].message.content` |
| TTS | `POST {base}/v1/audio/speech` | `{model, input, voice, response_format:"pcm", sample_rate}` | 原始 PCM 字节流 |

- 密钥**只**从 `SILICONFLOW_API_KEY` 读（沿用 `cloud/__init__.py` 既有门，legacy
  `*_config.h` 硬编码教训）；base_url / 模型名 / 超时读 `config.toml` 的
  `[rr3-brain.cloud]`；人设 / `max_tokens` / 历史轮数 / 音色按 legacy
  `llm_config.h` / `tts_config.h` 原值移植（系统提示词、`bella` 音色、150 token、4 轮）。
- **mock 后端**：环境变量 `RR3_BRAIN_CLOUD=mock` 时三家换确定性行为（ASR 回固定文本、
  LLM 回显、TTS 回正弦 PCM）。默认 `real`，缺 key 时三家调用报"缺环境变量"错误——
  **不静默降级**。mock 是测试注入点，不是产品路径；它的存在让 e2e 与单测在本机
  （key 已泄露、轮换未确认）可跑，真云链路待 key 就位后用同一份代码换环境验证。

- 否决 **e2e 里起假 HTTP 服务器**：多一类进程与端口管理，mock 注入在进程内完成同样的
  "云客户端代码路径被走到"的证据（只有 socket 那一跳不同，那一跳归真云验证）。
- 否决 **key 缺失时自动回 mock**：静默降级会让"真云没配好"看起来像"一切正常"，
  与 ipct003 的 SKIP 语义同一原则——条件不满足要显式说，不许装没发生。

### 裁定 3：对话回路归属 brain（唯一编排者），utterance 判停 = VAD offset 边沿 或 时长上限

brain 订阅（经 rrd 广播全收）`rr3-sense/audio/pcm`（stream，`pcm_b64` s16le）与
`rr3-sense/audio/vad_start`（event，onset/offset 边沿），对话状态机：

1. **onset** → 起一轮 utterance，开始积累 PCM；
2. **offset** → 判停（sense 的 VAD 已按 `silence_tail_ms` 判静默）；**时长上限**
   `max_utterance_ms`（config.toml 新增，继承 legacy `asr_begin(max_seconds)` 的
   精神——连续语音无 offset 时也要有界）到点同样判停；
3. 判停 → ASR →（空文本弃轮）→ LLM（带历史）→ `rr3.call` **`rr3-interact/tts/text`**
   （事务面，应答 `played_ms`）→ 发布 `rr3-brain/intent/result`（event：user_text /
   reply_text / trace）；
4. 同时应答本模块 exposes 的 `rr3-brain/llm/completion` 点名。

失败语义与 rr3-sense/rr3-interact 同一契约：**业务失败**（云端 4xx/5xx、空文本、
被调方拒答）折错误应答/日志弃轮，进程不死；**传输失败**（总线写不出去、心跳被拒）
`exit(1)` 交 supervisor。TTS 合成出的 PCM 在 M4 是**验证性消费**（非空 + 时长换算入
日志）——真播放要板上的 ALSA sink，interact 现有播放面是占位正弦，播的还是文字经
`rr3-interact/tts/text` 那一跳；PCM 直达播放面随 M5 板上集成裁定。

- 否决 **brain 直接订 VAD 自己判静默**：sense 的 VAD 边沿就是"判停摘要"（topics.toml
  注明边沿负载只带摘要不带闭环数据），brain 重复一套静默计时等于两本真值。
- 否决 **TTS 结果发布事件让 interact 拉取**：播放是请求-应答语义（interact 的
  `rr3-interact/tts/text` 是 registered request 主题），经 `rr3.call` 走事务面，
  与 ADR-0007 决策一的分层一致。

### 裁定 4：四真进程 e2e 落在 `tools/sim`（`--brain` 演练），编排与既有演练同构

`rr3-sim --brain`：夹具（临时目录 + 套接字 + 三份 wire manifest：sense 的 entry 指向
`modules/rr3-sense/target/debug/rr3-sense`，interact 同理，brain 指向演练现场生成的
**shim 脚本**——`exec python3 -m rr3_brain` + `PYTHONPATH` 指仓内包，对位"装机时由
安装步骤写绝对 entry"）→ 起 rrd（boot spawn rr3-sense，`RR3_HAL_BACKEND=sim`、
`RR3_HAL_SIM_WAVE=sine`，sine 必过 VAD 起音阈值）→ 外部起 interact 与 brain
（`RR3_BRAIN_CLOUD=mock` + 专属 config：`max_utterance_ms` 压短）→ 有界等待 brain
stdout 出现"对话轮完成"证据 → `rr3-ctl status` 断言三模块 running 且
`denied_publish = 0` → 报告 + 收尾（`--keep` 同既有语义）。

理由：编排home 已有（rr3-sim 的夹具/等待/巡检/收尾全是现成件），e2e 与 F4 演练共用
同一套定位与报告形状；四真进程 + 真 UDS + 真 rrd 路由，只有云端是 mock（裁定 2）。
编排代码是 Unix-only（uds + 真进程），与 main.rs 既有 cfg 边界一致。

## 明确不做（M4 边界）

1. **真云联调**：key 轮换就位后按裁定 2 换 `RR3_BRAIN_CLOUD=real` 验证，属运维动作
   不属本轮代码交付。
2. **PCM 直达播放面**（裁定 3 第 4 点）。
3. **流式 ASR**（legacy 的边录边传是 ESP32 内存约束的产物；本机整段 WAV 一次上传）。
4. **stream payload 类型的上行**：pcm 在线上仍以 Event JSON（`pcm_b64`）承载——
   STREAM 类型上行在 rrd 侧未放行（conn.rs 拒），真正的流控与 shm 面随第②步
   （pcm STREAM schema）与 M5。
5. **brain 之外的第二个 Python 模块 / 共享 py 包提取**（裁定 1 否决项 2）。

## 后果

- 正面：全语言栈闭环首次成立——Rust 基座 + Python 认知在同一套 128B 线格式上对话；
  `rr3-brain` 从"零依赖骨架"变"零依赖实装"（依赖预算零增量）；e2e 复用 rr3-sim 全部
  既有编排件。
- 负面：`bus.py` 手写协议栈与 rr-ipc 之间存在漂移风险——防线是 golden hex 逐字节用例 +
  e2e 真总线回路（两端谁漂移谁红）；mock/real 双路意味着"云客户端已验证"必须区分
  路径陈述（mock 全绿 ≠ 真云可用）。
- 校验项：brain 单测 stdlib `unittest`（独立工程本地实跑，条数登记 conventions §2，
  类比 tools/rrctl 的登记方式）；`tools/sim` 新增用例随 ④ 步编译面进七步门之外的
  本地测试；check.sh 七步既有判据一条不改。
