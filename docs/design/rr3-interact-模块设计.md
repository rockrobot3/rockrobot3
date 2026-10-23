# rr3-interact 模块设计（框架 v1.3）

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程（host）；`start_priority = 60`、`mem_limit_mb = 96`、`restart_policy = "always"` | `module.toml` |
| 格 | `ui.face` · `audio.playback` | `contracts/slots.toml` |
| 件 | `face` · `player`；组合件成分：`panel/`（状态面 HTTP）、mpsc 动作通道 | `atoms/`、`src/` |
| 主题 | RPC `face/expression`、`tts/text`；订阅 `connect/net/link` + `rr3-brain/audio/tts_pcm`（MC-1 下行载荷流，2026-10-13 申报随实现补齐）；发布零（`asr/text`/`dialog/turn` 已 retired、`intent/result` 订阅已撤——I-5/I-7，见 §14） | `module.toml` |
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
| player | ~~`rr3_tts_duration_ms`~~（**MC-1 退役**：占位估时长随 MC-1② 删除，播放时长改由样点数导出） | — | — | — |
| player | `rr3_tts_parse_args(&Value) -> Result<(String, u64),_>`（text + pcm_bytes 必填）· `rr3_tts_play(sink: &mut dyn Rr3AudioSink, samples: &[i16]) -> Result<u64,_>`（20ms/320 样点分块透传写 sink）· `rr3_tts_result(text, played_ms)`（应答形状 `{played_ms, chars}` 保留） | PCM 样点 → 播放 → 实播时长 | 无 | 写失败 / 缓冲对账不过 → 业务 error |
| 共同目标签名 | `Rr3FaceFrame` 与 `Rr3Display`/`Rr3AudioSink` → `rr-hal-api`；`sink` 参数由 `&dyn` → 注入句柄槽；**"被组合件调用"保持不变**（件不起线程） | | | |

## §2 状态机（face 表情机）

白名单 43 态（**七核心态**含 `listen`/`turn` 可点名 + **char 单字符档** 36 态
`charA..charZ/char0..char9`，2026-10-15 板上批随 MAX7219 真件档扩容——机制零改动，
只扩白名单与帧表数据）；`talk` 由播放自动驱动——**"不可点名"是策略**：
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
| face | `dyn Rr3Display` ← `rr3_hal_display()` 环境档解析（`RR3_HAL_BACKEND=esp32` 桥后端 + `RR3_HAL_PERIPH_UART` 设备路径；`[hardware]` **不扩键**——MCP-2 随 R31 Q2 作废登记，九键闭合表维持；缺 env 落 `Rr3UnavailableSource`，探活失败 `exit(1)`） | `non_pointable ← [rr3-interact.face] non_pointable`（**I-11 已落**：不可点名集；`default_expression: string` 现仍无人读）；帧推进周期 250 ms（代码常量） | 无屏 → 格缺，不调帧机 |
| player | `dyn Rr3AudioSink` ← `[hardware].alsa_device` | `prebuffer_ms=500`、`volume_pct=70`（**MC-2 已落**：config `[rr3-interact.player]` 键回归 + `atoms/player/assemble.toml` ops 槽两条，组合件读入透传——缺键回落默认、类型错/负数/增益越界 0..=1000 逐键拒 boot；机制面 `rr3_tts_apply_gain` i64 中间量整型饱和、`rr3_tts_prebuffer_ready` 起播水位 legacy `0cf029e` 语义）；字符估时长已随 MC-1 删（播放时长由样点数导出，20ms 分块 = 契约字面量 `RR3_TTS_BLOCK_SAMPLES=320`/`RR3_TTS_SAMPLE_RATE=16_000`） | 无 sink / 缓冲对账不过 → `tts/text` 回拒因 + 计数，**不得静默吞文本** |
| panel | 无句柄（TCP） | `RR3_PANEL_ADDR` env；`bind`/`port` 在 config 里但**未被读** → 二者取一（删或读） | 地址非法 → 起不来即 `exit(1)` |

## §4 主循环与时序预算

真实常量：`RR3_POLL_WAIT = 250 ms`、`RR3_FACE_TICK = 250 ms`、`face_tick_at: Instant`；
动作经 mpsc `act_rx.try_recv()` 在主循环串行代执行（**sink/face 所有权不出主循环** —— 全仓范例形状）。

