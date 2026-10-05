# rrd 在册表参考（阶段机 · 退避 · 发布归属）

> **代码即文档**：本档的阶段名、常量值、门序与拒绝文案全部取自
> [`rrd/src/registry.rs`](../../base/runtime/rrd/src/registry.rs) 的字面量，
> 其中 `Rr3Phase` 的 `as_str` 四个词与 `Rr3RegisterError` / `Rr3PublishDeny` 的
> `Display` 文案是**线上词表**（下行 state 帧与集成侧按字面比对）。断言口径分档：
> 线上词表里**只有两枚 `Display`**——`Rr3RegisterError` 三条 + `Rr3PublishDeny` 三条，共六串文案，
> 在 `registry::tests` 的 `register_error_display_is_verbatim` 与
> `publish_deny_display_distinguishes_the_three_layers` 里逐字 `assert_eq` 钉住；
> `Rr3Phase::as_str` 的四个阶段词是同一个 tests 模块里另一组逐字断言
> （`phase_names_are_the_wire_vocabulary`）。集成侧
> （`tests/register_publish.rs`）按 `contains` 子串区分"被哪一层拒的"；错误码映射
> （`rr3_rpc_status_for_register` / `rr3_rpc_status_for_publish_deny`，`rpc.rs` 的两枚映射函数）在 `rpc.rs` 的
> `register_error_maps_to_status_code` 与 `publish_deny_maps_to_status_code` 里
> 逐枚举断言判别值。**两点提醒**：启动横幅（`rr3_banner`）只是 `contains` 片段、并非逐字；
> 非 Unix 退出文案 `RRD_UNSUPPORTED` 本 HEAD **没有任何 Rust 用例按字面断言它**——改这些
> 字面量都要同批查本文与相关 README、脚本。**改代码必须同批改本档对应字面量**。
> 接线进度属实况，唯一落点 [`../status.md`](../status.md)；本档只写接口契约。

---

## 1 类型与共享方式

| 项 | 事实 | 出处 |
|---|---|---|
| 类型 | `RrdRegistry`，字段 `modules`（`BTreeMap` 名→条目）、`by_task`（task→名）、`next_task`、`restart_base_ms`、`denied_publish`、`crash_streak`（`BTreeMap` 名→崩溃连击数）、`owned`（`BTreeSet` 名）、`stopped`（`BTreeSet` 名）、`providers`（能力→提供者名集合，**boot 仲裁产物**、只含非 base 能力；写入方 `rr3_reg_set_providers`，运行期不重排）、`low_power`（全局低电旗，T4b：conn 转发 `rr3-power/battery/low` 时置位、supervisor 的 spawn 漏斗读它拒新 spawn——与 `denied_publish` 同款 conn 与 supervisor 两路共见守护态，**只升不降**，见 §10 末行） | `registry.rs` 的 `RrdRegistry` 结构体 |
| 构造 | `RrdRegistry::new(restart_base_ms)`，`next_task` 起点 = `RR3_TASK_FIRST_MODULE`，其余账全部为空 | `registry.rs` 的 `RrdRegistry::new` |
| 条目 | `Entry { name, src_task, phase, facts, last_beat_ms, registered_at_ms, tx }`——**两颗时间基准**：判死看 `last_beat_ms`（多久没喂狗）、恢复归零看 `registered_at_ms`（这一世活了多久），不许混用 | `registry.rs` 的 `Entry` 结构体 |
| 平台 | 平台无关：只碰 `BTreeMap` / `mpsc` / `u64` 单调毫秒；UDS 与进程派生在 `conn.rs` / `daemon.rs`（整体 `#[cfg(unix)]`） | `registry.rs` 的模块级文档注释 |
| 共享 | 守护进程持一份 `Arc<Mutex<RrdRegistry>>`，每连接一份 `Rr3ConnCtx` 引用同一把锁；watchdog 与 supervisor 两条线程拿的都是同一份 `shared` 的克隆 | `daemon.rs` 的 `rr3_daemon_run` 中 `shared` 创建与 watchdog 启动段 |
| 取锁 | `rr3_lock`：锁中毒只可能来自 panic 的持锁者，**继续用比弃连接/比整个守护停摆好**（`unwrap_or_else(into_inner)`）。自 `08d460f` 起它是 `lib.rs` 里的 `pub(crate)` **单点**，`conn.rs`（应答与收尾）与 `daemon.rs`（watchdog 扫描与广播）、`supervisor.rs`（收尸与起进程）共用，不再各写一份 | `lib.rs` 的 `rr3_lock` |
| task 分配 | `next_task` 用 `saturating_add(1)` 单调递增；0 与 1 永不出现 | `registry.rs` 的 `rr3_reg_add` 中 task 分配段 |

## 2 常量表

