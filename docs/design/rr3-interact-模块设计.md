# rr3-interact 模块设计（框架 v1.1）

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程（host）；`start_priority = 60`、`mem_limit_mb = 96`、`restart_policy = "always"` | `module.toml` |
| 格 | `ui.face` · `audio.playback` | `contracts/slots.toml` |
| 件 | `face` · `player`；组合件成分：`panel/`（状态面 HTTP）、mpsc 动作通道 | `atoms/`、`src/` |
| 主题 | RPC `face/expression`、`tts/text`；订阅 `brain/intent/result`、`connect/net/link`；申报发布 `asr/text`、`dialog/turn`（**零发布**） | `module.toml` |
| 句柄 | `rr3_hal_display()` → `dyn Rr3Display`；`rr3_hal_audio_sink()` → `dyn Rr3AudioSink` | `src/composite.rs` |
| 参数 | `prebuffer_ms=500`、`volume_pct=70`、`default_expression="idle"`、`log_level`、`bind`、`port` | `config.toml`——**本 crate 无 toml 依赖、无任何读者** |
| 健康 | 心跳 1000 / 看门狗 5000 ms | `[health]` |

## §1 件契约表

| 件 | 公开符号（现状） | 输入 → 输出 | 状态 | 失败语义 |
|---|---|---|---|---|
| face | `rr3_face_frames(expr: &str) -> &'static [Rr3FaceFrame]` | 表情名 → 帧表（静态） | 无 | — |
| face | `Rr3Face::rr3_face_set(&mut self, expr: &str) -> Result<(&'static str, &'static str), String>` | 白名单名 → `(当前, 前一)` | `current` + 帧游标 | 不认的名 → 业务 error |
| face | `rr3_face_begin_talk(&mut self) -> &'static str` / `rr3_face_end_talk(&mut self) -> &'static str` | talk 挂起/恢复 | `talk` 挂起位 | 不可能失败 |
| face | `rr3_face_tick(&mut self) -> (&'static str, usize)` | 一拍 → `(表情, 帧号)` | 帧游标 | — |
| face | `rr3_face_parse_args(&Value) -> Result<String,_>` · `rr3_face_result(current, previous) -> Value` | 点名参数/应答 | 无 | 业务 error |
| player | `rr3_tts_duration_ms(text: &str) -> u64` | 文本 → 占位时长（60 ms/字符封顶 10 s） | 无 | — |
| player | `rr3_tts_parse_args(&Value) -> Result<String,_>` · `rr3_tts_play(sink: &dyn Rr3AudioSink, text: &str) -> Result<u64,_>` · `rr3_tts_result(text, played_ms)` | 文本 → 播放 → 实播时长 | 无 | 写失败 → 业务 error |
| 共同目标签名 | `Rr3FaceFrame` 与 `Rr3Display`/`Rr3AudioSink` → `rr-hal-api`；`sink` 参数由 `&dyn` → 注入句柄槽；**"被组合件调用"保持不变**（件不起线程） | | | |

## §2 状态机（face 表情机）

七态白名单（含 `listen`/`turn` 可点名，`talk` 由播放自动驱动**不可点名**）；不变式：
① 点名非法态 → 拒且**状态不动**；② **权威序：显式点名 > 播放自动态**——`begin_talk` 挂起当前表情，
播放途中若被显式 `set` 点名，挂起态**作废**（`suspended = None`），`end_talk` 于是回**默认态**而不是
回挂起前的表情（`face/mech.rs` 的 `rr3_face_set` + 用例 `explicit_set_interrupts_talk_suspend` 钉死了这一条）；
未被点名打断时，`end_talk` 才回挂起前的表情；
③ 帧游标在换态时归零（每态从帧 0 起）；④ 推进只由 `tick` 驱动，**不随总线流量漂移**。
`player` 无状态（一次调用一次播放）；`panel` 无状态（读缓存 + 转发）。

## §3 注入面

| 件 | `handles` | `ops` 槽（现状 vs 目标） | 非法收敛 |
|---|---|---|---|
| face | `dyn Rr3Display` ← `[hardware]` **无显示键**（现只有 ALSA/PWM/GPIO/I2C/link 九键） → 需扩键（走 C4 契约评审） | `default_expression: string` = `"idle"`（现无人读）；帧推进周期 250 ms（代码常量） | 无屏 → 格缺，不调帧机 |
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
| panel 十二端点 | HTTP | GET/POST | 状态/硬件/系统九 + scan/connect/forget 透传 | 不是总线契约面，但属**对外可观测面**（要留在册） |

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
| I-1 | `config.toml` 六参数：**被读或删除**（不允许第三种状态）；音量/预缓冲若保留则进 ops 槽 | 批I | C5 |
| I-2 | 显示句柄的 `[hardware]` 键扩位（走契约评审） | 批J | C4 |
| I-3 | 件面句柄换 `rr-hal-api` 同步签名 | 批I | C5 |
| I-4 | **改立可达的那条**：`spawn` 失败不得用 `?` 退出 accept 循环（单连接起不来只丢该连接并计数，面板必须继续服务）；`live` 计数顺带加 RAII 守卫，防未来新增 panic 点时变成静默泄漏。回归 = 起不来一个连接后面板仍可访问 | 批K | — |
| I-5 | `asr/text`、`dialog/turn` 处置（retired 首例，与 C14 同批） | **待裁** | — |
| I-6 | 面板是否进 rtos/bare 档定案（本册建议：host 档专属） | **待裁** | — |
| I-7 | `intent/result` 的申报订阅面处置：要么补上真实消费者（表情随意图联动），要么按 ADR-0019 修订 2 的 retired 机制把这条订阅义务解除（与索引 §2 的 `asr/text`/`dialog/turn` 同批走 C14）。停在"在册但无人读"是最坏状态——依赖仲裁会当它是活边 | 契约评审（批J 前） | C14 |

## §11 待证与敞口

1. 真屏与真回放从未发生（显示真后端未实装、喇叭/麦未采购）。
2. `volume_pct` / `prebuffer_ms` 对应的行为**未实装**，不是"配了没读"那么轻——播放路径里没有缓冲与音量控制。
3. 面板鉴权模型 = 单机可信 + 只绑回环；对外暴露的决策随 M5 定，本册不预设。
4. `live` 计数泄漏的**可达性未证**：今天找不到能让连接线程 panic 的路径，所以"累计到上限 → 永久 503"
   只能作敞口登记（触发条件 = 面板转发链里第一次引入 `unwrap`/`expect`）。别把它写成现状缺陷。
5. RPC 载荷**没有 schema 真值点**：`contracts/events/` 只覆盖 event 主题，`face/expression` 与 `tts/text`
   的应答形状今天由件实现 + 面板取键方式共同"习惯性"锁定。补 RPC 载荷 schema 是契约级动作（与 C4 同批），
   在它落地前 §8 的"腿二"无处引值——这是本册 §8 唯一标"期望值来源不足"的原因。
