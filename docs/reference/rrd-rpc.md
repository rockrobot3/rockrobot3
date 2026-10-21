# rrd 上行 RPC 参考（JSON-RPC 2.0 over UDS）

> **代码即文档**：本档的方法白名单、参数约束、应答形状与错误码全部取自
> [`rrd/src/rpc.rs`](../../base/runtime/rrd/src/rpc.rs) 与
> [`rrd/src/conn.rs`](../../base/runtime/rrd/src/conn.rs) 的字面量。
> 断言口径要分档说清，免得改字面量时踩空：**错误码映射**
> （`rr3_rpc_status_for_register` / `rr3_rpc_status_for_publish_deny`）在
> `rpc.rs` 的 `register_error_maps_to_status_code` / `publish_deny_maps_to_status_code`
> 用例里逐枚举 `assert_eq` 判别值；`Rr3RegisterError` /
> `Rr3PublishDeny` 的 `Display` 文案在 `registry.rs` 的
> `register_error_display_is_verbatim` / `publish_deny_display_distinguishes_the_three_layers`
> 用例逐字 `assert_eq` 钉住，
> 集成侧（`tests/register_publish.rs`）则按 `contains` 子串区分"被哪一层拒的"。
> 启动横幅见 [`rrd-cli.md`](rrd-cli.md) §5.1；**非 Unix 退出文案 `RRD_UNSUPPORTED`
> 本 HEAD 没有任何 Rust 用例按字面断言它**（只有 `lib.rs` 的 `RRD_UNSUPPORTED` 常量定义、`main.rs` 的 `rr3_run`
> 使用），改它要同批查 README 与按字面匹配的脚本——详见 [`rrd-cli.md`](rrd-cli.md) 顶部。
> **改代码必须同批改本档对应字面量**。
> 接线进度属实况，唯一落点 [`../status.md`](../status.md)；本档只写接口契约。

要调 rrd，先读 §1 的帧层前提，再读 §3 的方法表。

---

## 1 帧层前提（不是 JSON 层，但决定你的请求收不收）

| 项 | 要求 | 出处 |
|---|---|---|
| 承载 | `Rr3Frame`：128B 定长头 + payload（`opcode` = `opcode::SEND`），payload 前 2 字节是 `Rr3PayloadType` 小端前缀 | `rr-ipc/src/protocol/body.rs`、`conn.rs` 的 `rr3_out_frame` |
| payload 类型 | 上行 `Rr3PayloadType::Request` 走 RPC（§3：模块四方法 + 控制六方法 `rr3.ctl.*`）、`Rr3PayloadType::Event` 走发布面（`rr3_publish`，见 §7）、`Rr3PayloadType::Response` 走 `rr3.call` 应答回路（见 §8）；注册后其余类型（`Notification` 冒充上行）才回 `Rr3StatusCode::Invalid` **且连接不断**（`conn.rs` 的 `rr3_serve_frame`） | `rpc.rs` 模块头注、`conn.rs` 的 `rr3_serve_frame` |
| `dst_task` | 必须是 `RR3_TASK_RRD`（值 `1`）。注册前违反 → 先回错误帧再断开；注册后违反 → 按协议违规收尾，**走死亡记账** | `conn.rs` 的 `rr3_handshake`、`conn.rs` 的 `rr3_serve_frame` |
| `src_task` | 由 rrd 分配，用户态**不得自设**（AOS-STD-IPC-024）。注册帧里它还没被分配，rrd 只把对端自报值当作拒答的 `dst_task` 回显用；注册后 `src_task` 必须等于分配值，伪造者**那一帧被拒、连接不断**（应答带回原 `id` + `Invalid`） | `conn.rs` 的 `rr3_handshake`、`conn.rs` 的 `rr3_serve_frame` |
| 应答寻址 | rrd 的应答是**点对点**：帧头 `dst_task` = 该连接的 task，`src_task` = `RR3_TASK_RRD`；`RR3_BROADCAST_DST_TASK`（值 `0`）只用于 rrd 的下行广播与发布转发 | `conn.rs` 的 `rr3_out_frame`、`daemon.rs` 的 `rr3_state_frame` |
| 并发形状 | 应答由 reader 线程写、广播由该连接的 writer 线程写，共用一把 `Mutex` 且一帧一次 `write_all` → **整帧原子，相对顺序不保证**。取锁只有一个单点：`lib.rs` 的 `pub(crate) fn rr3_lock`（`conn.rs`/`daemon.rs`/`supervisor.rs` 与 F3 起的日志环 `rr3_log` 共用它——日志环让 rr3_lock 在平台无关代码里也有了调用方，**原 `#[cfg(unix)]` 已摘除**），毒锁语义照代码自述——"中毒只可能来自 panic 的持锁者，继续用比弃连接/比整个守护停摆好"，`lock()` 的结果经 `unwrap_or_else` 交给 `PoisonError::into_inner()` 继续用；写侧那把 `Mutex<UnixStream>`、在册表那把 `Mutex<RrdRegistry>` 与日志环那把 `Mutex<VecDeque<String>>` 都走它，所以中毒口径全仓只登记这一处。客户端要按类型/字段挑帧，别按到达序号 | `conn.rs` 模块头注、`conn.rs` 的 `rr3_write`、`lib.rs`（`rr3_lock`/`rr3_log`/`rr3_log_tail`） |

