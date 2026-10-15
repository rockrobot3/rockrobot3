# rr3-act 模块设计（框架 v1.3）

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程（host）；`start_priority = 40`、`mem_limit_mb = 48`、`restart_policy = "always"` | `module.toml` |
| 格 | `act.servo` · `act.led` · `act.relay`（全 body 层） | `contracts/slots.toml` |
| 件 | `servo` · `led` · `relay`；非件成分 `intent.rs`（接入面）、`reflex.rs`（守护面宿主） | `atoms/`、`src/` |
| 主题 | RPC `servo/command`、`led/effect`、`relay/set`；发布 `servo/state`(stream)、`relay/state`(event)、`motor/reflex`(notification)；订阅 `rr3-brain/intent/result` | `module.toml`、各 `atom.toml` |
| 句柄 | `rr3_hal_servo()` → `dyn Rr3PwmServo`；`rr3_hal_gpio_out(role)`（role 白名单 `led`/`relay`）→ `dyn Rr3GpioOut` | `src/composite.rs`、`atoms/{led,relay}/mech.rs` |
| 参数 | `angle_limit_deg=[70,110]`（分轴：本面默认轴 pan，板侧按轴各自裁决 pan[70,110]/tilt[80,100]，见 §3）、`move_timeout_ms=800`、`state_min_interval_ms=50`、`blink_period_ms=500`、`loop_hz=1000`、`saturate_notice_ms=500` | `config.toml`（`RR3_ACT_CONFIG`） |
| 健康 | 心跳 1000 / 看门狗 5000 ms | `[health]` |

## §1 件契约表

| 件 | 公开符号 | 输入 → 输出 | 状态 | 失败语义 |
|---|---|---|---|---|
| servo | `rr3_servo_parse_args(args: &Value, cfg: &Rr3ServoCfg) -> Result<(u16, u64), String>` | 点名参数 + 行程/时限配置 → `(角度, 时限ms)` | 无 | 业务 error（缺参/类型错/越界逐条点名） |
| servo | `rr3_servo_state_body(seq: u64, angle_deg: u16) -> Value` | 序号 + 落角 → 流载荷 | 无 | — |
| servo | `Rr3ServoCfg { angle_min_deg, angle_max_deg, move_timeout_ms, state_min_interval_ms }`（件入口声明，config 形状） | — | — | — |
| servo | 借用机制核：`rr_core::control::Rr3RateGate`（re-export 保路径） | `rr3_gate_allow(now_ms) -> bool` | `last_ms` | — |
| led | `rr3_led_parse_args(args: &Value) -> Result<Rr3LedEffect, String>`（白名单 on/off/blink） | → 枚举 | 无 | 业务 error |
| led | `Rr3Led::rr3_led_new(gpio: Arc<dyn Rr3GpioOut>, blink_period_ms: u64) -> Self` | 句柄 + 周期 → 控件 | **自持 worker 线程** + `current` | `spawn` 失败 → **`expect` panic** |
| led | `Rr3Led::rr3_led_apply(e: Rr3LedEffect) -> Result<(Rr3LedEffect, Rr3LedEffect), String>` | 效果 → `(生效, 前一)` | `current` | worker 已退 → 业务 error（进程不亡） |
| relay | `rr3_relay_parse_args(args) -> Result<bool,_>` · `rr3_relay_apply(gpio: &dyn Rr3GpioOut, on) -> Result<bool,_>` · `rr3_relay_result` · `rr3_relay_counter_key(on)` · `rr3_relay_state_body(seq, on)` | 参数 → 落电平 → 应答 / 计数键 / 流载荷 | 无（`relay_seq` 在组合件） | 业务 error |
| 共同目标签名 | `use rr_hal::…` **全部 → `rr_hal_api`（同步、无 `Arc<dyn>`、无 `rr3_block_on`）**；led → `rr3_led_tick(now_ms, state) -> Option<bool>`（线程上收到组合件） | | | |

## §2 状态机

- **`Rr3LedEffect`**：`Off | On | Blink`（无隐式态）；不变式：`apply` 立即落目标电平，blink 从**亮相**起步，不等下一半拍。
- **`Rr3RateGate`**（机制核）：不变式"状态变化必发布，两次发布至少隔 `min_interval_ms`"；被限掉的这次**不缓存不补发**（流是摘要不是逐拍镜像）。
- **饱和检测 `Rr3SaturationDetector`**（机制核）：`run >= need` 且 episode 首报恰一次；任一非饱和窗清零复位。`Rr3ReflexCfg{loop_hz, saturate_notice_ms}` → `rr3_reflex_window_plan` 出 `(window_ms=250, need, expect_ticks)`；窗内实跑 tick `< expect/2` 才算饱和（sleep 粒度的系统性超期不算）。
- 非法输入收敛方向：角度越程 → **点名拒收**（件面 `rr3_servo_check_travel` 出 error、不落角；与板侧 `servo_set` 同口径 fail-closed）——安全核 `rr3_travel_clamp` 只是裁决被绕过时的兜底（合法输入恒等，不是 RPC 语义）；参数缺/类型错 → 拒绝且不落电平。

## §3 注入面