| 常量 | 值 | 含义 | 出处 |
|---|---|---|---|
| `RR3_TASK_RRD` | 1 | rrd 自己的 task；上行 `dst_task` 必须是它 | `rr-bus/src/lib.rs` 的 `RR3_TASK_RRD` 常量 |
| `RR3_TASK_FIRST_MODULE` | 2 | 模块 task 的分配起点 | `rr-bus/src/lib.rs` 的 `RR3_TASK_FIRST_MODULE` 常量 |
| `RR3_BROADCAST_DST_TASK` | 0 | 只用于 rrd 下行广播；**不是任何模块的 task** | `rr-bus/src/lib.rs` 的 `RR3_BROADCAST_DST_TASK` 常量 |
| `RR3_REG_MAX_MODULES` | 16 | 在册模块上限；`Display` 文案里的"上限 16"就是它 | `registry.rs` 的 `RR3_REG_MAX_MODULES` 常量 |
| `RR3_RESTART_BACKOFF` | `[1, 2, 4, 8, 16, 32, 60]`（7 档，末档封顶 60×base） | 重启退避倍数表 | `registry.rs` 的 `RR3_RESTART_BACKOFF` 常量 |
| `RR3_RESTART_FAILURE_CAP` | 5 | 连续失败到顶即 `failed` 并**永久停止重启** | `registry.rs` 的 `RR3_RESTART_FAILURE_CAP` 常量 |
| `RR3_EXPIRE_HEARTBEAT_TIMEOUT` | `heartbeat_timeout` | 到期回报的 `reason`，目前唯一取值；注册前超时的判定在 conn 层，届时新增取值即可 | `registry.rs` 的 `RR3_EXPIRE_HEARTBEAT_TIMEOUT` 常量 |
| `RR3_RESERVED_OWNER` | `rr3-runtime` | 内建命名空间；注册与发布两处都按它判冒充 | `topics_load.rs` 的 `RR3_RESERVED_OWNER` 常量 |

## 3 阶段（`Rr3Phase`）与活边

四个名字一套词表（`Rr3Phase`，见 `registry.rs`）。`as_str()` 的回显就是下行
`rr3-runtime/module/state` 帧里 `phase` 字面量，两边同一个词：

| 变体 | `as_str()` | 语义 |
|---|---|---|
| `Registered` | `registered` | 已注册、**尚未收到任何心跳**：此刻不判活（legacy 教训 76c0ad8） |
| `Running` | `running` | 收到过至少一颗心跳 |
| `Degraded` | `degraded` | required 能力的提供者**全部**离开 Running（ADR-0009 裁定 3）；置位方 `rr3_reg_degrade_after`、解除方 `rr3_reg_undegrade_after` |
| `Failed` | `failed` | 到期判死（`rr3_reg_expire`）、非 owned 的连接收尾（`rr3_reg_death`）、子进程退出码收尸（`rr3_reg_child_exit`）三路置位并**保留**条目。注意 `rr3.unregister` 走 `rr3_reg_remove` 是**直接把条目删掉**、不经 `Failed`，两条退场路径别混 |

当前可发生的转移（真值见 `registry.rs` 的 `Rr3Phase` 文档注释）：

| 边 | 由谁触发 | 出处 |
|---|---|---|
| `registered → running`（刷新到期基准；conn 收到 `Promoted` 时**广播一次** running state，`conn.rs` 的 `rr3_serve_request` Promoted 广播段） | `rr3_reg_beat(src_task, now_ms) -> Rr3Beat` | `registry.rs` 的 `rr3_reg_beat` |
| `registered \| running → failed`（心跳超 `watchdog_ms`；同一趟里兼做恢复归零） | `rr3_reg_expire(now_ms)` | `registry.rs` 的 `rr3_reg_expire` |
| `registered \| running → failed`（连接收尾；**owned 的名字不让位之前不动相位**，见 §7.1） | `rr3_reg_death(name, clean_exit, now_ms) -> Rr3Death` | `registry.rs` 的 `rr3_reg_death` |
| `registered \| running → failed`（子进程退出码，supervisor 收尸） | `rr3_reg_child_exit(facts, clean_exit, now_ms) -> Rr3ChildExit` | `registry.rs` 的 `rr3_reg_child_exit` |
| `running → degraded`（`provider` 离开 Running 后，它提供的某能力再无任何 **Running** 提供者 → 依赖该能力的在册 `Running` 模块逐个置位；多提供者时任一存活即满足） | `rr3_reg_degrade_after(provider) -> Vec<(String, u64)>` | `registry.rs` 的 `rr3_reg_degrade_after` |
| `degraded → running`（`provider` 回到 Running 那一拍重算，**全部** required 能力重新就位才回——只回来一个提供者不代表别的依赖也好了） | `rr3_reg_undegrade_after(provider) -> Vec<(String, u64)>` | `registry.rs` 的 `rr3_reg_undegrade_after` |

两条要记住的限定：