## 2 JSON-RPC 信封约束

`rr3_rpc_parse_request`（`rpc.rs`）的逐条门禁。违反任一条即
`Err((Rr3StatusCode::Invalid as i32, 文案))`，**解析器不产别的码**，conn 层因此不重建第二套映射。

| 约束 | 不满足时的文案（逐字模板） | 出处 |
|---|---|---|
| payload 必须是 UTF-8 | `payload 非 UTF-8: {e}` | `rpc.rs` 的 `rr3_rpc_parse_request` |
| 必须是合法 JSON | `JSON-RPC 解析失败: {e}` | `rpc.rs` 的 `rr3_rpc_parse_request` |
| `jsonrpc` 必须**等于字符串** `2.0` | `jsonrpc 必须等于 "2.0"` | `rpc.rs` 的 `rr3_rpc_parse_request` |
| `id` 必须能按无符号整数取出（`as_u64`） | `缺 id（要求无符号整数）` | `rpc.rs` 的 `rr3_rpc_parse_request` |
| `method` 必须是字符串 | `缺 method（要求字符串）` | `rpc.rs` 的 `rr3_rpc_parse_request` |
| `method` 必须在十方法白名单内（模块四 + 控制 `rr3.ctl.*` 六，见 §3） | `未知方法 {method_str}`（**不许当 no-op 放行**） | `rpc.rs` 的 `rr3_rpc_parse_request` |
| `params` 可缺省 | 缺省时为 `Value::Null`，不是错误；各方法自己校验 | `rpc.rs` 的 `rr3_rpc_parse_request` |

解析产物是 `Rr3RpcRequest { id, method, params }`（`rpc.rs`）。
应答构造只有两个出口：`rr3_rpc_build_result`（`rpc.rs`）与
`rr3_rpc_build_error`（`rpc.rs`），形状分别是

```text
{"jsonrpc":"2.0","id":<id>,"result":{…}}
{"jsonrpc":"2.0","id":<id>,"error":{"code":<i32>,"message":"…"}}
```

成功应答不带 `error` 字段，错误应答不带 `result` 字段。解析失败时 conn 层用
`rr3_best_effort_id` 尽力取 JSON 的 `id`，取不到用 `0`（`conn.rs`）——
**客户端判失败要看 `code`，别只看 `id`**。

## 3 十个方法（白名单 = `rpc.rs` 的 `Rr3RpcMethod` 枚举）

**模块四方法**（已注册连接；各自语义见 §4/§6/§7/§8）：

| `method` | 变体 | `params` 要求 | 成功 `result` 形状 | 出处 |
|---|---|---|---|---|
| `rr3.register` | `Register` | `params.manifest`：非空字符串（manifest **全文文本**，不是路径）；`params.name` **不被读取**，身份取自 manifest 里的 `[manifest].name` | `{"src_task":<分配到的 u64>}` | `rpc.rs` 的 `Rr3RpcMethod::Register` 变体、`conn.rs` 的 `rr3_handshake` |
| `rr3.heartbeat` | `Heartbeat` | 无必填 `params`（`json!({})` 即可）；可选 `counters` 对象（`{字符串键: u64 值}`）——0.0.4 批B 起，模块把自报计数（如 `asr_ok`/`relay_on`）随心跳上行。**值 = 自上一拍以来的增量**（b3c 2026-10-21 钉死：客户端记已发水位每拍发差值，rrd 按 `模块名/键` 增量归并，`rr3.ctl.counters` 聚合表 = 生命周期累计；旧线上客户端曾发全量快照被当增量再收，计数二次方膨胀到 40 亿量级）。缺省 / 非对象 / 空对象都视为"不带"；非法值或空键**逐对静默跳过，不报错**（`rr3_rpc_heartbeat_counters`，`rpc.rs`）——心跳失败语义太重，脏计数不值一条拒绝 | `{"beat":"accepted"}` | `conn.rs` 的 `rr3_serve_request`（成功支；同一分支的失败面见 §5 `TimedOut` 行） |
| `rr3.unregister` | `Unregister` | 无必填 `params` | `{"removed":<bool>}` | `conn.rs` 的 `rr3_serve_request` |
| `rr3.call` | `Call` | `params.topic`：非空字符串（被调方须在 manifest `[rpc].exposes` 声明）；`params.args` 可缺省（缺省为 `Null`，被调方自己校验） | **不回 `result` 给调用方**——成功即转发，调用方的应答是被调方沿 trace_id 回路送回的 Response（见 §8） | `rpc.rs` 的 `rr3_rpc_call_params`、`conn.rs` 的 `rr3_serve_request`、`conn.rs` 的 `rr3_route_call` |

