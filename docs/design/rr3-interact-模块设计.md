# rr3-interact 模块设计（框架 v1.3）

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程（host）；`start_priority = 60`、`mem_limit_mb = 96`、`restart_policy = "always"` | `module.toml` |
| 格 | `ui.face` · `audio.playback` | `contracts/slots.toml` |
| 件 | `face` · `player`；组合件成分：`panel/`（状态面 HTTP）、mpsc 动作通道 | `atoms/`、`src/` |
| 主题 | RPC `face/expression`、`tts/text`；订阅 `brain/intent/result`、`connect/net/link`；申报发布 `asr/text`、`dialog/turn`（**零发布**） | `module.toml` |
| 句柄 | `rr3_hal_display()` → `dyn Rr3Display`；`rr3_hal_audio_sink()` → `dyn Rr3AudioSink` | `src/composite.rs` |
| 参数 | `[rr3-interact.face] non_pointable = ["talk"]`（不可点名集） | `config.toml`——**I-11 起装载**（缺省 manifest 同目录，env `RR3_INTERACT_CONFIG` 可覆盖）；`toml` 依赖随本单入 `Cargo.toml` |
| 健康 | 心跳 1000 / 看门狗 5000 ms | `[health]` |

## §1 件契约表

| 件 | 公开符号（现状） | 输入 → 输出 | 状态 | 失败语义 |
|---|---|---|---|---|
| face | `rr3_face_frames(expr: &str) -> &'static [Rr3FaceFrame]` | 表情名 → 帧表（静态） | 无 | — |
| face | `Rr3Face::rr3_face_set(&mut self, expr: &str, non_pointable: &[&str]) -> Result<(&'static str, &'static str), String>` | 白名单名 → `(当前, 前一)`；`non_pointable` = 本形态不可点名集（I-11 注入） | `current` + 帧游标 | 不认的名 / 命中排除集 → 业务 error |
| face | `rr3_face_begin_talk(&mut self) -> &'static str` / `rr3_face_end_talk(&mut self) -> &'static str` | talk 挂起/恢复 | `talk` 挂起位 | 不可能失败 |
| face | `rr3_face_tick(&mut self) -> (&'static str, usize)` | 一拍 → `(表情, 帧号)` | 帧游标 | — |
| face | `rr3_face_parse_args(&Value) -> Result<String,_>` · `rr3_face_result(current, previous) -> Value` | 点名参数/应答 | 无 | 业务 error |
| player | `rr3_tts_duration_ms(text: &str) -> u64` | 文本 → 占位时长（60 ms/字符封顶 10 s） | 无 | — |
| player | `rr3_tts_parse_args(&Value) -> Result<String,_>` · `rr3_tts_play(sink: &dyn Rr3AudioSink, text: &str) -> Result<u64,_>` · `rr3_tts_result(text, played_ms)` | 文本 → 播放 → 实播时长 | 无 | 写失败 → 业务 error |
| 共同目标签名 | `Rr3FaceFrame` 与 `Rr3Display`/`Rr3AudioSink` → `rr-hal-api`；`sink` 参数由 `&dyn` → 注入句柄槽；**"被组合件调用"保持不变**（件不起线程） | | | |

## §2 状态机（face 表情机）

七态白名单（含 `listen`/`turn` 可点名）；`talk` 由播放自动驱动——**"不可点名"是策略**：
排除哪个子集由 config `[rr3-interact.face] non_pointable` 注入（I-11），缺省含 `talk`，
某形态可改数据放开。不变式：
① 点名非法态 → 拒且**状态不动**；② **权威序：显式点名 > 播放自动态**——`begin_talk` 挂起当前表情，
播放途中若被显式 `set` 点名，挂起态**作废**（`suspended = None`），`end_talk` 于是回**默认态**而不是
回挂起前的表情（`face/mech.rs` 的 `rr3_face_set` + 用例 `explicit_set_interrupts_talk_suspend` 钉死了这一条）；
未被点名打断时，`end_talk` 才回挂起前的表情；
③ 帧游标在换态时归零（每态从帧 0 起）；④ 推进只由 `tick` 驱动，**不随总线流量漂移**。
`player` 无状态（一次调用一次播放）；`panel` 无状态（读缓存 + 转发）。