- **`Degraded` 的置位 / 解除 / 续命三条都已实装**（ADR-0009 裁定 3；不再是"随 M5 落地"）：
  - **只续命不升级**：`rr3_reg_beat` 遇 `Degraded` 归 `Kept`（`registry.rs` 的 `rr3_reg_beat`），
    "一颗心跳抹平降级标注"的欠账已结清。
  - **置位**：`rr3_reg_degrade_after(provider)`——`provider` 的死亡**已先在本表落账**
    （三条死亡路各自先记账再传播，否则"它自己还 Running"会挡住判空），只置位、
    **不记账不出重启计划**（裁定 4：降级不是崩溃，不吃预算不经 supervisor）。
  - **解除**：`rr3_reg_undegrade_after(provider)`——逐模块重算，`rr3_deps_all_satisfied`
    要求 **全部** required 能力就位；不在 `providers` 表里的能力视为 base 内建、永远在位
    （线上补注册的模块不做二次裁定，ADR-0009 不做清单 2 的同一条边界）。
  - **线侧接线四处**：watchdog 判死（`daemon.rs`）、supervisor 收尸（`supervisor.rs`）、
    conn 的死亡与复活两路（`conn.rs`）各在记账后调用传播方法，并照返回的 `(名, task)`
    广播 state。`providers` 表由 `rr3_reg_set_providers` 在 boot 仲裁时一次性写入
    （含被仲裁拒绝的模块——它们的 provides 仍计入可满足性宇宙）。
- 另有第五个词 `unregistered` **不属** `Rr3Phase`：由 `rr3_phase_for_exit(clean_exit)`
  映射（干净退场 → `unregistered`，否则 `failed`，`registry.rs` 的 `rr3_phase_for_exit`），conn 收尾与
  supervisor 收尸两个广播点都必须经它（`conn.rs` 的 `rr3_conn_finish`、`supervisor.rs` 的 `rr3_absorb_exit`）。

## 4 时钟约定

到期判定用**显式单调毫秒**，不存 `Instant`：`Instant` 不能回溯，就没法驱动"刚好在窗口内 /
刚好超窗"两种情形（`registry.rs` 的模块级文档注释不变量 2）。调用方读数以进程起点 `t0` 为基准
（`rr3_now_ms(t0)`，`daemon.rs` 的 `rr3_now_ms`），条目里存 `last_beat_ms` 与 `registered_at_ms`
两颗基准（`Entry` 的 `last_beat_ms` 与 `registered_at_ms` 字段）。
差值用 `saturating_sub`：真出现时钟倒退时宁可判"未到期"，也不回绕成巨大差值把健康模块集体判死
（`rr3_reg_expire` 的判死段；差值提成 `elapsed_ms` 绑定，判据与回报共用同一个数）。

## 5 `rr3_reg_add`：门序即语义

签名 `(facts: &Rr3ManifestFacts, tx: Sender<Rr3Frame>, now_ms: u64) -> Result<u64, Rr3RegisterError>`，
成功回**分配到的 task**（`registry.rs` 的 `rr3_reg_add`）。门序固定为**保留名 → 同名在册（`failed` 除外）→
超额 → 分配 task**：

| 序 | 门 | 命中时 | 说明 |
|---|---|---|---|
| 1 | `facts.name` == `RR3_RESERVED_OWNER` | `Rr3RegisterError::ReservedName` | `rr3-runtime` 是 rrd 自己的号段，冒充即最危险的伪造面 |
| 2 | 同名条目在册且相位不是 `Failed` | `Rr3RegisterError::Duplicate { name, src_task }` | 拒重复**不改**已在册条目的状态，也不换它的 task |
| 3 | 表满且**不是**替换式重注册 | `Rr3RegisterError::Capacity { limit }` | 容量门认 `superseded`：表满但同名条目是 `Failed` 时旧条目即将腾出槽位，这是合法替换，不得被容量拒（`rr3_reg_add` 的容量门段） |
| 4 | `debug_assert!(facts.watchdog_ms > 0)` | debug 构建当场 panic | `watchdog_ms = 0` 一入库就会被到期扫描秒判死；`manifest.rs` 的 `req_int` 已在文本读取期拒非正整数，这里是防绕过入口直接构造 `pub` 字段的第二道钉。位置在**三条拒绝门之后**，否则带坏 watchdog 的重复/保留名/超额注册会先 panic 而让调用方拿不到拒绝理由（`rr3_reg_add` 的 `debug_assert` 段） |

其余事实：

- **入库前不动表**：待替换条目的 task 号先记下，真正的摘除延后到门与断言全过之后
  ——被 `catch_unwind` 吞掉的 panic 不许留下"已改一半的表"（`rr3_reg_add` 的 superseded 记录段、`rr3_reg_add` 的摘除与插入段）。
- **`Failed` 后同身份可重注册**：旧条目整体替换、**换新 task 号**，旧号从 `by_task` 里摘掉，
  迟到的旧心跳既喂不到新条目也存不活旧条目（`rr3_reg_add` 的 superseded 记录段、`rr3_reg_add` 的摘除与插入段）。
- **崩溃预算是表级账**：旧的 `Entry.restarts` 已删除——它随条目替换归零，"连续 5 次"永远撞不到。
  现在预算在 `crash_streak`（按 **name** 计，`RrdRegistry` 的 `crash_streak` 字段），重注册**不清零**
  （`rr3_reg_add` 的 `crash_streak` 注释段），唯一写入方是 `rr3_charge`、唯一清空方是 `rr3_reg_expire` 的恢复归零段（见 §6）。
- `entry` 的绝对性门禁**不在这里**：那是 conn 层（`conn.rs` 的 `rr3_handshake` entry 绝对性门禁段）的职责；
  存在性/可执行位只查本守护要 spawn 的那批——supervisor 的 `rr3_boot_job_from_manifest`
  在 boot 那一刻查（`rr3_boot_job_from_manifest` 的 entry 可执行位检查段），走线上注册的外来模块 rrd 永不 spawn 它（E-1）。

