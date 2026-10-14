# contracts —— 字节级共享契约（[SC] 唯一事实源）

Rust 基座（`base/runtime/rr-ipc`）与 Python brain（`rr3-brain`）对同一份文件做编解码；brain 侧
何时接入属实况，见 [docs/status.md](../docs/status.md)。
任何一侧改格式，必须先改这里，且只增不改。

## 内容

| 文件 | 作用 | 稳定级 |
|---|---|---|
| `envelope.schema.json` | A-IPC 128B 消息头逐字节偏移表 + payload 类型/flags/错误码枚举 | L1 |
| `golden/event-header-128.hex` | 一条真实头的 hex 向量（IPC-T-001 的**同机**基准；跨架构等值另见下） | L1 |
| `topics.toml` | 主题所有权注册表：`[[owner]]` + `[[topic]]` 两组表（校验入口共三处——`rr3-mkmod` 生成自检、`rr-registry` 契约测试、rrd 在册表判定 `RrdRegistry::rr3_reg_publish_check`；职责与顺序见 [docs/architecture/architecture.md §6.3](../docs/architecture/architecture.md)，第三个入口的线上接线进度属实况，见 [docs/status.md](../docs/status.md)） | L2 |
| `slots.toml` | 能力格坐标系总表：`[[slot]]`（name/layer/note），三层 base/body/cognition（查册门真值见下"slots.toml 裁定"） | L2 |
| `events/*.schema.json` | 各模块事件负载 schema（命名规则与模板见 [`events/README.md`](events/README.md)） | L2 |

> **golden 向量的效力边界**：它钉住的是"本机 Rust 编解码与契约字节逐位一致"（IPC-T-001 侧），
> 跨架构（x86 与 aarch64 布局一致，即 IPC-T-003）的等值证明由**契约外**的对拍脚本
> `tools/ci/ipct003.sh` 承载（宿主 / linux-arm64 容器 / xtensa 编译期 const / 本 golden 多路
> 对拍）——2026-10-11 CI ipct003 job `--strict` 四路全绿回执在案，IPC-T-003 已移入
> `envelope.schema.json` 的 `x-rock3-v0.0.1-coverage`；实况见 [docs/status.md](../docs/status.md)。

> **`envelope.schema.json` 是"给人读、给测试对照"的数据契约，不是喂校验器的 schema**：它该读、
> 该对照，但**不该**喂给 JSON Schema 校验器——文件里的 `"$schema": ".../draft/2020-12/schema"` 键
> 是从模板带过来的残迹（vestigial），文件真正承载的是 `x-rock3-*` 偏移表与冻结枚举值，按 JSON
> Schema 语义解释会得出错误结论。（`events/*.schema.json` 落定时则相反：那才是给校验器用的负载
> schema。）契约的执行入口分两层说：envelope 字节级由 `cargo test -p rr-ipc`（偏移/枚举对齐
> 与 `tests/envelope_meta.rs` 的元数据守护，读 JSON 而非 Rust 自比；守护条数的唯一登记处是
> [docs/process/conventions.md](../docs/process/conventions.md) §2，本文件不复述数字）与
> `bash tools/ci/check.sh`（manifest、主题前缀与注册表形状）兜住；主题前缀归属则在生成/契约
> 两路自检之外有第三处入口——rrd 在册表 `RrdRegistry::rr3_reg_publish_check`（判定逻辑已交付，
> 是否接到线上连接路径以 [docs/status.md](../docs/status.md) 为准；三入口职责见
> [docs/architecture/architecture.md](../docs/architecture/architecture.md) §6.3）。

## topics.toml 裁定