## §3 注入面

| 件 | `handles` | `ops` 槽（现状 vs 目标） | 非法收敛 |
|---|---|---|---|
| face | `dyn Rr3Display` ← `[hardware]` **无显示键**（现只有 ALSA/PWM/GPIO/I2C/link 九键） → 需扩键（走 C4 契约评审） | `non_pointable ← [rr3-interact.face] non_pointable`（**I-11 已落**：不可点名集；`default_expression: string` 现仍无人读）；帧推进周期 250 ms（代码常量） | 无屏 → 格缺，不调帧机 |
| player | `dyn Rr3AudioSink` ← `[hardware].alsa_device` | `prebuffer_ms`=500、`volume_pct`=70（**现无人读**）、字符时长 60 ms 与 10 s 封顶（代码常量） | 无 sink → `tts/text` 回 `Unsupported`，**不得静默吞文本** |
| panel | 无句柄（TCP） | `RR3_PANEL_ADDR` env；`bind`/`port` 在 config 里但**未被读** → 二者取一（删或读） | 地址非法 → 起不来即 `exit(1)` |

## §4 主循环与时序预算

真实常量：`RR3_POLL_WAIT = 250 ms`、`RR3_FACE_TICK = 250 ms`、`face_tick_at: Instant`；
动作经 mpsc `act_rx.try_recv()` 在主循环串行代执行（**sink/face 所有权不出主循环** —— 全仓范例形状）。

| 输出/动作 | 名义周期 | 最坏延迟 | 备注 |
|---|---|---|---|
| 表情帧推进 | 250 ms | 一拍 + 一次 display 写 | 帧长由帧表决定 |
| TTS 播放 | 事件驱动 | `60 ms × 字符数`（封顶 10 s） | 与 `prebuffer_ms` 无关（后者未被实现） |
| panel 应答 | 每连接一线程 | HTTP 一次往返 | 并发上限 `RR3_PANEL_MAX_CONNS`。`live` 计数是 fetch_add→handle→fetch_sub、**无 RAII 守卫**——机制上确有泄漏；但 `rr3_panel_handle/execute` 全链走 `map_err`/`unwrap_or_else`，**无可达 panic 点**，所以"累计到上限永久 503"只是可达性未证的敞口（§11）。**真正可达的是另一条**：`spawn` 失败走 `?` 会让 accept 循环整体退出 = 面板静默消失（工单 I-4 改立这一条） |
| 与心跳 | 独立线程 | — | 主循环卡死 → 心跳仍续（同 sense/act 的漏检类别） |

## §5 主题面契约

| 主题 | kind | 触发 | 载荷 | 现状与申报差 |
|---|---|---|---|---|
| `face/expression` | RPC | 点名 | 入参 `{expression}`；应答 `{expression, previous}`（`rr3_face_result` 的键是 **expression**，不是 current；面板读 `result.previous`） | 一致 |
| `tts/text` | RPC | 点名（brain 调） | 入参 `{text}`；应答 `{played_ms, chars}`（**不回显 text**；面板 beep 读 `played_ms`） | 一致 |
| `intent/result`（订） | event | **申报订阅但本单元无消费者**——`composite.rs` 的 Event 分支只匹配 `RR3_NET_LINK_TOPIC`，intent 帧收下即弃 | `actions[]` | **申报超前**：意图执行在 act 一侧，interact 订它却没有代码读它（工单 I-7） |
| `net/link`（订） | event | 消费（面板展示 + 表情线索） | up/ssid/rssi/mode | 一致 |
| `asr/text`、`dialog/turn` | event 申报发布 | — | 在册 schema | **零发布**（`tools/sim` 才发）→ retired 候选 |
| panel 端点（全集 = server.rs `RR3_PANEL_ROUTES`） | HTTP | GET/POST | 状态/硬件/系统九 + scan/connect/forget 透传 | 不是总线契约面，但属**对外可观测面**（要留在册） |

## §6 降级矩阵

