# rr3-act 模块设计（框架 v1）

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程（host）；`start_priority = 40`、`mem_limit_mb = 48`、`restart_policy = "always"` | `module.toml` |
| 格 | `act.servo` · `act.led` · `act.relay`（全 body 层） | `contracts/slots.toml` |
| 件 | `servo` · `led` · `relay`；非件成分 `intent.rs`（接入面）、`reflex.rs`（守护面宿主） | `atoms/`、`src/` |
| 主题 | RPC `servo/command`、`led/effect`、`relay/set`；发布 `servo/state`(stream)、`relay/state`(event)、`motor/reflex`(notification)；订阅 `rr3-brain/intent/result` | `module.toml`、各 `atom.toml` |
| 句柄 | `rr3_hal_servo()` → `dyn Rr3PwmServo`；`rr3_hal_gpio_out(role)`（role 白名单 `led`/`relay`）→ `dyn Rr3GpioOut` | `src/composite.rs`、`atoms/{led,relay}/mech.rs` |
| 参数 | `angle_limit_deg=[0,180]`、`move_timeout_ms=800`、`state_min_interval_ms=50`、`blink_period_ms=500`、`loop_hz=1000`、`saturate_notice_ms=500` | `config.toml`（`RR3_ACT_CONFIG`） |
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
- 非法输入收敛方向：角度越界 → **钳到端点**（不是拒绝），因为执行器已在动；参数缺/错 → 拒绝且不落电平。

## §3 注入面

| 件 | `handles` | `ops` 槽（名 / 类型 / 来源 / 默认） | 非法收敛 |
|---|---|---|---|
| servo | `dyn Rr3PwmServo` ← `[hardware].pwm_chip`、`pwm_channel` | `angle_min_deg`/`angle_max_deg` ← `angle_limit_deg`；`move_timeout_ms`；`state_min_interval_ms` | 端点颠倒 → 取小者为下界并钳到最保守端（`rr3_travel_clamp` 用例已钉） |
| led | `dyn Rr3GpioOut(role="led")` ← `gpio_chip`、`gpio_led_line` | `blink_period_ms`（默认 500，代码再 `.max(1)`） | 写失败 → `eprintln` 不退（妆面件，**例外需在此点名**） |
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
| `servo/command` | RPC | — | 点名 | `{angle_deg, timeout_ms?}` | 越界→钳位后成功；缺/错→error 点名 |
| `servo/state` | stream | 自增 | 变化 + 50 ms 限速 | `seq, angle_deg` | — |
| `led/effect` | RPC | — | 点名 | `{effect}` 白名单 | 不认点名白名单 |
| `relay/set` | RPC | — | 点名 | `{on}` | 写失败 error |
| `relay/state` | event | 自增 | **变化即发**（不做限速） | `seq, on` | — |
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
低电兜底不在本单元（在 power），但"低电时禁动"这条**尚未定义**——列入工单 A-5。

## §8 两腿对拍用例设计

| 件 | 输入序列 | 期望值来源 | 边界集 | 现状 |
|---|---|---|---|---|
| servo 参数裁决 | `angle ∈ {0, 45, 180, 200, -?}` + cfg(45,135) | 裁定语义（钳位、端点颠倒收敛） | 端点内/外/等号/颠倒 | ✅ rr-core `travel_clamps_into_range_and_survives_inverted_range` + 件面 parse 用例 |
| servo 限速门 | now = 1000 + 10k，k∈0..24 | 裁定语义"每 5 拍放 1 次" | 窗起 / 窗末等号 / 迟到拍 | ✅ 机制核 `same_control_graph_beat_for_beat_identical`（件面需引它做期望值） |
| led | 效果序列 `[On, Blink, Off, Blink]` + 半周期序列 | **期望 = 翻转序列白名单**（不得由件自己产出） | 半周期=1、worker 已死 | ✅ **已有三条**：`blink_toggles_level_and_off_stops_it`（翻转序列）、`on_and_off_take_effect_immediately`（立即落电平）、`apply_tracks_current_and_previous`（前后态）。⚠️ 缺的不是用例而是**合成时钟**——这三条用 `rr3_wait_until` 真等半周期，节拍来自 worker 线程自睡；A-1 去线程后期望序列必须由注入的 `now_ms` 产出，否则回归面在改线程模型时无人守护 |
| relay | `on` 序列 `[true,false,false,true]` | 载荷 + `relay_seq` 连贯（e2e 断言） | 重复同态不发 | ⚠️ 需补"同态不发"边界用例 |
| reflex | tick 密度 `[250,125,124,0]`/窗 | 半拍线判定 + episode 恰一次 | 恰半拍 / 低于半拍 / 孤立饱和窗 | ✅ 机制核三条；件面需补"环饿死 → 通告仍发得出"一条 |

## §9 不变量与红线断言

- 断言：任何进入 `Rr3PwmServo::set_angle` 的角度都满足 `min ≤ a ≤ max`（钳位后）。
- 断言：`servo/state` 两次发布间隔 ≥ `state_min_interval_ms`；`relay/state` 不受限速门管辖。
- 断言：`relay_seq` 跨 RPC/intent 两路连贯（已有 e2e）。
- 断言：反射环永不发总线帧、永不经 `⊙`（ADR-0007 裁定 1）。
- 断言：越权 intent 动作**只拒执行**，不断链、不改状态。
- 断言：三 RPC 名/参数裁决文案（点名缺哪个字段）零改动——面板与 legacy 依赖字面。

## §10 改造工单

| # | 动作 | 归属 | 前置 |
|---|---|---|---|
| A-1 | led 去线程/去 mpsc → `rr3_led_tick(now_ms, state)`，节拍归组合件；补翻转序列用例 | 批I | C7 |
| A-2 | 时基改 `Rr3TimeBase`（含 `rr-bus` 心跳），去掉 `SystemTime` 单调性误称 | 批I | — |
| A-3 | `loop_hz` 用 `try_from<u64> for u32` + 越界拒 boot | 批I | — |
| A-4 | servo/relay 句柄面换 `rr-hal-api` 同步签名（去 `rr3_block_on`） | 批I | C5 |
| A-5 | "低电禁动"语义定义（power 边沿 → act 闸门，落点在机制核安全核段） | 待裁（M6 关联） | — |
| A-6 | 环 tick 计入 counters（健康面可观测"活着但不动"） | 批K | C9 |

## §11 待证与敞口

1. 真舵机/真继电器动作证据未取（外设未采购）；sysfs PWM 与 chardev GPIO 只有代码 + 交叉编译证据。
2. 1 kHz 环的实时性从未在目标上量过——"1000 Hz"目前只是 config 里的一个数。
3. LED 写失败不退出的例外是否推广到其它妆面件（如 face），待定规则。