| 输出/动作 | 名义周期 | 最坏延迟 | 备注 |
|---|---|---|---|
| 表情帧推进 | 250 ms | 一拍 + 一次 display 写 | 帧长由帧表决定 |
| TTS 播放 | 事件驱动（`tts/text` 点名对账过才播） | 流时长 = 样点数 × 1000 / 16000 ms（20ms 分块写 sink） | 起播先过 `prebuffer_ms` 水位门（MC-2 落，legacy 0cf029e 语义：流完整未攒满也播——批量架构下对账蕴含水位，fail-closed 显式在场）；增益 `volume_pct` 整型饱和缩放后写 sink |
| panel 应答 | 每连接一线程 | HTTP 一次往返 | 并发上限 `RR3_PANEL_MAX_CONNS`。`live` 计数是 fetch_add→handle→fetch_sub、**无 RAII 守卫**——机制上确有泄漏；但 `rr3_panel_handle/execute` 全链走 `map_err`/`unwrap_or_else`，**无可达 panic 点**，所以"累计到上限永久 503"只是可达性未证的敞口（§11）。**真正可达的是另一条**：`spawn` 失败走 `?` 会让 accept 循环整体退出 = 面板静默消失（工单 I-4 改立这一条） |
| 与心跳 | 独立线程 | — | 主循环卡死 → 心跳仍续（同 sense/act 的漏检类别） |

## §5 主题面契约

