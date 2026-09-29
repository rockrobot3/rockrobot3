# rrd 在册表参考（阶段机 · 退避 · 发布归属）

> **代码即文档**：本档的阶段名、常量值、门序与拒绝文案全部取自
> [`rrd/src/registry.rs`](../../base/runtime/rrd/src/registry.rs) 的字面量，
> 其中 `Rr3Phase` 的 `as_str` 四个词与 `Rr3RegisterError` / `Rr3PublishDeny` 的
> `Display` 文案是**线上词表**（下行 state 帧与集成侧按字面比对）。断言口径分档：
> 这四组 `Display` 在 `registry.rs:528-563` 的单元用例里逐字 `assert_eq` 钉住，集成侧
> （`tests/register_publish.rs`）按 `contains` 子串区分"被哪一层拒的"；错误码映射
> （`rr3_rpc_status_for_register` / `rr3_rpc_status_for_publish_deny`）在 `rpc.rs:224-260`
> 逐枚举断言判别值。**两点提醒**：启动横幅（`rr3_banner`）只是 `contains` 片段、并非逐字；
> 非 Unix 退出文案 `RRD_UNSUPPORTED` 本 HEAD **没有任何 Rust 用例按字面断言它**——改这些
> 字面量都要同批查本文与相关 README、脚本。**改代码必须同批改本档对应字面量**。
> 接线进度属实况，唯一落点 [`../status.md`](../status.md)；本档只写接口契约。

---

## 1 类型与共享方式

| 项 | 事实 | 出处 |
|---|---|---|
| 类型 | `RrdRegistry`，字段 `modules`（`BTreeMap` 名→条目）、`by_task`（task→名）、`next_task`、`restart_base_ms`、`denied_publish` | `registry.rs:152-159` |
| 构造 | `RrdRegistry::new(restart_base_ms)`，`next_task` 起点 = `RR3_TASK_FIRST_MODULE`，`denied_publish` = 0 | `registry.rs:161-170` |
| 平台 | 平台无关：只碰 `BTreeMap` / `mpsc` / `u64` 单调毫秒；UDS 与进程派生在 `conn.rs` / `daemon.rs`（整体 `#[cfg(unix)]`） | `registry.rs:1-6` |
| 共享 | 守护进程持一份 `Arc<Mutex<RrdRegistry>>`，每连接一份 `Rr3ConnCtx` 引用同一把锁 | `daemon.rs:74-88` |
| 取锁 | `rr3_lock`：写侧锁中毒只可能来自 panic 的 writer，**继续用比弃连接好**（`into_inner`） | `conn.rs:61-64` |
| task 分配 | `next_task` 用 `saturating_add(1)` 单调递增；0 与 1 永不出现 | `registry.rs:230-231` |

## 2 常量表

| 常量 | 值 | 含义 | 出处 |
|---|---|---|---|
| `RR3_TASK_RRD` | 1 | rrd 自己的 task；上行 `dst_task` 必须是它 | `rr-bus/src/lib.rs:12` |
| `RR3_TASK_FIRST_MODULE` | 2 | 模块 task 的分配起点 | `rr-bus/src/lib.rs:15` |
| `RR3_BROADCAST_DST_TASK` | 0 | 只用于 rrd 下行广播；**不是任何模块的 task** | `rr-bus/src/lib.rs:9` |
| `RR3_REG_MAX_MODULES` | 16 | 在册模块上限；`Display` 文案里的"上限 16"就是它 | `registry.rs:25` |
| `RR3_RESTART_BACKOFF` | `[1, 2, 4, 8, 16, 32, 60]`（7 档，末档封顶 60×base） | 重启退避倍数表 | `registry.rs:28` |
| `RR3_RESTART_FAILURE_CAP` | 5 | 连续失败到顶即 `failed` 并**永久停止重启** | `registry.rs:31` |
| `RR3_EXPIRE_HEARTBEAT_TIMEOUT` | `heartbeat_timeout` | 到期回报的 `reason`，目前唯一取值；注册前超时的判定在 conn 层，届时新增取值即可 | `registry.rs:33-35` |
| `RR3_RESERVED_OWNER` | `rr3-runtime` | 内建命名空间；注册与发布两处都按它判冒充 | `topics_load.rs:17` |

## 3 阶段（`Rr3Phase`）与活边