| 件 | `handles` | `ops` 槽（名 / 类型 / 来源 / 默认） | 非法收敛 |
|---|---|---|---|
| servo | `dyn Rr3PwmServo` ← `[hardware].pwm_chip`、`pwm_channel` | `angle_min_deg`/`angle_max_deg` ← `angle_limit_deg`（**单区间**：本面默认轴 pan [70,110]，轴由 `RR3_HAL_PERIPH_AXIS` 选，改 `tilt` 须同步 config [80,100]——板侧按轴各自裁决）；`move_timeout_ms`；`state_min_interval_ms` | 端点颠倒 → 取小者为下界并钳到最保守端（`rr3_travel_clamp` 用例已钉） |
| led | `dyn Rr3GpioOut(role="led")` ← `gpio_chip`、`gpio_led_line` | `blink_period_ms`（默认 500，代码再 `.max(1)`）；`led_effects` ← `[rr3-act.led] effects`（灯效白名单**子集**，A-9；值集 on/off/blink 是契约字面量不许改、集合可注入，空组合法＝本形态一条不认） | 写失败 → `eprintln` 不退（妆面件，**例外需在此点名**） |
| relay | `dyn Rr3GpioOut(role="relay")` ← `gpio_relay_line` | 无（无常量） | 写失败 → 业务 error |
| reflex | 无（读 act 自身状态） | `loop_hz`、`saturate_notice_ms` | `loop_hz` 现状 `as u32` 裸截断 → 改为 `try_from` + 拒 boot（§10 A-3） |
| 策略位点 | — | `motor/reflex` 的通告节流属机制；**"什么算异常"属策略**（现无） | — |

## §4 主循环与时序预算

| 常量 | 值 | 位置 |
|---|---|---|
| `RR3_POLL_WAIT` | 250 ms | `src/composite.rs` |
| 反射环名义 | `loop_hz = 1000` → 1 ms/拍，窗 250 ms、通告 500 ms | `reflex.rs` + `rr3_reflex_window_plan` |
| 心跳 | 1000 ms（`rr-bus` 独立线程，`SystemTime` 喂拍点） | client |
| 看门狗 | 5000 ms | `[health]` |

**关键时序断言（现状问题都在这里）**：
1. `servo/state` 的门用**墙钟**（`composite.rs` 的 `rr3_now_ms` 取 `SystemTime`）→ 回拨 N ms 则该流停 N ms；
   设计要求：改 `Rr3TimeBase`，且回拨不得造成静默（换单调后此问题类别消失）。
2. RPC 端到端落角时延 = 点名应答（同步路径） + PWM 生效；**未实测**。
3. 环与心跳**不同线程**：环饿死时心跳仍续 → `rr3.ctl.health` 看不出"不动了"。设计：环tick 计入 counters，作为第 4 个健康量。
4. intent 路与 RPC 路共用 `relay_seq`/`servo` 流序 → 断言 seq 跨路连贯（e2e 已钉 RPC=1 → intent=2）。

## §5 主题面契约

| 主题 | kind | seq | 触发 | 载荷必填 | 失败映射 |
|---|---|---|---|---|---|
| `servo/command` | RPC | — | 点名 | `{angle_deg, move_ms?}` | 越程→**error 点名拒收**（件面 `rr3_servo_check_travel`，与板侧 `servo_set` 同口径 fail-closed）；缺/类型错/越 u16→error 点名。安全核 `rr3_travel_clamp` 只在裁决被绕过时兜底（合法输入恒等，非 RPC 语义） |
| `servo/state` | stream | 自增 | 变化 + 50 ms 限速 | `seq, angle_deg` | — |
| `led/effect` | RPC | — | 点名 | `{effect}` 白名单 | 不认点名白名单 |
| `relay/set` | RPC | — | 点名 | `{on}` | 写失败 error |
| `relay/state` | event | 自增 | **设计规定 = 变化即发**（`relay.rs`/`atom.toml`/`mech.rs` 三处注释同口径）；**实况 = 每令必发**——`composite.rs` 的 `relay_applied` 只表"这条命令落成了电平"，不含变化判定，重复同态也发一帧（缺陷 A-7） | `seq, on` | — |
| `motor/reflex` | notification | — | episode 首报 | 饱和窗数据 | 订阅者缺席也必须能发 |
| `intent/result`（订） | event | — | 消费 | `actions[]: {topic,args}` | 越权→拒执行折日志不断链 |

## §6 降级矩阵

| 格 | 缺格 | 句柄缺 | 句柄值错 | 后端未实装 | 参数非法 |
|---|---|---|---|---|---|
| `act.servo` | 本单元仍在（led/relay 独立），但"能动"形态不成立 | placeholder → 开机探活 `Unsupported` → **`exit(1)`** | 描述符作废且静默（批J 修） | — | 行程/时限非法 → 拒 boot（`bad_range()` 已有） |
| `act.led` | 只停灯效 | 同上 | 同上 | — | `blink_period_ms=0` → 代码 `.max(1)` 已兜 |
| `act.relay` | 只停继电器 + intent 路该动作拒执行 | 同上 | 同上 | — | — |

## §7 安全核接入（本单元是全项目最密处）

三条，全部落点在机制核、参数由本单元注入、**策略不可绕过**：
1. **限位**：`rr3_travel_clamp(angle, min, max)` 在 **RPC 与 intent 两路落角前**各调一次——
   不能绕过，因为钳位在"角度进执行器之前"这一唯一出口上（两路都走它，用例已钉）。
