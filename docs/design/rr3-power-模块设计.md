# rr3-power 模块设计（框架 v1.3）

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程（host 档）；`[resources].start_priority = 10`（全系统最早起）、`mem_limit_mb = 16`、`restart_policy = "always"` | `module.toml` |
| 格 | `power.battery`（base 层，恒在位）· `power.charger`（body 层） | `contracts/slots.toml`、`atoms/*/atom.toml` |
| 件 | `battery` · `charger`（各五件套） | `atoms/` |
| 主题 | 发布 `battery/state`、`battery/low`、`charger/state`；**订阅面为空**；**无 RPC** | `module.toml [events]/[rpc]` |
| 句柄 | `rr3_hal_power()` → `dyn Rr3Power`（`battery()`） | `src/composite.rs` |
| 参数 | `sample_period_ms=5000`、`state_period_ms=2000`、`low_voltage_mv=3300`、`cutoff_voltage_mv=3150`；**另有 `[rr3-power] log_level` 一项，解析函数既不读也不列**（孤儿键，§10 P-6） | `config.toml`，经 `RR3_POWER_CONFIG` 读 |
| 健康 | 心跳 2000 / 看门狗 10000 ms | `module.toml [health]` |

## §1 件契约表

| 件 | 公开符号（现状签名） | 输入 → 输出 | 状态 | 失败语义 |
|---|---|---|---|---|
| battery | `rr3_battery_state_body(seq: u64, st: Rr3BatteryState) -> Value` | 序号 + 读数 → state 载荷（`seq/voltage_mv/percent/charging`） | 无 | 不可能失败 |
| battery | `rr3_battery_low_body(seq: u64, st: Rr3BatteryState) -> Value` | 同上 → low 载荷（`seq/percent/voltage_mv` 三必填，`additionalProperties=false`） | 无 | 不可能失败 |
| battery | `pub use rr_core::control::Rr3LowEdge`（真值在机制核） | `rr3_low_edge_cross(voltage_mv: u16) -> bool` | `armed: bool` + 阈值 | — |
| battery | **目标** `→ fn(state: &Rr3BatteryState) -> Vec<u8>`（走 `rr3_body_encode`） | 去 `serde_json` | — | — |
| charger | `rr3_charger_state_body(seq: u64, charging: bool) -> Value` | 充电位 → 载荷 | 无 | 不可能失败 |
| 组合件 | `rr3_power_run()`（`src/composite.rs`） | manifest env → 主循环 | 两路到期时刻 | 见 §6 |

## §2 状态机（`Rr3LowEdge`，真值 `rr-core::control`）

不变式：**一次低电段恰报一次**；`voltage_mv <= low_voltage_mv` 算越阈（等号在内）；
**严格大于**阈值才重新武装；**武装本身不是事件**（`cross()` 返回 true 的唯一含义是"该发 battery/low"）。
非法输入收敛方向：读数不合法（`Unsupported`）→ 不喂边沿、不报低电（宁漏不误报）。

| 当前 armed | 读数 | 返回 | 次态 |
|---|---|---|---|
| true | `<= low` | **true**（发事件） | false |
| false | `<= low` | false | false |
| any | `> low` | false | true |

## §3 注入面

| 件 | `handles` | `ops` 槽 | 来源 | 默认 | 非法收敛 |
|---|---|---|---|---|---|
| battery | **现状**：`dyn Rr3Power` ← `RR3_HAL_BACKEND` 选档 + `[hardware].i2c_bus`、`ina219_addr`（描述符今天**没有** `[slots].present`，profiles 实有段只有 `[os] [bindings] [hardware] [enabled_slots]`）；**目标** `→` 由 `[slots].present:power.battery` 声明在位（随批J 七段落，见索引 D1/C2） | `low_voltage_mv: u16` | config | 3300 | 越界 → 拒 boot（fail-closed） |
| battery | — | `sample_period_ms: u64` | config | 5000 | `0` → **已钳到 1**（`composite.rs` 的 `u64_of(..)?.max(1)`），缺的是"钳制留痕"一行日志（§10 P-2 收窄为只补留痕） |
| charger | **现状：无独立句柄**，充电位由最近一次 battery 读数派生 | `state_period_ms: u64` | config | 2000 | 同上 |
| 全体 | `cutoff_voltage_mv: u16` = 3150 **只解析回显、不动作**（关机归 M6） | — | config | — | — |

## §4 主循环与时序预算