四个名字一套词表（`registry.rs:45-65`）。`as_str()` 的回显就是下行
`rr3-runtime/module/state` 帧里 `phase` 字面量，两边同一个词：

| 变体 | `as_str()` | 语义 |
|---|---|---|
| `Registered` | `registered` | 已注册、**尚未收到任何心跳**：此刻不判活（legacy 教训 76c0ad8） |
| `Running` | `running` | 收到过至少一颗心跳 |
| `Degraded` | `degraded` | 依赖的 base 能力未就位或整体降级 |
| `Failed` | `failed` | 到期判死（`rr3_reg_expire`），或连接收尾（崩溃 / EOF：`rr3_reg_death` 置位并**保留**条目）。注意 `rr3.unregister` 走 `rr3_reg_remove` 是**直接把条目删掉**、不经 `Failed`，两条退场路径别混 |

当前可发生的转移只有三条边（`registry.rs:37-44`）：

| 边 | 由谁触发 | 出处 |
|---|---|---|
| `registered → running`（并刷新到期基准） | `rr3_reg_beat(src_task, now_ms)` | `registry.rs:255-264` |
| `registered \| running → failed`（心跳超 `watchdog_ms`） | `rr3_reg_expire(now_ms)` | `registry.rs:269-288` |
| `registered \| running → failed`（连接收尾） | `rr3_reg_death(name, clean_exit, now_ms)` | `registry.rs:362-393` |

两条要记住的限定：

- **`Degraded` 今天没有任何置位路径**，`degraded → running` 那条边也就无从发生；
  它在此只是 spec §5.0 词表里的一个名字（`as_str()` 要回显它）。
  置位与持久化按注释随 rrd 实现计划的 Task 9/11 落地，进度见
  [`../status.md`](../status.md)。**届时 `rr3_reg_beat` 不得再无条件升 `Running`**
  （必须改成"degraded 只续命、不升级"），否则一颗心跳就把降级标注抹平（`registry.rs:41-43`、`registry.rs:248-254`）。
- 另有第五个词 `unregistered` **不属** `Rr3Phase`：干净退场时由 `conn.rs:426` 现算后进 state 帧。

## 4 时钟约定

到期判定用**显式单调毫秒**，不存 `Instant`：`Instant` 不能回溯，就没法驱动"刚好在窗口内 /
刚好超窗"两种情形（`registry.rs:11-13`）。调用方读数以进程起点 `t0` 为基准
（`rr3_now_ms(t0)`，`daemon.rs:25-28`），条目里只存 `last_beat_ms`（`registry.rs:123-124`）。
差值用 `saturating_sub`：真出现时钟倒退时宁可判"未到期"，也不回绕成巨大差值把健康模块集体判死
（`registry.rs:275-277`）。

## 5 `rr3_reg_add`：门序即语义

签名 `(facts: &Rr3ManifestFacts, tx: Sender<Rr3Frame>, now_ms: u64) -> Result<u64, Rr3RegisterError>`，
成功回**分配到的 task**（`registry.rs:177-246`）。门序固定为**保留名 → 同名在册（`failed` 除外）→
超额 → 分配 task**：

| 序 | 门 | 命中时 | 说明 |
|---|---|---|---|
| 1 | `facts.name` == `RR3_RESERVED_OWNER` | `Rr3RegisterError::ReservedName` | `rr3-runtime` 是 rrd 自己的号段，冒充即最危险的伪造面 |
| 2 | 同名条目在册且相位不是 `Failed` | `Rr3RegisterError::Duplicate { name, src_task }` | 拒重复**不改**已在册条目的状态，也不换它的 task |
| 3 | 表满且**不是**替换式重注册 | `Rr3RegisterError::Capacity { limit }` | 容量门认 `superseded`：表满但同名条目是 `Failed` 时旧条目即将腾出槽位，这是合法替换，不得被容量拒（`registry.rs:202-206`） |
| 4 | `debug_assert!(facts.watchdog_ms > 0)` | debug 构建当场 panic | `watchdog_ms = 0` 一入库就会被到期扫描秒判死；`manifest.rs` 的 `req_int` 已在文本读取期拒非正整数，这里是防绕过入口直接构造 `pub` 字段的第二道钉。位置在**三条拒绝门之后**，否则带坏 watchdog 的重复/保留名/超额注册会先 panic 而让调用方拿不到拒绝理由（`registry.rs:207-223`） |

其余事实：