1. 主题字符串**不进 128B 消息头**（ADR-0006 §2：topic 置于 payload 内）。本文件是"谁有权发布什么"的**离线注册表**，运行时用于校验与文档生成，不参与线格式。
2. 规范形式：`<owner>/<域>/<事件>`，三段。`owner` 必须等于某个模块的 `[manifest].name`（方案 §4：`^rr3-[a-z]+$`，**词干只许小写字母**，不含数字与连字符）；域与事件段用小写字母 + 下划线（`^[a-z_]+$`，与 capability 段同规则）。**逐条列举而非正则通配**，是为了让"新增主题必须改契约文件"成为显式动作（改契约 = 走 review）。
3. 每个 topic 的 owner **必须**在 `[[owner]]` 中登记；一个 topic 只有一个 owner（发布方唯一），订阅方可多个。
4. `kind` 取值 `request|event|stream|notification`（**4 种**），A-IPC 的 `payload_type` 枚举是
   **5 种**（REQUEST=1 / RESPONSE=2 / EVENT=3 / STREAM=4 / NOTIFICATION=5），两者**不是一一对应**：
   `kind` 说的是主题的语义用途，`payload_type` 是线上字节值，映射关系为——

   | `kind` | 对应 `payload_type`（冻结值） | 说明 |
   |---|---|---|
   | `request` | REQUEST(1) **+** RESPONSE(2) | 一次 RPC 往返在线上占两个 payload 类型；注册表按"主题"登记，故折叠为一项 |
   | `event` | EVENT(3) | 尽力投递，允许丢失 |
   | `stream` | STREAM(4) | 有序投递（ADR-0007 实时控制面候选） |
   | `notification` | NOTIFICATION(5) | 心跳 / 系统通知 |

   即：4 个 `kind` 覆盖 5 个 `payload_type`，唯一的折叠处是 `request`。注册表当前的 owner 数、
   topic 数与 kind 分布**就是 `topics.toml` 本身**，本文件不抄副本（抄来的数一次 append 就漂）。
   STREAM 类主题 v0.0.1 仅登记、不实现传输。
5. **上述形状是机器钉住的**：`cargo test -p rr-registry --test contract` 里的
   `registry_shape_is_pinned` 按 `contracts/topics.toml` 的解析结果断言 owner 数、topic 数、
   `kind` 分布，以及全部 `(name, owner, kind)` 三元组逐条在册。计数做成硬 pin 是
   **刻意的，不是禁止 append**：新增主题合法，但必须是一次有意识的编辑——同一次改动里同时更新
   `contracts/topics.toml`、该测试的期望值，以及任何把这些数字当事实抄进正文的文档
   （用例条数的唯一登记处是 [docs/process/conventions.md](../docs/process/conventions.md) §2）。
   删主题、改 kind、改 owner 数都会红。

## slots.toml 裁定（ADR-0012）

1. 能力格（slot）= 固定命名的能力位，只描述"这里需要/可以有什么"，不描述"谁来填"（谁填登记在
   [`profiles/`](../profiles/default.toml) 装配单，纯数据）。总表与 topics.toml 同级同纪律：**只增不改**，
   破坏性变更升 `schema_version`；**不留空格**——进表的都是有明确供给方或需求的格。
2. 分三层：**base**（`bus.publish`/`bus.subscribe`/`bus.rpc`/`net.online`/`power.battery`，
   即 `module.schema.toml` `[rules] base_capabilities` 的表内化，格名随原能力名）·
   **body**（11 格）· **cognition**（4 格）。layer 只取这三个值；格位命名两段式 `<域>.<格>`，
   与主题命名（`rr3-<模块>/<域>/<事件>`）不同轴，互不混写。
3. **查册门**（ADR-0012 裁定 5②，fail-closed）：manifest 的 `provides.capabilities` 与 `depends.*`
   引用的每个能力、profile `[bindings]`/`[faces]` 引用的每个格位，都必须命中本表。单件校验只保语法，
   "是否在册"由 `rr-registry` 契约测试在 CI 全集层判；基础层与 `base_capabilities` 被
   `slots_base_layer_is_pinned_to_base_capabilities` 逐条钉死等价——依赖仲裁（ADR-0009）与格位查册
   共用同一份 base 真值，第二份白名单不存在。