| 格 | 缺格 | 句柄缺 | 句柄值错 | 后端未实装 | 参数非法 |
|---|---|---|---|---|---|
| `ui.face` | 只停帧输出，点名回 `Unsupported` | placeholder → 开机探活失败即 `exit(1)`（host 档语义） | 描述符无显示键 → 现状并非"用默认"：`rr3_hal_display` **根本不读** `Rr3HalBindings`（sim 给 `Rr3SimDisplay([0;8])`，其余直接 placeholder）。要先让选择函数读绑定，扩键才有意义（§3 工单 I-2 前置） | MAX7219 真后端未实装（**格在位 + 能力缺 ≠ 格在位**，须分开记） | 非法表情名 → 拒且不动 |
| `audio.playback` | tts 产出可丢弃，但 **intent 仍要发**（意图与声音解耦） | 同上 | 同上 | ALSA sink 有码未实测 | 空文本 → error |
| 两格皆缺 | 本单元不该被启用（只剩面板 = rrd/`rr3-ctl` 职责） | | | | |

## §7 安全核接入

**无安全核**，但有一条**权限守护**语义必须保留：panel **零发布权**（只读查询 + 受控动作转发）、
`rr3.ctl.reboot` 走双重门（`cfg(linux)` 且 `RR3_REBOOT_OK=1`）、监听面缺省只绑回环。
断言：面板不得成为第二个发布者（ADR-0006）。

## §8 两腿对拍用例设计

| 件 | 输入序列 | 期望值来源 | 边界集 | 现状 |
|---|---|---|---|---|
| face 白名单 | 七态逐个点名 + 一个非法名 | 白名单常量（件外可核对）+ §2 不变式① | 非法名 / 与当前同名 | ✅ 解析用例齐 |
| face talk 挂起 | `set("happy") → begin_talk → set("listen") → end_talk` 序列 | §2 不变式②（**权威序**：挂起期被显式点名则挂起作废、`end_talk` 回默认态；未被打断才回前一态） | 连续两次 begin（幂等）/ 未 begin 就 end / 点名打断 | ✅ **已有四条**：挂起、恢复、嵌套幂等、显式打断（`face/mech.rs` 的 tests 面 `explicit_set_interrupts_talk_suspend` 等）；缺的是 §5 的载荷 schema 腿 |
| face 帧推进 | `tick × N`（N = 帧表长 × 2） | 帧表事实（`rr3_face_frames` 返回值的长度与位序） | 单帧态游标恒 0、双帧回绕、换态归零 | ✅ **已有三条**（同一 tests 面） |
| player 时长 | 文本长度 `{1, 100, 10000}` | 裁定语义（60 ms/字符、10 s 封顶） | 封顶边界、空文本 | ✅ 有 |
| 载荷两腿 | `face_result` / `tts_result` 输出 | **诚实登记：RPC 主题今天没有载荷 schema**——`contracts/events/` 只 4 份 event schema，`topics.toml` 对这两条只登记 owner/kind 与 `note`。期望值只能取**件实现现状 + 面板 HTML 的实际取键**（`panel/www` 读 `expression`/`previous`），并把"补 RPC 载荷 schema"记为契约级前置（C4 同批） | 字段缺失、多余字段 | ⚠️ 无用例；且**期望值来源不足**（腿二无处可引） |

## §9 不变量与红线断言

- 断言：`talk` 只被**显式点名**改写——播放期间除 `set` 之外没有任何路径能换表情（`end_talk` 的回落目标由 §2 权威序决定）。
- 断言：帧推进周期与总线流量无关（灌 0 帧与灌 100 帧，tick 序列相同）。
- 断言：面板零发布权（任何端点都不得直接 publish 总线主题）。
- 断言：`asr/text` / `dialog/turn` 在处置前保持"在册但不产"，**不得**为了绿而伪造发布。
- 断言：`Rr3FaceFrame` 位序（bit7 = 行最左）由 HAL 侧类型事实源锁定，模块只引用不复制。

## §10 改造工单