- **入库前不动表**：待替换条目的 task 号先记下，真正的摘除延后到门与断言全过之后
  ——被 `catch_unwind` 吞掉的 panic 不许留下"已改一半的表"（`registry.rs:189-192`、`registry.rs:224-229`）。
- **`Failed` 后同身份可重注册**：旧条目整体替换、**换新 task 号**，旧号从 `by_task` 里摘掉，
  迟到的旧心跳既喂不到新条目也存不活旧条目（`registry.rs:186-201`、`registry.rs:226-228`）。
- `restarts` 随旧条目一起作废：要跨重注册成立"连续 5 次"，必须由 daemon 层在派生新进程时
  把预算带进来，**本表不猜**（`registry.rs:187-189`）。
- `entry` 的绝对性门禁**不在这里**：那是 conn 层（`conn.rs:206-217`）的职责，
  存在性/可执行位随 spawn 那一刻才查（`registry.rs:174-176`）。

`Rr3RegisterError` 的 `Display` 文案逐字（`registry.rs:74-86`）：

```text
模块 {name} 已在册（task={src_task}），拒绝重复注册
模块名 {name} 属内建命名空间保留号，拒绝注册
在册模块已达上限 {limit}，拒绝注册
```

映射到错误码见 [`rrd-rpc.md`](rrd-rpc.md) §5：Duplicate / Capacity → `Busy`(-16)，
ReservedName → `Permission`(-22)。

## 6 `rr3_reg_beat` / `rr3_reg_expire`

| 入口 | 返回 | 判定 | 出处 |
|---|---|---|---|
| `rr3_reg_beat(src_task, now_ms) -> bool` | `false` = 未在册的 task、或条目已 `Failed`（**failed 的模块不得靠补心跳复活自己**） | 非 `Failed` 则写 `Running` + 刷新 `last_beat_ms`；未在册 task 的判定走 `by_task` 反查 | `registry.rs:248-264` |
| `rr3_reg_expire(now_ms) -> Vec<Rr3Expiry>` | 每tick 全表扫，只回报到期者：`Rr3Expiry { name, src_task, reason }`，`reason` 取 `RR3_EXPIRE_HEARTBEAT_TIMEOUT` | `now_ms - last_beat_ms >= facts.watchdog_ms` 即置 `Failed` 并把 `tx` 置 `None`（此后 fanout 不再往死连接塞）；已 `Failed` 的条目跳过、不重复回报；健康模块不受牵连 | `registry.rs:266-288` |

`rr3_reg_expire` 的文档自己写着"（唯一的监视线程调用）"——**那个线程还不存在**，
本 HEAD 里没有 tick 循环去驱动它。这属实况，见 [`../status.md`](../status.md)；
后果由那一档写清，本档不重复。

## 7 `rr3_reg_death`：重启计划

签名 `(name, clean_exit, now_ms) -> Option<Rr3RestartPlan>`（`registry.rs:355-393`）。
无条件先置 `Failed` 并丢弃 `tx`，再按 `facts.restart_policy` 决定给不给计划：

| `restart_policy` | 给计划的条件 | 备注 |
|---|---|---|
| `always` | 总是（未达上限时） | 非预期值（如大小写错的 `ALWAYS`）一律按 `never` 处理：不 panic、也不擅自重启（`registry.rs:372-376`） |
| `on-failure` | 仅 `clean_exit == false` | 干净退出不重启 |
| `never` | 从不 | — |
| 其他字符串 | 从不 | 校验在 manifest 读取期已挡，运行期只保证不猜 |

计划的形状 `Rr3RestartPlan { name, attempt, delay_ms, entry }`（`registry.rs:136-142`）：

- `attempt` = 递增后的 `restarts`（从 1 起）；`restarts >= RR3_RESTART_FAILURE_CAP` 即回 `None`。
- `delay_ms` = `RR3_RESTART_BACKOFF[attempt-1]`（索引按 `min(表长-1)` 收口）× `restart_base_ms`，
  乘算用 `checked_mul(...).unwrap_or(u64::MAX)`：**既不 panic 也不回绕**，溢出时唯一保守解是"永远等下去"。
- 表算出的 0 延迟（`restart_base_ms = 0`）在配置面已经进不来：`--restart-base-ms` 的下界由
  `cli.rs::parse_restart_base` 拦住（见 [`rrd-cli.md`](rrd-cli.md) §3），本表不替它裁语义。