| 主题 | kind | 触发 | 载荷 | 现状与申报差 |
|---|---|---|---|---|
| `face/expression` | RPC | 点名 | 入参 `{expression}`；应答 `{expression, previous}`（`rr3_face_result` 的键是 **expression**，不是 current；面板读 `result.previous`） | 一致 |
| `tts/text` | RPC | 点名（brain 调） | 入参 `{text, pcm_bytes}`（MC-1：字节总数 = 收流完整性唯一判据）；应答 `{played_ms, chars}`（**不回显 text**；面板 beep 读 `played_ms`） | 一致 |
| `rr3-brain/audio/tts_pcm`（订） | event stream | 消费（MC-1 收缓冲） | `{seq, format:"s16le", rate:16000, channels:1, ms, pcm_b64}`，20ms/640B 分帧，seq 模块级单调不随轮次复位 | 申报随实现落地（2026-10-13）；流是载荷通路，点名才是"该不该播"唯一判据（0.0.5 方案 §20 第 1 条 (a)） |
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
| face 白名单 | 全集 43 态逐个点名 + 一个非法名（char 档另钉白名单位次与 charH 横杠行字模） | 白名单常量（件外可核对）+ §2 不变式① | 非法名 / 与当前同名 | ✅ 解析用例齐 |
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
| I-1 | `config.toml` 六参数：**被读或删除**（不允许第三种状态）——**2026-10-12 对表结案**：六键已随批I **全删**（`modules/rr3-interact/config.toml` 现为空壳，文件头逐键写明删除理由：`log_level`/`default_expression` 无读者、`volume_pct`/`prebuffer_ms` 行为未实装不提前占键、`panel.bind`/`port` 与 `RR3_PANEL_ADDR_DEFAULT` 常量是第二真值点）。**余下的不是孤儿参数而是功能缺项**：音量与预缓冲的行为随 MC-1 真 PCM 通路落地时按"进 ops 槽"回归——**2026-10-13 随批M MC-2 兑现**：两键回归 config `[rr3-interact.player]` + `atoms/player/assemble.toml` ops 槽，行为实装（见 §14.1 I-14 行） | **本条已结**；行为实装 → **0.0.5 批M MC-2（已结）** | C5 |
| I-2 | 显示句柄的 `[hardware]` 键扩位（走契约评审）——**2026-10-12 随 R31 Q2 作废登记**：显示件改由 **ESP32 协处理器以 I2C 承载 SSD1306**，Pi 侧不再新增 SPI 键位，九键闭合表维持原样（**批J C4"不扩键"结案因此不需重开**）；本册保留的是清零判据（见 0.0.5 §18.4 与 MC-3）。**2026-10-14 再改判（本行两处作废）**：① "协处理器"定性随 [ADR-0021](../process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md) 裁定 1 作废——ESP32 是**独立形态**，不是谁的附件；② **C4"不扩键"结案随该 ADR 裁定 4 重开并作废**，显示面键位改由 `contracts/roles.toml` 角色词表导出，本件 `handles` 从此**可填**（新工单 **I-16**，施工 = [增补轮六](../process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) 批N1）。实况补一句：SSD1306 已由增补轮五批B **真板出字**，"Pi 侧无 SPI 键位"这句在 ESP32 形态上从来不是障碍，障碍是键集闭集本身 | **作废登记**；施工 = 增补轮五批B/批C，判据 = **0.0.5 批M MC-3** | C4 |
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
| I-14 | **播真 PCM 而非占位波形**（2026-10-12 立单，版本边界改判产物 = 索引 R-14 / ADR-0020 裁定 1）：`player` 收 `rr3-brain/audio/tts_pcm` 流（MCP-1 已落 `8d05590`：新增主题在册，载荷体复用上行 `rr3_pcm_event_body` 的 `{seq, format, rate, channels, ms, pcm_b64}` 形状）→ 解 `pcm_b64` → 写 `Rr3AudioSink`；时长由**样点数**导出，`RR3_TTS_MS_PER_CHAR = 60` 与 440 Hz 查表波形退役（站替台账台-2）；`volume_pct`/`prebuffer_ms` 的行为随本单实装并按"进 ops 槽"回归——**2026-10-13 全兑现（MC-1 + MC-2）**：通路半边随 MC-1 结；两参数随 MC-2 实装（`rr3_tts_apply_gain` 整型饱和增益 + `rr3_tts_prebuffer_ready` 起播水位，legacy `0cf029e` 语义） | **0.0.5 批M MC-1 + MC-2（均已结）**（前置 MCP-1 契约 append 已落 `8d05590`） | MCP-1 |
| I-15 | **面板 = 配置面**（[ADR-0021](../process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md) 裁定 6）：观测 / 配置 / 动作三面扩端点；**覆盖率门**——面板可配置项集 ⊇ （全部 `assemble.toml` 的 `ops` 槽 ∪ 描述符键 ∪ 角色集 ∪ 每轴执行件档案字段），集合差非空即红、红项逐条登 status.md 敞口表，**不许靠缩小可配置集让它绿**。今日实况：`RR3_PANEL_ROUTES` 的 14 端点中**读配置者为 0**（GET 全是运行态：status/data/logs/wifi-scan/model-keys）。写权限边界：可写策略量、不可写契约量；欠压阈与 `armed` 这类安全相关写须二次确认 + 旧值回显 + 审计行（沿用密钥面"值零回显"同一纪律）。观测面另加一条：**当前生效绑定快照**（描述符 + 应急 env 覆盖 + 档案三者**合成后**的那份，不是原始文件） | **增补轮六 批N4** | 批N1–N3 的形状（否则面板无处可显示）；D11/D12 |
| I-16 | **显示面句柄可注入 + 帧形状改判**：`atoms/face/assemble.toml` 头注那句"描述符九键无显示键（C4 裁定不扩键），本件暂无 `Rr3Display` 句柄 → handles 仍空"随 C4 作废解锁，本件 `handles` 从此可填；帧面 `Rr3FaceFrame = [u8; 8]`（注释自述"bit7 为该行最左点，MAX7219 行寄存器直排"）是**一块具体点阵的形状**，不该当跨形态契约——新立设备无关位图面（尺寸来自绑定），旧类型保留为点阵档的实现类型（只增不改，退役走 ADR-0019 的 retired 机制）。板侧那套显示恢复机制（亮度阶梯 / 整页重写 / 总线自愈 / 渲染恢复轮数上限）进能力面文档，并立一条通用要求：**恢复类接口必须有显式轮数上限与"每轮至多一次重初始化"**（真板实锤过 30s 内 56 次重初始化风暴那类白烧） | **增补轮六 批N1**（键集与句柄）+ 批N4（面板项） | 批N1 |
| I-17 | **本册是"策略住码内且无主"最集中的一处**（实测符号级）：`composite.rs` 的 `RR3_FACE_TICK = 250ms`、`panel/server.rs` 的 `RR3_PANEL_MAX_CONNS = 8` / `RR3_PANEL_HEADER_LIMIT = 8KiB` / `RR3_PANEL_BODY_LIMIT = 4KiB`、`SelfTest` 期望表情 `"happy"`、以及 `atoms/face/mech.rs` 里兜底表情 `"idle"` **四处硬编码**——而 `atoms/face/assemble.toml` 自己写着"`default_expression` 槽待落地"，即**文档预告的去处是幻影**（I-11 只迁了 `non_pointable` 集合）。本单把这五组各立 ops 槽并落 `config.toml`/profile；自检期望表情属产线取向，须可按形态改数据。**细则见 [增补轮六全案](../process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §15.1 M-2/M-3/M-10**；beep 恒败与 selftest 逐腿自述见同节 D15 裁定。**2026-10-19 对表（批N4 件五落地，五组硬编码全数清账）**：`RR3_FACE_TICK` → config `[rr3-interact.face] face_tick_ms`（缺键回落 250，`<1` 折 1），组合件主循环节拍消费；mech 兜底 `"idle"` → config `default_expression`（缺键回落 idle，白名单对账）→ ops 槽 `atoms/face/assemble.toml`（"待落地"预告就此兑现）→ `rr3_face_with_default` 构造器注入，`end_talk` 无挂起/`begin_talk` 幂等分支两回落点改吃注入值；SelfTest 期望 `"happy"` → config `[rr3-interact.selftest] face_expect`（白名单对账）；panel 五限额见 I-18 对表。face_tick_ms/face_expect/panel 五限额消费点在组合件面无原子件对应，走**组合件级 composite.toml ops 申报**（门 j 组合件申报面首启用），⑩ 门 d ops 导出面同批扩两来源并集，分母 70→79（ops 27→36，基线双行同批上调） | **批N4**（与覆盖率门 d 同批，否则门数不到这些项） | I-15；门 j（组合件申报面） |
| I-18 | **显示面"美术 + 限额 + 恒败自检腿"三件一并数据化**（2026-10-15 复盘核实）：① 表情美术（`RR3_FRAMES_*` 与 `RR3_FACE_FRAME_TABLE`）整张 8×8 位图住件内，与 `Rr3FaceFrame = [u8; 8]` 的 MAX7219 行排布是**同一处违例的两半**（I-16 管类型、本单管内容）——换 SSD1306 形态时二者都得动码，而它恰恰已在用 SSD1306；亮度/对比度在**模块面无影**（只有板固件的 `OLED_CONTRAST_LEVELS`）。② 面板自身六个限额/超时常量（`RR3_PANEL_MAX_CONNS`、`RR3_PANEL_READ_TIMEOUT`、`RR3_PANEL_HEADER_LIMIT`、`RR3_PANEL_BODY_LIMIT`、`RR3_PANEL_CALL_TIMEOUT`、`RR3_ADDR_DEFAULT`）连 config 都不走——**面板是"覆盖率门缺失"的活证**（I-15 的分子分母也数不到它们）。③ `/api/selftest` 三腿之一（beep）恒回业务错误、**无 pass/fail 判定**，自检"绿"里混着恒败腿（D15 裁定：恒败腿不得折算成通过）。本单落：显示件档案（帧表 + 亮度档 + 物理尺寸）、面板限额 ops 槽、自检逐腿输出 `{腿, pass\|fail\|n-a, 原因}`。**裁定原文见 [ADR-0022](../process/adr/ADR-0022-代码事实复盘-机制核纯净度与申报面逃逸与安全语义身体.md) 裁定 6 与全案 §17 批N5 出厂自检行**。**2026-10-19 对表（批N4 件五落地②③；①帧表半边已随批N3）**：② 面板五限额/超时落 config `[rr3-interact.panel]`（`max_conns`/`read_timeout_ms`/`header_limit`/`body_limit`/`call_timeout_ms`，缺段/缺键回落 legacy 常量值、四值下界 ≥1），`Rr3PanelLimits` 结构经 `Rr3PanelShared` 贯穿 serve/accept/handle/ctl_query，原五常量退役——`RR3_PANEL_ADDR_DEFAULT`/ENV 保持 env+常量双真值**不进 config**（批I I-1 裁定 bind/port 删除避免第三真值点，本单沿用同口径）；③ `/api/selftest` 升逐腿输出 `{legs:[{leg, pass, reason?}], verdict}`（pass 词表 pass|fail|n-a；verdict = 全腿 pass 才 pass），speaker 恒败腿如实 fail 不折算通过（D15），前端逐腿渲染同步 | **批N4**（与门 d 同批）；帧表半边随**批N3** | I-15；I-16；批N5 |
| I-19 | **面板覆盖率门的分子今天数不出来**（2026-10-15 证伪审查，全案 §18 F-19，索引 **L-24**）：门 ⑩ 原写"面板项集 ⊇ 可配置集"，两边都没有可数的定义。分母侧：**按"角色"计是错的**——一个角色带 6~8 个键（`controller`/`channel`/`physical`/`direction`/`rail`/`mux_group`/`failsafe`），面板只显示一个角色名就能刷绿 8 个键。分子侧：`panel/server.rs::RR3_PANEL_ROUTES` 是**端点表**，一个端点能渲染 N 项，**端点数 ≠ 面板项数**，而两者之间没有任何映射声明 ⇒ 判据落到门实现者的自由裁量上，正是 L-15"同一判据两套词表"的同型失败。本单两件事：① 每个面板端点带一条**数据化申报** `covers = [可配置集元素]`（与件 `handles`/`ops` 同一申报范式：不申报就不算数——未声明的端点**不计入分子**，而不是"默认全算"）；② 分母**降到字段级**：`ops[].slot` ∪ `pin_id.field` ∪ `roles.requires` 每项 ∪ `domains` 字段 ∪ 轴档案字段。差集仍由机器生成进 status.md 敞口表（数据结构 = ADR-0022 裁定 6.3）。**红项数不许靠缩分母减少**（本册 §13 的覆盖率红项口径同样适用于这两个集合的定义改动）——**2026-10-19 对表（批N4 件三落地，申报面＋分母＋门三件齐）**：`RR3_PANEL_ROUTES` 升三元组带 `covers` 申报（词表＝码内 `RR3_PANEL_COVER_SETS` 五名 ops/pin/role/domain/axis 单真值点，词表外/缺斜杠/多段硬红；今日 14 端点全 `&[]` 如实——读配置者 0）；`check.sh` 建 `step "⑩ 门 d 面板覆盖率对账·报告模式…"`：分母字段级五路并集＝70（ops 27＋pin 11＋role.requires 去重 7＋domain 11＋axis 14，五路自代码/契约常量导出）、差集逐条 " D " 行打印、双行基线 `tools/ci/gate-d-baseline.txt` 70/70（红账上限回涨 FAIL＋分母下限缩分母 FAIL＝"不许缩分母"机器化）；差集红账不进敞口表（status :435 章程），机器真值＝步 ⑩ 打印、开放项数据结构表（裁定 6.3）随件九同批立；红项随件九三面扩端点逐批转绿 | **批N4**（`covers` 申报面与端点扩面同批；集合定义随**批N1** 键集定形后写死） | 门 d（⑩）；I-15；I-18；全案 §7/§18 F-19 |
| I-20 | **面板写面的两条硬要求：落盘必须原子，生效路径必须说出口**（2026-10-15 第五轮读码核实，全案 §19.2 Y-1/Y-2，索引 **L-26**）：① 面板**现存唯一的写落盘**是 `panel/secrets.rs:95-105`——`OpenOptions::truncate(true).mode(0o600)` **原地覆盖，无 tmp/rename、无 `sync_all`**；而读侧 :44-67 遇坏 JSON 回 `Err` 且**不覆盖**。合起来的形状：**一次掉电正好卡在写中间 ⇒ 凭据文件从此恒 `Err`，写侧也不能自愈**（它只在合法 JSON 时才 merge）。仓内已有正确范式却没人抄：`rrd/src/counters.rs:110-124` 写 tmp → `sync_all()` → 同目录 `rename` → 失败清 tmp。裁定：**轴档案、域阈值、`armed`、凭据的写一律复用那一份原子序**，出现第二条写路径即红（L-23 在写路径上的形态）。② §7 / D11 写"面板可写阈值 + 二次确认"，实测阈值与接闸位在板侧是 `pub(crate) const`（`tools/esp32-periph-base/src/board.rs:204,207,213`）、全仓 `SIGHUP`/`reload` **零命中**、`RR3_PANEL_ROUTES` 14 端点**无任何阈值或 `armed` 写口**（`panel/server.rs:86-101`）⇒ 这句话在 rtos 档的真实含义是"**重烧固件**"，而设计从没说出来。裁定：**每个可写量必填 `applies = live｜reboot｜reflash`**，UI 显式告知，与 I-19 的 `covers` 同批进门 ⑩ 申报面（缺失即红，与 §5 缺失语义同一列）。**2026-10-19 对表（批N4 件四落地，两半全结）**：① 原子序下沉共享——`rr3_atomic_store` 实现自 `rrd/src/counters.rs` 原样迁 `rr-ipc/src/persist.rs`（凭据面 rr3-interact 不依赖 daemon crate，共享件住双方公共最低层，evidence.rs 同径先例；rrd 侧 `pub use` re-export 保 counters/axis 调用点零改动），`secrets.rs` 写侧退役 `truncate(true)` 原地覆盖改走共享序（父目录 0700 = tmp 窗口护城河，rename 后收 0600）——仓内落盘写路径仍只有这一份，persist.rs 模块注释登记三调用方；② applies 申报面建成——`RR3_PANEL_APPLY_MODES = [live, reboot, reflash]` 词表常量（单真值点），`RR3_PANEL_ROUTES` 升四元组 `(method, path, covers, applies)`，今日唯一申报 = `POST /api/model/key` → `live`（secrets 落盘后 brain 每次现读，无需重启/重烧；前端文案"brain 下一轮对话生效"与应答 `applies` 字段同义双面），其余 13 端点如实空；执法双门 = `applies_use_declared_vocabulary_only` 单测（词表内＋写口带 covers 必填＋申报面漂移 pin）＋check.sh 步 ⑩ applies 段（词表打空/词表外/缺失/行形制坏四路 fail-closed 实测全红过），申报面硬红不进红账口径 | **批N4**（与 I-19 同批）；secrets 改走原子序**可先行**——纯可靠性修复，不依赖新契约 | I-15；I-19；门 d（⑩）；索引 L-26；全案 §7/§19.2 |
| I-21 | **播放的"成功"有两个假通过口，且面板把业务错误折算成 200**（2026-10-15 第六轮追流核实，全案 §20.6，索引 **L-27**）：① `composite.rs:346-349` 的 `rr3_tts_pcm_integrity(pcm_bytes, buffered)` 判据是 `buffered == pcm_bytes` ⇒ **`(0, 0)` 直接 `Ok`**。于是云返回 200 空体时走"0 帧 → 0 字节 → 对账通过 → `played_ms=0` → 计 `tts_play_ok`"，即**静默假成功**：完整性判据把"什么都没发"当成了"全发到了"。修法：加下界（`pcm_bytes > 0`，或按 `text` 长度推最小时长），并规定 **`played_ms == 0` 不得记 `ok`**。② 播放点名失败时 brain 只 print 一行就照常记 `chat_rounds`（`composite.py:265-291`，见 brain **B-16**）⇒ 本单元的应答语义要能被上游**当成判决**用，而不是被吞掉；本册负责把 `tts/text` 的 `Err` 载荷写清楚（拒因 + 是否可重试）。③ 面板侧 `panel/server.rs:598-606`：被调方返回业务错误 `{"error":…}` 时走的是 `Ok(result)` 分支 ⇒ **HTTP 200 + `ok:true`**，只有传输层失败才 502。脚本与产线按状态码判成败会**永远成功**。修法：`BusCall` 出口按载荷里有无 `error` 决定状态码与 `ok` 字段（业务失败 → `ok:false`），并与 I-19 的 `covers`/I-20 的 `applies` 同一批处置。**2026-10-19 对表（批N4 件六一笔 ①③ 结清）**：① `rr3_tts_pcm_integrity` 加零字节下界且 `played_ms==0` 折 `tts_play_reject` 不记 `tts_play_ok`；③ 面板 `BusCall` 出口载荷判等（`rr3_bus_reply_status`，业务错误 422+`ok:false` 与 502 传输失败分码）；② 应答拒因随同笔落定；②侧 brain 计账拆 `rounds_replied`/`rounds_audible`（B-16 同笔，`chat_rounds` 退役），本行取证时点写法保留 | **已结（批N4 件六一笔）** | I-14；I-15；I-19；I-20；门 d（⑩）；B-16；全案 §20.6/§20.7；索引 L-27 |

## §11 待证与敞口

1. 真屏与真回放从未发生（显示真后端未实装、喇叭/麦未采购）。
2. ~~`volume_pct` / `prebuffer_ms` 对应的行为未实装~~——**已随 0.0.5 批M MC-2 结清（2026-10-13）**：增益与水位落 mech.rs 机制纯函数 + config `[rr3-interact.player]` 注入（§3/§13 已同批更新）；真回放（喇叭）仍敞口。
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
| `atoms/face/face.rs : RR3_FACE_EXPRESSIONS` | `["idle","blink","talk","think","happy","listen","turn"]` + `charA..charZ`/`char0..char9` 共 43 态（2026-10-15 板上批扩容） | 表情白名单 | 有（`rr3_face_set`/`rr3_face_frames`） | **契约字面量—不许动**（面板下拉框与 schema 都在读；集合扩容走契约评审——char 档即按此三处同批：schema enum / topics note / 本常量） |
| `atoms/face/face.rs : RR3_FACE_TALK` | `"talk"` | "不可点名"的排除项 | 有 | 契约字面量；**规则本身（"播放态不可点名"）已外移** → config `[rr3-interact.face] non_pointable` + ops 槽 `non_pointable`（**I-11 已落**，让形态可选择放开——空集全放） |
| `atoms/face/mech.rs : Default / begin_talk / end_talk` | `"idle"` ×3 | 默认表情兜底 | 有 | ops 槽 `default_expression`（config 里同名键**今天无读者**——见下一行） |
| `config.toml [rr3-interact]` | `[rr3-interact.face] non_pointable = ["talk"]`、`[rr3-interact.player] prebuffer_ms = 500 / volume_pct = 70` | 不可点名集 + 起播水位/播放增益（策略数据） | 有（组合件 `rr3_interact_config_parse`；**I-11 起装载**，env `RR3_INTERACT_CONFIG` 可覆盖） | 死键六项已随 I-1 删；face 键是 I-11 落点（**可注入的是集合，"某形态放开 talk"= 改数据**）。player 两键**已随 MC-2 实装回归**（缺键回落默认 500/70，类型错/负数/越界拒 boot） |
| `atoms/player/mech.rs` | `RR3_TTS_BLOCK_SAMPLES=320`、`RR3_TTS_SAMPLE_RATE=16_000`（`RR3_TTS_MS_PER_CHAR`/`RR3_TTS_MAX_MS`/`AMP`/`HZ` 已随 MC-1 删除） | 20ms 分块与采样率（透传播放的分块基准）+ **MC-2 增益/水位纯函数**（`rr3_tts_apply_gain` i64 中间量饱和夹断、`rr3_tts_prebuffer_ready` min 水位） | 有 | 两项 = 与 sense/线格式同源的**契约字面量**（双写记债：应引同一真值）；估时长公式与占位波形已随 MC-1 退役（台-2 清零），播放时长由样点数导出；增益/水位的**策略值走 config 注入**（MC-2），函数只承载机制 |
| `src/composite.rs` | `RR3_POLL_WAIT=250ms`、`RR3_FACE_TICK=250ms`、`"beep"`、`rate=16000 ch=1`、`8x8` 文本 | 节拍 / 自检词汇 / 回显 | 有 | 两项节拍 = ops 槽（与 act/sense/power/connect 的 POLL_WAIT 同批）；`"beep"` = 契约字面量（面板读它） |
| `src/panel/server.rs` | `RR3_PANEL_ADDR_DEFAULT="127.0.0.1:8080"`、`RR3_PANEL_MAX_CONNS=8`、`RR3_PANEL_READ_TIMEOUT/CALL_TIMEOUT=5s`、`HEADER_LIMIT=8*1024`、`BODY_LIMIT=4*1024` | 监听面与防护上界 | 有 | 并发/超时/上界 = **ops 槽**（安全相关的上界可留件内，但必须登记）；地址缺省 = config `[panel] bind/port` 的正解，今天两处真值 |
| `src/panel/server.rs` 路由 + `src/panel/www/index.html` | 11 条 `/api/*` + `/`、三枚透传主题、四枚 `rr3.ctl.*` 方法、id 1/2/3/4、body 键 `on`/`expression`；页面**逐字复读**这些串并读 `result.played_ms/previous/ssid/connected` | 端点白名单 = 对外可观测面 | 有 | **契约字面量—不许动**；但"后端一份、页面一份"的**双写是债**（改一端必漂移）→ 由 `/api/meta` 下发或生成器产页（I-12） |
| `atoms/face/mech.rs : rr3_face_set` 报错文案 | `"不可被点名（本形态不可点名集 talk）"`、`"不认表情 {expr:?}（允许 …）"` | 应答文案 | 有 | **契约字面量—不许动**（I-11 后"不可点名"拒因由注入集渲染，空集显示 `(空)`） |
| `atoms/face/mech.rs : RR3_FRAMES_*` | 11 帧 `0b0110_0110` 一类 8×8 位模式 | 渲染**数据**（不是阈值） | 有 | 不是策略，但也不该焊在 `mech.rs`（R-12：换屏必须重画帧）——**2026-10-13 MC-3 复核余数**：R31 Q2 后板面数据落点 = ESP32 板侧渲染（批C `expr_eye_base` 胶囊眼基态 + 微动作已落 `tools/esp32-periph-base/src/oled.rs`），但宿主生产链仍走位图变体（composite 250ms tick 推 `RR3_FRAMES_*` → 桥 `frame` 变体 → 板 `frame_mode`），件内表仍是唯一真值——余数 = 出帧链切板面数据 + 件内表退役/降 sim 档专用，归属待裁（0.0.5 方案 §19 MC-3 行） |
| `src/panel/server.rs : SelfTest` | `"happy"` | 自检用表情 | 有 | ops 槽（自检期望不该硬编码） |
| `atoms/face/assemble.toml` | `handles = []`、`ops = [{slot="non_pointable" ← rr3-interact.face.non_pointable}]`；player 件 `handles = [{Rr3AudioSink ← alsa_device}]`、`ops = [{prebuffer_ms ← rr3-interact.player.prebuffer_ms}、{volume_pct ← rr3-interact.player.volume_pct}]`（**MC-2 落**） | 注入面申报 | **face 报 ops（I-11）/ player 报 handle + ops（L-5/MC-2）** | face 无可用显示句柄（`[hardware]` 连显示键都没有，I-2 前置 C4）→ `handles` 仍空，但 ops 非空故不写 `no_injection`（rr-registry 形状级规则：该键只允许两数组都空）。`default_expression` 槽待显示键落地时补 |

## §14 基线对表（代码 `22783ea`，2026-10-10；本节是本册唯一"现在时"）

| 本册旧断言 | 现在的事实（符号为证） | 判定 |
|---|---|---|
| §12 player 卡在"每拍 `Vec<i16>` + `std` `sin`"（I-10） | **占位波形整体已随 MC-1② 退役（2026-10-13，`f959e3a`）**：`rr3_tts_play` 透传 `&[i16]` 按 320 样点/20ms 分块写 `Rr3AudioSink`，时长由样点数导出；LUT 查表与估时常量删除 | **I-10 已结**（终态 = MC-1 落地，§14.1 I-10 行同批更新） |
| §1/§10 载荷上移（I-8） | `rr3_face_result`/`rr3_tts_result`/两个 `*_parse_args` 在 `src/composite.rs`；件面 `json!` 零命中 | **I-8 已结** |
| §5 "`asr/text`、`dialog/turn` 申报发布却零发布（I-5 待执行）" | `contracts/topics.toml` 两条置 `status="retired"`；`rr3-interact/module.toml` 现 `publishes = []`、`subscribes = ["rr3-connect/net/link"]`（`intent/result` 订阅申报已撤）（注：结案时快照；MC-1 起 `subscribes` 又增 `rr3-brain/audio/tts_pcm`，见 §0/§5） | **I-5、I-7 已结**（R-3 落地） |
| §13 五个死配置键（I-1） | `config.toml` 头注逐键写明删除理由，`log_level`/`default_expression`/`prebuffer_ms`/`volume_pct`/`bind`/`port` 不再挂"看着生效"的面 | **I-1 已结**；**新查出 5 个同类**（`[rr3-sense.audio].bit_depth`、player 两键、panel `bind`/`port`）已并回 0.0.5 §17 孤儿参数条 |
| §12 face 三处 std/alloc 形状 | **未结**：`RR3_FACE_EXPRESSIONS.join("/")` 仍产堆、`rr3_face_set -> Result<(_, _), String>` 仍是 `String` 错型、`std::mem::replace` 未改 `core::`（另：`mech.rs` 内 f64 仅测试面） | 批L **L-3**；**批N24 件二核实注记（2026-10-23）**：第三分句判定过时——face/mech.rs :678/:689 均已是 `core::mem::replace`，该分句核实即结；join 与 String 错型两分句仍真，行维持待批N24 件四收 |
| §3/§12 显示句柄 `[hardware]` 无键（I-2） | **本轮裁定为"不扩键"**：`descriptor.rs` 九键闭合表原样，`rr3_hal_display` 只有 sim/placeholder 两分支（真屏未实装）。扩键评审（C4）以"不扩"结案，真屏到位时再走一次 append | **I-2 关闭**（判定改，不是实现改） |
| §12/§13 `handles`/`ops` 申报 | **L-5 已结（批M MC-10 L-5，2026-10-13）**：player 报 `Rr3AudioSink`←`alsa_device` 句柄；face 无可用注入面（显示句柄键未立、config 空壳）→ 明写 `no_injection = true` | — |
| §4 panel `live` 计数与 accept 循环（R-5 改判的可达缺陷） | `rr3_panel_accept_loop` 抽缝落地：spawn 失败弃置该连接并 `panel_spawn_fail` 点账，accept 循环继续服务；`Rr3ConnSlot` RAII 守卫管 `live` 计数（Drop 归还，panic 展开不泄漏） | **I-4 已结**（批M M-β 3bc30f1，2026-10-12） |

**本册新净余数**：批L **L-3**。**L-5 已结（批M MC-10 L-5）**；**I-12/I-9 已结**（批M MC-8③ `ba90367`）。

### §14.1 增量对表（2026-10-12，代码基线 `78625ae`；全仓总表见 [索引 §13.4](模块设计索引.md)）

| 项 | 复验实况（符号） | 判定 / 新落点 |
|---|---|---|
| I-1 孤儿参数 | `config.toml` 现为**空壳**，头注逐键写明六键全删的理由（含 `volume_pct`/`prebuffer_ms`"行为未实装不提前占键"） | **结**；行为实装随 **MC-1/MC-2** 回归并按"进 ops 槽"落——**MC-2 已落（2026-10-13）**：两键回归 `[rr3-interact.player]` + ops 槽，行为实装 |
| I-2 显示 `[hardware]` 扩键 | R31 **Q2** 已裁显示件 = SSD1306 I2C **挂 ESP32 协处理器** | **作废登记**（Pi 侧 SPI 扩键需求消失，批J C4"不扩键"结案不需重开）；清零判据留 **MC-3**。**2026-10-14 再改判**："协处理器"定性与"C4 不需重开"两句同时作废——见本册 I-2 行与 [ADR-0021](../process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md) 裁定 1/4；本件句柄可填的施工落点 = **I-16** / 增补轮六批N1 |
| I-3 件面同步签名 | 随批I `rr-hal-api` 落地，件面 `rr3_block_on` 零命中 | **结** |
| I-4 `spawn` 失败退出 accept 循环 | **已结（批M M-β 3bc30f1，2026-10-12）**：spawn 失败改弃连点账（`RR3_PANEL_SPAWN_FAIL_KEY`，心跳自报可见）继续服务；`Rr3ConnSlot` RAII 守卫防 panic 泄漏。用例先红后绿：必败注入 `stack_size(usize::MAX)`（实测 macOS 对 `stack_size(1)` 静默 clamp 使 spawn 成功，`usize::MAX` 才 EINVAL 确定性失败） | 已结 |
| I-9 / I-12 端点数与双写 | **已结（批M MC-8③，2026-10-12）**：端点唯一真值 = `RR3_PANEL_ROUTES` 常量表，文案由表长推导；页面注释撤手写计数指向该表 | 已结 |
| I-10 player 跨档化（占位波形） | **已结（批M MC-1②，2026-10-13 `f959e3a`）**：占位正弦与估时长删除，`rr3_tts_play` 透传 `&[i16]` 按 320 样点/20ms 分块写 sink，时长由样点数导出；台-2 随此清零 | **已结**（`volume_pct`/`prebuffer_ms` 参数已随 MC-2 实装，见下行） |
| I-11 `talk` 不可点名规则 | **已结（批M MC-10 I-11，2026-10-13）**：件内 `if` 去硬编码 → config `[rr3-interact.face] non_pointable = ["talk"]` + `atoms/face/assemble.toml` ops 槽 `non_pointable`；interact 重开 config 装载面（`toml` 入依赖），组合件读入透传；空集全放（改数据不改码） | 已结 |
| I-14（本册新立） | 真 PCM 播放通路——**通路半边已结（MC-1，2026-10-13）**：收流解码 + 点名对账排空 + 透传写面（§1/§4/§5 已同批更新）；**参数半边已结（MC-2，2026-10-13）**：`rr3_tts_apply_gain`（整型饱和增益）+ `rr3_tts_prebuffer_ready`（起播水位，legacy `0cf029e` 语义）落 mech.rs 纯函数，config `[rr3-interact.player]` 缺键回落默认、类型错/负数/越界拒 boot，ops 槽两条同批；引擎侧（本地 Piper 产流）归增补轮五批E | **已结**；闭环余批E |
| F-2 显示面（本册职责边界） | 施工落点在**增补轮五批B/批C**（R31 Q2），本册只持"真后端名可自述 + fail-closed 报缺"的清零判据 | 与 0.0.5 §18.4 分账一致，勿在本册找 SPI 驱动工单 |