真实常量（`src/composite.rs`）：`RR3_POLL_WAIT = 100 ms`；`next_battery` / `next_charger` **初值 = now
（首拍即采，兜底链路不等第一个周期）**；到期后 `next = now + period`（相对节拍，与 `rr-core::runtime` 同纪律）。

| 输出 | 名义周期 | 最坏延迟路径（低电事件从越阈到总线可见） | 与心跳关系 |
|---|---|---|---|
| `battery/state` | 5000 ms | 采样点错位 + 1 拍 | 独立 |
| `battery/low` | 边沿触发 | **≤ 采样周期 5000 ms + 一次 publish**（须实测，非推算） | 心跳 2000 ms 不同线程 |
| `charger/state` | 2000 ms | **播种自开机探活**（`last = Some(first)`）→ 到期即发，"首拍不发"这条旧说法不成立；最坏 = 周期 + 一拍 | 独立 |
饿死可见性：主循环若卡在 `battery()`（ioctl/i2c 阻塞），心跳线程仍续命 → **"活着但不报低电"漏检**。
设计要求：把"最近一次成功采样时刻"计入 counters，由 rrd 侧做**数据新鲜度**判据（不新增相位）。

## §5 主题面契约

| 主题 | kind | seq 规则 | 触发 | 必填 | 失败映射 |
|---|---|---|---|---|---|
| `battery/state` | stream | 本流自增（从 1） | 每采样拍 | 三字段 + seq | 句柄错 → 不发布并记 counters |
| `battery/low` | event | 与本流连贯 | 边沿（§2） | `seq/percent/voltage_mv` | 同上 |
| `charger/state` | event | 自增 | 周期，**派生位** | seq + charging | 同上 |

## §6 降级矩阵

| 格 | 缺格 | 句柄缺 | 句柄值错 | 后端未实装 | 参数非法 |
|---|---|---|---|---|---|
| `power.battery` | **base 层不允许缺**（恒在位）——无电池板的表达见 §10 工单 P-1 | `rr3_hal_power` 落 placeholder → 开机探活 `Unsupported` → **`exit(1)`** | 描述符键拼错 → 现状整份描述符作废且**静默**（`.ok()` 吞错）→ 首次调用才错 | linux 后端在（INA219）；sim 恒在位 | 周期 0 → 现状允许（§10 工单） |
| `power.charger` | 只停本流，不影响兜底 | 同上不独立存在（派生） | 同 battery | **已有真判定，不是恒 false**：linux 侧 `linux_power.rs` 取 INA219 电流符号（`current_raw < 0` 即充电），sim 侧读 `RR3_HAL_SIM_BATTERY_CHARGING`（缺省 false，可显式置真且载荷带 backend 名）。真件未采购 ⇒ 该判定**从未在真硬件上验过**（§11） | — |

## §7 安全核接入

低电兜底是本单元唯一的守护动作：**边沿判定在机制核**（`Rr3LowEdge`），本单元只提供读数与发布，
**不能**被策略绕过（brain 停机也不影响 battery/low 上报）；**关断动作未实装**（MC-8① 判 (c)，
2026-10-13：`cutoff_voltage_mv` **声明未生效、本版不关断**），
归 M6——设计定：关断落点在机制核安全核段，触发方在本单元，与 act 限位同级。

## §8 两腿对拍用例设计

| 件 | 输入序列 | 期望值来源 | 边界集 | 现状测试 |
|---|---|---|---|---|
| battery（边沿） | `[3900, 3300, 3250, 3200, 3301, 3301, 3200]` | `Rr3LowEdge` 裁定语义（等号算低、严格大于才武装、武装不返 true） | 等号 / 刚过 / 连续低 | ✅ `low_edge_fires_once_per_episode`、`low_edge_rearms_only_above_threshold`（在 rr-core，件面 re-export） |
| battery（载荷） | `st(3300, 8)` 连喂 3 拍，seq 递增 | `contracts/events/power-battery-low.schema.json` 逐字段 + `additionalProperties=false` | seq 起点、字段缺失 | ✅ **low 已有** `low_body_matches_schema_exactly`；⚠️ 缺的是 `battery/state` 那条（今天没有 state schema，也没有载荷用例） |
| charger | `charging` 序列 `[false,false,false,true]`（值由句柄决定，不由件决定） | 载荷两字段形状 + **本流 seq 自增** | 首拍即发（播种自开机探活） | ✅ 已有 `charger_body_carries_own_seq_and_charging`（断言 `seq==3`），旧版"缺 seq 断言"的说法作废 |

## §9 不变量与红线断言

