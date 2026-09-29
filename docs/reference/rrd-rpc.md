# rrd 上行 RPC 参考（JSON-RPC 2.0 over UDS）

> **代码即文档**：本档的方法白名单、参数约束、应答形状与错误码全部取自
> [`rrd/src/rpc.rs`](../../base/runtime/rrd/src/rpc.rs) 与
> [`rrd/src/conn.rs`](../../base/runtime/rrd/src/conn.rs) 的字面量。
> 断言口径要分档说清，免得改字面量时踩空：**错误码映射**
> （`rr3_rpc_status_for_register` / `rr3_rpc_status_for_publish_deny`）在
> `rpc.rs:224-260` 的单元用例里逐枚举 `assert_eq` 判别值；`Rr3RegisterError` /
> `Rr3PublishDeny` 的 `Display` 文案在 `registry.rs:528-563` 逐字 `assert_eq` 钉住，
> 集成侧（`tests/register_publish.rs`）则按 `contains` 子串区分"被哪一层拒的"。
> 启动横幅见 [`rrd-cli.md`](rrd-cli.md) §5.1；**非 Unix 退出文案 `RRD_UNSUPPORTED`
> 本 HEAD 没有任何 Rust 用例按字面断言它**（只有 `lib.rs:27` 定义、`main.rs:13/99`
> 使用），改它要同批查 README 与按字面匹配的脚本——详见 [`rrd-cli.md`](rrd-cli.md) 顶部。
> **改代码必须同批改本档对应字面量**。
> 接线进度属实况，唯一落点 [`../status.md`](../status.md)；本档只写接口契约。

要调 rrd，先读 §1 的帧层前提，再读 §3 的方法表。

---

## 1 帧层前提（不是 JSON 层，但决定你的请求收不收）

| 项 | 要求 | 出处 |
|---|---|---|
| 承载 | `Rr3Frame`：128B 定长头 + payload（`opcode` = `opcode::SEND`），payload 前 2 字节是 `Rr3PayloadType` 小端前缀 | `rr-ipc/src/protocol/body.rs`、`conn.rs:74-88` |
| payload 类型 | 上行 `Rr3PayloadType::Request` 走三方法 RPC、`Rr3PayloadType::Event` 走发布面（`rr3_publish`，见 §7）；注册后其余类型（`Notification` / `Response` 冒充上行）才回 `Rr3StatusCode::Invalid` **且连接不断**（`conn.rs:289-298`） | `rpc.rs:3-5`、`conn.rs:284-299` |
| `dst_task` | 必须是 `RR3_TASK_RRD`（值 `1`）。注册前违反 → 先回错误帧再断开；注册后违反 → 按协议违规收尾，**走死亡记账** | `conn.rs:153-161`、`conn.rs:268-270` |
| `src_task` | 由 rrd 分配，用户态**不得自设**（AOS-STD-IPC-024）。注册帧里它还没被分配，rrd 只把对端自报值当作拒答的 `dst_task` 回显用；注册后 `src_task` 必须等于分配值，伪造者**那一帧被拒、连接不断**（应答带回原 `id` + `Invalid`） | `conn.rs:140`、`conn.rs:272-283` |
| 应答寻址 | rrd 的应答是**点对点**：帧头 `dst_task` = 该连接的 task，`src_task` = `RR3_TASK_RRD`；`RR3_BROADCAST_DST_TASK`（值 `0`）只用于 rrd 的下行广播与发布转发 | `conn.rs:73-98`、`daemon.rs:50-58` |
| 并发形状 | 应答由 reader 线程写、广播由该连接的 writer 线程写，共用一把 `Mutex` 且一帧一次 `write_all` → **整帧原子，相对顺序不保证**。客户端要按类型/字段挑帧，别按到达序号 | `conn.rs:13-15`、`conn.rs:61-71` |

## 2 JSON-RPC 信封约束

`rr3_rpc_parse_request`（`rpc.rs:46-64`）的逐条门禁。违反任一条即
`Err((Rr3StatusCode::Invalid as i32, 文案))`，**解析器不产别的码**，conn 层因此不重建第二套映射。