`Rr3RegisterError` 的 `Display` 文案逐字（`registry.rs` 的 `Rr3RegisterError` `Display` 实现）：

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
| `rr3_reg_beat(src_task, now_ms) -> Rr3Beat` | 三态：`Rejected`（未在册的 task / 条目已 `Failed`，**failed 的模块不得靠补心跳复活自己**）、`Kept`（已 `Running`/`Degraded`：只刷新 `last_beat_ms` 续命、**不升级**）、`Promoted`（`registered → running` 的唯一升级，conn 据此广播一次 running state） | 未在册 task 的判定走 `by_task` 反查 | `registry.rs` 的 `rr3_reg_beat` |
| `rr3_reg_expire(now_ms) -> Vec<Rr3Expiry>` | 一趟扫描、两个生命周期决定：**①恢复归零**——`Running` 且活满一整条自己的窗口（`now - registered_at_ms >= watchdog_ms`）即清掉 `crash_streak[name]`；**②判死**——`now_ms - last_beat_ms >= watchdog_ms` 即置 `Failed` 并把 `tx` 置 `None`（此后 fanout 不再往死连接塞），同时经 `rr3_charge` 记账出 `Rr3Expiry.restart`。段序固定：`Failed` 跳过在前（不重复回报、本拍也不归零）→ 归零段 → 判死段；健康模块不受牵连 | `registry.rs` 的 `rr3_reg_expire` |

`Rr3Expiry` 的字段形状见 `registry.rs` 的 `Rr3Expiry` 结构体，其中 `restart: Option<Rr3RestartPlan>` 是
到期那一路的**记账结果**（`always`/`on-failure` 都给计划、`never` 给 `None`）——daemon 侧收下后
`sup.rr3_restart(p)` 交给 supervisor 执行（`rr3_spawn_watchdog` 的 `sup.rr3_restart` 调用段）。后两个字段（`elapsed_ms` /
`watchdog_ms`）是**判定本身**的数字：`elapsed_ms = now_ms - last_beat_ms`（同一个绑定既当判据又进回报，
`rr3_reg_expire` 的判死段）、`watchdog_ms` 是**该模块自己**manifest 的值（不是全局常量）。它们必须由
判定处一并交出——判死日志要逐字回显"距上次心跳 Xms ≥ 超时 Yms"，而表外的人拿不到 `last_beat_ms`
（`rr3_reg_facts` 只给 manifest 事实、不给心跳时刻，`registry.rs` 的 `Rr3Expiry` 把这条理由写在结构体上）；
少了这两个字段，线级用例就只能靠掐时间来判"判得对不对"。单元侧由
`expiry_report_carries_the_numbers_that_decided_it` 钉住（`elapsed_ms` 必须是 `now - last_beat`
而非 `now - 注册时刻`）。两颗基准不许混：归零看 **registered_at_ms**、判死看 **last_beat_ms**。

### 6.1 驱动它的那条线程：接口形状

`rr3_reg_expire` 的文档注释自题"（唯一的监视线程调用）"（`registry.rs` 的 `rr3_reg_expire`）——**那条线程存在**：
[`daemon.rs`](../../base/runtime/rrd/src/daemon.rs) 的 `rr3_spawn_watchdog`，
由 `rr3_daemon_run` 在 accept 循环之前调用，循环体每拍执行
`rr3_lock(&shared).rr3_reg_expire(rr3_now_ms(t0))`（`daemon.rs` 的 `rr3_spawn_watchdog`）。三条形状（改线程先看这三条）：

- **先睡后扫**：进循环第一件事是 `thread::sleep(tick)`（`rr3_spawn_watchdog` 的循环体首行），注册完成那一刻不扫，
  第一拍落在 t+tick；到期判定因此自带一个 tick 的松弛量。
- **每拍只锁一次**：只有全表扫描那一次持锁（`rr3_spawn_watchdog` 的循环体），锁的粒度就是扫描本身，
  上限 `RR3_REG_MAX_MODULES` = 16 条。
- **广播在锁外**：判死与 `rr3_reg_fanout` 之间不持锁，`rr3_reg_fanout` 自己再锁一次
  （`rr3_spawn_watchdog` 的广播段）——刻意不写"持锁 sleep"那种把整个守护卡住的形状。

tick 的值就是 CLI 的 `--watchdog-tick-ms`（默认 200、拒 0），字面量见
[`rrd-cli.md`](rrd-cli.md) §2 §3。这条线程对外的可观察面只有 stdout 三行（行首 `rrd: `，
前缀的由来见 `rrd-cli.md` §5），逐字如下——`{...}` 是 `Rr3Expiry` 对应字段的替换位
（源里的格式串是四个 `{}`，`rr3_spawn_watchdog` 的判死日志格式串段）：

```text
watchdog 启动 tick={tick_ms}ms
watchdog 判死 {name} task={src_task} 距上次心跳 {elapsed_ms}ms ≥ 超时 {watchdog_ms}ms
watchdog 广播 failed 到 {n} 个连接
```

