# rr3-connect 模块设计（框架 v1.1）

## §0 单元身份证

| 项 | 值 | 来源 |
|---|---|---|
| 执行单元 | 独立进程（host）；`restart_policy = "always"`、`mem_limit_mb = 32` | `module.toml` |
| 格 | `net.wifi` · `net.bt`（body 层）；`net.online` 属 base 层（基座内建，本单元**供数不占有**） | `contracts/slots.toml` |
| 件 | `wifi` · `bt`；组合件成分：`session.rs`（云会话事务） | `atoms/`、`src/` |
| 主题 | 发布 `net/link`；RPC `cloud/session`、`wifi/scan`、`wifi/connect`、`wifi/forget`；订阅面为空；`[depends].optional = ["net.online"]` | `module.toml` |
| 句柄 | `rr3_hal_link()` → `dyn Rr3Link`（`state() -> Rr3LinkState`） | `src/composite.rs` |
| 参数 | `link_report_period_ms=5000`（**唯一有功能后果的一项**，进主循环节拍）；`roam_probe_enabled=false`、`tls_min_version="1.3"`、`timeout_ms=20000` 三项**只被解析并回显在启动日志里，全链无读者**——漫游探测无实现、TLS 下限无落地、云侧超时另有真值（`session.rs` 自己的 HTTP 超时） | `config.toml`（`RR3_CONNECT_CONFIG`）；孤儿项处置见 §10 N-6 |
| 健康 | 心跳 2000 / 看门狗 10000 ms | `[health]` |

## §1 件契约表

| 件 | 公开符号（现状） | 输入 → 输出 | 状态 | 失败语义 |
|---|---|---|---|---|
| wifi | `Rr3LinkEdge::rr3_link_edge_new()` / `rr3_link_edge_cross(&mut self, up: bool) -> bool` | 链路读数 → 翻转判定 | `last: Option<bool>`（**不是** `last_up: bool`）——首读无基线，`None` 时**第一次读数即算一次翻转**返回 true，用例喂的序列也带这一拍 | — |
| wifi | `rr3_link_body(seq: u64, st: &Rr3LinkState, mode: &str) -> Value` | 读数 + 配网态 → 流载荷 | 无 | — |
| wifi | `Rr3Provision::{rr3_provision_new, rr3_provision_mode() -> &'static str, rr3_provision_connect(ssid: String, pass: String), rr3_provision_forget()}` | 配网事务 | `mode: ap\|station` | 不落盘、重启即忘 |
| wifi | `rr3_wifi_{scan,connect,forget}_parse_args(...) -> Result<…,_>` + 三个 `*_result(...) -> Value` | 点名参数/应答 | 无 | 参数错 → 业务 error |
| bt | `Rr3BtPhase`（`off`/`discoverable`/`connected`）+ `rr3_bt_phase_name` | — | — | — |
| bt | `rr3_bt_state_new() -> Self`、`rr3_bt_phase(&self) -> Rr3BtPhase`（这两个**不返 bool**）+ `rr3_bt_power_on/link_up/link_down/power_off -> bool`（四个转移事件才返"是否发生转移"） | 事件 → 是否发生转移 | `phase` | 非法转移 → `false` 且**状态不动** |
| bt | `rr3_bt_backend_up_default() -> bool`（**件内读 `RR3_HAL_SIM_BT_UP`**） | env → 初值 | 无 | 非 `"1"` 一律按关 |
| 目标签名 | `Rr3LinkState.ssid: Option<String>` → 定长 + 长度；`connect(ssid: String, pass: String)` → 借用；bt 初值 → `ops` 注入 | | | |

## §2 状态机

**`Rr3BtState` 转移矩阵**（返回 true = 发生转移；非法组合一律拒且不动）：

| 当前 | `power_on` | `link_up` | `link_down` | `power_off` |
|---|---|---|---|---|
| `off` | → `discoverable` ✅ | ❌ | ❌ | 无操作（同态） |
| `discoverable` | 同态 ❌ | → `connected` ✅ | ❌ | → `off` ✅ |
| `connected` | ❌ | 同态 ❌ | → `discoverable` ✅ | **❌ 不可直接断电**——`rr3_bt_power_off` 只在 `discoverable` 生效，用例 `mech.rs` 明写"Connected 不能直接断电"；必须先 `link_down` 再 `power_off` |

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
| `net/link` 翻转 | 边沿 | 一拍采样内 | nmcli 子进程调用时长直接进这里；**`timeout_ms=20000` 今天管不到它**（该值无读者），所以最坏翻转延迟其实由 nmcli 自己的超时决定——这是 N-2 的真问题 |
| `wifi/scan` | 点名 | sim 立即 / 真后端未测 | — |
| `cloud/session` open | 点名 | 含云 RTT | 链路 down 时立即折 error（文案点"链路"），不排队重放 |
| 与心跳 | 2000 ms 独立线程 | — | 主循环卡在 nmcli 子进程时心跳仍续 → 同类"活着但不动"，counters 补 |

