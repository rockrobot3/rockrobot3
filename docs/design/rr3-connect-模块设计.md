# rr3-connect 模块设计

> 证据：`modules/rr3-connect/module.toml`、`atoms/{wifi,bt}/`、`src/{composite,session}.rs`、
> `config.toml`、`rr-hal` 的 `rr3_hal_link`、`contracts/topics.toml`。

## 1 职责

本机**出网与近场链路的唯一事实源**：链路状态流 + 配网面 + 云会话事务 + 蓝牙状态骨架。
它是别的单元判"能不能上云"的依赖，但不是决策者。格账：`net.wifi` + `net.bt`；
`net.online` 属 base 层（基座内建，本单元供数不占有）。

## 2 事实面

| 件 / 成分 | 面 | 现状 |
|---|---|---|
| `wifi` 件 | `net.wifi` | `net/link` 状态流（up / ssid / rssi + `mode`，翻转必发 + 周期保底，断链时**强制抹掉 ssid/rssi**）；配网三 RPC（`scan`/`connect`/`forget`）；`Rr3Provision`（ap/station 两态纯函数，凭据内存态、重启即忘） |
| `bt` 件 | `net.bt` | `Rr3BtState` 三态（off/discoverable/connected）转移矩阵纯函数，非法转移拒且不动；**主题不登记**（D5/Q5 裁定：日志环 + `rr3-ctl` 已承接）；sim 初值靠 `RR3_HAL_SIM_BT_UP` 注入 |
| `session`（组合件成分） | 接入 | `cloud/session` 点名 open 应答；失败折 error（文案点"链路"）不断链；凭据走 env（`RR3_BRAIN_CLOUD=mock` 注入点），不落盘 |

参数面：`link_report_period_ms = 5000`、`roam_probe_enabled = false`、`tls_min_version = "1.3"`、
`timeout_ms = 20000`。健康 2000 / 10000 ms；`[depends].optional = ["net.online"]`（**可选是对的**：
本单元自己就是 online 的供数方）。

## 3 件级切分与闭包（设计目标）

- `wifi` 件的三处策略今天长在件里，必须外移：**"密码至少 8 位"**（legacy 原词、配网页依赖）、
  **sim 假 AP 三行表**（`rock-sim` 等）、**`scan` 结果的确定性内容**。设计：件留"参数裁决的形状"
  （必填、类型、长度上限从 ops 来），具体数值与假数据表落 config / sim 后端
  （`RR3_HAL_SIM_LINK_SSID_DEFAULT` 已是这个方向的现成落点，扩成整表即可）。
- `bt` 件的 env 初值改 `handles`/`ops` 注入（批I 的第二个样板，第一个是 led）。
- `Rr3LinkState.ssid: Option<String>` 是 `⊙` 跨片传不动的形状之一（值类型去 `String` 的样本）。

## 4 职责四面

| 面 | 落点 |
|---|---|
| 接入 | 链路面（nmcli / sim）+ 配网三点名 + `cloud/session` |
| 运行 | 配网状态机推进、扫描/连接/遗忘事务 |
| 状态 | `net/link`（脑与面板都读它） |
| 守护 | 断链时抹身份信息（防"死链读数"）；凭据不落盘；TLS 下限声明 |

## 5 时序设计

`net/link` 周期保底 5000 ms + 翻转必发；心跳 2000 ms（online 变化到全总线可见的最坏延迟 = 上报周期，
**这条要实测**，因为依赖它的降级决策全在此之上）。待给量：nmcli 子进程调用超时与 `timeout_ms = 20000`
的关系（现是两个独立的数，容易互相矛盾）、配网事务的端到端时限。

## 6 降级与缺格语义

| 情形 | 设计行为 |
|---|---|
| `net.wifi` 缺（无 wifi 板） | `net/link` 不产生；依赖它的 brain 按依赖环 `Degraded`（现状机制在，但见索引 §2：探测恒真使降级环不转） |
| `net.bt` 缺 | 只停 bt 面，无主题即无下游 |
| 句柄在、后端未实装（板上 NetworkManager 不在） | 显式 `Unsupported`，**不得**回落 sim 报假在线 |
| `mode = ap` 期间 | `net/link` 的 up 语义要单独定（AP 态不是"出网就绪"）——现未定，属敞口 |

## 7 红线

`net/link` 载荷（含 `mode` 的只增纪律）、配网三点名的字段名与文案（`ssid`/`pass`、"密码至少 8 位"
是 legacy 面板依赖的**契约字面量**，动它等于断配网页）、bt 不登记主题的决定、凭据不落盘。

## 8 待证与敞口

1. 真链路（nmcli / wpa_supplicant 持久化 / BlueZ）全部后置板上批，今天只有 sim 证据。
2. AP 态与 online 语义的关系未定（§6 末行）。
3. 云会话联调在范围外（凭据与真云侧归 M6 / 生态层）。
