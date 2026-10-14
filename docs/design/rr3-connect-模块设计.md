# rr3-connect 模块设计（框架 v1.3）

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
| wifi | `Rr3Provision::{rr3_provision_new, rr3_provision_mode() -> &'static str, rr3_provision_connect(ssid: String, pass: String), rr3_provision_forget()}` | 配网事务 | `mode: ap\|station` | 件内不写文件；凭据由后端承载持久化（MC-6） |
| wifi | `rr3_wifi_{scan,connect,forget}_parse_args(...) -> Result<…,_>` + 三个 `*_result(...) -> Value` | 点名参数/应答 | 无 | 参数错 → 业务 error |
| bt | `Rr3BtPhase`（`off`/`discoverable`/`connected`）+ `rr3_bt_phase_name` | — | — | — |
| bt | `rr3_bt_phase(&self) -> Rr3BtPhase`（**不返 bool**）+ `rr3_bt_power_on/link_up/link_down/power_off -> bool`（四个转移事件才返"是否发生转移"） | 事件 → 是否发生转移 | `phase` | 非法转移 → `false` 且**状态不动** |
| bt | `rr3_bt_state_new(up: bool) -> Self`（初值经 config `[rr3-connect.bt]` up 数据化注入，缺省关——0.0.5 批I N-1：原 `rr3_bt_backend_up_default()` 的 `RR3_HAL_SIM_BT_UP` env 读已删，13 件唯一非 test 面 env 读清零） | config → 初值 | 无 | — |
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

无机械安全核。守护语义**两条成立**（断链抹身份防假新鲜、凭据由后端承载持久化且件内不写文件——MC-6 已结 `084d081`），**一条只是声明**：
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
- 断言：件内不写文件（`Provision` 无文件写路径；持久化由后端承载——MC-6）。

## §10 改造工单