到期那一路**已经产计划**：`Rr3Expiry.restart` 由 `rr3_charge` 记账得出（见 §6），daemon 侧
`sup.rr3_restart(p)` 只做一次 `mpsc::send` 交给 supervisor（`rr3_spawn_watchdog` 的 `sup.rr3_restart` 调用段）——这条线程里
绝不 spawn、不睡眠、不 wait。计划的执行与"跳过/取消"语义见 §7.3；接到哪一步属实况，
唯一落点 [`../status.md`](../status.md)。

## 7 死亡与记账：三路汇入 `rr3_charge`

三条公开的死亡路——conn EOF（`rr3_reg_death`）、watchdog 到期（`rr3_reg_expire` 判死段）、
子进程退出码（supervisor 收尸走 `rr3_reg_child_exit`）——的**记账全部汇入 `rr3_charge`**
（`registry.rs` 的 `rr3_charge`）：`crash_streak` 的唯一写入方就是它，别处一律不许碰。先判后加：
策略说不重启（`never` / `on-failure` 且干净退出）或 `crash_streak >= RR3_RESTART_FAILURE_CAP`
直接回 `None`，**不增长** streak；给计划时 `attempt = streak + 1`。

计划的形状 `Rr3RestartPlan { name, attempt, delay_ms, entry }`（`registry.rs` 的 `Rr3RestartPlan` 结构体）：

- `delay_ms` = `RR3_RESTART_BACKOFF[attempt-1]`（索引按 `min(表长-1)` 收口）`.saturating_mul(restart_base_ms)`：
  **既不 panic 也不回绕**，溢出时饱和封顶，唯一保守解是"永远等下去"。
- 表算出的 0 延迟（`restart_base_ms = 0`）在配置面已经进不来：`--restart-base-ms` 的下界由
  `cli.rs::parse_restart_base` 拦住（见 [`rrd-cli.md`](rrd-cli.md) §3），本表不替它裁语义。
- `crash_streak` 按 **name** 计、跨重注册成立（重注册不清零，`rr3_reg_add` 的 `crash_streak` 注释段）；
  唯一清空方是 `rr3_reg_expire` 的恢复归零段（见 §6）。

### 7.1 conn 收尾那一路：`rr3_reg_death`

签名 `(name, clean_exit, now_ms) -> Rr3Death`（`registry.rs` 的 `rr3_reg_death`）。判定序固定
（`rr3_reg_death` 的 match 判定段，**序不是风格**：不在册必须先于 owned 判定，否则 unregister 后
"等退出码"会让 `unregistered` 永远没人广播）：

| 判定 | 回报 | conn 该做什么 |
|---|---|---|
| 条目已 `Failed` | `Rr3Death::Silent` | 不重复广播、不重复记账（watchdog 已判死，R-A4 在这条上结清） |
| 不在册（unregister 之后的收尾） | `Rr3Death::Broadcast { plan: None }` | 照广播 `unregistered` |
| 在册且 **owned** | `Rr3Death::DeferredToExitStatus` | **一句都不广播**：相位、记账、广播三件事全让给退出码那一路（§7.2） |
| 其余 | `Rr3Death::Broadcast { plan }` | 置 `Failed` + 丢 `tx` + 广播 state；计划写日志（`rr3_conn_finish` 的 `Broadcast` 分支） |

- `Rr3Death` 三态定义在 `registry.rs` 的 `Rr3Death` 枚举，`rr3_plan()` 只从 `Broadcast` 取计划。
- `now_ms` 在本函数里**没有读者**（签名冻结，conn 层照样传 `rr3_ctx_now_ms()`）：
  死亡时刻不进基准（`rr3_reg_death` 的文档注释）。

### 7.2 子进程退出码那一路：`rr3_reg_child_exit` 与所有权

supervisor 收尸的唯一落点（`registry.rs` 的 `rr3_reg_child_exit`），回报 `Rr3ChildExit` 三态（`registry.rs` 的 `Rr3ChildExit` 枚举）：

| 回报 | 条件 | supervisor 该做什么 |
|---|---|---|
| `Crashed { src_task, clean_exit, plan }` | 条目在册且活着 | 相位、句柄、账这一次到位；按 `rr3_phase_for_exit(clean_exit)` 广播（干净 → `unregistered`，否则 `failed`） |
| `EarlyExit { plan }` | 注册成功之前就退出（含 spawn 失败） | 没有条目可广播，但账已按 name 记过（坏 entry 不能空转重起） |
| `Silent` | 从未注册且已 `stopped`，或早已 `Failed` | 不双计（watchdog 已判死的封印） |

**所有权标记**（`RrdRegistry` 的 `owned` 字段）：`rr3_reg_own` / `rr3_reg_unown` / `rr3_reg_is_owned`。
`name ∈ owned` ⟺ "这个进程是我起的，它的退出码才是死因的权威"。写入方只有 supervisor 的
`rr3_boot_spawn`——**在 `Command::spawn` 之前** own：子进程可能在同一毫秒内注册完并死掉，
晚一步 own 会让 conn 那一路先看到 EOF 按"非 owned"广播 `unregistered`，把退出码能给出的
`failed` 抢掉（本设计里唯一一处"次序决定语义"）。清除方两处：起进程失败当场 `rr3_reg_unown`、
`rr3_reg_child_exit` 收完这一次尸撤下（同名新实例的 EOF 不该等一个不再来的退出码）。