| 约束 | 不满足时的文案（逐字模板） | 出处 |
|---|---|---|
| payload 必须是 UTF-8 | `payload 非 UTF-8: {e}` | `rpc.rs:47-48` |
| 必须是合法 JSON | `JSON-RPC 解析失败: {e}` | `rpc.rs:49-50` |
| `jsonrpc` 必须**等于字符串** `2.0` | `jsonrpc 必须等于 "2.0"` | `rpc.rs:51-53` |
| `id` 必须能按无符号整数取出（`as_u64`） | `缺 id（要求无符号整数）` | `rpc.rs:54-56` |
| `method` 必须是字符串 | `缺 method（要求字符串）` | `rpc.rs:57-59` |
| `method` 必须在三方法白名单内 | `未知方法 {method_str}`（**不许当 no-op 放行**） | `rpc.rs:60-62` |
| `params` 可缺省 | 缺省时为 `Value::Null`，不是错误；各方法自己校验 | `rpc.rs:63` |

解析产物是 `Rr3RpcRequest { id, method, params }`（`rpc.rs:38-43`）。
应答构造只有两个出口：`rr3_rpc_build_result`（`rpc.rs:83-86`）与
`rr3_rpc_build_error`（`rpc.rs:88-95`），形状分别是

```text
{"jsonrpc":"2.0","id":<id>,"result":{…}}
{"jsonrpc":"2.0","id":<id>,"error":{"code":<i32>,"message":"…"}}
```

成功应答不带 `error` 字段，错误应答不带 `result` 字段。解析失败时 conn 层用
`rr3_best_effort_id` 尽力取 JSON 的 `id`，取不到用 `0`（`conn.rs:100-106`）——
**客户端判失败要看 `code`，别只看 `id`**。

## 3 三个方法（白名单 = `Rr3RpcMethod`，`rpc.rs:13-36`）

| `method` | 变体 | `params` 要求 | 成功 `result` 形状 | 出处 |
|---|---|---|---|---|
| `rr3.register` | `Register` | `params.manifest`：非空字符串（manifest **全文文本**，不是路径）；`params.name` **不被读取**，身份取自 manifest 里的 `[manifest].name` | `{"src_task":<分配到的 u64>}` | `rpc.rs:75-77`、`conn.rs:192-195`、`conn.rs:234-241` |
| `rr3.heartbeat` | `Heartbeat` | 无必填 `params`（`json!({})` 即可） | `{"beat":"accepted"}` | `conn.rs:404-407` |
| `rr3.unregister` | `Unregister` | 无必填 `params` | `{"removed":<bool>}` | `conn.rs:413-417` |

取值助手对字符串参数的口径一致（`param_str`，`rpc.rs:66-73`）：非 `str`、空串、
缺键一律 `Invalid`，文案 `缺 params.{key}（要求非空字符串）`。
`rr3_rpc_topic_param`（`rpc.rs:79-81`）按同一口径取发布帧体里的 `topic`，线上调用点是
发布入口 `rr3_publish`（`conn.rs:333`，见 §7）。

三个方法的差异在收尾：`rr3.unregister` 返回 `Rr3Flow::QuitClean`，条目直接
`rr3_reg_remove` 移出在册表、**不走** `restart_policy`（那是崩溃语义）；
EOF 同样算干净收尾（`conn.rs:413-418`、`conn.rs:470`）。

## 4 注册握手的额外门禁与错误码

`rr3_handshake`（`conn.rs:128-259`）按下表顺序检查；**每一次拒绝都先回错误帧再断开连接**
（只断连不回码等于让客户端挂在超时上，`conn.rs:116-121`）。注册前的失败
**不做死亡记账、也不广播 state**（`conn.rs:458` 对 `rr3_handshake` 结果的 `?` 早返回，绕过 `conn.rs:480` 的 `rr3_conn_finish`）。

| 序 | 检查 | 码 | 文案要点 |
|---|---|---|---|
| 1 | 首帧能读出来 | 无从应答 | `首帧读取失败: {e:?}`（只进日志） |
| 2 | payload 前缀合法 | `Invalid` | `首帧 payload 前缀非法: {e:?}` |
| 3 | `dst_task` == `RR3_TASK_RRD` | `Invalid` | `首帧 dst_task 必须为 1，收到 {n}` |
| 4 | 类型是 `Request` | `Invalid` | `首帧 payload 类型必须是 Request，收到 {ty:?}` |
| 5 | RPC 信封合法（§2） | `Invalid` | `首帧 RPC 畸形: {msg}` |
| 6 | 方法必须是 `rr3.register` | `Invalid` | `注册前第一条 RPC 必须是 rr3.register，收到 {method}` |
| 7 | `params.manifest` 取值 | `Invalid` | `缺 params.manifest（要求非空字符串）` |
| 8 | manifest 文本 → `Rr3ManifestFacts` | `Invalid` | 点名字段的校验错，多条以 `; ` 连接（如 `heartbeat_ms`、`restart_policy`） |
| 9 | `rr3_registry_validate_manifest_text` 对主题集复验 | `Invalid` | 报告里的 `errors` 以 `; ` 连接 |
| 10 | `manifest.entry` 绝对性（`rr3_manifest_entry_is_absolute`，即 `/` 开头） | `Invalid` | `manifest.entry 必须是绝对路径，收到 {entry}` |
| 11 | 进在册表 `rr3_reg_add` | 见下表映射 | `Rr3RegisterError` 的 `Display` 文案逐字 |