4. **上述形状是机器钉住的**：`cargo test -p rr-registry --test contract` 里的
   `slots_registry_shape_is_pinned` 断言全部 20 条 `(name, layer)` 逐条在册、层分布 5/11/4。
   新增格位合法，但必须是一次有意识的编辑——同一次改动里同步更新 `contracts/slots.toml`、
   该测试常量与计数、`profiles/default.toml`（如涉及绑定面），以及把这些数字当事实抄进正文的文档。

## 演进规则（ADR-0007 决策二）

1. **只增不改**：既有字段、偏移、枚举值不得变更语义
2. 破坏性变更必须升 `envelope.schema.json` 顶层的 **`x-schema-version`** major（该键是 2026-09-29
   追加的元数据键，当前值 1，由 `cargo test -p rr-ipc --test envelope_meta` 与 Rust
   `RR3_CONTRACT_SCHEMA_VERSION` 逐值钉住），并在 `topics.toml` 登记受影响主题。
   `topics.toml` 与 `base/runtime/rr-registry/src/module.schema.toml` 各自另有同名整型键
   `schema_version`（当前同为 1），三者不要混写
3. 消息头扩展**只允许写 reserved 区**并经 `flags` 标记启用（保持与 A-IPC 标准逐字节兼容）
4. 门禁检查以 `bash tools/ci/check.sh` 为**唯一入口**（本地与 CI 同一命令，命名沿用方案 D7 的"三检查"；
   远程在 atomgit，平台自动触发待确认，见方案 §7 遗留项）：
   ① workspace 编译（`--all-targets`，含测试目标）/ ①b workspace 全量测试 /
   ② manifest schema 校验 + 注册表形状 pin / ②b mkmod 输出即契约用例 / ③ 主题前缀归属 /
   ④ 仓内所有独立 Rust 工程编译 / ⑤ 文档门（已跟踪 md 不得为空、相对链接不得断）/
   ⑥ 独立工程测试接线 / ⑦ §13 静态断言四件 / ⑧ 件级判据三门（对拍计数·依赖闭包·注入面校验）
   （步序、命令与每步条数的权威表见 [docs/process/conventions.md](../docs/process/conventions.md) §2，
   本文件不抄数字）

### 演进登记（逐条留痕）

- **2026-09-29 · `envelope.schema.json` 追加顶层元数据键 `x-schema-version`（值 1）**：
  只加一个给人读/给测试对照的整型元数据键，**不动任何偏移、字段宽度、枚举值与线上行为**，
  故属"只增不改"允许范围，**不是** schema major 升级（版本仍为 1）。加它的理由是演进规则 2 此前
  无实物可指：`RR3_CONTRACT_SCHEMA_VERSION`（rrd 横幅的 "contracts schema v1"）当时无人断言。
- **2026-09-29 · `envelope.schema.json` 的 `x-rock3-v0.0.1-coverage` 去掉 `IPC-T-003`**，并新增
  `x-rock3-v0.0.1-uncovered` 记录 IPC-T-003（跨架构字节布局一致）待 M2 的交叉编译等值测试。
  理由一句话：**coverage 列表是"关于测试的陈述"，纠正它不是契约变更**——它不触碰偏移、枚举、
  magic 与 golden 向量，因此不适用"只增不改"的冻结约束，而是评审裁定的许可项。
- **2026-09-30 · IPC-T-003 就绪登记（仍是"关于测试的陈述"更正，非契约变更）**：
  `x-rock3-v0.0.1-uncovered` 里 IPC-T-003 的描述更新为"夹具与脚本就绪、双架构真跑待容器环境"。
  新增三件：`rr-ipc/src/bin/ipct003_dump.rs`（golden 头 + 五种 payload 类型各一帧的 hex 夹具）、
  `tools/ci/ipct003.sh`（宿主 / linux-arm64 容器 / golden 三方对拍；**不进门禁步序**，
  登记于 conventions.md §2 的平台条件脚本）、header.rs 的编译期 const 断言（偏移连续、
  头长 128、magic LE 字节、payload 枚举值，随任意目标编译求值）。偏移、枚举、magic、
  golden 向量与线上行为一字未动；Docker 装好后容器真跑，跑成即可把该条移入 coverage。