**控制六方法 `rr3.ctl.*`**（F3 / [ADR-0010](../process/adr/ADR-0010-控制通道与查询面.md)，只对**未注册**
的控制连接开放，`conn.rs` 的 `rr3_serve_ctl`）：

| `method` | 变体 | `params` 要求 | 成功 `result` 形状 | 出处 |
|---|---|---|---|---|
| `rr3.ctl.status` | `CtlStatus` | 无必填 | `{"modules":[{"name","task","phase"}], "denied_publish":<u64>}`——在册快照按 task 升序，与下行 state 的 phase 词表一致 | `conn.rs` 的 `rr3_serve_ctl` |
| `rr3.ctl.health` | `CtlHealth` | 无必填 | `{"modules":[{"name","task","phase","heartbeat_ms","watchdog_ms","since_beat_ms"}]}`——后三数是**判定用的数回显**（条目自己的配置 + `now - last_beat`） | `conn.rs` 的 `rr3_serve_ctl`、`registry.rs` 的 `rr3_reg_health_snapshot` |
| `rr3.ctl.logs` | `CtlLogs` | 无必填 | `{"lines":["rrd: …", …]}`——rrd 内存日志环（512 行环形，`rr3_log` 单点落环、不落盘）尾部副本 | `conn.rs` 的 `rr3_serve_ctl`、`lib.rs` 的 `rr3_log`/`rr3_log_tail` |
| `rr3.ctl.ota` | `CtlOta` | 无必填 | **无**——恒拒：`Permission`（-22，`AIRY_EPERM`）+ 文案点 M6（契约状态码是冻结集，不自造 `NotSupported`） | `conn.rs` 的 `rr3_serve_ctl` |
| `rr3.ctl.counters` | `CtlCounters` | 无必填 | `{"counters":{"<模块名>/<键>":<u64>, …, "boots":<u64>}}`——运维计数聚合快照（0.0.4 批B，Q1/Q2 裁定）：心跳可选 `counters` 按 `模块名/键` 前缀归并进在册表（`rr3_reg_counters_merge`，饱和加法），`boots` 是 rrd 自有裸键（每次启动自增，不走心跳）；持久化路径与语义见 [`rrd-cli.md`](rrd-cli.md) 的 `RR3_DATA_DIR`。**聚合与落盘在心跳 Rejected 之后也做**——运维账不跟判活走 | `conn.rs` 的 `rr3_serve_ctl`、`registry.rs` 的 `rr3_reg_counters_*` 三方法、`counters.rs` |
| `rr3.ctl.reboot` | `CtlReboot` | 无必填 | `{"reboot":true}`（`systemctl reboot` 已发起）——0.0.4 批C（Q3 裁定）：执行面**双重门**，`cfg(target_os = "linux")` **且** env `RR3_REBOOT_OK=1` 同时成立才真跑 `Command::new("systemctl").arg("reboot")`（解锁旋钮由 rr3.service 注入）；任一不成立 → `Permission`（-22）+ 文案 `reboot 执行面未解锁（需 cfg(linux) 且 env RR3_REBOOT_OK=1，缺省恒拒）`——恒拒路径可测（单测钉拒绝面），解锁面留装机环境，CI 拿不到真重启能力 | `conn.rs` 的 `rr3_reboot_unlocked`/`rr3_serve_ctl` |
| `rr3.ctl.wiring` | `CtlWiring` | 无必填 | `{"available":<bool>, "pins":[…], "domains":[…], "wires":[…], "axis":[…], "roles":[…], "checks":[{"item","pass","reason"}]}`——装配快照读口（批N4 件九笔4，白名单六→七 ctl）：boot 装载链产物（[pins]×wiring 全交叉判据含 ⑦-c1 额定加总族，批N5 件五笔3 起）的 OnceLock 副本，判据真值单点在 `assembly.rs` 的 `rr3_assembly_cross_checks`，面板只透传不重判；`--manifest` 调试路径 `available:false` | `conn.rs` 的 `rr3_serve_ctl`、`assembly.rs` 的 `rr3_assembly_render` |
| `rr3.ctl.contracts` | `CtlContracts` | 无必填 | `{"available":<bool>, "contracts":[{"file","sha256","bytes"}], "checks":[{"item","pass","reason"}]}`——契约完整性审计读口（批N5 件五笔4，白名单七→八 ctl）：boot 装载链验收后契约四册（topics/slots/roles/devices）sha256 记账快照＋自检时点重读对比的"齐全且 sha 对"判果一应答给出（缺册＝齐全破、sha 漂＝sha 对破），判据真值单点在 `contracts.rs` 的 `rr3_contracts_checks`；`--manifest` 调试路径（快照未在册）如实 `available:false` | `conn.rs` 的 `rr3_serve_ctl`、`contracts.rs` 的 `rr3_contracts_json` |

