# rr3-connect 模块设计（框架 v1）

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程（host）；`restart_policy = "always"`、`mem_limit_mb = 32` | `module.toml` |
| 格 | `net.wifi` · `net.bt`（body 层）；`net.online` 属 base 层（基座内建，本单元**供数不占有**） | `contracts/slots.toml` |
| 件 | `wifi` · `bt`；组合件成分：`session.rs`（云会话事务） | `atoms/`、`src/` |
| 主题 | 发布 `net/link`；RPC `cloud/session`、`wifi/scan`、`wifi/connect`、`wifi/forget`；订阅面为空；`[depends].optional = ["net.online"]` | `module.toml` |
| 句柄 | `rr3_hal_link()` → `dyn Rr3Link`（`state() -> Rr3LinkState`） | `src/composite.rs` |
| 参数 | `link_report_period_ms=5000`、`roam_probe_enabled=false`、`tls_min_version="1.3"`、`timeout_ms=20000` | `config.toml`（`RR3_CONNECT_CONFIG`） |
| 健康 | 心跳 2000 / 看门狗 10000 ms | `[health]` |

## §1 件契约表

| 件 | 公开符号（现状） | 输入 → 输出 | 状态 | 失败语义 |
|---|---|---|---|---|
| wifi | `Rr3LinkEdge::rr3_link_edge_new()` / `rr3_link_edge_cross(&mut self, up: bool) -> bool` | 链路读数 → 翻转判定 | `last_up` | — |
| wifi | `rr3_link_body(seq: u64, st: &Rr3LinkState, mode: &str) -> Value` | 读数 + 配网态 → 流载荷 | 无 | — |
| wifi | `Rr3Provision::{rr3_provision_new, rr3_provision_mode() -> &'static str, rr3_provision_connect(ssid: String, pass: String), rr3_provision_forget()}` | 配网事务 | `mode: ap\|station` | 不落盘、重启即忘 |
| wifi | `rr3_wifi_{scan,connect,forget}_parse_args(...) -> Result<…,_>` + 三个 `*_result(...) -> Value` | 点名参数/应答 | 无 | 参数错 → 业务 error |
| bt | `Rr3BtPhase`（`off`/`discoverable`/`connected`）+ `rr3_bt_phase_name` | — | — | — |
| bt | `Rr3BtState::{rr3_bt_state_new, rr3_bt_phase, rr3_bt_power_on, rr3_bt_link_up, rr3_bt_link_down, rr3_bt_power_off} -> bool` | 事件 → 是否发生转移 | `phase` | 非法转移 → `false` 且**状态不动** |
| bt | `rr3_bt_backend_up_default() -> bool`（**件内读 `RR3_HAL_SIM_BT_UP`**） | env → 初值 | 无 | 非 `"1"` 一律按关 |
| 目标签名 | `Rr3LinkState.ssid: Option<String>` → 定长 + 长度；`connect(ssid: String, pass: String)` → 借用；bt 初值 → `ops` 注入 | | | |

## §2 状态机

**`Rr3BtState` 转移矩阵**（返回 true = 发生转移；非法组合一律拒且不动）：

| 当前 | `power_on` | `link_up` | `link_down` | `power_off` |
|---|---|---|---|---|
| `off` | → `discoverable` ✅ | ❌ | ❌ | 无操作（同态） |
| `discoverable` | 同态 ❌ | → `connected` ✅ | ❌ | → `off` ✅ |
| `connected` | ❌ | 同态 ❌ | → `discoverable` ✅ | → `off` ✅ |

**`Rr3Provision`**：`ap ⇄ station`，`connect` 置 station、`forget` 清凭据回 ap；不变式：凭据只在内存、
`mode` 随 `net/link` 一起发布（append 字段）。
**`Rr3LinkEdge`**：与 `Rr3LowEdge` 同族语义（翻转必发 + 周期保底），断链时**设备面强制抹掉
`ssid`/`rssi`**（不许从死链读信息）——这条是守护语义，不是渲染细节。

## §3 注入面

| 件 | `handles` | `ops` 槽 | 策略位点（现状在件内，须外移） |
|---|---|---|---|
| wifi | `dyn Rr3Link` ← `[hardware].link_ifname` | `link_report_period_ms`=5000；`timeout_ms`=20000；`roam_probe_enabled`=false；`tls_min_version`="1.3" | ① **"密码至少 8 位"**（legacy 面板字面依赖）；② **sim 假 AP 三行表**（`rock-sim`…，`RR3_HAL_SIM_LINK_SSID_DEFAULT` 已是同类落点）；③ `scan` 结果的确定性内容 → 全部迁 sim 后端/config |
| bt | 无（真后端未实装） | 初值 `backend_up_default`（现由件读 env） | 主题不登记的决定属 D5/Q5 裁定，留 ADR 记录 |

## §4 主循环与时序预算

`RR3_POLL_WAIT = 100 ms`；`next_report = now + link_report_period_ms`（相对节拍）。