**显式退场标记**（`registry.rs` 的 `rr3_reg_remove`）：`rr3_reg_remove` 把名字记进 `stopped`，
`rr3_reg_add` 移出——已显式停掉的模块，其子进程后来退出时不得再被记账成一次崩溃。

**达上限查询**（`registry.rs` 的 `rr3_reg_hit_cap`）：`rr3_reg_hit_cap(name)` 单开一个查询而不是让
`rr3_charge` 返回三态——策略说"不重启"与预算说"到顶了"是两件不同的话，日志文案不同
（"停止重启 {name}：连续崩溃达上限，运行时降级（spec §5.3）"）。连击数观测面：
`rr3_reg_crash_streak(name)`（`registry.rs` 的 `rr3_reg_crash_streak`），只读不写。

### 7.3 执行计划的那条线程：supervisor 形状与日志字面量

重启与 boot 的唯一执行者是 [`supervisor.rs`](../../base/runtime/rrd/src/supervisor.rs)——
全 daemon 唯一的执行线程 + 唯一 watchdog 线程（E-5），收尸与重启必须串行：

| 事实 | 说明 |
|---|---|
| `jobs` 只由 `--manifest` 填（E-1） | "我能起谁"的全集；**外来模块的计划一律跳过**（`跳过重启 {name}：不是本守护起的进程`，连它的 entry 都不碰）。代价：非本守护起的模块永不重启 |
| 每轮顺序固定 | 收件 → `rr3_reap`（扫尸）→ `rr3_fire_due`（到期执行）；`recv_timeout` 的等待取 `sweep` 与最近到期时刻的**较小者**，否则长 tick 下重启永不执行 |
| 退避不是睡眠（E-4） | 计划进 `due: Vec<(Instant, Rr3RestartPlan)>` 到期才起；`delay_ms == u64::MAX` 时 `checked_add` 溢出、打 `退避不可表示` 并丢弃，**绝不 sleep**——一个模块的 60 秒退避不许停摆整台机器的收尸 |
| 环境注入三件（E-6） | `RR3_BUS_PATH` / `RR3_MANIFEST` / `RR3_SIM_PEER_ROLE`（值来自 `--boot-role`，原样透传不校验）；子进程 `stdout(null)`、`stderr(inherit)`——绝不混进 rrd 自己的 stdout 管道 |
| boot 期配置错误不吃预算（E-3） | entry 不存在/不是文件/无执行位 → `拒绝启动`，不 own、不 spawn、不记账；那是确定性配置错误，不是运行时降级 |

日志字面量（行首 `rrd: `；`rr3_assert_no_log` 的 needle 与这些字面量同源，改前要一起看）：

```text
supervisor 启动 sweep={ms}ms
spawn {name} pid={pid} entry={entry}[ attempt={n}]
拒绝启动：manifest {path} 读取失败: {e}
拒绝启动：manifest {path} 解析失败: {errs}
拒绝启动 {name}：entry {entry} 不存在或不可执行
跳过重启 {name}：不是本守护起的进程
取消重启 {name}：已有在册实例
停止重启 {name}：连续崩溃达上限，运行时降级（spec §5.3）
{name} 子进程退出 status={status} → 广播 {phase} 到 {n} 个连接
{name} 未注册即退出 status={status}（无 state 可广播）
{name} 子进程退出 status={status}，终态已有人记过
退避不可表示，视为永久停止 {name}
杀掉挂死的 {name} pid={pid}
supervisor 已退出，boot 投递丢弃: {name}
supervisor 已退出，重启投递丢弃: {name}
```

`spawn ` 只用于"成功起了一个进程"那一行；起不来一律写 `起进程失败: {e}`。

## 8 发布归属：四道门的**次序**就是安全语义

`RrdRegistry::rr3_reg_publish_check(sender_name, topic, topics) -> Result<(), Rr3PublishDeny>`
是**关联函数**（不碰 `self`，看不见在册表），生产路径与测试路径完全同码
（`registry.rs` 的 `rr3_reg_publish_check`）——线上调用点是发布入口 `rr3_publish`（`conn.rs` 的 `rr3_publish`，见
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
  最严重的定性。rrd 自己是该命名空间唯一合法发布者，走下行广播路径，**不调本函数**（`rr3_reg_publish_check` 的文档注释）。
- **④ 不是重复劳动**：首段 == `sender_name` 只是主题串的**语法**事实，登记 `owner` ==
  `sender_name` 才是**在册事实**。`rr3_topic_set_check` 的"首段 == owner"前置只在
  `rr3_topics_load` 的装载路径上强制，而 `rr3_registry_topic_set_from_text` 本身是 public，
  绕开 check 造一张"首段是 rr3-sima、登记 owner 是 rr3-simb"的表并不费力——安全判定不押在
  "上游一定查过"上（`rr3_reg_publish_check` 的末段 owner 比对）。