| # | 动作 | 归属 | 前置 |
|---|---|---|---|
| I-1 | `config.toml` 六参数：**被读或删除**（不允许第三种状态）——**2026-10-12 对表结案**：六键已随批I **全删**（`modules/rr3-interact/config.toml` 现为空壳，文件头逐键写明删除理由：`log_level`/`default_expression` 无读者、`volume_pct`/`prebuffer_ms` 行为未实装不提前占键、`panel.bind`/`port` 与 `RR3_PANEL_ADDR_DEFAULT` 常量是第二真值点）。**余下的不是孤儿参数而是功能缺项**：音量与预缓冲的行为随 MC-1 真 PCM 通路落地时按"进 ops 槽"回归 | **本条已结**；行为实装 → **0.0.5 批M MC-2** | C5 |
| I-2 | 显示句柄的 `[hardware]` 键扩位（走契约评审）——**2026-10-12 随 R31 Q2 作废登记**：显示件改由 **ESP32 协处理器以 I2C 承载 SSD1306**，Pi 侧不再新增 SPI 键位，九键闭合表维持原样（**批J C4"不扩键"结案因此不需重开**）；本册保留的是清零判据（见 0.0.5 §18.4 与 MC-3） | **作废登记**；施工 = 增补轮五批B/批C，判据 = **0.0.5 批M MC-3** | C4 |
| I-3 | 件面句柄换 `rr-hal-api` 同步签名 | 批I | C5 |
| I-4 | **改立可达的那条**：`spawn` 失败不得用 `?` 退出 accept 循环（单连接起不来只丢该连接并计数，面板必须继续服务）；`live` 计数顺带加 RAII 守卫，防未来新增 panic 点时变成静默泄漏。回归 = 起不来一个连接后面板仍可访问 | 批K | — |
| I-5 | `asr/text`、`dialog/turn` 处置（retired 首例，与 C14 同批） | **已裁（索引 §9 R-3）待执行**——契约评审（批J 前） | C14 |
| I-6 | 面板是否进 rtos/bare 档定案（本册建议：host 档专属） | **待裁** | — |
| I-7 | `intent/result` 的申报订阅面处置：要么补上真实消费者（表情随意图联动），要么按 ADR-0019 修订 2 的 retired 机制把这条订阅义务解除（与索引 §2 的 `asr/text`/`dialog/turn` 同批走 C14）。停在"在册但无人读"是最坏状态——依赖仲裁会当它是活边 | 契约评审（批J 前） | C14 |
| I-8 | **载荷组装上移承载层**：`rr3_face_result` / `rr3_tts_result` 的 `Value` 生产移出件面（依据 = ADR-0019 修订 4）；件只交 `(current, previous)` 结构体与 `played_ms` | 批I | — |
| I-9 | 面板自述矛盾收口——**已结（0.0.5 批M MC-8③）**：端点唯一真值 = `RR3_PANEL_ROUTES` 常量表，`SURFACE` 与 `serve` 行的端点数由表长推导（文案不手写数字），表↔分派漂移由 `route_table_matches_dispatch` 单测钉死 | 已结 | — |
| I-10 | player 跨档化：占位波形 `sin`/`PI`/`AMP`/`HZ` 改 32 项查表并移入 sim 后端；每拍 `Vec<i16>` 改静态 320 样点缓冲或调用方借用 | 批I | C5 |
| I-11 | "不可点名表情"规则（`talk`）从件内 `if` 改 ops 槽 `non_pointable`；白名单值本身仍是契约面，只把**规则**外移——**2026-10-13 由 0.0.5 批M MC-10 落**：值落 config `[rr3-interact.face] non_pointable`（config 键 + `atoms/face/assemble.toml` ops 槽 `non_pointable ← rr3-interact.face.non_pointable`），interact 自此**装载 config**（`toml` 依赖入本 crate），组合件读入透传 `rr3_face_set`。白名单对账（非在册名拒 boot）、空数组合法（放开全部点名） | 批M MC-10（已结） | C1 |
| I-12 | 面板端点串"后端 + 页面"双写消解——**已结（0.0.5 批M MC-8③）**：页面注释撤手写计数、指向后端 `RR3_PANEL_ROUTES` 唯一真值；页面内的端点字面量是功能调用（fetch 目标）非文案，保留 | 已结 | — |
| I-13 | **妆面件失败语义规则表落地**（D8 本轮立单，此前索引挂着却无任何工单）：把"哪些件属妆面（写失败只 `eprintln` 不退）、哪些绝不属（执行器与播放路径必须 `exit(1)` 交监督复位）"写成一处数据（`contracts/` 或 ops 侧的件分类表），框架通用约定 1 引它而不是各册自说。建议范围：led / face 属妆面，**player 待 D8 定**（interact §6 已定"意图与声音解耦"，倾向属妆面但不含 sink 写失败以外的路径） | 批I（表）+ 契约评审（范围） | C17 |
| I-14 | **播真 PCM 而非占位波形**（2026-10-12 立单，版本边界改判产物 = 索引 R-14 / ADR-0020 裁定 1）：`player` 收 `rr3-brain/audio/tts_pcm` 流（MCP-1 已落 `8d05590`：新增主题在册，载荷体复用上行 `rr3_pcm_event_body` 的 `{seq, format, rate, channels, ms, pcm_b64}` 形状）→ 解 `pcm_b64` → 写 `Rr3AudioSink`；时长由**样点数**导出，`RR3_TTS_MS_PER_CHAR = 60` 与 440 Hz 查表波形退役（站替台账台-2）；`volume_pct`/`prebuffer_ms` 的行为随本单实装并按"进 ops 槽"回归 | **0.0.5 批M MC-1 + MC-2**（前置 MCP-1 契约 append 已落 `8d05590`） | MCP-1 |