- 断言：一次低电段 `battery/low` 恰发 1 次（用例级）。
- 断言：首拍即采——boot 后第一个 `state_period` 内必见一次 `battery/state`。
- 断言：`charging` **只能来自句柄读数**（sim 的 env 注入或 INA219 电流符号），件与组合件都不得自造恒真/恒假；
  载荷里必须同时能读出"这个位是哪个后端给的"（`wifi/scan` 的 `backend` 标注同纪律）。
  ——本条取代旧版"标定前恒 false"的写法：诚实面的正确形式是**标注来源**，不是把值钉死。
- 断言：本单元**无 RPC、无订阅**——任何新增请求面都破坏"兜底不依赖总线"这一设计前提。
- 断言：三主题名与载荷字段零改动（`contracts/topics.toml` 对表）。

## §10 改造工单

| # | 动作 | 归属 | 前置 |
|---|---|---|---|
| P-1 | `power.battery` 在"无电池板"上的表达：base 层恒在位与之冲突，需一次 ADR 级评审（retired 或改层） | 待裁 | — |
| P-2 | 钳制**已存在**（`u64_of(..)?.max(1)`），本单收窄为：钳制时打一行留痕（哪个键、原值、钳后值）；采样失败计入 counters（数据新鲜度） | 批I | C7 |
| P-3 | 描述符错误在 boot 期可见（`rr-hal` 四选择函数不再 `.ok()` 吞） | 批J | — |
| P-4 | ~~件载荷去 `serde_json`（后续批）~~ **并入 P-7**（同一依据 = ADR-0019 修订 4；两条各挂一批是本轮审查查出的 M-1 口径互斥，现统一由 P-7 承担） | 批I（随 P-7） | — |
| P-5 | **改判**：`charger` 不需要独立句柄——`charging` 已由 `Rr3Power` 读数的电流符号派生（INA219 侧），再立一个句柄就是第二真值点。要标定的不是充电位而是 `percent` 的**电压→容量曲线**（两点或多点），随真件批做 | 真件批（0.0.6） | 采购 |
| P-6 | `config.toml` 的 `[rr3-power] log_level` 是孤儿键（解析函数不读、全仓无读者）：**被读或删除**，不许第三种状态（与 interact I-1 同纪律） | 批I | — |
| P-7 | **载荷组装上移承载层**：`rr3_battery_state_body` / `rr3_battery_low_body` / `rr3_charger_state_body` 的 `json!` 生产移到 `src/composite.rs`，件面只交 `Rr3BatteryState` 与 `bool`（依据 = ADR-0019 修订 4）。**做完本单元两件即 `core`-only**，是"件级 no_std 可达"的第一个样板；验收 = 三主题载荷字节逐字不变（schema 用例继续绿） | 批I | — |
| P-8 | 描述符 `[supplies]` 补**平台看门狗**位（bare 档挂死无主承载层可救，只能靠平台 WDT；四类供给今天不含它，是真缺口不是笔误——见索引 §11 审计 L-2）：本册负责给出"低电边沿在 bare 档的本地兜底动作"需求（与 cutoff 未实装同条链）——**看门狗位已随批J 落（C15/G6）**，本条余下的是**动作半边** | **0.0.5 批M MC-5**（与 act A-5、批L L-8 **三单合一**，索引 R-15 已裁语义；不再本册单独挂单） | MC-5 |
| P-9 | **第七供给位 能源域**（[ADR-0021](../process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md) 裁定 2）：`[[supplies.domain]]` 键集（`name/nominal_mv/current_cap_ma/sense/current_sense/mild_mv/severe_mv/switchable/armed`）+ 新能力面 `Rr3Rail`（逐域电压/电流与档位）+ 两张格 `power.rail`/`power.budget` 与主题 `rr3-power/rail/state`。今日事实：轨压机制与双阈值核**已在板 crate 真跑**（`tools/esp32-periph-base/src/power.rs` 的 `RailMonitor`/`tier_of`/`RailJudge`，复用 `rr_mech::control::Rr3LowEdge`），**但 `rr-hal-api` 与本册对它零命中**——本单是"归属搬家"，机制不动 | **增补轮六 批N2** | 批N1（角色与键集形状）；阈值/分压比**取值**待真板对拍，未对拍前 `armed=false` 不接闸 |
| P-10 | 件的 `[[atom.demand]]` 申报面（`rail`/`peak_ma`/`sustained_ma`/`critical`，**第七轮 §21 T-5 再补一键 `concurrency_group`**：装载校验 ⑨ 的"并发"唯一放松源，此前**只有引用没有住处**；缺省 = 不分组 = 同域全体并发 = 最坏，显式空数组合法且必须显式写，同 `assemble.rs` 的 `no_injection` 范式）+ 装载期电流加总校验（超域上限拒 boot 并点名组合）+ 运行期三档降载（`Mild` 串行与降亮、`Severe` 拒动沿用 MC-5 `Rr3LowGate` 语义：**拒执行面、不禁妆面、回阈不自动重放**）。板侧今天的 `AXIS_SETTLE_MS` 串行整定与 `OLED_CONTRAST_LEVELS` 逐级降亮，是这三层的**未经建模的事后补救版本**——它们是行为，不是数据结构，所以换板学不到 | **增补轮六 批N2** | P-9；逐域电流采样硬件（D10） |
| P-11 | **承接 L-14：器件级探测的需求面**。实况是宿主探针 `rrd/src/probe.rs` 的 `Rr3LinuxProbe::rr3_face_present` 对 I2C **只判 `/dev` 下存在任一 `i2c-` 条目**，从不向从机地址探问——于是 `power.charger` 只要 I2C 总线在就**恒判在位**，INA219 没接也绿，缺格降级环在本单元这一格上不转。本册负责给出需求：`[pins]`/描述符里的 `addr` 必须被"读回特征字节/寄存器 ID 比对"用过一次，读不到即该格 `Degraded` 并点名 `role` + `addr`；**INA219 的特征寄存器（0x00 = 0x2268）是现成的判据源**，与 P-5 的"电流符号派生"共用同一句柄，不另立句柄（防第二真值点）。细则见 [全案](../process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §12.1 | **增补轮六 批N1**（与键集同批：没有 `addr` 就没有可比的东西） | 批N1；与 sense S-11 同一把用例 |
| P-12 | **自述文案与载荷字段无对账（本单元是标本）**：`atoms/battery/battery.rs` 的 `SURFACE` 写作"电压/电量/**温度**摘要"，而 `rr3_battery_state_body` 与 `rr-hal-api::Rr3BatteryState` 都只有 `voltage_mv/percent/charging`——**没有 temperature 字段**。这条不是排版错：`SURFACE` 是对外可见面（会进面板与文档），字段是线上字节，两者今天没有任何机械关系（同族违例见 sense S-12 与索引 L-18）。本单两件事：① 文案改到与 schema 逐字一致，或补字段并走契约 append；② 认下拟增门 l"**自述文案 ↔ `contracts/events/*.schema.json` 字段判等**"作为收敛机制（一次立门，六单元同享）。另记本单元**缺采样龄期与自检项**：读数只有失败计数、无"最后一帧距今多久"的新鲜度面，产线无法判"电池读数是不是旧的"（与 P-2 同族，P-2 已登记、本单补新鲜度） | **批N1**（门 l 随批N1 立，文案同批改）；新鲜度计数随**批N2** | 全案 §16.2 收窄记录；门 l |
| P-13 | **`[hardware]` 物理绑定键与 `[pins]` 是同一件事的两套语法**（2026-10-15 证伪审查核实，全案 §18 F-1/F-2，索引 **L-23**）：现役注入路径是 `件 handles[].desc_key → [hardware] → descriptor.rs → rr-hal/src/backend.rs:327` 取 `b.pwm_chip?`（申报处实读 `modules/rr3-act/atoms/servo/assemble.toml:9-11`），而 ADR-0021 新立的 `[pins]` 是**旁挂第八段** ⇒ 装载期说的是 A 份、运行时用的是 B 份，**而门只判 A 份**。同族第二处：本册 §3.1 那三行域绑定位写作 `sense = "adc:gp34"` / `current_sense = "i2c:ina219@0x41"` / `switch = "gpio:en_t1"`——**同格注记自己写着"角色名，非裸引脚"，而 `gp34` 就是裸引脚名**，且这三处不在 `[pins]` 里，八条装载校验一条都打不到它（双占同一引脚照样绿）。本单三件事：① **六个物理键**（`pwm_chip`/`pwm_channel`/`gpio_chip`/`gpio_led_line`/`gpio_relay_line`/`i2c_bus`+`ina219_addr`）从 `[hardware]` **下移**为 `[pins]` 派生视图，`[hardware]` 只留 `alsa_device`/`link_ifname`；同批必改 `rr-registry` 的 `RR3_ASSEMBLE_DESC_KEYS`、六份 `handles[].desc_key`、`descriptor.rs`、`backend.rs`（**契约面变更，与 `schema_version` 2→3 同批**）；② 域的三个绑定位**改引 `pin_id`**（`pin_id` 等式见全案 §4.2），从而落进校验①③⑤ 的作用域；③ 认下新门 **⑨-n 注入路径唯一**（两处并存即拒 boot；迁移完成后物理键出现在 `[hardware]` 即红）。连带把本册 §3.1 示例里那个不在册的 `rail24` 改用在册域名（全案 §18 F-5：**文档示例值会被装机档抄**） | **批N1**（①②③同批，缺一即副本滞后红） | 全案 §18 F-1/F-2/F-5、§4.1b/§4.2；索引 L-23；门 n；ADR-0021 修订 2 |
| P-14 | **能源侧的运行期语义四条全空**（2026-10-15 第五轮读码核实，全案 §19.2 Y-3/Y-4/Y-6，索引 **L-26**）：① **闸门复位即开闸**——`rr-mech/src/control.rs:99-125` 的 `Rr3LowGate{blocked:false}` 是**纯内存**，`rr3_low_gate_new()` 一律返回开闸，而本周 BOD 复位循环的形状正是"欠压 → 复位 → 开闸 → 再推舵机"；设计里"回阈不自动重放"这句在代码上的真身是"**没有重放实现**"（闸门不持队列），于是它既不是"实现了禁止重放"也没人写过"复位后该是什么"。② **用于判定的读数没有龄期**——`power.rs:87-95` `tier_of` 是纯电平分类（迟滞只在 `Rr3LowEdge` 里，:105-125），档位与轨压是 RAM 快门值（`state.rs:63-64`，`PeriphState::boot()` 重启回 `Ok`），采样停摆时旧值继续被拿去做拒动判据；`rr3_now_ns()` 取不到 epoch 时 **`unwrap_or(0)`**（`rr-bus/client.rs:97`）= 假时间戳。③ **阈值与接闸位没有生效通路**——`RAIL_MILD_MV`/`RAIL_SEVERE_MV`/`RAIL_GATE_ARMED` 是 `pub(crate) const`（`board.rs:204,207,213`），全仓 `SIGHUP`/`reload` 零命中 ⇒ §7 "面板可写阈值"在 rtos 档等于**重烧固件**。④ **`low_power` 只升不降**（`rrd/conn.rs:611` 注释自陈，全仓无 `low_power = false` 写入点）——这条已在册（敞口表），本单认领它的解除路径。本单四件事：`Rr3RailState` 加 `age_ms` + 域加 `max_age_ms`（并入校验⑥-c，**不新增判定位**）；**复位后默认不开闸**，须先取到一次合法样本（`age_ms` 在限内）才进入 `trip/release`；可写量标 `applies`；`low_power` 补降级路径。另把 `rrd` 那个进程判活线程**改称"逻辑 watchdog"**，与平台 WDT 分开（它今天被多处当成"看门狗已具备"引用） | **批N2**（①②随域与 trait 落地）；③随**批N4**（面板写面）；④随**批N4**；称谓订正即刻 | P-2/P-9/P-11；门 c；索引 L-26；全案 §19.2/§19.3 |
| P-15 | **档位没有"读数不可信"这个取值**（2026-10-15 第七轮闭合性审查，全案 §21.2 T-6，索引 **L-28 的近亲：状态词缺不信任态**）：P-14 那条裁定与装载校验 ⑥-c 只给**闸门这一条路**加了 `age_ms ≤ max_age_ms` 门槛，但 `Rr3RailState{mv, tier}` **没有龄期字段**、`tier_of` 是纯电平分类（`power.rs:87-95`）⇒ 面板 / brain / 日志三个消费者读到的 `tier`，与"新鲜 / 陈旧 / 从未取到"**在接口上不可区分**；`rr3_now_ns()` 取不到 epoch 折 `0` 还会让 `age_ms` 自己先变成假数（Y-4 后半）。**判定面与展示面共用一份不敢信的数据**，这是 L-26 的续集而不是新病，但要靠"状态词有没有不信任态"这把尺才照得出来**修法**：`Rr3RailState` 补 `age_ms`、`Rr3RailTier` 补 **`Unknown`**，运行期规则一条写完——`age_ms > max_age_ms` **或** `rr3_now_ns()` 取不到基准 ⇒ `tier = Unknown` ⇒ **执行面按 `Severe` 那一侧处置**（闸门保持置位 / 不许解禁），**展示面原样显示 `Unknown`、不许折算成 `OK`**；`rr3-power/rail/state` 主题同步带 `Unknown`。细则唯一落点 = 全案 §6 末段与 §3.1 轨压行。 | **批N2**（与 P-14 同批，域与 `Rr3Rail` 落地时一起改形状） | P-9；`Rr3Rail` 立模；⑥-c 的 `max_age_ms` 键；门 c。**执行面按保守侧处置、展示面原样显示**这两条各一条用例 |

## §11 待证与敞口

1. 真电池读数从未发生（INA219 未采购）→ §4 的"最坏延迟 ≤5 s + 一拍"是**推算**，须板上直方图证实。
2. 边沿在**掉电重启后**即新段——设计意图如此，但没有跨重启的用例证据。
3. 要标定的是 `percent` 的**电压→容量曲线**（形状未定义，随真件批）；`charging` 的电流符号判定虽有代码，
   **从未在真硬件上验过**——INA219 未采购，今天只有 sim 注入与交叉编译两条证据。

## §12 跨档动作（host / rtos / bare 逐件）

发布权判定：`battery/state`、`battery/low`、`charger/state` 都由本单元**主动发布**——但自足 bare 形态不宣称
发布主题（ADR-0018 修订 2 ①），所以那一档里本单元的件只跑**本地低电边沿**（安全核同族），不发帧。
**本单元是 13 件里跨档代价最低的一处**：`Rr3BatteryState` 已是 `Copy`、`Rr3LowEdge` 已在 `rr-core::control`，
件面除 `json!` 之外零 std/零 alloc/零 dyn——载荷上移一做完，`battery`/`charger` 就是 `core`-only。

| 件 | host（今天） | rtos（esp-rs std + FreeRTOS） | bare（no_std 自足） | 卡住它的**具体那一行依赖** → 动作 |
|---|---|---|---|---|
| battery | ✅（sim 与 INA219 两后端） | ⚠️ 只差 HAL 形状 | ⚠️ 只差 HAL 形状 | ① `rr_hal::Rr3Power::battery()` 是 `#[async_trait] + Send + Sync`，件不碰它（调用在 `composite.rs`），但件签名里的 `Rr3BatteryState` 来自 `rr_hal` → 批I F1 把值类型挪进 `rr-hal-api`；② `rr3_battery_state_body` / `rr3_battery_low_body` 的 `json!` → **上移承载层**（P-7），件面即可去 `serde_json`；③ `Rr3LowEdge` 已 re-export 保路径 ✅ |
| charger | ✅ | ✅ 同 battery | ✅ 同 battery | 一处：`rr3_charger_state_body(seq, charging) -> Value` 上移（P-7）。件内除 `json!` 外无任何平台面——**它是"件应当长什么样"的样板** |
| 组合件（`rr3_power_run`） | ✅ | ✅ 逻辑不变 | ⚠️ 形态差异全在这里 | 三档差异**不该**下进件层：host 由 rrd 监督（注册/心跳/重启）；rtos 由平台 task 承载（描述符静态分配 task 号，ADR-0018 修订 2 ①）；bare 无监督面，**挂死只能靠平台看门狗**——本仓四类供给里没有"看门狗"位，这是真实缺口（ADR-0018 修订 4 ② 补 `[supplies]` 平台看门狗位）。低电边沿在 bare 档仍然跑（它是件+机制核），但**没人收帧**：兜底动作必须本地化（拒动/降速由本地安全核执行，不等总线）——机制原语已备（`Rr3LowEdge` 回阈+迟滞判定 + MC-5 的 `Rr3LowGate` 闸门原语），**动作接线在安全核段**（MC-5 §20 ② ④：bare 档自判出 `RR3G` 守护行），载体待 0.0.7 真板；`cutoff_voltage_mv` 于 0.0.5 **声明未生效**（MC-8① 判 (c)） |

**动作序**：批I F1 → P-7（载荷上移）→ 单元即可标"rtos/bare 可达"。
**微型形态里的本单元**：`power.battery` 是 base 层"恒在位"格，与"纯 USB 供电的微型形态"直接冲突（索引 D3 待裁）——
本册 §10 P-1 是这一档最该先裁的一条，否则 rtos/bare 形态的依赖仲裁里永远挂着一个不可能满足的格。

## §13 策略字面量清单

| 文件 : 符号 | 字面量 | 语义 | 读者 | 去处 |
|---|---|---|---|---|
| `src/composite.rs : rr3_power_config_parse` | 缺段/缺键一律 `Err("config 缺 [rr3-power.{name}] 段" / "缺 {key}（0–65535 整数）")`；**无任何数值缺省**；仅 `.max(1)` | 配置面 fail-closed | 有 | **本仓正面样板**：策略全在 `config.toml`，代码不藏缺省值。其余五册都该收敛到这个形状（sense 的 `default()` 三份重复就是反例） |
| `src/composite.rs` + `battery.rs : Rr3BatteryCfg.cutoff_voltage_mv` | config `3150` | 关断阈值 | **机器读者 = 注入门**（`atoms/battery/assemble.toml` 的 ops 槽 → `rr3_registry_assemble_injection_gaps` 按点路径取值，⑧-c 门仓级强制）；**非** `println!("…cutoff={}mV")` 回显（回显不算读者） | **MC-8① 判 (c)（2026-10-13）**：键保留为策略数据（保 M6 意图），**本版不关断**（关断动作归 M6 关机链，需 T1/T2 硬件）——启动日志显式标注"声明未生效"，从此不再有"回显即当作已实现"的中间态（见方案 §20 第 2 条 ⑤） |
| `composite.rs : low_voltage_mv` | config `3300` → `Rr3LowEdge::rr3_low_edge_new` | 低电阈值 | 有（**本单元唯一有后果的阈值**） | ✅ 已合规（policy 在 config，判据在机制核） |
| `composite.rs : sample_period_ms` / `state_period_ms` | `5000` / `2000` | 拍点 | 有 | ✅ 已合规；`.max(1)` 钳位补留痕（P-2） |
| `composite.rs : RR3_POLL_WAIT` | `100ms` | 收帧空转代价 | 有 | ops/config 键（与五册 POLL_WAIT 同批归一） |
| `composite.rs : RR3_POWER_CONFIG` / `RR3_MANIFEST` | env 键名 | 配置入口 / 接入 | 未设 `RR3_POWER_CONFIG` → 回落 manifest 同目录 `config.toml`（**静默换源**，文件缺失才 `Err`）；`RR3_MANIFEST` 未设 → `exit(1)` ✅ | 静默换源要在日志里写明"配置来源 = X"，否则运维改了包内 config 却不知加载的是哪一份 |
| `atoms/battery/mech.rs : rr3_battery_state_body / rr3_battery_low_body` | 键名 `seq/voltage_mv/percent/charging`、`seq/percent/voltage_mv` | 载荷契约 | 有（schema `additionalProperties=false`） | **契约字面量—不许动**；上移承载层后仍逐字保留（P-7 的验收 = 载荷字节零改动） |
| `atoms/charger/mech.rs : rr3_charger_state_body` | `seq`/`charging` | 载荷契约 | 有 | 同上 |
| `rr-hal` sim 后端 | `RR3_HAL_SIM_BATTERY_CHARGING`（缺省 false） | sim 充电位 | 有 | ✅ 落位正确（假数据在 sim 后端，不在件里） |
| `rr-hal/src/linux_power.rs` | `charging: current_raw < 0` | 判定式 | 有 | 判定归后端；符号约定（正=放电）要写进 `rr-hal-api` 的文档面，否则换一片电流检测芯片就要重读代码 |
| `config.toml [rr3-power] log_level` | `"info"` | 日志级 | **无读者** | 被读或删除（P-6） |
| `atoms/{battery,charger}/assemble.toml` | `handles = []` / `ops = []` | 注入面申报 | **零申报** | 本单元策略虽已大体在 config，但 `handles` 空意味着批J 的注入校验门在这里**无内容可校验**——落地时先补 `power` 句柄申报（`i2c_bus`/`ina219_addr` → 描述符键），把它做成校验门的第一个真值样本 |

## §14 基线对表（代码 `22783ea`，2026-10-10；本节是本册唯一"现在时"）

| 本册旧断言 | 现在的事实（符号为证） | 判定 |
|---|---|---|
| §13 末行"本单元策略全无申报位，§3 的 ops 槽是纸面" | **本册成了全仓第一个真值样板**：`atoms/battery/assemble.toml` 填 `handles = [{trait="Rr3Power", desc_key="i2c_bus"}, {trait="Rr3Power", desc_key="ina219_addr"}]` + 3 条 `ops`（`sample_period_ms`/`low_voltage_mv`/`cutoff_voltage_mv` ← `rr3-power.battery.*`），`charger` 填 1 条 `ops`（`state_period_ms`）；读者确实存在（`rrd` `rr3_assemble_scan` + `rr-registry` 注入面用例） | **索引 L-3 的破环法被采纳**（门与数据同批、以 power 为样板） |
| §3 句柄来源写 `[slots].present:power.battery`（当时是幻影） | 现在 `[slots].present` 真存在（`profiles/*.toml` `schema_version = 2`，`esp32-mini.toml` 有三格升序声明），且 `rrd` `rr3_profile_boot_inputs` 真校验；但电源句柄的实际取值键仍是 `[hardware].i2c_bus`/`ina219_addr`（经 `assemble.handles.desc_key` 申报） | 幻影断言作废，按上句新事实读 |
| §12 "载荷上移后即 core-only"（P-7） | 件面 `rr3_battery_state_body`/`rr3_battery_low_body`/`rr3_charger_state_body` 已移到 `src/composite.rs`；`battery`/`charger` 件面除类型 import 外零 `json!` | **P-7 已结** |
| §1 "`Rr3Power::battery()` 是 `#[async_trait] + Send + Sync`" | 契约面 `rr-hal-api::Rr3Power` 同步、`Rr3BatteryState { voltage_mv: u16, percent: u8, charging: bool }` 为 `Copy` 且已在 `rr-hal-api` | §12 该行作废 |
| §6/§10 R-2："不立独立 charger 句柄、标定对象是 percent 曲线" | 实现采纳（charger `handles = []`，charging 由读数派生） | **R-2 一致** |
| §10 P-6 孤儿 `log_level` | 已删（`config.toml` 头注写明"解析函数不读、全仓无读者"） | **P-6 已结** |
| §10 P-2 钳制留痕 | `.max(1)` 仍在；未复验是否补了留痕日志 | **P-2 部分**（当时判语，现由 §14.1 判结——不回改） |
| §10 P-3 描述符吞错 | 已改：`rr-hal` 选择器不再 `.ok()`，`descriptor.rs` `rr3_binding_set` 打"描述符键 {key} 取值不认 / 未在册"，`rr-hal` 的 linux/sim/… 六处 `.ok()` 吞错收口 | **P-3 已结** |
| §10 P-8 / 索引 L-5：bare 档本地兜底 | 装载门已判 `tier=bare` 必须有 watchdog（G6）；**"低电禁动"已落实现**，cutoff 仍只解析（本版不关断） | **已结（批M MC-5 `071dcad`+`8b5b57e`，2026-10-13）**：与 A-5 / 批L L-8 三单合一，`Rr3LowGate` 接 `rr3-runtime/system/power`（原挂"未结、必须与 A-5 合并"已兑现） |

### §14.1 增量对表（2026-10-12，代码基线 `78625ae`；全仓总表见 [索引 §13.4](模块设计索引.md)）

| 项 | 复验实况（符号） | 判定 / 新落点 |
|---|---|---|
| P-2 钳制留痕 + 采样失败计数 | `src/composite.rs` 解析期 `eprintln` 打"键名/原值/钳后值" + 用例钉 0→1ms 收敛；`composite.rs:118` 的 `rr3_counter_bump("battery_read_fail", 1)` 把采样失败计入 counters | **结（整条）** |
| P-2 后半（上一条已并入） | 上一版本册 §14 写"仍无 `rr3_counter_*`"系我 grep 漏了带下划线的键名，实况有 `battery_read_fail` | **收回，随 P-2 整条结**；"活着但不动"族仍欠的是 sense S-3 与 act A-6 两处 |
| P-8 / A-5 / 批L L-8 三单 | 语义由索引 **R-15** 结案（触发源唯一 = rrd 的 `rr3-runtime/system/power` 广播；禁执行面不禁妆面；回阈 + 迟滞解禁；bare 档安全核段自判 `Rr3LowEdge` 出 `RR3G` 行） | **已结（批M MC-5 `071dcad`+`8b5b57e`，2026-10-13）**——原判"一个动作 → MC-5"已兑现；本册不再单独挂"本地兜底"账 |
| `cutoff_voltage_mv` | 有解析、**机器读者 = 注入门**（ops 槽）+ 启动日志显式"声明未生效"；**本版不关断** | **结 → MC-8① 判 (c)（2026-10-13）**：键保留为策略数据、明写不关断（关断动作归 M6 关机链）；`println` 回显不再充当"已实现"，键的机器读者是注入门（⑧-c 门） |
| P-5 电压→容量曲线标定 | 需两点以上真读数 | 留 **0.0.6 真件档**（载体依赖，不进开发窗） |