- **③ 指的是主题不在册**（首段与发送方相符、名字拼错或没登记），**不是**指发送方那个模块
  没在册：本函数不碰 `self`（`Rr3PublishDeny` 的 `NotRegistered` 变体文档注释）。

`owner` 回显只有一种写法（`registry.rs` 的 `owner_for_echo`）：在册但登记文本没写
`owner` 键（解析成空串）与根本不在册（`None`）**都渲染 `?`**，两处构造点共用它。
两条都是**终判拒绝**，别把 `owner=?` 读成待办事项。

`Rr3PublishDeny` 的 `Display` 文案逐字（`registry.rs` 的 `Rr3PublishDeny` `Display` 实现）：

```text
拒绝 {topic}：rr3-runtime/* 是守护进程的下行广播，模块不得冒充
越权发布 {topic}（owner={owner}，发送方={sender}）
未注册主题 {topic}，拒绝发布
```

**调用方义务（本函数替不了你保证）**：`rr3_reg_publish_check` 收 `sender_name: &str`、
不碰 `self`，所以它既不知道这个名字是否在册（从未注册、或已 `Failed` 的名字照样能拿到
`Ok`），也不知道它是不是报文里编出来的——唯一的防线建在调用侧：传进来的发送方**绝不得
取自帧体或注册报文里自称的 `name` 字段**（`rr3_reg_publish_check` 的文档注释）。HEAD 的线上发布分支
（`rr3_publish`）用 `me.name` 兑现这条前提——它是 `Rr3Peer` 随握手成功才存在的、
rrd 在注册时收下并分配的在册身份（`rr3_handshake` 的 `rr3_reg_add` 调用段），帧上的 `src_task` 又已在分发前被强制
等于 `me.task`（`rr3_serve_frame` 的 `src_task` 校验段），而 `me.name` 与 `me.task` 出自同一次 `rr3_reg_add`
（`rr3_handshake` 的 `Rr3Peer` 构造段、注册调用与发布入口分别在 `rr3_handshake` 与 `rr3_publish`）。`registry.rs` 的 doc 注把这套义务表述成"由
`rr3_reg_name_of(src_task)` 反查"，线上是换了一种等价做法（登记期身份 + `src_task` 强校验）
把它守住的；`rr3_reg_name_of` 本身目前仍无生产调用点。进度归 [`../status.md`](../status.md)。

越权**只计数不牵连**：`rr3_reg_note_denied` 对 `denied_publish` 做**饱和加**
（撞顶 `u32` 后停在原值，绝不回绕成 0 抹平审计线索），`rr3_reg_denied` 读数；
一次坏发布既不改发布者生命周期状态，也不影响其它连接的转发（`registry.rs` 的 `rr3_reg_note_denied` / `rr3_reg_denied`、`registry.rs` 的模块级文档注释不变量 3）。

## 9 `rr3_reg_fanout`：只做广播，**不读** `dst_task`

签名 `(frame: &Rr3Frame, exclude_task: u64) -> usize`（`registry.rs` 的 `rr3_reg_fanout`）。

| 事实 | 说明 |
|---|---|
| 筛选条件只有两个 | 条目 `src_task != exclude_task` **且** `tx` 还是 `Some`（已到期/已崩溃的条目句柄当场丢弃） |
| `frame.header.dst_task` | **原样带上、本函数不读它**。所以它**不是单播入口**（签名冻结，不改）：一帧的 `dst_task` 写着某个具体模块，实际收到的是别的活连接。要点名发给某一个模块，走 M3 的单播入口 `rr3_reg_send_to`（§10） |
| 返回值 | 成功**入队**数；接收者已 `drop` 只是少计，不算错、不 panic |
| `exclude_task` 用 `RR3_TASK_RRD` 的现值 | 1 不是任何模块的 task，等于"谁都不排除"：`conn.rs` 的注册广播与退场广播都这样调用（`rr3_handshake` 的注册广播段、`conn.rs` 的 `rr3_conn_finish`）；业务发布那次的 fanout 排除的是发布者自己（`rr3_publish` 的 fanout 段，`me.task`），watchdog 判死那次的 fanout 排除的是被判死的模块自己（`rr3_spawn_watchdog` 的广播段，`ex.src_task`），supervisor 收尸那一路的广播排除的是退出条目自己的 task（`supervisor.rs` 的 `rr3_absorb_exit`，`crashed.src_task`）——这几处都不再用 `RR3_TASK_RRD` |
| 锁与投递 | 入队发生在持有在册表锁的期间；真正的字节写盘在各连接自己的 writer 线程 → 入队次序 ≠ 上线次序，客户端按类型/字段挑帧（见 [`rrd-rpc.md`](rrd-rpc.md) §1） |

## 10 其余入口（签名即契约）