| # | 动作 | 归属 | 前置 |
|---|---|---|---|
| N-1 | 三处策略出件（8 位规则 / sim AP 表 / bt env 初值）→ ops + sim 后端 | **批I**（本轮定死，M-4 消解：原写"批I/J"二义；bt 去 env 本就在批I F3 清单内） | C5 |
| N-2 | nmcli 子进程必须有自己的超时并被 `timeout_ms` 管：今天该值无读者，最坏翻转延迟由子进程自由决定 | 批I | — |
| N-3 | `Rr3LinkState.ssid` 去 `String`（`⊙` 跨片形状） | 批I | C5 |
| N-4 | AP 态与 `up` 的语义定案并写进 schema 注记 | **待裁** | — |
| N-5 | 真链路（wpa_supplicant 持久化 / BlueZ）**按 2026-10-12 版本边界改判拆成两半**：① **凭据持久化 = 开发项**，搬入 0.0.5 批M **MC-6**（linux 侧由 NetworkManager 承载持久化、`forget` 真删除，主机侧可用临时 `NMCONF_HOME` 夹具测）；② **BlueZ 真后端 = 不在 F 清单内**，本轮不写码——**已落（MC-9 裁定 (a)，2026-10-12）：`net.bt` 改记 `placeholder` + 触发条件（配网备选通道立项 / BlueZ 真后端批），bt 件 SURFACE/注释写明"骨架，不 claim 射频"**，"骨架却记 normal"属承诺 > 事实（索引 R-16） | **MC-6**（持久化）/ **MC-9 已结**（bt 三态） | 采购（仅影响验证半边） |
| N-6 | 三个无读者的声明项（`roam_probe_enabled` / `tls_min_version` / `timeout_ms` 的语义归一）：**实装、或删除、或改为 ops 槽并写清生效点**，不许停在"回显即当作已实现"。与 interact I-1、power P-6 同纪律，同批做以免三份册子各说各话 | 批I | — |
| N-7 | **载荷组装上移承载层**：`rr3_link_body` 与三枚 `*_result` 的 `json!` 移到 `src/composite.rs`/`session.rs`，件只交 `Rr3LinkState` + `mode` + 判定（依据 = ADR-0019 修订 4）；验收 = `net/link` 与三 RPC 应答字节逐字不变 | 批I | — |
| N-8 | 对外自述里的栈名（`BlueZ`、`NetworkManager`/`D-Bus`）从 SURFACE 与 println 撤出，改由 `rr3_hal_backend_resolve` 交后端名——**换协议栈不该要求改代码**，这是本单元最直白的"换芯片不改码"检验点 | 批I | — |
| N-9 | sim 假 AP 表（三条 SSID/RSSI/auth）整表从 `atoms/wifi/mech.rs` 迁 `rr-hal` sim 后端，与 `RR3_HAL_SIM_LINK_SSID_DEFAULT` 合流成一处真值 | 批I | — |
| N-10 | **资源与链路形状**（[ADR-0021](../process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md) 裁定 3 的第二个语义域）：① `[pins].mux_group` 除引脚互斥外还要覆盖**射频共存**（WiFi 与 BT 共天线在 ESP32 上是真实资源冲突，与 GPIO18/19 那两次同类）；② **板侧凭据存储立对象**——rtos 形态自己配网，而今天凭据只有宿主侧仓外 0600 文件那一条路，板侧既无 `cred_store` 键也无读取窄缝，"值零回显"必须是**契约要求**而不是实现细节；③ UART 资源参数进绑定（`link.uart.baud` / `link.uart.tx_ring` / `audio.chunk_frames`）——板侧注释已实锤"缺省 256B TX 环让 mic 组帧线程被线速拖住、采样率从 16.5kHz 崩到 4.5kHz"，那是**资源参数住常量**的同型病例（16KB 环 + 680ms→11.7ms 是修复，但修复住在一块板的注释里） | **增补轮六 批N1**（键与形状）+ 批N4（凭据面面板可见） | 批N1；D12（BLE 面板路径） |
| N-11 | **三条**：① 配网最短密码位住**组合件**码内（`src/composite.rs` 的 `pass.len() < 8`），`atoms/wifi/assemble.toml` 的 `ops` 只有 `link_report_period_ms`/`timeout_ms` 两槽、**无 `min_pass_len`**——而 §14 曾把 N-1 判"已结"，那是**半边判整条**（改判随门 j 把组合件申报面进门禁）；② "rock-sim" 假 AP 名有**两处**（`composite.rs` 与 `src/session.rs`），L-9→0.0.8 W-B 那一条只覆盖前者；③ 探针卡死与"真的没网"在现场不可分——act 有 A-6、sense 有 S-3，**connect 无单**，本单补 counters 侧。**另承接 D5/D12 裁定**：`up` 只表链路关联、AP 态须在文案里点名；BLE 的占位语义改注为"bare 档运维入口"（不立 rtos 必达）。细则见 [全案](../process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §15.1 M-6 与 §12 D5/D12 | ① **批N4**（+ 门 j 随批N1）；② **0.0.8 W-B**；③ **0.0.5 批M 余数**（挂 0.0.8 W-B 同批） | C17；门 j |

## §11 待证与敞口

1. 真链路全链未证（今天只有 sim 与 nmcli 代码路径）。
2. `net/link` 最坏延迟是推算（5 s + 一拍），需要板上直方图。
3. 云侧联调在范围外（凭据/M6）。

## §12 跨档动作（host / rtos / bare 逐件）

发布权判定：`net/link` 由本单元**自主发布**，且它同时是全系统降级判断的粒度源——所以"要不要序列化面"在
本单元不是理论问题：**凡启用 `net.wifi` 的档就必须能发 JSON 字节**。推论是形态层面的硬约束：
`bare` 档**不启用** `net.wifi`/`net.bt`（没有链路后端，也就没有链路可报），链路由经桥的 host 侧代报。

| 件 | host（今天） | rtos（esp-rs std + FreeRTOS） | bare（no_std 自足） | 卡住它的**具体那一行依赖** → 动作 |
|---|---|---|---|---|
| wifi | ✅（sim 与 nmcli 两后端） | ⚠️ HAL 形状 + 载荷面 | ❌ 两处硬点 | ① `Rr3LinkState.ssid: Option<String>` 把 alloc 焊进值类型 → 批I F1 改"定长 + 长度"（`⊙` 跨片形状，N-3）；② `rr3_provision_connect(ssid: String, pass: String)` 与凭据字段 `Option<(String,String)>` → bare 档若真要本地存凭据，改固定槽 `[u8; N]` + 长度（N 由契约定，不是随手 32）；③ `rr3_link_body -> Value`（`json!` 五处）→ P-7 同法上移承载层（N-7）；④ `&dyn`/`rr3_block_on` 清零 |
| bt | ✅（骨架，BlueZ 后端未建） | ✅ **13 件里最干净的一件** | ✅ 同左 | 原唯一硬点 `rr3_bt_backend_up_default()` 里的 `std::env::var` **已随 0.0.5 批I N-1 清零**（函数删除，初值经 config `[rr3-connect.bt]` up 数据化注入——全仓 13 件唯一非 test 面 env 读自此不存在；批J G4 落 ops 槽时再迁数据载体）。状态机三态与矩阵本身零依赖，`#[allow(dead_code)]` 只说明主题面未登记（D5），不构成跨档障碍 |
| session（组合件成分，非件） | ✅ mock 云 | **不适用**：云会话属主承载层能力 | **不适用** | `tls_min_version`/`timeout_ms` 零读者在这里最刺眼：它属于"主承载层的对外承诺"，却写在 bare 档根本不会编译的代码里。去处 = 随 ADR-0016 Rust 化时落进真 HTTP 客户端配置，今天标"声明未生效"（N-6） |

**动作序**：批I F1 → N-3（`ssid` 去 String）→ N-7（载荷上移）→ bt 去 env（N-1）。
**微型形态里的本单元**：`esp32-mini` 那类经桥形态**不该**带 `net.wifi` 格——桥就是它的链路（architecture §12.7
"经桥接"拓扑）。把 `net.wifi` 塞进微型形态是常见的裁剪误解，索引 §10 的三个形态里我把它显式排除并写了理由。

## §13 策略字面量清单

| 文件 : 符号 | 字面量 | 语义 | 读者 | 去处 |
|---|---|---|---|---|
| `atoms/wifi/mech.rs : rr3_wifi_connect_parse_args` | `8` + `Err("密码至少 8 位（legacy 配网语义）")` | 口令规则 | 有；面板 `placeholder="至少 8 位"` **逐字复读** | 值 = ops 槽 `min_pass_len`；**文案 = 契约字面量—不许动**（legacy 依赖）。两处字面重复是债：页面该由 `/api/meta` 拿 |
| `atoms/wifi/mech.rs : rr3_wifi_scan_result` | 三条假 AP：`rock-sim/-42/true`、`rock-sim-guest/-58/true`、`neighbour-2g/-71/false`、`"backend":"sim"` | **sim 假数据** | 有（scan 点名直回） | 最该搬走的一条：假 AP 表在**件**里，`RR3_HAL_SIM_LINK_SSID_DEFAULT` 已是同类落点的正解 → 整表迁 `rr-hal` sim 后端（N-1）。件里留假数据 = bare 档要么带上它要么删码，两坏 |
| `atoms/wifi/mech.rs : rr3_provision_mode` | `"ap"` / `"station"` | `net/link.mode` 词汇 | 有 | **契约字面量—不许动** |
| `atoms/wifi/mech.rs : rr3_link_body` / `*_result` | 键 `seq/up/ssid/rssi_dbm/mode`、`ok/connected/ssid`、`{ok:true,mode:"ap"}` | 载荷契约 | 有（面板逐字读 `connected`、`ssid`） | **契约字面量—不许动**；上移后逐字保留（N-7 验收） |
| `wifi.rs` | `"rr3-connect/net/link"`、`wifi/scan`·`wifi/connect`·`wifi/forget`、`Rr3LinkCfg{link_report_period_ms, roam_probe_enabled}` | 主题与 cfg 形状 | 主题有读者 | 主题 = 契约字面量（与 `module.toml` 双写记债） |
| `src/composite.rs : Rr3LinkCfg.roam_probe_enabled` | config `false` | 漫游探针开关 | **无读者**（只 `println`；mech 注释自认） | 标"声明未生效"或删除（N-6） |
| `src/session.rs : Rr3SessionCfg` | `tls_min_version:"1.3"`、`timeout_ms:20000` | 云侧下限与超时 | **无读者**（注释自认"与 cutoff 同款口径"；真超时另有 HTTP 值） | 同上（N-6）；本册 §7 已把"TLS 下限"从守护语义里降级，**不再声称三条守护** |
| `src/session.rs : rr3_session_parse_args/result` | 只认 `action="open"`；`"未知 action: {action}（只认 \"open\"）"`；`"链路未就绪（net/link up=false），云端会话拒绝建立"`；`session:"opened"`、`endpoint:"mock"` | 会话契约 | 有 | **契约字面量—不许动**；`endpoint:"mock"` 是诚实面，不得删（R4 口径） |
| `atoms/bt/bt.rs : RR3_HAL_SIM_BT_UP_ENV` + `mech.rs : rr3_bt_backend_up_default` | `"RR3_HAL_SIM_BT_UP"`，`matches!(.., Ok("1"))`——**只有 `"1"` 算上**，`0`/未设/杂值一律 Off | 出厂不开蓝牙（显式拒绝宽容解析） | 有 | ✅ 落位正确（env 注入口在件内是过渡形态，N-1 改 ops）；**方向值得作为范本**：对照 `base/os/distro/README.md` 记的 `RR3_HAL_SIM_LINK_UP` 缺省 `1`＝"未设即在线"，两处 opt-in/opt-out 方向相反（索引审计 L-4） |
| `atoms/bt/bt.rs : SURFACE`、`mech.rs`、`src/composite.rs : println!` | `"BlueZ"`（**运行期打印**，非仅注释） | 后端自述 | 有 | 换芯片/换栈时这些串要跟着改 = **违背"换板不改代码"**。去处：后端名由 `rr3_hal_backend_resolve` 交上来，SURFACE 行只写"bt 面（后端=动态）"（N-8）。同类：`wifi.rs : SURFACE` 的 `"NetworkManager/D-Bus"` |
| `atoms/wifi/mech.rs`、`atoms/bt/mech.rs` | `NetworkManager`/`D-Bus`/`wpa_supplicant`/`NVS`/`BlueZ` 注释 | 厂商/栈名出现在件声明面 | — | ⑦-a 禁词表（`rr-registry/src/atom.rs::RR3_BOARD_TOKENS`）**只扫 `rr-core`/`rr-ipc::protocol`/`transport/mod.rs`，不扫 module mech** → 今天不红。但 ADR-0018 修订 3 ③ 定的"禁词作用件声明面"与这里冲突：注释里的栈名会随 SURFACE/println 出到对外面。**处置见索引 §11 审计 L-6**：禁词作用域要分"代码常量/运行期输出/纯注释"三档，别一刀切 |
| `atoms/{wifi,bt}/assemble.toml` | `handles = []` / `ops = []`（bt 那份注释写"sim 初值走 env 注入口，不占 config 面"） | 注入面申报 | **零申报** | 注释即声明是散文不是数据——批J G4 落地时这条注释必须变成 `ops` 里的一行，否则校验门无内容 |

## §14 基线对表（代码 `22783ea`，2026-10-10；本节是本册唯一"现在时"）

| 本册旧断言 | 现在的事实（符号为证） | 判定 |
|---|---|---|
| §12 "bt 唯一硬点是件内 `std::env::var`（N-1）" | 函数与 env 键双双退役，件面改 `rr3_bt_state_new(up: bool)`，初值经 **ops 申报**注入（`atoms/bt/assemble.toml`：`up` ← `rr3-connect.bt.up`） | **N-1 已结**，且 bt 从"零申报"变成**已申报的三件之一** |
| §12/§13 wifi 凭据 `String`（N-3） | `Rr3LinkState.ssid` 改 `Option<Rr3SsidText { bytes: [u8; 32], len: u8 }`（契约面）；**件内 `rr3_provision_connect(&mut self, ssid: String, pass: String)` 与 `credentials: Option<(String, String)>` 仍是堆类型** | **部分结**：值类型侧结、凭据侧未结 → 批L **L-3** |
| §12 "`&dyn`/`rr3_block_on` 清零（I-3/N-3 同族）" | 契约面同步、`rr3_block_on` 不存在；本册件面不持句柄（调用在 `composite.rs`） | 已结 |
| §0/§13 三个零读者配置项（N-6/N-2） | `roam_probe_enabled` 与 `tls_min_version` **删除**；`timeout_ms` **实装为 nmcli 子进程超时**并迁入 wifi 段 | **N-2、N-6 已结**（三选一的"实装/删除"两条各用一次） |
| §13 sim 假 AP 表在件里（N-9） | `rr3_wifi_scan_result` 的三条假 AP **仍在 `atoms/wifi/mech.rs`** | **N-9 未结** → 批L **L-9**（整表迁 sim 后端，与 `RR3_HAL_SIM_LINK_SSID_DEFAULT` 合流） |
| §13 栈名出现在 SURFACE/println（N-8） | 未见改动：`wifi.rs`/`bt.rs` 的 `SURFACE` 与 `composite.rs` 的蓝牙行仍打 `BlueZ`/`NetworkManager`/`D-Bus`；**而 ⑦-a 现在会扫 `modules/*/atoms/*/*.rs`**（剥行注释后 grep 器件/厂商禁词，重跑 0 命中——说明这些串不在禁词表内，不在扫到的文件内，或两者皆非） | **N-8 未结**；并登记敞口：**禁词表与运行期自述之间仍有缝**（表里没有 `BlueZ`/`NetworkManager`）→ 批L **L-10**（禁词表补栈名或改由后端名下发） |
| §3/§13 `handles`/`ops` 申报 | `atoms/wifi/assemble.toml` **已填**（批M MC-10 L-5，2026-10-13）：报 `Rr3Link`←`link_ifname` 句柄 + `link_report_period_ms`/`timeout_ms` ops；bt 早填为样板 | **L-5 已结** |

### §14.1 增量对表（2026-10-12，代码基线 `78625ae`；全仓总表见 [索引 §13.4](模块设计索引.md)）

**先订正上一版本册对表的一条错判**：§13/§14 曾写"`wifi.rs`/`bt.rs` 的 SURFACE 与 `composite.rs` 仍打
`BlueZ`/`NetworkManager`/`D-Bus`，N-8 未结"。**复验不成立**——`atoms/wifi/wifi.rs : SURFACE` 现文是
"链路面（**后端=动态，由 HAL 选择面交名**）"，`atoms/bt/bt.rs` 亦无栈名；这三个串今天只出现在
文档性注释里，不在 SURFACE/println。故：

| 项 | 复验实况 | 判定 / 新落点 |
|---|---|---|
| N-8 对外自述撤协议栈名 | 已撤，改由 `rr3_hal_backend_resolve` 交后端名 | **结**（"换协议栈不改码"这条在本单元已成立） |
| 批L L-10 禁词表补栈名 | 动机（对外面写死栈名）随 N-8 消失 | **降为可选清理**，归 0.0.8 W-B，不卡任何批 |
| N-9 / L-9 sim 假 AP 整表 | 表已**离开件面**，现居 `src/composite.rs : rr3_wifi_scan_result`（带 `backend:"sim"` 诚实标注） | 件面半边结；"整表迁 `rr-hal` sim 后端、与 `RR3_HAL_SIM_LINK_SSID_DEFAULT` 合流"半边归 **0.0.8 W-B** |
| N-5 真链路两半拆分 | 凭据持久化 = 可写码（NM 承载持久化 + `forget` 真删）；BlueZ = 不在 F 清单 | 持久化 → **MC-6**；bt → **MC-9**（建议 `net.bt` 改记 `placeholder`，索引 R-16 的"承诺 > 事实"案例） |
| N-3/N-2/N-6/N-7 | `Rr3SsidText`、`timeout_ms` 实装为 nmcli 超时、三键删净、载荷上移 | **结**（与 §13 判定一致，此处只补一句：本册 §10 表的"归批"列按已收口批次写，不再重开） |