- **2026-09-30 · `contracts/events/` 落首个真实事件 schema `sense-audio-vad_start.schema.json`
  （schema_version 1，M3）**：新增文件，append 不改既有，`topics.toml` 无需变更（
  `rr3-sense/audio/vad_start` 已在册）。schema 形状以 `rr3-sense/src/audio/mod.rs` 的
  `rr3_vad_event_body` 为事实源：同主题承载 onset/offset 两种边沿，`rms_peak` 仅 onset 携带
  （用 `allOf`+`if/then` 表达），`seq` 是 vad 事件流自己的计数、与 `rr3-sense/audio/pcm` 的
  stream seq 各自独立。`rr3-sense/audio/pcm` 的 STREAM 负载 schema 未落（pcm_b64 编码的
  冻结时机属 M5 shm 迁移裁定，ADR-0007），登记为已知空缺，见 [docs/status.md](../docs/status.md)。
- **2026-09-30 · trace_id 语义登记（rock3 对 128B 头 `trace_id` 字段的用法细化，非契约变更）**：
  128B 头的 `trace_id`（offset 8，uint64）在 rock3 v0.0.1 的线上语义为——
  **0 = 非代理**（普通发布 / 直发帧的缺省值）；**`rr3.call` 代理请求的 trace_id = 调用方 task**
  （rrd 转发时写入，被调方应答须原样带回，rrd 按它把 Response 改道回调用方连接——
  实现在 `rrd/src/conn.rs` 的 `rr3_route_call` / `rr3_route_response`，客户端侧为
  `rr-bus/src/client.rs` 的 `rr3_reply_call`）。这与 `envelope.schema.json` 里 trace_id 的
  OpenTelemetry 注释是分层关系：字节布局与"不解释即透传"是 L1 冻结面，rock3 的用法语义
  登记在本文件演进记录层，不改偏移、不改字段宽度、不改线上行为。计划中的 OpenTelemetry
  串联（单调递增 + 随机后缀）留具身化阶段，届时如与本条冲突走"只增不改"评审。
- **2026-10-01 · `contracts/slots.toml` 落盘（ADR-0012 裁定 1，能力格坐标系）**：新增文件（20 格 =
  base 5 + body 11 + cognition 4），append 不改既有，`topics.toml` 无需变更。同批落盘
  `profiles/default.toml`（装配单，纯数据）与 rr-registry 查册门（`src/slots.rs` +
  `tests/contract.rs` 全集层用例：manifest/profile 双侧查册、基础层与 `base_capabilities`
  钉死等价、格位表形状 pin）。基础层格名随 `base_capabilities` 原名（两段式），ADR-0012
  正文"基础层单段"的示例记法不采用——两份名单必须逐条相等，这是查册门与仲裁共用真值的前提。
- **2026-10-04 · Q1/Q4 裁定落档 + D3 契约 append（0.0.2 版本开发方案 T2 批次）**：
  `slots.toml` 20→**21** 格（append `act.relay`，body 层；立格判据真实依赖者 = M5 验收
  "语音控继电器"，legacy/core/relay 迁移映射在册）；`topics.toml` 21→**24** 条（append
  `rr3-act/relay/set` request L1、`rr3-power/battery/low` event L1、`rr3-runtime/system/power`
  notification L2——低电兜底点名广播，Q4 裁定）；新增第二个事件负载 schema
  `events/power-battery-low.schema.json`（形状最小：seq / percent / voltage_mv；
  生产者 rr3-power 的 battery 实装落地后为形状唯一事实源）。同批更新 rr-registry 形状 pin
  （24 条三元组、kind 8-9-3-4、格位 21 / 层分布 5-12-4）。**2026-10-15 注**：本行是该批次的历史记录，
  不改写；**现值为 30 条主题（event 10 / request 12 / notification 4 / stream 4，8 owner，2 条 retired）
  与 21 格（normal 18 / placeholder 3 / retired 0）**——计数唯一事实源是 `topics.toml` 与 `slots.toml` 本身，
  任何文档复述这个数字都必须以两份表为准（本 README 下文 ③ 段已按现值更新，两处并存即是漂移教训，
  见 `docs/process/adr/ADR-0021`/`ADR-0022` 的"计数分母要写死且要对"一条）；
  `modules/rr3-act/module.toml`
  （provides `act.relay` + exposes `rr3-act/relay/set`）、`profiles/default.toml`
  （bindings 16 条 + faces）。battery/low 定 L1：兜底触发面负载取最小集、不再扩字段，
  演化走"只增不改"评审。