## §5 主题面契约

| 主题 | kind | seq | 触发 | 载荷必填 | 申报与实现 |
|---|---|---|---|---|---|
| `net/link` | **event**（`contracts/topics.toml` 登记的就是 event，本册旧版写 stream/event 两可是笔误） | 自增 | 翻转必发 + 5 s 保底 | `seq, up, ssid?, rssi_dbm?, mode`（键名是 **rssi_dbm**，不是 rssi） | 一致；⚠️ `contracts/events/` 里**没有** net/link 的载荷 schema，§8 的"腿二"只能引件内 golden 与面板取键 |
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

无机械安全核。守护语义**两条成立**（断链抹身份防假新鲜、凭据不落盘），**一条只是声明**：
`tls_min_version="1.3"` 今天没有任何读者，把它写成"下限"是给未实装的能力提前记账（§10 N-6 处置）；
另有**面板→本单元的转发不含裁决**（裁决在 owner，即本件的 `*_parse_args`）。

## §8 两腿对拍用例设计

| 件 | 输入序列 | 期望值来源 | 边界集 | 现状 |
|---|---|---|---|---|
| bt 矩阵 | 八种事件序列（含 `connected` 直接断电） | §2 矩阵（件外可写成常量表） | 同态、非法转移 | ✅ 件内矩阵用例 |
| link 边沿 | `up` 序列 = **首读无基线** + `[true,true,false,false,true]`（`last: Option<bool>` 要求第一拍先建立基线） | 翻转必发 + 断链抹 ssid（裁定） | 首拍基线、重复同态、翻转 | ✅ 已有：设备侧 `sim.rs` 的 `sim_link_reads_up_ssid_rssi_and_blanks_them_when_down` 与件面 `mech.rs` 的 nulls 断言，e2e 侧另有一条——旧版"只有 e2e 有"不成立 |
| 载荷两腿 | 三 RPC 应答 | **`contracts/events/` 里没有 wifi/net 的载荷 schema**，期望值只能取件内 golden + 面板取键（与 interact 同病，N-6） | 必填缺失、多余字段 | ✅ 已有 `wifi_scan_and_forget_shapes`（断 `backend:"sim"`、三条 items 的 `ssid/rssi/auth`、forget 回 `mode:"ap"`）；⚠️ 缺 connect 应答的"永不因请求成功而置真"这条 |
| provision | `connect → forget → connect` 序列 | §2 不变式（重启即忘 = 新进程新态） | 未连先忘 | ✅ 已有 `provision_roundtrips_ap_station`；⚠️ 未连先忘这一条确实还没有 |
| 密码长度 | `pass.len() ∈ {0,7,8}` | 裁定字面（legacy 契约"密码至少 8 位"） | **等号 8** | ⚠️ 现有用例用的是 9 位与 5 位，**恰好的 8 位没测**——边界用例缺的正是这条，旧版"✅ 有"要打折 |

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
| N-2 | nmcli 子进程必须有自己的超时并被 `timeout_ms` 管：今天该值无读者，最坏翻转延迟由子进程自由决定 | 批I | — |
| N-3 | `Rr3LinkState.ssid` 去 `String`（`⊙` 跨片形状） | 批I | C5 |
| N-4 | AP 态与 `up` 的语义定案并写进 schema 注记 | **待裁** | — |
| N-5 | 真链路（wpa_supplicant 持久化 / BlueZ）随板上批 | 0.0.6 后续 | 采购 |
| N-6 | 三个无读者的声明项（`roam_probe_enabled` / `tls_min_version` / `timeout_ms` 的语义归一）：**实装、或删除、或改为 ops 槽并写清生效点**，不许停在"回显即当作已实现"。与 interact I-1、power P-6 同纪律，同批做以免三份册子各说各话 | 批I | — |

## §11 待证与敞口

1. 真链路全链未证（今天只有 sim 与 nmcli 代码路径）。
2. `net/link` 最坏延迟是推算（5 s + 一拍），需要板上直方图。
3. 云侧联调在范围外（凭据/M6）。
