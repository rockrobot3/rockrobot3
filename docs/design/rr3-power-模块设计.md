# rr3-power 模块设计（框架 v1）

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
**不能**被策略绕过（brain 停机也不影响 battery/low 上报）；**关断动作未实装**（cutoff 只回显），
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
| P-4 | 件载荷去 `serde_json`（L0 迁移清单在册） | 后续批 | C7 |
| P-5 | **改判**：`charger` 不需要独立句柄——`charging` 已由 `Rr3Power` 读数的电流符号派生（INA219 侧），再立一个句柄就是第二真值点。要标定的不是充电位而是 `percent` 的**电压→容量曲线**（两点或多点），随真件批做 | 真件批（0.0.6） | 采购 |
| P-6 | `config.toml` 的 `[rr3-power] log_level` 是孤儿键（解析函数不读、全仓无读者）：**被读或删除**，不许第三种状态（与 interact I-1 同纪律） | 批I | — |

## §11 待证与敞口

1. 真电池读数从未发生（INA219 未采购）→ §4 的"最坏延迟 ≤5 s + 一拍"是**推算**，须板上直方图证实。
2. 边沿在**掉电重启后**即新段——设计意图如此，但没有跨重启的用例证据。
3. 要标定的是 `percent` 的**电压→容量曲线**（形状未定义，随真件批）；`charging` 的电流符号判定虽有代码，
   **从未在真硬件上验过**——INA219 未采购，今天只有 sim 注入与交叉编译两条证据。