## §11 待证与敞口

1. 真屏与真回放从未发生（显示真后端未实装、喇叭/麦未采购）。
2. `volume_pct` / `prebuffer_ms` 对应的行为**未实装**，不是"配了没读"那么轻——播放路径里没有缓冲与音量控制。
3. 面板鉴权模型 = 单机可信 + 只绑回环；对外暴露的决策随 M5 定，本册不预设。
4. `live` 计数泄漏的**可达性未证**：今天找不到能让连接线程 panic 的路径，所以"累计到上限 → 永久 503"
   只能作敞口登记（触发条件 = 面板转发链里第一次引入 `unwrap`/`expect`）。别把它写成现状缺陷。
5. RPC 载荷**没有 schema 真值点**：`contracts/events/` 只覆盖 event 主题，`face/expression` 与 `tts/text`
   的应答形状今天由件实现 + 面板取键方式共同"习惯性"锁定。补 RPC 载荷 schema 是契约级动作（与 C4 同批），
   在它落地前 §8 的"腿二"无处引值——这是本册 §8 唯一标"期望值来源不足"的原因。

## §12 跨档动作（host / rtos / bare 逐件）

发布权判定：本单元两件都**只被点名、不自主发主题**（`face/expression`、`tts/text` 是 RPC；申报发布的
`asr/text`/`dialog/turn` 走 retired，见索引 §9 R-3）。**被点名方需要的编码面比发布方更薄**——应答体可以
由承载层组装，件面只交结构体。panel 是 host 档专属组件（rtos/bare 无面板：D6 建议已在本册 §10 I-6 挂待裁）。

| 件 | host（今天） | rtos（esp-rs std + FreeRTOS） | bare（no_std 自足） | 卡住它的**具体那一行依赖** → 动作 |
|---|---|---|---|---|
| face | ✅ | ⚠️ 只差 HAL 与载荷面 | ⚠️ 三处 std/alloc 形状 | ① `use std::mem::replace`（全限定调用）→ `core::mem::replace`，一行；② `rr3_face_parse_args -> Result<String, String>` 与 `rr3_face_set -> Result<(&'static str, &'static str), String>` 的 `String` 错型 → `Rr3FaceErr` 枚举 / `&'static str`（bare 档无堆时的错型面）；③ 错误文案里 `RR3_FACE_EXPRESSIONS.join("/")` 产 `String` → 白名单串改常量（值本就固定，`join` 是白白分配）；④ `rr3_face_result -> Value` 载荷上移承载层（I-8）；⑤ `Rr3Display::draw` 是 `#[async_trait]` 句柄，批I F1 后同步化 |
| player | ✅ | ⚠️ 分配与数学面 | ❌ 需两处改造 | ① 每 20 ms 一块 `Vec<i16>` + `collect` → 静态 320 样点缓冲（16k/20ms 恰为 320）或调用方给 `&mut [i16]`；② `rr3_tts_sample` 用 `std::f64::consts::PI` 与 `sin` → 查表 32 项正弦 + 整型插值（占位波形，精度不敏感，误差预算：听感等价即可）；③ `&dyn Rr3AudioSink` bare 禁 trait 对象 → 泛型；④ `rr3_block_on` 清零；⑤ `rr3_tts_result -> Value` 上移（I-8） |
| panel（组合件成分，非件） | ✅ | **不适用**：rtos 档无面板（能力面清单里就不该出现） | **不适用** | 跨档动作只有一条：`panel` 的存在与否由描述符 `[enabled_slots]` + 承载档决定，**不得**在件或组合件里用 `#[cfg(target_os)]` 自我裁剪（那是把形态选择权焊回代码）。对外自述矛盾（I-9）已随批M MC-8③ 收口：端点真值归 `RR3_PANEL_ROUTES` 一处。 |