2. **时限**：`move_timeout_ms = 800`（超时即停，不给堵转留机会）——参数可注入，语义不可改。
3. **饱和通告**：`Rr3SaturationDetector`（机制核），episode 首报恰一次。
低电兜底不在本单元（在 power），但"低电时禁动"这条**已定义并落码**——A-5 由索引 R-15 结案（2026-10-12），批M MC-5 于 `071dcad`+`8b5b57e`（2026-10-13）落 `Rr3LowGate` 接 `rr3-runtime/system/power`，执行面 RPC/intent 两路拒动、妆面不禁（欠板上实录）。

## §8 两腿对拍用例设计

| 件 | 输入序列 | 期望值来源 | 边界集 | 现状 |
|---|---|---|---|---|
| servo 参数裁决 | `angle ∈ {0, 45, 180, 200, -?}` + cfg(45,135) | 裁定语义（钳位、端点颠倒收敛） | 端点内/外/等号/颠倒 | ✅ rr-core `travel_clamps_into_range_and_survives_inverted_range` + 件面 parse 用例 |
| servo 限速门 | now = 1000 + 10k，k∈0..24 | 裁定语义"每 5 拍放 1 次" | 窗起 / 窗末等号 / 迟到拍 | ✅ 机制核 `same_control_graph_beat_for_beat_identical`（件面需引它做期望值） |
| led | 效果序列 `[On, Blink, Off, Blink]` + 半周期序列 | **期望 = 翻转序列白名单**（不得由件自己产出） | 半周期=1、worker 已死 | ✅ **已有三条**：`blink_toggles_level_and_off_stops_it`（翻转序列）、`on_and_off_take_effect_immediately`（立即落电平）、`apply_tracks_current_and_previous`（前后态）。⚠️ 缺的不是用例而是**合成时钟**——这三条用 `rr3_wait_until` 真等半周期，节拍来自 worker 线程自睡；A-1 去线程后期望序列必须由注入的 `now_ms` 产出，否则回归面在改线程模型时无人守护 |
| relay | `on` 序列 `[true,false,false,true]` | 载荷 + `relay_seq` 连贯（e2e 已断 RPC=1 → intent=2） | **重复同态不得续发**（设计规定；现状每令必发 = 缺陷 A-7，用例随修一起出，先出即红） | ⚠️ 连贯性有、同态抑制无用例 |
| reflex | tick 密度 `[250,125,124,0]`/窗 | 半拍线判定 + episode 恰一次 | 恰半拍 / 低于半拍 / 孤立饱和窗 | ✅ 机制核三条；件面需补"环饿死 → 通告仍发得出"一条 |

## §9 不变量与红线断言

- 断言：任何进入 `Rr3PwmServo::set_angle` 的角度都满足 `min ≤ a ≤ max`（钳位后）。
- 断言：`servo/state` 两次发布间隔 ≥ `state_min_interval_ms`；`relay/state` 不受限速门管辖。
- 断言：`relay/state` 只在电平**实际变化**时发布（申报口径；现状未成立 → 工单 A-7，修好后此条进 e2e）。
- 断言：`relay_seq` 跨 RPC/intent 两路连贯（已有 e2e）。
- 断言：反射环永不发总线帧、永不经 `⊙`（ADR-0007 裁定 1）。
- 断言：越权 intent 动作**只拒执行**，不断链、不改状态。
- 断言：三 RPC 名/参数裁决文案（点名缺哪个字段）零改动——面板与 legacy 依赖字面。

## §10 改造工单