- **2026-10-05 · 0.0.3 批C 契约 append（0.0.3 版本收口方案 T3，Q1 裁定 + T3 补裁定）**：
  `topics.toml` 24→**25** 条（append `rr3-act/relay/state` event L2——继电器通断状态摘要、
  变化即发；0.0.2 Q1"relay/state 留真实需要时再 append"的真实需要此刻成立：意图执行效果的
  可观测面，e2e 证据链需要状态回读）；新增两个事件负载 schema：
  `events/brain-intent-result.schema.json`（intent/result 载荷 pin——对话记录四字段 +
  `actions` 结构化动作清单，元素 `{topic, args}`，词汇即契约主题零新造）与
  `events/act-relay_state.schema.json`（seq / on 两字段最小面）。同批更新 rr-registry
  形状 pin（25 条三元组、kind 8-10-3-4）、`modules/rr3-act/module.toml`（publishes
  + `rr3-act/relay/state`）。topics.toml 只增不改维持；schema_version 均为 1（新文件）。
- **2026-10-05 · 0.0.4 批D 契约 append（0.0.4 版本洛克三号消化方案 D4，Q3 裁定）**：
  `topics.toml` 25→**28** 条（append `rr3-connect/wifi/scan` · `wifi/connect` ·
  `wifi/forget` request L1——legacy/core/panel 配网面迁移承接：scan 回 sim 模拟列表带
  `backend:"sim"` 诚实标注、connect 沿用 legacy `ssid`+`pass` 字段名与密码≥8 位语义、
  forget 承接 `/api/factory_reset` 的 WiFi 语义且 ≠ M6 整机恢复出厂；板上 NetworkManager
  cfg 门后置，方案 P4）。同批更新 rr-registry 形状 pin（28 条三元组、kind 11-10-3-4）、
  `modules/rr3-connect/module.toml`（exposes + 三条）。`rr3-connect/net/link` 事件体以
  `mode` 字段（ap|station）增补配网态——负载只增不改，无 schema 文件（net/link 负载
  schema 本就未落，见 status.md 已知空缺）。topics.toml 只增不改维持。
- **2026-10-05 · 0.0.5 Q3 裁定落档（pcm STREAM schema 维持已知空缺，零契约改动）**：
  `rr3-sense/audio/pcm` 的 STREAM 负载 schema **维持空缺不冻结**，并把 2026-09-30
  条目"冻结时机属 M5 shm 迁移裁定"的原表述升级为——**schema 冻结与 rrd shm 接线
  绑定同一份后续 ADR**。裁定依据：[ADR-0008](../docs/process/adr/ADR-0008-传输层trait定型与uds实现.md)
  裁定 4"不写半成品"——pcm_b64 JSON 是 UDS 承载期的编码，shm 零拷贝接线后大概率改
  环形直读，给注定换承载的形状写冻结件即半成品（冻结一次、返工一次不如一次冻结到位）。
  本条只登记裁定口径，不新增、不修改任何契约文件；接线的实施（conn.rs / sense / brain
  多模块工程批）届时另立专项方案，冻结动作在专项内完成。