**动作序**：批I F1 → I-3（句柄同步化）→ I-8（载荷上移）→ player 查表化（I-10）。
**微型形态里的本单元**：`ui.face` 若进 rtos（MAX7219 那类 8×8 件），face 是**最适合先跨档的件**——
状态机已是 `&'static str` + `[u8;8]`，唯一硬点是错型 `String` 与 `join`；player 反而最重（波形生成 + 缓冲）。

## §13 策略字面量清单

| 文件 : 符号 | 字面量 | 语义 | 读者 | 去处 |
|---|---|---|---|---|
| `atoms/face/face.rs : RR3_FACE_EXPRESSIONS` | `["idle","blink","talk","think","happy","listen","turn"]` | 表情白名单 | 有（`rr3_face_set`/`rr3_face_frames`） | **契约字面量—不许动**（面板下拉框与 schema 都在读；集合扩容走契约评审） |
| `atoms/face/face.rs : RR3_FACE_TALK` | `"talk"` | "不可点名"的排除项 | 有 | 契约字面量；**规则本身（"播放态不可点名"）已外移** → config `[rr3-interact.face] non_pointable` + ops 槽 `non_pointable`（**I-11 已落**，让形态可选择放开——空集全放） |
| `atoms/face/mech.rs : Default / begin_talk / end_talk` | `"idle"` ×3 | 默认表情兜底 | 有 | ops 槽 `default_expression`（config 里同名键**今天无读者**——见下一行） |
| `config.toml [rr3-interact]` | `[rr3-interact.face] non_pointable = ["talk"]` | 不可点名集（策略数据） | 有（组合件 `rr3_interact_config_parse`；**I-11 起装载**，env `RR3_INTERACT_CONFIG` 可覆盖） | 死键六项已随 I-1 删；本键是 I-11 落点（**可注入的是集合，"某形态放开 talk"= 改数据**）。`prebuffer_ms`/`volume_pct` 的行为随 I-14 实装后回归 |
| `atoms/player/mech.rs` | `RR3_TTS_MS_PER_CHAR=60`、`RR3_TTS_MAX_MS=10_000`、`RR3_TTS_FRAME_MS=20`、`RR3_TTS_SAMPLE_RATE=16_000`、`AMP=12_000.0`、`HZ=440.0` | 时长公式与占位波形 | 有 | 时长两项 = **ops 槽**（策略）；`FRAME_MS/SAMPLE_RATE` = 与 sense/线格式同源的**契约字面量**（双写记债：应引同一真值）；`AMP/HZ` = sim 后端参数（注释自认"只借 rr-hal sim 参数"→ 移后端，I-10） |
| `src/composite.rs` | `RR3_POLL_WAIT=250ms`、`RR3_FACE_TICK=250ms`、`"beep"`、`rate=16000 ch=1`、`8x8` 文本 | 节拍 / 自检词汇 / 回显 | 有 | 两项节拍 = ops 槽（与 act/sense/power/connect 的 POLL_WAIT 同批）；`"beep"` = 契约字面量（面板读它） |
| `src/panel/server.rs` | `RR3_PANEL_ADDR_DEFAULT="127.0.0.1:8080"`、`RR3_PANEL_MAX_CONNS=8`、`RR3_PANEL_READ_TIMEOUT/CALL_TIMEOUT=5s`、`HEADER_LIMIT=8*1024`、`BODY_LIMIT=4*1024` | 监听面与防护上界 | 有 | 并发/超时/上界 = **ops 槽**（安全相关的上界可留件内，但必须登记）；地址缺省 = config `[panel] bind/port` 的正解，今天两处真值 |
| `src/panel/server.rs` 路由 + `src/panel/www/index.html` | 11 条 `/api/*` + `/`、三枚透传主题、四枚 `rr3.ctl.*` 方法、id 1/2/3/4、body 键 `on`/`expression`；页面**逐字复读**这些串并读 `result.played_ms/previous/ssid/connected` | 端点白名单 = 对外可观测面 | 有 | **契约字面量—不许动**；但"后端一份、页面一份"的**双写是债**（改一端必漂移）→ 由 `/api/meta` 下发或生成器产页（I-12） |
| `atoms/face/mech.rs : rr3_face_set` 报错文案 | `"不可被点名（本形态不可点名集 talk）"`、`"不认表情 {expr:?}（允许 …）"` | 应答文案 | 有 | **契约字面量—不许动**（I-11 后"不可点名"拒因由注入集渲染，空集显示 `(空)`） |
| `atoms/face/mech.rs : RR3_FRAMES_*` | 11 帧 `0b0110_0110` 一类 8×8 位模式 | 渲染**数据**（不是阈值） | 有 | 不是策略，但也不该焊在 `mech.rs`：随批F5a 显示 trait 落 sim 后端 / 描述符数据面（帧表 = 板面事实） |
| `src/panel/server.rs : SelfTest` | `"happy"` | 自检用表情 | 有 | ops 槽（自检期望不该硬编码） |
| `atoms/face/assemble.toml` | `handles = []`、`ops = [{slot="non_pointable" ← rr3-interact.face.non_pointable}]`；player 件 `handles = [{Rr3AudioSink ← alsa_device}]` | 注入面申报 | **face 报 ops（I-11）/ player 报 handle（L-5）** | face 无可用显示句柄（`[hardware]` 连显示键都没有，I-2 前置 C4）→ `handles` 仍空，但 ops 非空故不写 `no_injection`（rr-registry 形状级规则：该键只允许两数组都空）。`default_expression` 槽待显示键落地时补 |