连接语义与模块四方法不同：ctl 连接**不注册、不进在册表、不受 watchdog 管**，
一问一答即断（rrd 应答后直接收掉这条连接，握手旁支在 `rr3_handshake` 第 6 步**之前**分流，
`conn.rs` 的 `rr3_handshake`）；应答按对端**自报**的 `src_task` 回（与注册期拒答同口径）。已注册模块
连接上行 `rr3.ctl.*` 一律 `Permission` 拒（`rr3_serve_request` 显式 OR 臂，`conn.rs` 的 `rr3_serve_request`），
**连接不断**；`rr3.ctl.*` 的畸形（非 Request / 非白名单方法）仍走 §4 既有拒绝门。
判定依据 `rpc.rs` 的 `Rr3RpcMethod::is_ctl` + 显式 OR 臂两处共同看护方法面。

取值助手对字符串参数的口径一致（`rpc.rs` 的 `param_str`）：非 `str`、空串、
缺键一律 `Invalid`，文案 `缺 params.{key}（要求非空字符串）`。
`rpc.rs` 的 `rr3_rpc_topic_param` 按同一口径取发布帧体里的 `topic`，线上调用点是
`conn.rs` 的发布入口 `rr3_publish`（见 §7）；`rpc.rs` 的 `rr3_rpc_call_params`
取点名帧的 `topic` + `args`，线上调用点是 `conn.rs` 的 `rr3_route_call`（见 §8）。

四个方法的差异在收尾：`rr3.unregister` 返回 `Rr3Flow::QuitClean`，条目直接
`rr3_reg_remove` 移出在册表、**不走** `restart_policy`（那是崩溃语义）；
EOF 同样算干净收尾（`conn.rs` 的 `rr3_serve_request`、`conn.rs` 的 `rr3_conn_serve`）。`rr3.register` / `rr3.heartbeat` /
`rr3.call` 的成功与业务拒绝都返回 `Rr3Flow::Keep`——连接保持，下一帧照常服务。

## 4 注册握手的额外门禁与错误码

`conn.rs` 的 `rr3_handshake` 按下表顺序检查；**每一次拒绝都先回错误帧再断开连接**
（只断连不回码等于让客户端挂在超时上，`conn.rs` 的 `rr3_handshake` 自述）。注册前的失败
**不做死亡记账、也不广播 state**（`conn.rs` 的 `rr3_conn_serve` 对 `rr3_handshake` 结果的 `?` 早返回，绕过 `rr3_conn_finish`）。

> **F3 例外（§3 控制六方法）**：第 6 步之前有一处分流——首帧方法 ∈ `rr3.ctl.*` 时直接进
> `rr3_serve_ctl`（`conn.rs` 的 `rr3_handshake`），不注册、即答即断，**不落进本表**。本表 1–5 步
> （读取 / 前缀 / `dst_task` / 类型 / RPC 信封）对 ctl 帧同样有效。