- **2026-10-07 · 0.0.6 批F2 契约 append（判向-转向编排 sim 段，方案判据 4/8）**：
  `events/sense-audio-vad_start.schema.json` 的 onset 负载**增可选 `doa` 字段**——
  `{deg: -90..90 整数, bin: left|center|right}`，双麦 ILD 粗判向的落档格位（±90° 三档
  分档由 sense 内部 `rr3_doa_fix` 产出，sim 注入正门 `RR3_HAL_SIM_DOA_DEG`）。
  **零新契约**：vad_start 事件已在册、schema_version 仍 1（append 兼容扩展——offset
  边沿不带 doa，与 `rms_peak` 同形状纪律，`allOf`+`if/then` 表达）；生产者
  `rr3-sense/src/audio/mod.rs` 的 `rr3_vad_event_body` 为形状唯一事实源。舵机转向
  指令复用在册 request `rr3-act/servo/command` 与事件 `rr3-act/servo/state`，词表意图
  经 `brain-intent-result` 的 `actions` 元素落 topic，均零改动。
- **2026-10-07 · 0.0.6 批F3 契约面复核（表情扩展 sim 档，方案判据 9，零契约改动）**：
  `rr3-interact/face/expression` 白名单 5→**7** 态（增 `listen`/`turn`——判据 9 点名的
  听取/转向两态；`talk` 仍由 TTS 播放自动驱动、不可点名）。**零 schema 改动**：主题
  在册 request L1 不动，仅 `topics.toml` 该条 note 追加两态措辞（白名单逐条列举语义
  的同源更新，非载荷/枚举变更）；帧序列（8x8 点阵位图，blink/talk 双帧）是模块内
  实现面，进契约须随批F5a 显示 trait 立格时一并裁定，本批不冻结。
- **2026-10-11 · 0.0.7 批E 契约 append（跨芯片 e2e round-trip 点名对，0.0.7 批E 契约裁定）**：
  `topics.toml` 28→**29** 条（append `rr3-sentinel/diag/probe` request L2——跨芯片哨兵的
  探针点名主题：rr-registry F2 门要求 `[rpc].exposes` 主题在册，e2e round-trip 判据即
  sim_peer 点名、哨兵 `result.echo` 原样回带）。同批新增 owner `rr3-sentinel`
  （path `tools/espidf-sentinel`）——哨兵**总线身份**同批由 `rr3-espidf-sentinel` 改名合规
  （词干纯小写是 `^rr3-[a-z]+$` 硬规则，module.schema.toml `module_name` 门；工程名/目录名
  espidf-sentinel 是 crate 名非总线身份，不变）。同批更新 rr-registry 形状 pin（29 条三元组、
  owner 8、kind 12-10-3-4）。topics.toml 只增不改维持。
- **2026-10-12 · 0.0.5 第 IV 轮 MCP-0 契约级评审（MC-0 合并件：MCP-1 + MCP-2 作废登记 + MCP-3，承
  [0.0.5 方案 §19 MC-0 / §18.4 防重规则①](../docs/process/dev/0.0.5版本洛克三号对齐与裁定方案.md)）**：
  ① **MCP-1**：`topics.toml` 29→**30** 条（append `rr3-brain/audio/tts_pcm` stream L2——brain 侧
  TTS PCM 下行流，F-4 真人声通路的契约半边；载荷体**逐字段复用** `rr3-sense/audio/pcm` 已定形状
  `{seq, format, rate, channels, ms, pcm_b64}`（`rr3-sense` composite.rs `rr3_pcm_event_body`
  为形状事实源），对 TTS 引擎中立（云 CosyVoice2 / 本地 Piper 同一份通路，R31 Q3）；上行
  `audio/pcm` 与 `audio/vad_start` 字节零改动；envelope.schema.json 与 128B 头不动）。本条与
  增补轮五批C（display/servo 帧进城）与批D（音频上行）**合成一次改表**——批C/批D 复用在册主题
  `rr3-interact/face/expression`、`rr3-act/servo/command|state`、`rr3-sense/audio/pcm`，零新增
  登记，全轮仅此一次 `topics.toml` 变更。② **MCP-2 作废登记**：`[hardware]` SPI 扩键评审需求随
  R31 Q2（显示件由 ESP32 协处理器以 I2C 承载 SSD1306）消失，Pi 侧不再新增 SPI 键位，九键闭合表
  维持原样——作废留痕，别让"没做"读成"漏做"。③ **MCP-3**：`rr3-act` `module.toml` 申报面变更
  （`[events].subscribes` 增 `rr3-runtime/system/power`）——低电禁动闸门消费点（MC-5 施工件），
  走 C14/R-3 同一评审程序；其余申报面零改动。同批更新 rr-registry 形状 pin（30 条三元组、
  owner 8 不变、kind 12-10-4-4）与 `rrd` topics_load 测试计数（33 = 30+3）。