## §14 基线对表（代码 `22783ea`，2026-10-10；本节是本册唯一"现在时"）

| 本册旧断言 | 现在的事实（符号为证） | 判定 |
|---|---|---|
| §12 player 卡在"每拍 `Vec<i16>` + `std` `sin`"（I-10） | 改栈上 `[i16; RR3_TTS_BLOCK_SAMPLES = 320]`，波形走 `rr_hal::sim::rr3_sim_tts_block`（32 项 LUT）；`sin` 只在测试夹具 | **I-10 已结** |
| §1/§10 载荷上移（I-8） | `rr3_face_result`/`rr3_tts_result`/两个 `*_parse_args` 在 `src/composite.rs`；件面 `json!` 零命中 | **I-8 已结** |
| §5 "`asr/text`、`dialog/turn` 申报发布却零发布（I-5 待执行）" | `contracts/topics.toml` 两条置 `status="retired"`；`rr3-interact/module.toml` 现 `publishes = []`、`subscribes = ["rr3-connect/net/link"]`（`intent/result` 订阅申报已撤） | **I-5、I-7 已结**（R-3 落地） |
| §13 五个死配置键（I-1） | `config.toml` 头注逐键写明删除理由，`log_level`/`default_expression`/`prebuffer_ms`/`volume_pct`/`bind`/`port` 不再挂"看着生效"的面 | **I-1 已结**；**新查出 5 个同类**（`[rr3-sense.audio].bit_depth`、player 两键、panel `bind`/`port`）已并回 0.0.5 §17 孤儿参数条 |
| §12 face 三处 std/alloc 形状 | **未结**：`RR3_FACE_EXPRESSIONS.join("/")` 仍产堆、`rr3_face_set -> Result<(_, _), String>` 仍是 `String` 错型、`std::mem::replace` 未改 `core::`（另：`mech.rs` 内 f64 仅测试面） | 批L **L-3** |
| §3/§12 显示句柄 `[hardware]` 无键（I-2） | **本轮裁定为"不扩键"**：`descriptor.rs` 九键闭合表原样，`rr3_hal_display` 只有 sim/placeholder 两分支（真屏未实装）。扩键评审（C4）以"不扩"结案，真屏到位时再走一次 append | **I-2 关闭**（判定改，不是实现改） |
| §12/§13 `handles`/`ops` 申报 | **L-5 已结（批M MC-10 L-5，2026-10-13）**：player 报 `Rr3AudioSink`←`alsa_device` 句柄；face 无可用注入面（显示句柄键未立、config 空壳）→ 明写 `no_injection = true` | — |
| §4 panel `live` 计数与 accept 循环（R-5 改判的可达缺陷） | `rr3_panel_accept_loop` 抽缝落地：spawn 失败弃置该连接并 `panel_spawn_fail` 点账，accept 循环继续服务；`Rr3ConnSlot` RAII 守卫管 `live` 计数（Drop 归还，panic 展开不泄漏） | **I-4 已结**（批M M-β 3bc30f1，2026-10-12） |