| 序 | 检查 | 码 | 文案要点 |
|---|---|---|---|
| 1 | 首帧能读出来 | 无从应答 | `首帧读取失败: {e:?}`（只进日志） |
| 2 | payload 前缀合法 | `Invalid` | `首帧 payload 前缀非法: {e:?}` |
| 3 | `dst_task` == `RR3_TASK_RRD` | `Invalid` | `首帧 dst_task 必须为 1，收到 {n}` |
| 4 | 类型是 `Request` | `Invalid` | `首帧 payload 类型必须是 Request，收到 {ty:?}` |
| 5 | RPC 信封合法（§2） | `Invalid` | `首帧 RPC 畸形: {msg}` |
| 6 | 方法必须是 `rr3.register`（`rr3.ctl.*` 已在更早处分流，见上） | `Invalid` | `注册前第一条 RPC 必须是 rr3.register，收到 {method}` |
| 7 | `params.manifest` 取值 | `Invalid` | `缺 params.manifest（要求非空字符串）` |
| 8 | manifest 文本 → `Rr3ManifestFacts` | `Invalid` | 点名字段的校验错，多条以 `; ` 连接（如 `heartbeat_ms`、`restart_policy`） |
| 9 | `rr3_registry_validate_manifest_text` 对主题集复验 | `Invalid` | 报告里的 `errors` 以 `; ` 连接 |
| 10 | `manifest.entry` 绝对性（`rr3_manifest_entry_is_absolute`，即 `/` 开头） | `Invalid` | `manifest.entry 必须是绝对路径，收到 {entry}` |
| 11 | 进在册表 `rr3_reg_add` | 见下表映射 | `Rr3RegisterError` 的 `Display` 文案逐字 |

第 8/9/10 步的存在理由：M2 的 manifest 从**线上**来，没有"所在目录"可依据，所以
`entry` 必须是绝对路径；在仓的 `modules/*/module.toml` 是**契约工件**而非注册输入
（装机时由安装步骤写绝对 `entry`），这条门禁不吃在仓夹具（`conn.rs` 的 `rr3_handshake`）。

## 5 错误码映射（本档的核心表）

判别值取自契约，不自造：`rr-ipc/src/protocol/header.rs` 的 `Rr3StatusCode` 枚举，
数值与名字见 [`../../contracts/envelope.schema.json`](../../contracts/envelope.schema.json) 的 `error_codes`。

| 码 | 判别值 | 契约名 | 触发面 | 出处 |
|---|---|---|---|---|
| `Invalid` | -2 | `AIRY_EINVAL` | §2 全部、§4 的 2–10 步、`src_task` 伪造、`Event` 之外的非 `Request` 上行、注册后重复 `rr3.register`、`Rr3PublishDeny::NotRegistered`（在册表第三道门，**经映射函数**得出）。发布面的**形状错**是另外两支：帧体非 JSON、体里缺/坏 `topic`——`conn.rs` 的 `rr3_publish` 直接回 `Invalid`，**既不套进 `Rr3PublishDeny` 枚举、也不计数**（`rr3_reg_note_denied` 只在归属被拒时调，`conn.rs` 的 `rr3_publish`） | `rpc.rs` 的 `rr3_rpc_status_for_register` / `rr3_rpc_status_for_publish_deny`、`conn.rs` 的 `rr3_serve_frame` |
| `Busy` | -16 | `AIRY_EBUSY` | `Rr3RegisterError::Duplicate`、`Rr3RegisterError::Capacity` | `rpc.rs` 的 `rr3_rpc_status_for_register` |
| `Permission` | -22 | `AIRY_EPERM` | `Rr3RegisterError::ReservedName`；`Rr3PublishDeny::ReservedNamespace`、`Rr3PublishDeny::ForeignPrefix`；**F3 起**：`rr3.ctl.ota` 恒拒（文案点 M6，`conn.rs` 的 `rr3_serve_ctl`）与已注册连接上行 `rr3.ctl.*`（`conn.rs` 的 `rr3_serve_request`，**不断连**）；**0.0.4 批C 起**：`rr3.ctl.reboot` 执行面未解锁（双重门，见 §3 该行） | `rpc.rs` 的 `rr3_rpc_status_for_register` / `rr3_rpc_status_for_publish_deny` |
| `TimedOut` | -1004 | `AIRY_ETIMEDOUT` | `rr3.heartbeat` 的 task 已过期或不在册：`该 task 已过期或不在册，需重新注册` | `conn.rs` 的 `rr3_serve_request` |
| `Ok` | 0 | `AIRY_EOK` | 不单独上帧：成功走 `result` 而不是 `error` | `header.rs` 的 `Rr3StatusCode::Ok` 变体 |

两个映射函数是**唯一**真值点，别在调用方重写一遍：