| 输出 | 名义周期 | 最坏延迟 | 为什么这条延迟重要 |
|---|---|---|---|
| `net/link` 周期保底 | 5000 ms | 5000 ms + 一次 publish | **所有依赖 out 的降级判断的粒度就是它**（brain 的 `net.online` 可选依赖、面板显示） |
| `net/link` 翻转 | 边沿 | 一拍采样内 | nmcli 子进程调用时长直接进这里——与 `timeout_ms=20000` 是两个独立的数（未对齐，工单 N-2） |
| `wifi/scan` | 点名 | sim 立即 / 真后端未测 | — |
| `cloud/session` open | 点名 | 含云 RTT | 链路 down 时立即折 error（文案点"链路"），不排队重放 |
| 与心跳 | 2000 ms 独立线程 | — | 主循环卡在 nmcli 子进程时心跳仍续 → 同类"活着但不动"，counters 补 |

## §5 主题面契约

| 主题 | kind | seq | 触发 | 载荷必填 | 申报与实现 |
|---|---|---|---|---|---|
| `net/link` | stream/event | 自增 | 翻转必发 + 5 s 保底 | `seq, up, ssid?, rssi?, mode` | 一致 |
| `wifi/scan` | RPC | — | 点名 | `{backend:"sim", items:[{ssid,rssi,auth}]}` | 一致（`backend` 标注是诚实面，不得删） |
| `wifi/connect` | RPC | — | 点名 | `{ok, connected, mode, ssid}` | `connected` **如实取链路读数**，不 fake |
| `wifi/forget` | RPC | — | 点名 | 清 + 回 ap | 一致 |
| `cloud/session` | RPC | — | 点名 | open 应答 | mock 注入点 `RR3_BRAIN_CLOUD=mock` |
| bt | — | — | **不登记主题**（D5 裁定） | — | 一致；状态可经 panel/日志环读 |

## §6 降级矩阵

| 格 | 缺格 | 句柄缺 | 句柄值错 | 后端未实装 | 参数非法 |
|---|---|---|---|---|---|
| `net.wifi` | 不产 `net/link` → 依赖方 `Degraded`（现状探测恒真使环不转，批J 修） | placeholder → 显式 `Unsupported` | 描述符作废且静默（批J） | nmcli 在 linux 有码、**板上未证** | `ssid` 空 / 密码 <8 → 业务 error 点名 |
| `net.bt` | 只停状态机（无主题即无下游） | — | — | BlueZ 真后端后置 | 非法转移 → 不动 + false |
| `net.online`（base） | 恒在位；本单元缺位时基座报"未就绪"而不是本单元报 | — | — | — | — |
| AP 态语义 | **未定**：`mode = ap` 时 `up` 该不该为真（AP ≠ 出网就绪）——本册主张 `up = 已关联且可出站`，AP 态 `up=false, mode="ap"` | | | | |

## §7 安全核接入

无机械安全核。守护语义三条：断链抹身份（防假新鲜）、凭据不落盘、TLS 下限声明；
另有**面板→本单元的转发不含裁决**（裁决在 owner，即本件的 `*_parse_args`）。

## §8 两腿对拍用例设计

| 件 | 输入序列 | 期望值来源 | 边界集 | 现状 |
|---|---|---|---|---|
| bt 矩阵 | 八种事件序列（含 `connected` 直接断电） | §2 矩阵（件外可写成常量表） | 同态、非法转移 | ✅ 件内矩阵用例 |
| link 边沿 | `up` 序列 `[true,true,false,false,true]` | 翻转必发 + 断链抹 ssid（裁定） | 重复同态 / 翻转 | ⚠️ 需补"断链必抹身份"的用例（现在只有 e2e 侧） |
| 载荷两腿 | 三 RPC 应答 | `contracts/topics.toml` + schema | 必填缺失、多余字段 | ✅ connect 有；⚠️ scan/forget 需补 schema 实例断言 |
| provision | `connect → forget → connect` 序列 | §2 不变式（重启即忘 = 新进程新态） | 未连先忘 | ⚠️ 需补 |
| 密码长度 | `pass.len() ∈ {0,7,8}` | 裁定字面（legacy 契约） | 等号 8 | ✅ 有（正是"期望值来自裁定"的样本） |

## §9 不变量与红线断言

- 断言：`net/link` 在 `up=false` 时 `ssid`/`rssi` 必为空。
- 断言：`connect` 的 `connected` 只取读数，**永不**因请求成功而置真。
- 断言：配网三点名的字段名与文案（`pass`、"密码至少 8 位"）是 legacy 面板依赖的契约字面量。
- 断言：bt 不登记主题（新增即契约变更，需评审）。
- 断言：凭据不落盘（`Provision` 无文件写路径）。

## §10 改造工单

| # | 动作 | 归属 | 前置 |
|---|---|---|---|
| N-1 | 三处策略出件（8 位规则 / sim AP 表 / bt env 初值）→ ops + sim 后端 | 批I/J | C5 |
| N-2 | `timeout_ms` 与 nmcli 子进程超时的关系定案（一个真值点） | 批I | — |
| N-3 | `Rr3LinkState.ssid` 去 `String`（`⊙` 跨片形状） | 批I | C5 |
| N-4 | AP 态与 `up` 的语义定案并写进 schema 注记 | **待裁** | — |
| N-5 | 真链路（wpa_supplicant 持久化 / BlueZ）随板上批 | 0.0.6 后续 | 采购 |

## §11 待证与敞口

1. 真链路全链未证（今天只有 sim 与 nmcli 代码路径）。
2. `net/link` 最坏延迟是推算（5 s + 一拍），需要板上直方图。
3. 云侧联调在范围外（凭据/M6）。
