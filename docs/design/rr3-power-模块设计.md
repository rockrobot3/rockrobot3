# rr3-power 模块设计

> 证据：`modules/rr3-power/module.toml`、`atoms/{battery,charger}/`、`src/composite.rs`、`config.toml`、
> `rr-core/src/control.rs`（低电边沿真值）、`rr-hal` 的 `rr3_hal_power`。

## 1 职责

**唯一能在基座失能时独立兜底的单元**：电池读数、低电边沿、充电态。关断决策在本地完成，
不等总线、不等 brain（`architecture §7.3`）。格账：`power.battery`（base 层，恒在位）+
`power.charger`（body 层）。

## 2 事实面

| 件 | 格 | 发布 | 机制 |
|---|---|---|---|
| `battery` | `power.battery` | `battery/state`（周期流，首拍即采）· `battery/low`（边沿事件） | 边沿语义真值在 `rr-core::control::Rr3LowEdge`（件 re-export）：**越阈第一拍恰报一次、严格大于阈值才重新武装、武装本身不是事件** |
| `charger` | `power.charger` | `charger/state` | `rr3_charger_state_body(seq, charging)`——充电位由**最近一次电池读数**派生，首拍在电池之前不发 |

参数面：`sample_period_ms = 5000`、`state_period_ms = 2000`、`low_voltage_mv = 3300`、
`cutoff_voltage_mv = 3150`（**只解析回显、不动作**，关机与充电策略归 M6）；
`[rpc].exposes` 为空（订阅面为空是设计：兜底不依赖请求）；健康 2000 / 10000 ms；
`start_priority = 10`（最早起）、`mem_limit_mb = 16`。sim 电压曲线钉死 `3200 mV + 10 mV/%`，
linux INA219 侧 `charging` **诚实恒 false**（待两点标定）。

## 3 件级切分与闭包（设计目标）

- 两件的公开面已是纯函数（吃 `Rr3BatteryState` 出 JSON），改造点只有一个：值类型进 `rr-hal-api`
  并把边沿件的时钟从"调用方注入 ms"这条既有纪律保持住（勿回退成读钟）。
- `charger` 的"从电池读数派生"是**权宜而非设计**：真件到位后充电位应来自充电器自己的句柄
  （TP4056 类硬件面），届时要新增句柄而不是继续派生——这条要写进注入面 `handles` 的演进注记。
- `cutoff_voltage_mv` 的动作化（关断）是**守护面**的扩展，不是 battery 件的私有事：设计定为
  安全核动作（与 act 的限位同级），落点在机制核，触发方在本单元。归 M6 批，本册不声称已有。

## 4 职责四面

| 面 | 落点 |
|---|---|
| 接入 | 电源句柄（无 RPC、无订阅——**故意如此**：兜底链不能依赖请求） |
| 运行 | 5 s 采样 + 2 s 状态流 |
| 状态 | 两流 + boot 回显四个参数（回显是"配置真的生效了"的唯一证据面） |
| 守护 | 低电边沿（本单元的核心价值）；关断待 M6 |

## 5 时序设计

采样 5000 ms / 上报 2000 ms / 心跳 2000 ms / 看门狗 10000 ms。三个待给量：低电事件从**越过阈值**
到**总线上可见**的最坏延迟（现状 = 采样周期 + 一拍心跳，须实测确认而不是推算）、边沿在重启后的
重新武装语义（掉电重启即新段，设计需明确"新执行单元 = 新边沿段"）、INA219 读数抖动对边沿的影响
（未标定期望区间，标定时一并给）。

## 6 降级与缺格语义

- `power.battery` 是 base 层——**恒在位、恒不降级**（`contracts/slots.toml` 注记）。这意味着
  "无电池的板"在本契约体系里不成立；若真出现（纯 USB 供电的微型形态），要么给它一个假在位的
  battery 件（**反对：撒谎**），要么立一次 retired 评审（推荐，ADR-0019 修订 2 ②）。
- `power.charger` 缺 → 只停 `charger/state`，不影响 battery 与兜底。
- 读数失败（句柄在、读不出）→ 显式 `Unsupported`，**不得**沿用上一次读数继续发流（假新鲜）。

## 7 红线

两主题名与载荷形状（`battery/low` 的 schema `additionalProperties = false`，三字段必填）、
边沿语义的四条判据、首拍即采、`charging` 未标定前恒 false、无 RPC 面。

## 8 待证与敞口

1. 真电池读数从未发生（INA219 未采购）；板上 `charging` 恒 false 是设计不是缺陷，但**必须有标定批收口**。
2. base 层电池格在"无电池板"上的表达（§6）待裁。
3. 关断动作（M6）与两点标定（真件批）都在本单元之外，本册只占位不声称。