第 8/9/10 步的存在理由：M2 的 manifest 从**线上**来，没有"所在目录"可依据，所以
`entry` 必须是绝对路径；在仓的 `modules/*/module.toml` 是**契约工件**而非注册输入
（装机时由安装步骤写绝对 `entry`），这条门禁不吃在仓夹具（`conn.rs:206-217`）。

## 5 错误码映射（本档的核心表）

判别值取自契约，不自造：`Rr3StatusCode`（`rr-ipc/src/protocol/header.rs:103-109`），
数值与名字见 [`../../contracts/envelope.schema.json`](../../contracts/envelope.schema.json) 的 `error_codes`。

| 码 | 判别值 | 契约名 | 触发面 | 出处 |
|---|---|---|---|---|
| `Invalid` | -2 | `AIRY_EINVAL` | §2 全部、§4 的 2–10 步、`src_task` 伪造、`Event` 之外的非 `Request` 上行、注册后重复 `rr3.register`、`Rr3PublishDeny::NotRegistered`（含发布帧体缺/坏 `topic` 的形状错，`conn.rs:333-336`） | `rpc.rs:97-111`、`conn.rs:394-401` |
| `Busy` | -16 | `AIRY_EBUSY` | `Rr3RegisterError::Duplicate`、`Rr3RegisterError::Capacity` | `rpc.rs:99` |
| `Permission` | -22 | `AIRY_EPERM` | `Rr3RegisterError::ReservedName`；`Rr3PublishDeny::ReservedNamespace`、`Rr3PublishDeny::ForeignPrefix` | `rpc.rs:100-110` |
| `TimedOut` | -1004 | `AIRY_ETIMEDOUT` | `rr3.heartbeat` 的 task 已过期或不在册：`该 task 已过期或不在册，需重新注册` | `conn.rs:408-409` |
| `Ok` | 0 | `AIRY_EOK` | 不单独上帧：成功走 `result` 而不是 `error` | `header.rs:104` |

两个映射函数是**唯一**真值点，别在调用方重写一遍：

- `rr3_rpc_status_for_register`（`rpc.rs:97-102`）——`conn.rs:225` 是唯一生产调用点。
- `rr3_rpc_status_for_publish_deny`（`rpc.rs:104-111`）——三种拒法里
  `ReservedNamespace` / `ForeignPrefix` 是权限、`NotRegistered` 是非法，**都不是忙**；
  线上调用点在发布入口 `rr3_publish`（`conn.rs:354`，见 §7）。

`Rr3RegisterError` 与 `Rr3PublishDeny` 的 `Display` 文案逐字，登记在
[`rrd-registry.md`](rrd-registry.md)；集成侧按这些子串区分"被哪一层拒的"。

## 6 心跳的方向：别把下行广播当上行喂

`rpc.rs:3-5` 的头注是这条的权威表述，逐字要点：

- `rr3-runtime/system/heartbeat` 与 `rr3-runtime/module/state` 是 rrd 的**下行广播**，
  **不是**模块的上行心跳。
- **模块要喂狗只能调 `rr3.heartbeat` RPC**（§3）。发布主题喂不了狗：`rr3-runtime/*`
  是内建命名空间（常量 `RR3_RESERVED_OWNER`，值 `rr3-runtime`，`topics_load.rs:17`），
  任何模块发它都会被 `rr3_reg_publish_check` 的第一道门判成
  `Rr3PublishDeny::ReservedNamespace`；用这个名字注册则直接
  `Rr3RegisterError::ReservedName` → `Permission`。