- `rr3_rpc_status_for_register`（`rpc.rs`）——`conn.rs` 的 `rr3_handshake` 是唯一生产调用点。
- `rr3_rpc_status_for_publish_deny`（`rpc.rs`）——三种拒法里
  `ReservedNamespace` / `ForeignPrefix` 是权限、`NotRegistered` 是非法，**都不是忙**；
  线上调用点在 `conn.rs` 的发布入口 `rr3_publish`（见 §7）。

`Rr3RegisterError` 与 `Rr3PublishDeny` 的 `Display` 文案逐字，登记在
[`rrd-registry.md`](rrd-registry.md)；集成侧按这些子串区分"被哪一层拒的"。

## 6 心跳的方向：别把下行广播当上行喂

`rpc.rs` 模块头注是这条的权威表述，逐字要点：

- `rr3-runtime/system/heartbeat` 与 `rr3-runtime/module/state` 是 rrd 的**下行广播**，
  **不是**模块的上行心跳。
- **模块要喂狗只能调 `rr3.heartbeat` RPC**（§3）。发布主题喂不了狗：`rr3-runtime/*`
  是内建命名空间（`topics_load.rs` 的 `RR3_RESERVED_OWNER` 常量，值 `rr3-runtime`），
  任何模块发它都会被 `rr3_reg_publish_check` 的第一道门判成
  `Rr3PublishDeny::ReservedNamespace`；用这个名字注册则直接
  `Rr3RegisterError::ReservedName` → `Permission`。
- 契约侧的登记（[`../../contracts/topics.toml`](../../contracts/topics.toml)，本档不抄表）：
  两条主题 `kind` 都是 `notification`、`owner` 都是 `rr3-runtime`，
  `rr3-runtime/system/heartbeat` 的注记写明 `dst_task=0` 广播。

下行 `rr3-runtime/module/state` 的帧形状（`daemon.rs` 的 `rr3_state_frame`）：
payload 类型 `Notification`（`daemon.rs` 的 `rr3_state_frame`），`src_task` = `RR3_TASK_RRD`（`daemon.rs` 的 `rr3_state_frame`），
`dst_task` = `RR3_BROADCAST_DST_TASK`（`daemon.rs` 的 `rr3_state_frame`），
体是 `{"topic":"rr3-runtime/module/state","module":<name>,"phase":<str>,"src_task":<u64>}`。
`phase` 词表 = `registered` / `running` / `degraded` / `failed`（`Rr3Phase` 的 `as_str`，四个词面由
`registry.rs` 的 `phase_names_are_the_wire_vocabulary` 用例逐条 `assert_eq` 钉住）
**再加一条** `unregistered`：连接干净收尾时由 `conn.rs` 的 `rr3_conn_finish` 现算，不属 `Rr3Phase`。

**谁发 state**：构造帧的函数只有一个（`daemon.rs` 的 `rr3_state_frame`），调用它的是五个点——握手
（成功补一条 `registered` 与向后到的连接补发在册快照，`conn.rs` 的 `rr3_handshake`）、Promoted 恢复
（`running` 与 undegrade 补发，`conn.rs` 的 `rr3_serve_frame`）、连接自己收尾（`conn.rs` 的
`rr3_conn_finish`）、watchdog 到期判死（`daemon.rs` 的 `rr3_spawn_watchdog`）、supervisor 收尸
（子进程退出相位与 degrade 传播，`supervisor.rs` 的 `rr3_absorb_exit`）。其中退场与判死这条是
`rr3_conn_finish` 自述所说的"**三条公开路**"之二（第三条是子进程退出码，走 supervisor）：
连接收尾那处按 `clean_exit` 现算阶段名（`unregistered` / `failed`，`conn.rs` 的 `rr3_conn_finish`），watchdog 那处的
阶段名取 `Rr3Phase::Failed.as_str()` 而不写字面量（`daemon.rs` 的 `rr3_spawn_watchdog` 及其注释）。一条连接的退出只发
一条 state、一次判死只回报一条，靠的是两处各自 `rr3_reg_fanout` 恰好一次（`conn.rs` 的 `rr3_conn_finish`、
`daemon.rs` 的 `rr3_spawn_watchdog`）且 `rr3_reg_expire` 跳过已 `Failed` 的条目（`registry.rs` 的 `rr3_reg_expire`），所以判死过的
模块不会被第二拍再回报一次——线上由 `watchdog_broadcasts_exactly_one_failed_state`
（`base/runtime/rrd/tests/e2e_watchdog.rs`）钉住。