- **2026-10-12 · 0.0.5 增补轮五 批C 负载 schema 落档（路线二：板上不进城，宿主 HAL 桥后端
  直连，零 topics.toml 改动）**：新增两份 **request 类**负载 schema（本目录首批 request 类——
  顶层钉请求 args 形状、应答 result 形状钉 `$defs`）：`events/act-servo-command.schema.json`
  （`rr3-act/servo/command`——写变体 `{angle_deg, move_ms?, axis?}` / 读变体 `{axis?}`（板侧
  扩展）双形状；应答 `{angle_deg, move_ms}`；axis 为板侧帧面扩展键）与
  `events/interact-face-expression.schema.json`（`rr3-interact/face/expression`——expression
  变体（7 态白名单）/ frame 变体（板侧扩展 HAL 渲染，8x8 点阵 → OLED 64x64）双形状；应答
  `{expression, previous}` / `{applied}`）。双面承载共同 pin：模块面 rr3-act composite.rs /
  rr3-interact face mech.rs，板侧帧执行面 tools/esp32-periph-base/src/main.rs 的
  frame_handle_servo/frame_handle_face。两条主题均在册 request L1 不动——批C 复用在册主题
  零新增登记（MCP-1 防重规则①口径），topics.toml 全轮冻结维持。

## 标准歧义登记（每半年随 ADR-0004 复查核对）

- **magic 的字节序**：A-IPC 标准 §2.2/§2.3 以 C 语义声明 `magic` **字段值**为 `0x41524531`
  并规定所有多字节字段小端序（AOS-STD-IPC-003/020，L1 稳定级），据此线上字节为
  `31 45 52 41`；而同一文档的注释 `'A''R''E''1'` 与 §7.2 Wireshark 伪代码
  `buffer(0,4):uint()`（大端读）给出 `41 52 45 31`。两处结论相反。
  **rock3 采规范条目（L1）为准：字段值 0x41524531，LE 存储，线上字节 31 45 52 41。**
  理由是与真实 C 实现互通（`hdr.magic = AIRY_IPC_MAGIC` 在小端机落地即此字节序），
  而非与 ASCII 注释互通。若标准维护方认定注释为准，本文件、`envelope.schema.json`
  与 `golden/*.hex` 一起升 major。
- **payload type 前缀占几字节**：标准 §7.2/§7.3 说"payload 类型由 payload 体首字段携带"，
  但 §3.2–3.6 的 C 结构体首字段分别是 `request_id`/`event_id`/`stream_id`，未写清该前缀
  的宽度与对齐。**v0.0.1 只冻结 128B 头与 5 个 payload 类型枚举值，payload 视为不透明
  JSON 字节**；前缀编码留 M2 决策（届时同步影响 IPC-T-004 的 golden 向量）。
  Rust 侧实现按 **2 字节小端**取值（`rr-ipc` 的 `RR3_BODY_TYPE_PREFIX_LEN`，可照抄的字面量见
  [docs/reference/rrd-rpc.md](../docs/reference/rrd-rpc.md) §1），**契约本体尚未把这一宽度写进
  `envelope.schema.json` 的冻结范围**——把它升进契约是一次契约变更（走 review），不是实现细节。