| 入口 | 返回 | 语义 | 出处 |
|---|---|---|---|
| `rr3_reg_remove(name) -> bool` | 是否真删了 | 连带摘掉 `by_task` 映射并把名字记进 `stopped`（显式退场，之后子进程退出码不得再记账成崩溃，`rr3_reg_add` 移出）；task 号**不回收**；重复移除回 `false` | `registry.rs` 的 `rr3_reg_remove` |
| `rr3_reg_live(name) -> bool` | — | 相位不是 `Failed` **且** 句柄是 `Some`，两者缺一即不活；不在册也是 `false` | `registry.rs` 的 `rr3_reg_live` |
| `rr3_reg_name_of(src_task) -> Option<String>` | — | task→名反查，测试与运维可用；**目前无生产调用点**——conn 层的 `src_task` 校验直接用连接自带的 `me.task`（`rr3_serve_frame` 的 `src_task` 校验段），`sender_name` 用登记期身份 `me.name`（`conn.rs` 的 `rr3_publish`），都不经此函数（见 §8 调用方义务） | `registry.rs` 的 `rr3_reg_name_of` |
| `rr3_reg_facts(name) -> Option<Rr3ManifestFacts>` | 入库那份的克隆 | 不在册回 `None`（不 panic、不给半数据）；spawn 序靠它取 `start_priority` 与 `entry` | `registry.rs` 的 `rr3_reg_facts` |
| `rr3_reg_own(name)` / `rr3_reg_unown(name)` / `rr3_reg_is_owned(name) -> bool` | — | 所有权标记，语义与写入/清除方见 §7.2 | `registry.rs` 的 `rr3_reg_own` / `rr3_reg_unown` / `rr3_reg_is_owned` |
| `rr3_reg_hit_cap(name) -> bool` | — | `crash_streak[name] >= RR3_RESTART_FAILURE_CAP`；为什么单开查询见 §7.2 | `registry.rs` 的 `rr3_reg_hit_cap` |
| `rr3_reg_crash_streak(name) -> u32` | — | 某 name 当前的崩溃连击数（表级、跨重注册成立）：观测面，不写字 | `registry.rs` 的 `rr3_reg_crash_streak` |
| `rr3_reg_child_exit(facts, clean_exit, now_ms) -> Rr3ChildExit` | 三态 | 子进程退出码那一路，supervisor 收尸用；语义见 §7.2 | `registry.rs` 的 `rr3_reg_child_exit` |
| `rr3_reg_snapshot() -> Vec<(String, u64, Rr3Phase)>` | — | 按 **task 升序**（不是名字序）：状态广播与运维输出处要靠它对齐 task 号，名字序只是 `BTreeMap` 的顺带结果，故显式排序。后到连接补发快照用的就是它（`rr3_handshake` 的快照补发段，排除自己同名条目） | `registry.rs` 的 `rr3_reg_snapshot` |
| `rr3_reg_callee_for(topic) -> Option<(String, u64)>` | (名， task) | M3 `rr3.call` 的被调方查找：在**相位非 `Failed` 且句柄在**、`[rpc].exposes` 含该主题的在册条目里取 **task 最小**者（同主题多提供方时的确定性裁定）；无人匹配回 `None`。唯一生产调用点 `rr3_route_call`（`conn.rs` 的 `rr3_route_call`，[`rrd-rpc.md`](rrd-rpc.md) §8） | `registry.rs` 的 `rr3_reg_callee_for` |
| `rr3_reg_send_to(task, frame) -> bool` | 是否成功入队 | M3 点名**单播**：按 `by_task` 找到那条连接的 `Sender` 投一帧（`rr3_reg_fanout` 签名冻结不动，单播由本函数承担）。不在册 / 句柄已 drop / 收件人已 drop 都回 `false`，由调用方决定回什么错。两个生产调用点都在 conn 层的 `rr3.call` 面上：转发请求（`rr3_route_call` 的投递段）与应答回路改道（`conn.rs` 的 `rr3_route_response`） | `registry.rs` 的 `rr3_reg_send_to` |
| `rr3_reg_low_power_raise() -> bool` / `rr3_reg_low_power_is_set() -> bool` | 是否上升沿 / 旗现状 | T4b 全局低电旗（0.0.2 方案 Q4 裁定）：`raise` 在**上升沿**回 `true`——调用方（conn 转发点）据此点名广播一次 `rr3-runtime/system/power`（Notification / src=rrd / dst=广播 / 体恰四字段；帧构造在 `daemon.rs` 的 `rr3_system_power_frame`，钩子序见 [`rrd-rpc.md`](rrd-rpc.md) §7）；重复 `battery/low` 只打"已生效"日志不重复广播。**只升不降**：不提供解除口——解除归充电策略（rr3-power 演进与 M6）。`is_set` 的唯一消费者是 supervisor 的 spawn 漏斗（`rr3_boot_spawn` 拒一切新 spawn：boot 与重启一问全挡；杀而不重启属关机策略，不在这里） | `registry.rs` 的 `rr3_reg_low_power_raise` / `rr3_reg_low_power_is_set`、`conn.rs` 的低电兜底段、`supervisor.rs` 的 `rr3_boot_spawn` |

## 11 相关文档

- 上行 RPC 的方法、参数与错误码：[`rrd-rpc.md`](rrd-rpc.md)
- 启动参数（`--restart-base-ms` 的上下界就钉在这里）：[`rrd-cli.md`](rrd-cli.md)
- 生命周期设计语义：[`../architecture/architecture.md`](../architecture/architecture.md) §4.2
- 主题事实源：[`../../contracts/topics.toml`](../../contracts/topics.toml)（本档不抄表）
- 实况与在制品：[`../status.md`](../status.md)