**本册新净余数**：批L **L-3**。**L-5 已结（批M MC-10 L-5）**；**I-12/I-9 已结**（批M MC-8③ `ba90367`）。

### §14.1 增量对表（2026-10-12，代码基线 `78625ae`；全仓总表见 [索引 §13.4](模块设计索引.md)）

| 项 | 复验实况（符号） | 判定 / 新落点 |
|---|---|---|
| I-1 孤儿参数 | `config.toml` 现为**空壳**，头注逐键写明六键全删的理由（含 `volume_pct`/`prebuffer_ms`"行为未实装不提前占键"） | **结**；行为实装随 **MC-1/MC-2** 回归并按"进 ops 槽"落 |
| I-2 显示 `[hardware]` 扩键 | R31 **Q2** 已裁显示件 = SSD1306 I2C **挂 ESP32 协处理器** | **作废登记**（Pi 侧 SPI 扩键需求消失，批J C4"不扩键"结案不需重开）；清零判据留 **MC-3** |
| I-3 件面同步签名 | 随批I `rr-hal-api` 落地，件面 `rr3_block_on` 零命中 | **结** |
| I-4 `spawn` 失败退出 accept 循环 | **已结（批M M-β 3bc30f1，2026-10-12）**：spawn 失败改弃连点账（`RR3_PANEL_SPAWN_FAIL_KEY`，心跳自报可见）继续服务；`Rr3ConnSlot` RAII 守卫防 panic 泄漏。用例先红后绿：必败注入 `stack_size(usize::MAX)`（实测 macOS 对 `stack_size(1)` 静默 clamp 使 spawn 成功，`usize::MAX` 才 EINVAL 确定性失败） | 已结 |
| I-9 / I-12 端点数与双写 | **已结（批M MC-8③，2026-10-12）**：端点唯一真值 = `RR3_PANEL_ROUTES` 常量表，文案由表长推导；页面注释撤手写计数指向该表 | 已结 |
| I-10 player 跨档化（占位波形） | `atoms/player/mech.rs` 仍是 32 项查表产 440 Hz 波形 + `RR3_TTS_MS_PER_CHAR = 60` | **并入库 §18.2 台-2 与 MC-1/MC-2**（占位波形整体退役即本单终点，不再单独挂账） |
| I-11 `talk` 不可点名规则 | **已结（批M MC-10 I-11，2026-10-13）**：件内 `if` 去硬编码 → config `[rr3-interact.face] non_pointable = ["talk"]` + `atoms/face/assemble.toml` ops 槽 `non_pointable`；interact 重开 config 装载面（`toml` 入依赖），组合件读入透传；空集全放（改数据不改码） | 已结 |
| I-14（本册新立） | 真 PCM 播放通路，见 §10 表 | **MC-1 + 增补轮五批E** |
| F-2 显示面（本册职责边界） | 施工落点在**增补轮五批B/批C**（R31 Q2），本册只持"真后端名可自述 + fail-closed 报缺"的清零判据 | 与 0.0.5 §18.4 分账一致，勿在本册找 SPI 驱动工单 |