- `now_ms` 在本函数里**没有读者**（签名冻结，conn 层照样传 `rr3_ctx_now_ms()`）：写进
  `last_beat_ms` 是死写，因为 `rr3_reg_expire` 跳过 failed、`rr3_reg_beat` 拒 failed、
  同身份重注册会整条替换 `Entry`（`registry.rs:357-361`）。
- 计划**只算不执行**：`conn.rs:429-435` 把它写进日志（`spawn 见 Task 11`），进程派生本 HEAD 一行都没有。
  进度归 [`../status.md`](../status.md)。

## 8 发布归属：四道门的**次序**就是安全语义

`RrdRegistry::rr3_reg_publish_check(sender_name, topic, topics) -> Result<(), Rr3PublishDeny>`
是**关联函数**（不碰 `self`，看不见在册表），生产路径与测试路径完全同码
（`registry.rs:290-343`）——线上调用点是发布入口 `rr3_publish`（`conn.rs:341`，见
[`rrd-rpc.md`](rrd-rpc.md) §7）：

| 门 | 条件 | 拒绝 | 映射码 |
|---|---|---|---|
| ① | `rr3_topics_is_reserved_topic(topic)`（首段 == `RR3_RESERVED_OWNER`） | `Rr3PublishDeny::ReservedNamespace { topic }` | `Permission` |
| ② | 主题首段 != `sender_name` | `Rr3PublishDeny::ForeignPrefix { topic, owner, sender }` | `Permission` |
| ③ | `topics.rr3_topic_set_contains(topic)` 为假 | `Rr3PublishDeny::NotRegistered { topic }` | `Invalid` |
| ④ | `topics.rr3_topic_set_owner(topic) != Some(sender_name)`（含取不到 owner） | `Rr3PublishDeny::ForeignPrefix { topic, owner, sender }` | `Permission` |

三条不可互换的理由：

- **顺序不是洁癖**：`rr3-runtime/*` 的冒充者在主题集里可以"合法在册"（`owner` 就写
  `rr3-runtime`），先查前缀或先查在册会把它判成 `ForeignPrefix` / `Ok` 而不是"冒充"这条
  最严重的定性。rrd 自己是该命名空间唯一合法发布者，走下行广播路径，**不调本函数**（`registry.rs:294-296`）。
- **④ 不是重复劳动**：首段 == `sender_name` 只是主题串的**语法**事实，登记 `owner` ==
  `sender_name` 才是**在册事实**。`rr3_topic_set_check` 的"首段 == owner"前置只在
  `rr3_topics_load` 的装载路径上强制，而 `rr3_registry_topic_set_from_text` 本身是 public，
  绕开 check 造一张"首段是 rr3-sima、登记 owner 是 rr3-simb"的表并不费力——安全判定不押在
  "上游一定查过"上（`registry.rs:305-310`）。
- **③ 指的是主题不在册**（首段与发送方相符、名字拼错或没登记），**不是**指发送方那个模块
  没在册：本函数不碰 `self`（`registry.rs:97-99`）。

`owner` 回显只有一种写法（`owner_for_echo`，`registry.rs:144-150`）：在册但登记文本没写
`owner` 键（解析成空串）与根本不在册（`None`）**都渲染 `?`**，两处构造点共用它。
两条都是**终判拒绝**，别把 `owner=?` 读成待办事项。

`Rr3PublishDeny` 的 `Display` 文案逐字（`registry.rs:102-115`）：

```text
拒绝 {topic}：rr3-runtime/* 是守护进程的下行广播，模块不得冒充
越权发布 {topic}（owner={owner}，发送方={sender}）
未注册主题 {topic}，拒绝发布
```

**调用方义务（本函数替不了你保证）**：`rr3_reg_publish_check` 收 `sender_name: &str`、
不碰 `self`，所以它既不知道这个名字是否在册（从未注册、或已 `Failed` 的名字照样能拿到
`Ok`），也不知道它是不是报文里编出来的——唯一的防线建在调用侧：传进来的发送方**绝不得
取自帧体或注册报文里自称的 `name` 字段**（`registry.rs:298-304`）。HEAD 的线上发布分支
（`rr3_publish`）用 `me.name` 兑现这条前提——它是 `Rr3Peer` 随握手成功才存在的、
rrd 在注册时收下并分配的在册身份（`conn.rs:242`），帧上的 `src_task` 又已在分发前被强制
等于 `me.task`（`conn.rs:272`），而 `me.name` 与 `me.task` 出自同一次 `rr3_reg_add`
（`conn.rs:310-313`、`conn.rs:341`）。`registry.rs` 的 doc 注把这套义务表述成"由
`rr3_reg_name_of(src_task)` 反查"，线上是换了一种等价做法（登记期身份 + `src_task` 强校验）
把它守住的；`rr3_reg_name_of` 本身目前仍无生产调用点。进度归 [`../status.md`](../status.md)。