- 契约侧的登记（[`../../contracts/topics.toml`](../../contracts/topics.toml)，本档不抄表）：
  两条主题 `kind` 都是 `notification`、`owner` 都是 `rr3-runtime`，
  `rr3-runtime/system/heartbeat` 的注记写明 `dst_task=0` 广播。

下行 `rr3-runtime/module/state` 的帧形状（`rr3_state_frame`，`daemon.rs:38-61`）：
payload 类型 `Notification`，`src_task` = `RR3_TASK_RRD`，`dst_task` = `RR3_BROADCAST_DST_TASK`，
体是 `{"topic":"rr3-runtime/module/state","module":<name>,"phase":<str>,"src_task":<u64>}`。
`phase` 词表 = `registered` / `running` / `degraded` / `failed`（`Rr3Phase` 的 `as_str`）
**再加一条** `unregistered`：连接干净收尾时由 `conn.rs:426` 现算，不属 `Rr3Phase`；
一条连接的退出只发一条 state（`conn.rs:423-436`）。

`rr3-runtime/system/heartbeat` 这条本 HEAD **没有任何代码在发**：rrd 目前只下行
`rr3-runtime/module/state`；`rr3.heartbeat` 的应答是点对点 `result`，不是广播。
"谁在什么时候发心跳主题"属实况，见 [`../status.md`](../status.md)。

## 7 发布面（`Rr3PayloadType::Event` 已接到线上）

上行 `Event` 帧不再只是预留：`rr3_serve_frame` 把它分派给 `rr3_publish`（`conn.rs:286`、
`conn.rs:314-382`），三层归属判定就此进入线上流量。曾经"接口已冻结、生产调用点缺位"的
三个入口现在都有线上调用点（形状以本档与 [`rrd-registry.md`](rrd-registry.md) §8 为准；
"接到了哪一段"属实况，以 [`../status.md`](../status.md) 为准）：

| 入口 | 冻结的形状 | 线上调用点 |
|---|---|---|
| `rr3_rpc_topic_param` | 取发布帧体的 `topic`，非空字符串口径同 `param_str`（`rpc.rs:79-81`） | `conn.rs:333` |
| `RrdRegistry::rr3_reg_publish_check` | `(sender_name, topic, topics) -> Result<(), Rr3PublishDeny>` | `conn.rs:341` |
| `rr3_rpc_status_for_publish_deny` | `&Rr3PublishDeny -> Rr3StatusCode`（映射见 §5） | `conn.rs:354` |

`rr3_publish` 的处理序：取 `topic`（缺键或类型错 → 回 `Invalid` + 保留连接，**不按崩溃收尾、
且不计数**，`conn.rs:320-337`）→ `rr3_reg_publish_check` 判归属，被拒则 `rr3_reg_note_denied` 计数并按
`rr3_rpc_status_for_publish_deny` 回错误帧 + `Rr3Flow::Keep`（`conn.rs:340-357`，**不断连**：
发布越权是数据面问题，不带走控制面，与 `src_task` 伪造同一口径）→ 放行则保留原体转发，
头改写成发布者 task + `RR3_BROADCAST_DST_TASK`，`rr3_reg_fanout(&frame, me.task)` 排除发布者自己
（`conn.rs:364-380`）。

一条由 `conn.rs` 承担的调用方义务：`sender_name` 取的是**这条连接注册时确立的在册身份**
`me.name`（`Rr3Peer` 随握手成功才构造，`conn.rs:242`），转发前帧上的 `src_task` 已被强制等于
`me.task`（`conn.rs:272-283`），所以它既**不取自帧体、也不取自注册报文里自称的 `name` 字段**——
这正是 `registry.rs:298-303` 那句"唯一防线建在调用侧"在线上成立的方式。`registry.rs` 那段注释
设想的取名途径 `rr3_reg_name_of(src_task)` 本 HEAD 仍无生产调用点：`rr3_publish` 用的是注册时
缓存的 `me.name`，二者都是"连接持有的身份"、都不来自帧体，语义等价。判定函数本身是关联函数、
看不见在册表，替不了调用方保证这件事。

## 8 相关文档

- 在册表阶段机与发布归属四道门：[`rrd-registry.md`](rrd-registry.md)
- 启动参数、横幅、退出码：[`rrd-cli.md`](rrd-cli.md)
- 帧与字节布局：[`../../contracts/README.md`](../../contracts/README.md) 与
  [`../architecture/architecture.md`](../architecture/architecture.md) §6.1–§6.2