`rr3-runtime/system/heartbeat` 这条本 HEAD **没有任何代码在发**：rrd 目前只下行
`rr3-runtime/module/state`；`rr3.heartbeat` 的应答是点对点 `result`，不是广播。
"谁在什么时候发心跳主题"属实况，见 [`../status.md`](../status.md)。

## 7 发布面（`Rr3PayloadType::Event` 已接到线上）

上行 `Event` 帧不再只是预留：`conn.rs` 的 `rr3_serve_frame` 把它分派给 `rr3_publish`（`conn.rs`），
三层归属判定就此进入线上流量。曾经"接口已冻结、生产调用点缺位"的
三个入口现在都有线上调用点（形状以本档与 [`rrd-registry.md`](rrd-registry.md) §8 为准；
"接到了哪一段"属实况，以 [`../status.md`](../status.md) 为准）：

| 入口 | 冻结的形状 | 线上调用点 |
|---|---|---|
| `rr3_rpc_topic_param` | 取发布帧体的 `topic`，非空字符串口径同 `param_str`（`rpc.rs`） | `conn.rs` 的 `rr3_publish` |
| `RrdRegistry::rr3_reg_publish_check` | `(sender_name, topic, topics) -> Result<(), Rr3PublishDeny>` | `conn.rs` 的 `rr3_publish` |
| `rr3_rpc_status_for_publish_deny` | `&Rr3PublishDeny -> Rr3StatusCode`（映射见 §5） | `conn.rs` 的 `rr3_publish` |

`rr3_publish` 的处理序：取 `topic`（缺键或类型错 → 回 `Invalid` + 保留连接，**不按崩溃收尾、
且不计数**，`conn.rs` 的 `rr3_publish`）→ `rr3_reg_publish_check` 判归属，被拒则 `rr3_reg_note_denied` 计数并按
`rr3_rpc_status_for_publish_deny` 回错误帧 + `Rr3Flow::Keep`（`conn.rs` 的 `rr3_publish`，**不断连**：
发布越权是数据面问题，不带走控制面，与 `src_task` 伪造同一口径）→ 放行则保留原体转发，
头改写成发布者 task + `RR3_BROADCAST_DST_TASK`，`rr3_reg_fanout(&frame, me.task)` 排除发布者自己
（`conn.rs` 的 `rr3_publish`）。

**低电兜底钩子（T4b，0.0.2 方案 Q4 裁定）挂在转发点之后**：原帧已照常扇出后，若 `topic == rr3-power/battery/low`
（`conn.rs` 的 `RR3_BATTERY_LOW_TOPIC` 常量），调 `rr3_reg_low_power_raise`（[`rrd-registry.md`](rrd-registry.md) §10）——
上升沿则取已解析体里的 `percent` / `voltage_mv` 透传构造 `rr3-runtime/system/power` 帧（`daemon.rs` 的
`rr3_system_power_frame`，缺键按 0），以 `src = RR3_TASK_RRD` 再扇出一次并打 `低电兜底触发：battery/low percent=… voltage_mv=… → 置全局低电旗、停止新 spawn，广播 rr3-runtime/system/power 到 {n} 个连接`；
非上升沿只打 `低电兜底已生效：重复 battery/low 不重复广播（同一次低电段只点名一次）`。**兜底是额外动作，
不截断事件流**——battery/low 本身在上一步已作为普通事件转发；supervisor 的 spawn 漏斗据旗拒新 spawn。
**解除路径（批N5 件二笔3，全案 §17 运维留痕）**：同一钩子点若 `topic == rr3-power/battery/ok`（`RR3_BATTERY_OK_TOPIC`），
调 `rr3_reg_low_power_lower`——下降沿则同式构造 `reason = "battery-ok"` 的点名帧扇出并打
`低电兜底解除：battery/ok percent=… voltage_mv=… → 清全局低电旗、恢复新 spawn，…`；旗未置位时只打
`低电兜底本未触发：重复 battery/ok 不重复广播（旗未置位无从解除）`。两个沿一个对：置旗听 low、清旗听 ok，
同一次低电段各点名恰一次。