越权**只计数不牵连**：`rr3_reg_note_denied` 对 `denied_publish` 做**饱和加**
（撞顶 `u32` 后停在原值，绝不回绕成 0 抹平审计线索），`rr3_reg_denied` 读数；
一次坏发布既不改发布者生命周期状态，也不影响其它连接的转发（`registry.rs:345-353`、`registry.rs:13`）。

## 9 `rr3_reg_fanout`：只做广播，**不读** `dst_task`

签名 `(frame: &Rr3Frame, exclude_task: u64) -> usize`（`registry.rs:422-434`）。

| 事实 | 说明 |
|---|---|
| 筛选条件只有两个 | 条目 `src_task != exclude_task` **且** `tx` 还是 `Some`（已到期/已崩溃的条目句柄当场丢弃） |
| `frame.header.dst_task` | **原样带上、本函数不读它**。所以它**不是单播入口**（签名冻结，不改）：一帧的 `dst_task` 写着某个具体模块，实际收到的是别的活连接。要点名发给某一个模块，由 conn 层直接用那条连接的 `Sender` |
| 返回值 | 成功**入队**数；接收者已 `drop` 只是少计，不算错、不 panic |
| `exclude_task` 用 `RR3_TASK_RRD` 的现值 | 1 不是任何模块的 task，等于"谁都不排除"：`conn.rs` 的注册广播与退场广播都这样调用（`conn.rs:243`、`conn.rs:428`）；业务发布那次的 fanout 排除的是发布者自己（`conn.rs:379`，`me.task`），不用 `RR3_TASK_RRD` |
| 锁与投递 | 入队发生在持有在册表锁的期间；真正的字节写盘在各连接自己的 writer 线程 → 入队次序 ≠ 上线次序，客户端按类型/字段挑帧（见 [`rrd-rpc.md`](rrd-rpc.md) §1） |

## 10 其余入口（签名即契约）

| 入口 | 返回 | 语义 | 出处 |
|---|---|---|---|
| `rr3_reg_remove(name) -> bool` | 是否真删了 | 连带摘掉 `by_task` 映射；task 号**不回收**；重复移除回 `false` | `registry.rs:395-403` |
| `rr3_reg_live(name) -> bool` | — | 相位不是 `Failed` **且** 句柄是 `Some`，两者缺一即不活；不在册也是 `false` | `registry.rs:405-410` |
| `rr3_reg_name_of(src_task) -> Option<String>` | — | task→名反查，测试与运维可用；**目前无生产调用点**——conn 层的 `src_task` 校验直接用连接自带的 `me.task`（`conn.rs:272`），`sender_name` 用登记期身份 `me.name`（`conn.rs:341`），都不经此函数（见 §8 调用方义务） | `registry.rs:412-415` |
| `rr3_reg_facts(name) -> Option<Rr3ManifestFacts>` | 入库那份的克隆 | 不在册回 `None`（不 panic、不给半数据）；spawn 序靠它取 `start_priority` 与 `entry` | `registry.rs:417-420` |
| `rr3_reg_snapshot() -> Vec<(String, u64, Rr3Phase)>` | — | 按 **task 升序**（不是名字序）：状态广播与运维输出处要靠它对齐 task 号，名字序只是 `BTreeMap` 的顺带结果，故显式排序。后到连接补发快照用的就是它（`conn.rs:250-256`，排除自己同名条目） | `registry.rs:436-443` |

## 11 相关文档

- 上行 RPC 的方法、参数与错误码：[`rrd-rpc.md`](rrd-rpc.md)
- 启动参数（`--restart-base-ms` 的上下界就钉在这里）：[`rrd-cli.md`](rrd-cli.md)
- 生命周期设计语义：[`../architecture/architecture.md`](../architecture/architecture.md) §4.2
- 主题事实源：[`../../contracts/topics.toml`](../../contracts/topics.toml)（本档不抄表）
- 实况与在制品：[`../status.md`](../status.md)