| # | 动作 | 归属 | 前置 |
|---|---|---|---|
| A-1 | led 去线程/去 mpsc → `rr3_led_tick(now_ms, state)`，节拍归组合件；**现有三条用例改用合成时钟**（真等半周期 → 注入 `now_ms` 逐拍推），行为序列逐字不变 | 批I | C7 |
| A-2 | 时基改 `Rr3TimeBase`（含 `rr-bus` 心跳），去掉 `SystemTime` 单调性误称 | 批I | — |
| A-3 | `loop_hz` 用 `try_from<u64> for u32` + 越界拒 boot | 批I | — |
| A-4 | servo/relay 句柄面换 `rr-hal-api` 同步签名（去 `rr3_block_on`） | 批I | C5 |
| A-5 | "低电禁动"语义定义（power 边沿 → act 闸门，落点在机制核安全核段）——**2026-10-12 由索引 §13.3 裁定 R-15 结案**（四条语义：触发源唯一 = rrd 的 `rr3-runtime/system/power` 广播；禁执行面、不禁妆面；回阈 + 迟滞解禁且不自动重放被拒的令；bare 档由安全核段自判 `Rr3LowEdge` 出 `RR3G` 行），详见 [0.0.5 §20 第 2 条](../process/dev/0.0.5版本洛克三号对齐与裁定方案.md) | **0.0.5 批M MC-5**——**已结（`071dcad`+`8b5b57e`，2026-10-13）**（前置 MCP-3 申报面评审；与 P-8 / 批L L-8 合并成一个动作） | MCP-3 |
| A-6 | 环 tick 计入 counters（健康面可观测"活着但不动"） | 批K | C9 |
| A-8 | **载荷组装上移承载层**：`rr3_servo_state_body` / `rr3_led_*` / `rr3_relay_result` 之类 `-> Value` 的函数从件面移到 `src/composite.rs`，件只交结构体与判定（依据 = ADR-0019 修订 4；做完 `serde_json` 从件面清零） | 批I | — |
| A-9 | 意图开放门与灯效白名单从硬编码 `match` 改**数据化**——**2026-10-13 由 0.0.5 批M MC-10 落**：意图开放门落 config 键 `[rr3-act.intent] open_topics`（组合件读入透传；因 ⑧-c 注入面门只扫 `atoms/*/assemble.toml`、组合件层无 ops 校验位，故**不进 assemble.toml**）；灯效白名单落 `atoms/led/assemble.toml` 的 ops 槽 `led_effects ← rr3-act.led.effects`。契约字面量不许改（on/off/blink、两条执行臂主题），可注入的只是**集合**；缺键/越界值拒 boot（fail-closed），空数组合法（本形态一条不认）| **0.0.5 批M MC-10**（结）| C1 |
| A-10 | `intent.rs : EXPOSED` 内联 `"rr3-act/led/effect"` 改引 `led::RR3_LED_EFFECT_TOPIC`（消第二真值点） | 批I | — |
| A-11 | `config.toml [rr3-act] log_level` 孤儿键：被读或删除（与 P-6/N-6/I-1/B-8 同批） | 批I | — |
| A-12 | **件失败语义分类表**（D8 本轮立单，此前索引挂着无工单）：`led`/`face` 属妆面（写失败折日志不退）、`servo`/`relay`/`player` 属执行与回放（传输性失败 `exit(1)` 交监督）——分类落**一处数据**，框架通用约定 1 引该表，各册不再各写各的例外。与 interact I-13 同一张表，谁先做谁负责 | 批I（表）+ 契约评审（范围含 player 与 D8） | C17 |
| A-7 | `relay/state` 补变化判定：`rr3_relay_apply` 已有 `(生效, 前一)` 形状可判，只在 `Some(on)` 且电平真变时续 `relay_seq` 并发布；同时补同态抑制用例。**注意守恒**：`tools/sim/chain.rs` 与 legacy 面板按"每命令一帧"数过这条流，改语义要同批改它们的期望，不能只改件 | 批I | — |
| A-13 | **关节成为对象**（[ADR-0021](../process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md) 裁定 5）：新立 `Rr3Actuator`（`rr3_profile`/`rr3_enable`/`rr3_goto`/`rr3_position`/`rr3_store_profile`）+ `Rr3Position{deg, source}`（`Commanded`/`DutyRegister`/`Encoder` 三态，把"回读不是真实角度"写进类型）+ **执行件档案**为机械行程唯一真值点。今日三处真值互斥、且其中两处的注释**错声称**板侧是 pan[70,110]（实为板侧 `Axis::range()` 的 [60,120]）：servo schema 的线格式域 0–180 / `config.toml` 的 `angle_limit_deg = [70, 110]` / 板侧 `PAN_LIMIT_DEG`+`TILT_LIMIT_DEG` 派生集。后果是可执行集合真的不同（pan=115 在模块面被拒、在板侧帧面受理）。收敛后判据 = 增补轮六门 g"行程字面量读者数 1" | **增补轮六 批N3** | 批N1；`Rr3Store` 窄缝（rtos 档档案落 NVS） |
| A-14 | **标定与复位**（用户点名"电机怎么复位标定"）：定性 = 开环位置舵机**标不出角度**，只能标**脉宽域与机械窗的关系**（中点、方向符号、上下软挡块脉宽、到位是否释放激磁）。四路线 A 工装 / B 电流找挡块（依赖 P-9 的逐域电流，`hard_stop = unknown` 时**代码级禁用**）/ C 开环自恢复 + 软挡块（建议默认，待 D13 裁）/ D 换闭环执行器（新格新 trait，本版不做）。复位三义分开写：**归中**（需已标定）/ **归安全姿态**（形态默认，进 profile）/ **失能归位**（`[pins].failsafe = release`，复位循环里不再反复堵转）。**未标定轴不得进自动模式**（只许点动且带 deadline）；标定动作本身是受控堵转，必须限流限时限次数限单步、失败回退上一份档案——否则标定流程变成第二次烧舵机的路径 | **增补轮六 批N3**（手册页同批改 [`真件装配与标定手册`](../process/dev/真件装配与标定手册.md)） | A-13；D13 |
| A-15 | **`RR3_POLL_WAIT` 是四份不是"一份"**：实测 act/connect/interact/power 四份 `composite.rs` **各有一份同名常量**（而 sense 根本没有它，所以"五册同批处理"那句话事实即错）。本单负责①一处定义、其余引它（消灭四真值点）②收帧等待进 ops 槽（策略出件）。附带：`atoms/led/mech.rs` 的 `.max(1)` 钳位方向与 `reflex` 的 `1_000_000` µs 换算——前者是收敛取向（策略，须带留痕，与 P-2 同纪律）、后者是机制（该有断言）。**细则与全仓同类项清单见 [增补轮六全案](../process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §15.1 M-1/M-9** | **0.0.8 W-B**（形状与归属清理）+ 面板可见随批N4 | 全案 §8 门 k（"同批"句式须带单号）建成 |
| A-16 | **"超时即停"是文档事实，不是代码事实**（2026-10-15 全代码复盘核实）：`config.toml` 注"超时即停，不给堵转留机会"、`rr3_handle_servo` 的 println 称"实机按时限走反射环闭环"，但实测 `move_timeout_ms` 的唯一功能读者是 `atoms/servo/mech.rs::rr3_servo_check_timeout`（**入参裁决**：缺省取上限、超限拒收），随后只被回显进应答 `move_ms`；`src/reflex.rs` 环体只有 `sleep(period)` 节拍——**没有到时停轴路径**。本单两件事：① 即刻把注释与 println 降级为"入参时限裁决"（不许再称保护已生效）；② 真时限保护随 `Rr3Actuator::rr3_goto(deg, deadline_ms)`（A-13）落地，判据是一条**执行面用例**：注入超时 ⇒ 断言停轴/释放激磁。另：意图越权（`open_topics` 外的令）今天只 println，**无 counters 留痕**，与本单同批补计数。**通用化后的判据见 [ADR-0022](../process/adr/ADR-0022-代码事实复盘-机制核纯净度与申报面逃逸与安全语义身体.md) 裁定 5 与 architecture §13 拟门 m"保护性语义须有动作读者"" | ① **批N1**（纯文案，即刻）② **批N3**（随 A-13）| A-13；门 m |
| A-17 | **"单真值点"这把门本身有两处判不准**（2026-10-15 证伪审查，全案 §18 F-23/F-25，索引 **L-23/L-24**）：① 门 ⑨-e 用 `grep` 数标识符读者，可**行程的第二读者是以数据形式存在的**——实读 `atoms/servo/assemble.toml:16-17`，`angle_min_deg` 与 `angle_max_deg` 两个 `ops` 槽的 `src_key` **同引 `rr3-act.servo.angle_limit_deg`**；档案落地后若只删码内字面量，config 那份仍被 `src_key` 引着 ⇒ **两处真值、门照样绿**。② ADR-0022 裁定 5 通用化后的判据是一张**动词表**（"即停/兜底/关断/保护/拒动"），换个词就绕开——本单元现成例子：`[pins].failsafe` 的语义注写的是"**避免**复位循环里反复堵转"，无一动词命中，却与 `move_timeout_ms` 是同一类保护宣称。本单三件事：① 门 ⑨-e 的**读者集合定义**扩为"码内标识符 ∪ `ops[].src_key` ∪ `[pins]`/`domains` 引用位 ∪ 生成器输入"，判据改写为"该键在**档案之外**的读者数为 0"；② 本单元每个 config/档案键标**读者三态** `{裁决｜动作｜仅回显}`（已知两处"只回显"候选：`settle_ms`、`warmup_ms`——登记为"只回显"不丢人，写了保护语义却没动作侧才丢人）；③ `failsafe` 归入该标注执法对象，并在产线自检加一条手感项"复位后两轴应可自由转动"（全案 §17 逐腿表，批N5） | ① **批N1**（标注面与读者集合定义）② **批N3**（动作侧随 A-13/A-14）③ **批N5**（自检项） | 门 e；门 m；A-16；ADR-0022 修订 1；全案 §18 F-23/F-25/F-17 |
| A-18 | **复位后的执行面默认值必须取保守侧，而且要有用例**（2026-10-15 第五轮读码核实，全案 §19.2 Y-3，索引 **L-26**）。实测 `base/runtime/rr-mech/src/control.rs:99-125`：`Rr3LowGate { blocked: bool }` 是**纯内存**，`rr3_low_gate_new()` → `blocked: false`，**没有任何持久化**；`tools/esp32-periph-base/src/state.rs:81-86` 的 `PeriphState::boot()` 把档位打回 `Ok`。而本周 BOD 复位循环的形状正是"**欠压 → 复位 → 闸门开 → 再推舵机**"——**闸门在最能伤害它的那一刻自动解除**。设计侧 §3.1 只写了"回阈 + 迟滞解禁、不自动重放"，从没写"复位后该是什么"，所以这不是实现违例而是**语义空洞**；同理 §6 的 `calibrated=false 不得进自动` 在 rtos 档依赖一个不存在的载体（见 sense **S-14**）。本单三件事：① 把 §6 **复位三义扩为四义**（第 ④ 义：复位后的闸门与标定态），规定**复位后默认不开闸**，须先取到一次合法轨压样本（`age_ms` 在 `max_age_ms` 限内，见 power **P-14**）才进入 `trip/release` 判定；② 一条**执行面用例**：注入"复位后首拍即令动"⇒ 断言被拒并点名"闸门未武装 / 无合法样本"，而不是断言回显了什么（范式 = A-16 第②件）；③ 文档措辞订正——"回阈不自动重放"在代码上的真身是"**没有重放实现**"（闸门不持队列），**不许把它当成"禁止重放已被执法"引用** | **批N2**（①与 `age_ms` 同批）；②随**批N3** `Rr3Actuator`；③即刻（纯文案） | A-13；A-16；A-17；P-14；S-14；全案 §6/§19.2；索引 L-26 |

## §11 待证与敞口

1. 真舵机/真继电器动作证据未取（外设未采购）；sysfs PWM 与 chardev GPIO 只有代码 + 交叉编译证据。
2. 1 kHz 环的实时性从未在目标上量过——"1000 Hz"目前只是 config 里的一个数。
3. LED 写失败不退出的例外是否推广到其它妆面件（如 face），待定规则。

## §12 跨档动作（host / rtos / bare 逐件）

发布权判定：`servo` 发 `servo/state`(stream)、`relay` 发 `relay/state`(event)、`reflex` 发 `motor/reflex` ——
三者在**自己发主题**的档里才需要载荷面；bare 自足形态只跑本地反射环与安全核，**不发主题**（ADR-0018 修订 2 ①），
那里的件面不需要序列化。控制环与 `⊙` 永不同路（ADR-0007 裁定 1），所以反射环天生跨档，载荷面反而不跨档。

| 件 | host（今天） | rtos（esp-rs std + FreeRTOS） | bare（no_std 自足） | 卡住它的**具体那一行依赖** → 动作 |
|---|---|---|---|---|
| servo | ✅（`rr3_servo_*` + `Rr3RateGate` 已复用 `rr-core::control`） | ⚠️ 不能 | ⚠️ 不能 | ① `rr3_servo_parse_args(&Value, …) -> Result<_, String>`：`Value` 是 serde 类型、`String` 是 alloc 错型 → 改 `Rr3ServoCmd { angle_deg: u16, move_ms: u64 }` + `Rr3ServoErr` 枚举（错型面与 `Rr3HalError` 同族）；② 舵机句柄今天从 `rr3_hal_servo()` 拿 `Box<dyn>`（在 `composite.rs`），件签名里的 `Rr3PwmServo` 是 `#[async_trait]` + `Send + Sync`，落角经 `rr3_block_on` → 批I F1 换 `rr-hal-api` 同步 trait；③ 载荷 `rr3_servo_state_body -> Value` 上移承载层（A-8） |
| led | ✅（但**件内自持 worker 线程 + mpsc**） | ⚠️ 不能按现形状（`std::sync::mpsc` + `thread::Builder::spawn`；FreeRTOS 下要换队列） | ❌ 最重的一件 | ① 去线程：`rr3_led_tick(now_ms, state) -> Option<bool>`，节拍归组合件（A-1，且**先**把三条真等用例改合成时钟——R-4）；② `Arc<dyn Rr3GpioOut>` → bare 档禁 trait 对象（ADR-0018 修订 2 ②）→ 泛型 `<G: Rr3GpioOut>` 或由承载层持有；③ `recv_timeout(Duration::from_secs(3600))` 的"长眠"语义消失后，非 blink 态必须显式表示（件内 `mode/level/next_toggle_ms` 三字段），不许用超时兜；④ `rr3_block_on` 清零 |
| relay | ✅ | ⚠️ 只差 HAL 形状 | ⚠️ 只差 HAL 形状 + dyn | ① `rr3_relay_apply(&dyn Rr3GpioOut, on)` → bare 禁 `dyn` → 泛型；② 同样去 `rr3_block_on`；③ `&Value` 解析 → `bool`/结构体；④ `rr3_relay_counter_key(on) -> "relay_on"/"relay_off"` 是**运维词汇**，留在件面但进 §13 豁免清单（counters 面按这两个字符串聚合） |
| reflex（组合件成分，非件） | ✅ | ✅ 纯 `rr-core::control` + 注入 `now_ms` | ✅ 同左（这是全项目**唯一今天就能 bare 跑的回路**，也是"微型形态仍有限位/饱和守护"的依据） | 无跨档障碍；只两条小动作：`loop_hz` 的 `as u32` 裸截断 → `try_from` + 拒 boot（A-3）、`1_000_000` µs 换算常数进 `rr-core::runtime` 的时基面 |

**动作序**：批I F1（`rr-hal-api`）→ A-4（servo/relay 同步句柄）→ A-1（led 去线程，先建合成时钟护栏）→ A-8（载荷上移，新工单）。
其中 A-8 覆盖全仓 11 件，是"件级 no_std"的真正总开关：**它做完，`serde_json` 从件面消失，bare 件的闭包降到 `core`**。

## §13 策略字面量清单

| 文件 : 符号 | 字面量 | 语义 | 读者 | 去处 |
|---|---|---|---|---|
| `src/composite.rs : rr3_act_run` | `rr3_hal_gpio_out("led")` / `("relay")` | **GPIO role token**（白名单在 `rr-hal/backend.rs` 的 `match role`，白名单外 `None` → fail-closed） | 有 | role 词留在件/组合件是对的（它是格位语义），但 `gpio_led_line`/`gpio_relay_line` 的**引脚**必须在描述符 `[hardware]`（批D④ 已落，此处只登记不重做） |
| `src/intent.rs : rr3_intent_action_handle` 的开放门 | 执行臂 `match` 只落 `relay/set` + `servo/command`；已归属但不在开放集者走 `"已归属但意图执行面未开放 {topic}（当前开放集 {}）"` | **意图开放门 = 纯业务策略**（led 故意不开放） | 有，且被单测逐字断言 | **0.0.5 批M MC-10 A-9 已落**：开放集 = config 键 `[rr3-act.intent] open_topics`（组合件读入透传），换形态删一条即改行为、裁剪=改数据（⑧-c 只扫 atoms 故不进 assemble.toml） |
| `src/intent.rs : EXPOSED` | 内联 `"rr3-act/led/effect"` 字面量（同文件另有两枚引常量） | 归属门白名单 | 有 | **双写即债**：改引 `led::RR3_LED_EFFECT_TOPIC` 单点（A-10） |
| `src/composite.rs : RR3_POLL_WAIT` | `250ms` | 收帧空转代价 | **无键无读者**（代码常量） | ops/config 键（与 sense/interact/power/connect 的 `POLL_WAIT` 同批处理，避免五册各说各话） |
| `src/reflex.rs : rr3_reflex_loop` | `1_000_000`（µs/s）、`cfg.loop_hz.max(1)`、`"saturate"`、topic `"rr3-act/motor/reflex"`、线程名 `"rr3-act-reflex"` | 环换算 / reason 词 / 主题 | `loop_hz` 有 config；`"saturate"` 是对外词汇 | `1_000_000` 与换算归 `rr-core::runtime` 时基面；**`"saturate"` = 契约字面量—不许动**（订阅方与面板按它分派）；线程名留 host 档 |
| `atoms/led/led.rs : Rr3LedEffect::{parse,as_str}` | `"on"/"off"/"blink"` | 灯效白名单 | 有；值集常量 `RR3_LED_EFFECT_VALUES` 单点，`atom.toml` 只在**注释**里提，不是机器字段 | **0.0.5 批M MC-10 A-9 已落**：值集 = 契约字面量（不许改）；可注入**子集**落 `atoms/led/assemble.toml` ops 槽 `led_effects ← rr3-act.led.effects`，组合件解析为 `Vec<Rr3LedEffect>` 透传件面白名单判定（空集＝一条不认） |
| `atoms/led/mech.rs` | `(blink_period_ms / 2).max(1)`、初始 `current = Off` / `level = false`、`"灯效 worker 已退出"` | 半周期推导 / 灭态缺省 | 有 | 半周期推导是机制（留件）；`.max(1)` 是策略钳位 → ops 校验；`Off` 缺省留件（执行器安全态，属安全核同族，**不许注入**） |
| `atoms/servo/mech.rs : rr3_servo_parse_args` | `"缺 angle_deg（int，行程内角度）"`、`"angle_deg 必须是非负整数"`、`"超出行程 [{}, {}]"`、`"move_ms 必须大于 0"`、`"超出时限上限 {}（不给堵转留机会）"`；缺 `move_ms` → 回落 `cfg.move_timeout_ms` | 裁决文案 + 缺省时限 | 文案被单测逐字断言 | **契约字面量—不许动**（面板与 legacy 依赖字面）；时限回落来源 config ✅ 合规 |
| `src/composite.rs : rr3_act_config_parse` | `.max(1)`（`blink_period_ms`、`loop_hz`）、`range.len() != 2`、`u16::try_from`、缺键即 `Err("config [rr3-act.{sect}] 缺 {key}")` | 校验与钳位 | 有 | 校验规则进 config schema（批J G4 的 ops 校验位）；**缺键硬报错 = fail-closed，正面样板**，与本仓"静默回落 sim"对照 |
| `src/composite.rs` / `module.toml` | `RR3_INTENT_TOPIC="rr3-brain/intent/result"`（与 `[events].subscribes` 双写）、`RR3_ACT_CONFIG`、`RR3_MANIFEST` | 订阅面/env | 有 | 主题名 = 契约字面量；`RR3_MANIFEST` 未设 → `exit(1)` fail-closed ✅ |
| `config.toml [rr3-act] log_level` | `"info"` | 日志级 | **无读者** | 被读或删除（P-6/N-6/I-1/B-8 同纪律，A-11） |
| `atoms/{servo,led,relay}/assemble.toml` | `handles` / `ops` 申报 | 注入面申报 | 有 | **L-5 已填**句柄与既有 config ops；**A-9 已补** led 的 `led_effects` ops 槽。`intent_open_topics` 因组合件层无 ops 校验位（⑧-c 只扫 atoms），落 config 键 `[rr3-act.intent] open_topics` 由组合件读——**不进 assemble.toml** |

## §14 基线对表（代码 `22783ea`，2026-10-10；本节是本册唯一"现在时"）

| 本册旧断言 | 现在的事实（符号为证） | 判定 |
|---|---|---|
| §1/§12 "`Rr3Led` 自持 worker 线程 + `mpsc` + `recv_timeout(3600s)`，A-1 待拆"；R-4"先建合成时钟再拆" | 线程与 mpsc **已删**：件面为 `rr3_led_new(u64)` + `rr3_led_apply(&mut self, u64, Rr3LedEffect)` + **`rr3_led_tick(now_ms: u64, &mut Rr3Led) -> Option<bool>`**，节拍由 `src/composite.rs` 每圈驱动 → 时钟天然为注入值 | **A-1、R-4 已结**（合成时钟不再是前置，而是既有形状） |
| §5 "`relay/state` 实况每令必发，与申报的变化即发不符（A-7）" | 件面新增 **`rr3_relay_state_due(bool, bool) -> bool`**，变化判定进机制、发布受其门控 | A-7 已结（`relay.rs`/`mech.rs` 在册；同态不续发 e2e 断言已补——intent 路 ⑤b（批I d21839a）+ RPC 路 ④b（批M M-β 3bc30f1），破坏性验证证牙齿） |
| §1 目标签名"载荷上移承载层（A-8）" | `rr3_servo_state_body`、`rr3_relay_result`、`rr3_led_parse_args`、`rr3_relay_parse_args` 全在 `src/composite.rs`；件面 `serde_json` 零命中 | **A-8 已结** |
| §1 "`use rr_hal::` → `rr_hal_api`（A-4）" | 契约面已同步化；件内 import 源为 `rr_hal_api`（经 re-export 保路径），`rr3_block_on` 不存在 | **A-4 已结** |
| §12 bare 档"`Arc<dyn Rr3GpioOut>` 需改泛型（禁 trait 对象）" | `led` 已无 `Arc`；**`rr3_relay_apply(&mut dyn Rr3GpioOut, bool)` 与 `rr3_tts_play(&mut dyn …)` 仍是 `&mut dyn`** | **未结** → 批L **L-3** |
| §1 错型面 | `rr3_servo_check_travel` / `rr3_servo_check_timeout` / `rr3_relay_apply` 返回 `Result<_, String>` | **未结** → 批L **L-3**（`Rr3HalError`/件枚举错型收敛） |
| §13 "`intent.rs : EXPOSED` 内联 led 主题串（A-10 双写）" | `const EXPOSED: [&str; 3]` 与 `"已归属但意图执行面未开放 …"` 仍在，led 通路仍封闭（注释写明"扩面随真实需要 append"） | **未结** → 批L **L-7**（串值单点），封闭本身按 R-3 不变 |
| §13 策略申报位 | `atoms/{servo,led,relay}/assemble.toml` **已填**（批M MC-10 L-5，2026-10-13）：servo 报 `Rr3PwmServo`←`pwm_chip`/`pwm_channel` 句柄 + `angle_min_deg`/`angle_max_deg`/`move_timeout_ms`/`state_min_interval_ms` ops；led 报 `Rr3GpioOut`←`gpio_chip`/`gpio_led_line` + `blink_period_ms`；relay 报 `Rr3GpioOut`←`gpio_relay_line` | **L-5 已结**（白名单 ops 续做见 A-9） |
| §11/框架通用约定 1 "妆面件失败语义例外" | 代码未变（led 写失败仍 `eprintln` 不退） | **A-12 未结**（D8 范围仍待你定） |

**本册新净余数**：批L **L-3**（`&mut dyn` + `String` 错型）。**L-5 已结（批M MC-10 L-5）**、**A-9 已结（批M MC-10 A-9，开放门/白名单数据化）**、**L-7 已结**（`intent.rs` 现引 `RR3_*_TOPIC` 三常量）。**A-2/A-3** 由 `rr3_led_tick(now_ms)` 与 ⑧-b 门一起收口。
**L-6 已随批M M-β 摘账**（2026-10-12：RPC 路同态断言落 e2e_act ④b，intent 路 ⑤b 原有）。

### §14.1 增量对表（2026-10-12，代码基线 `78625ae`；全仓总表见 [索引 §13.4](模块设计索引.md)）

| 项 | 复验实况（符号） | 判定 / 新落点 |
|---|---|---|
| A-2 时基改 `Rr3TimeBase` | `rr-bus/src/client.rs` 已立 `Rr3TimeBase`（`anchor: Instant::now()`），注释自陈"`SystemTime` 自称单调是误称，本型纠正" | **结**（旧正文不回改） |
| A-3 `loop_hz` 越界拒 boot | `rr3-act/src/composite.rs` 的 `u32::try_from(loop_hz_raw)` + 越界回"拒 boot"文案（注释点名 A-3） | **结** |
| A-10 / 批L L-7 内联主题串 | `intent.rs : EXPOSED` 现引 `RR3_SERVO_COMMAND_TOPIC`/`RR3_LED_EFFECT_TOPIC`/`RR3_RELAY_SET_TOPIC` 三常量 | **结**（从批L 与 MC-11 同时摘账） |
| A-5 低电禁动 | 语义由索引 **R-15** 结案（四条），原挂"待裁"栏 | **已结（批M MC-5 `071dcad`+`8b5b57e`，2026-10-13）**——`Rr3LowGate` 落 `rr3-act/src/composite.rs` 接 `rr3-runtime/system/power`，执行面 RPC/intent 两路拒动、妆面不禁（原裁定"施工落 0.0.5 MC-5；与 power P-8 / 批L L-8 合并成一个动作"已兑现） |
| A-6 环 tick 计入 counters | `src/reflex.rs` 立常量 `RR3_REFLEX_TICKS_KEY`（字面量唯一落点），速率窗结算点 bump（值 = 窗内实跑节拍数），README 计数归属表加行 | **已结（批M M-β 3bc30f1，2026-10-12）**：e2e_act ⑧ 经 `rr3.ctl.counters` 有界轮询点验 `rr3-act/reflex_ticks` 非零（先红后绿） |
| A-9 开放门与灯效白名单数据化 | 开放门改读 config `[rr3-act.intent] open_topics`（`intent.rs` 去件内硬编码开放判定，执行臂 `match` 与兜底臂保留）；灯效白名单落 `atoms/led/assemble.toml` ops 槽 `led_effects ← rr3-act.led.effects`（`led.rs` 出值集常量 `RR3_LED_EFFECT_VALUES` + `rr3_led_effects_join`，组合件透传子集）；两键均 fail-closed（缺键/越界值拒 boot）、空数组合法 | **已结（批M MC-10 A-9，2026-10-13）** |
| F-1 舵机承载路径 | R31 **Q2** 已裁舵机挂 ESP32 协处理器（LEDC）；Pi 侧 `linux_pwm.rs` 保留 | 施工落**增补轮五批B/批C**；`linux_pwm.rs` 不注销，降为无协处理器形态的兜底路径（本册 §12 跨档表按两条路径读）。**2026-10-14 定性订正**："协处理器"随 [ADR-0021](../process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md) 裁定 1 作废——ESP32 与树莓派是两台独立机器，所以这不是"主路径 + 兜底路径"，而是**两条平行形态各自的路径**；本行其余判据不变（行程与标定自批N3 起读执行件档案，见 **A-13**） |