一条由 `conn.rs` 承担的调用方义务：`sender_name` 取的是**这条连接注册时确立的在册身份**
`me.name`（`Rr3Peer` 随握手成功才构造，`conn.rs` 的 `rr3_handshake`），转发前帧上的 `src_task` 已被强制等于
`me.task`（`conn.rs` 的 `rr3_serve_frame`），所以它既**不取自帧体、也不取自注册报文里自称的 `name` 字段**——
这正是 `registry.rs` 的 `rr3_reg_publish_check` 文档注释里那句"唯一防线建在调用侧"在线上成立的方式。`registry.rs` 那段注释
设想的取名途径 `rr3_reg_name_of(src_task)` 本 HEAD 仍无生产调用点：`rr3_publish` 用的是注册时
缓存的 `me.name`，二者都是"连接持有的身份"、都不来自帧体，语义等价。判定函数本身是关联函数、
看不见在册表，替不了调用方保证这件事。

## 8 模块间点名 `rr3.call`（M3，请求路由 + 应答回路）

`rr3.call` 让模块 A 点名调用模块 B 的一个 `[rpc].exposes` 主题。rrd 在中间只做**路由**，
不做执行：请求沿在册表转发给被调方，应答沿帧头 `trace_id` 原路改道回调用方。

**trace_id 的 rock3 线上语义**（字节布局不动，语义登记见
[`../../contracts/README.md`](../../contracts/README.md) 演进登记 2026-09-30 条）：
`0` = 非代理（普通发布 / 直发帧的缺省值）；**`rr3.call` 转发帧的 `trace_id` = 调用方 task**。
被调方应答必须把 `trace_id` 原样带回——客户端侧的成对助手是
`rr-bus/src/client.rs` 的 `rr3_reply_call`（它拒绝 `trace_id = 0` 的转发帧，回 `Err`）。

请求方向（`conn.rs` 的 `rr3_route_call`；分派点 `conn.rs` 的 `rr3_serve_request`）：

1. `rpc.rs` 的 `rr3_rpc_call_params` 取 `params.topic` + `params.args`；畸形 → 回
   `Invalid` + `Keep`（**不断连**，下同）。
2. **查表、构帧、投递在同一把在册表锁内完成**（`conn.rs` 的 `rr3_route_call`）——避免"查到 callee、
   锁一放它就死了"的窗口里构出一帧注定无处投递的转发。查表用
   `registry.rs` 的 `rr3_reg_callee_for`：按 `[rpc].exposes` 找在册被调方。
3. 三种拒法，全部只回错误帧（`Invalid`）不断连：
   - **无人在册**：`没有在册模块 exposes 主题 {topic}（被调方须在 manifest 的 [rpc].exposes 里声明）`；
   - **自呼**：`主题 {topic} 的唯一被调方就是调用方自己，拒绝自呼`；
   - **投递前退场**：查表时还在、`registry.rs` 的 `rr3_reg_send_to` 投递时条目已不在册 →
     `主题 {topic} 的被调方在投递前退场`。
4. 成功转发帧的头三处改写：`src_task` = 调用方 task、`dst_task` = 被调方 task、
   `trace_id` = 调用方 task；**body 原样**（JSON-RPC `id` 是被调方应答配对的凭据，rrd 不碰）。

应答方向（`conn.rs` 的 `rr3_route_response`；分派点 `conn.rs` 的 `rr3_serve_frame`）：上行 `Response`
帧的 `trace_id` 就是投递地址——`0` 回 `Invalid`（`上行 Response 缺回路：trace_id 必须为
调用方 task（0 = 无从投递）`）；调用方已不在册（退场 / 被判死摘册）回 `Invalid`
（`应答的调用方 task={trace} 已不在册，无处投递`）；否则 body 原样、只把 `dst_task`
改写成 `trace_id` 定向回投。**被调方的业务性失败走 `Response.result` 槽**
（如 `{"error":"缺 params.text（要求非空字符串）"}`），JSON-RPC `error` 槽留给 rrd 的
路由拒绝——业务失败与传输失败在线上是两个槽位。

模块侧的完整闭环（连接 / 注册 / 收点名 / 应答）由 `rr-bus` 同步客户端承载
（`base/runtime/rr-bus/src/client.rs`，模块与 rrd 之间唯一的标准说话方式）；线级钉住
"头三处改写 + 三种拒法不断连"的是 `base/runtime/rrd/tests/module_call.rs`，
三真进程端到端（TTS / 表情 / 坏参数折 `result.error`）的是
`base/runtime/rrd/tests/e2e_audio_chain.rs`。

## 9 相关文档

- 在册表阶段机与发布归属四道门：[`rrd-registry.md`](rrd-registry.md)
- 启动参数、横幅、退出码：[`rrd-cli.md`](rrd-cli.md)
- 帧与字节布局：[`../../contracts/README.md`](../../contracts/README.md) 与
  [`../architecture/architecture.md`](../architecture/architecture.md) §6.1–§6.2
