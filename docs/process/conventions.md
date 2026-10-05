# rock3 开发规约（v0.0.1 起生效）

来源：`docs/process/dev/0.0.1版本洛克三号开发方案.md` §5。本文只收录**可机械检查**或与
评审判定直接相关的条目；架构语义以 `docs/architecture/architecture.md` 与 ADR 为准。

## 1 分支与提交

- `main` 受保护；日常在 `dev/*`。功能 `feat/<topic>`、修复 `fix/<topic>`、文档 `docs/<topic>`
- Conventional Commits：`feat|fix|docs|chore|refactor|security`（scope 用 crate/目录名，如 `feat(rr-ipc)`）
- 涉及架构决策的提交 body **必须**引用 ADR 编号（`引用：ADR-0006`）
- 一次提交只做一件事；`git mv` 类纯移动不得混入内容改动

## 2 机器可检查项（CI 七步，见 `tools/ci/check.sh`）

| 步 | 检查 | 命令 |
|---|---|---|
| ① | workspace 编译（含测试目标） | `cargo check --all-targets --workspace`（CWD = `base/runtime`） |
| ①b | workspace 全量测试 | `cargo test --workspace`（CWD = `base/runtime`） |
| ② | manifest schema + 注册表形状 pin | `cargo test -p rr-registry --test contract` |
| ②b | mkmod 输出即契约用例（方案 P3） | `cargo test --manifest-path tools/mkmodule/Cargo.toml` |
| ③ | 主题前缀归属 | `cargo test -p rr-registry topics_prefix` |
| ④ | 仓内所有独立 Rust 工程编译 | 逐工程 `cargo check --all-targets --manifest-path <dir>/Cargo.toml`（`modules/` + `tools/` + `base/ota/`，排除 `base/runtime/*`） |
| ⑤ | 文档门：已跟踪 md 不得为空、相对链接不得断、README 不得漏进导航 | 遍历 `git -c core.quotepath=false ls-files '*.md'`（**排除 `legacy/`**：那是冻结区，其内 README 允许残留 1 处已知旧路径，见 `docs/status.md`；不排除只会逼人改冻结区或加豁免名单），逐个判 `[ -s ]`，并取 markdown 链接括号里的相对路径目标判其是否存在；再把 `docs/README.md` 的链接按 `docs/` 归一成仓根路径，逐个已跟踪 README 对号（`docs/README.md` 自身是导航本体，不参与被检），未出现在导航里的打印 `导航漏链  <path>`；违规逐条点名后非 0 退出 |

入口脚本的标题写作"规约三检查"（沿用方案 D7 的三条语义命名），实际执行的是**七步**：
②b 是 ② 的姊妹步，把生成器的产物喂回同一份 manifest 契约，防 mkmod 与 CI 漂移；
①b 把 workspace 的 237 条（Windows，推算）/294 条（Unix）测试全部跑一遍，②/③ 只是其中被点名两批的定点诊断；
④ 覆盖仓内**所有独立 Rust 工程**（`modules/` 五个 + `tools/mkmodule`、`tools/rrctl`、
`tools/sim`、`base/ota/rr3-ota-ctl`）——它们各自是独立 workspace，①/①b 打不到，
没有 ④ 的话这些目录烂掉无人知晓（无 `Cargo.toml` 的目录如 Python 的 rr3-brain 自然跳过；
`base/runtime/*` 由 ①/①b 负责，④ 显式排除以免重复）。
⑤ 是文档门（空 md、断链与导航漏链），三条判据与 `legacy/` 的排除理由都写在上表那一行。
测试步（①b/②/②b/③）除退出码外还断言"至少一个测试真实通过"——cargo 对打空的过滤器仍以 0
退出，只看退出码会在过滤器笔误或测试集改名时静默放行。

测试面如实登记（2026-09-30 M2 Task 11b 收口（`rrd` 加**唯一的** supervisor 线程（`supervisor.rs`：
boot spawn、串行收尸 `try_wait`、到期重启与"先杀挂死旧世再起"的唯一漏斗）+ CLI `--boot-role` +
`Rr3Beat::Promoted` 处 `running` 第一次上线广播（E-8）+ 新测试目标 `rrd/tests/e2e_spawn_restart.rs`
5 条 + support 的 boot 起法与 `rr3_field_from_log`/`rr3_wait_until` 两个助手）后复测。
**本轮新增的是"真起子进程、真等退避、真重启"这一类证据**——上一轮"时间判定"与上上轮
"跨进程夹具"那两类的描述原文照旧适用。**两个平台的条数本就应当不等**，所以各记各的、
不合并成单一数字：Windows x86_64-pc-windows-msvc **177**（M1 板上就绪轮在 Task 11b 的 176 之上
再加 `rr-ota` 净增的 1 条 include_str 分区标签守护用例 `ota_labels_match_genimage_cfg`——
平台无关，两平台同加 1）、
WSL2 Linux x86_64 **212**（M1 轮 212 条 = Task 11b 实跑 211 + 同一条守护用例；逐目标相加而非算术倒推：
rr-bus 4 + rr-hal 4 + rr-ipc lib 26 + rr-ipc `envelope_meta` 4 + rr-ota 5 + rr-registry lib 15 +
契约 24 + rrd lib 96 + `src/main.rs` 0 + `src/bin/sim_peer.rs` 0 + `e2e_sim_peer` 5 +
`e2e_spawn_restart` 5 + `e2e_watchdog` 4 + `register_publish` 19 + `socket_banner` 1 = 212；
Task 10 记 197、Task 11a 记 205、Task 11b 记 211）。
`aarch64-unknown-linux-gnu` 交叉 + qemu-user
本轮未重跑，沿用上一轮"与 Linux x86_64 同数"的记录，不据此声称本轮。两数差额 35 条全部可枚举：
Unix 独有的 34 条线级用例（`register_publish` 19 + `socket_banner` 1 + `e2e_sim_peer` 5 +
`e2e_watchdog` 4 + `e2e_spawn_restart` 5）+ `cli::tests` 里那 1 条
`#[cfg(unix)]` 用例。cargo 1.95—1.97：
`#[cfg(unix)]` 的代码在 Windows 上**不参与编译**，故 Windows 条数只覆盖平台无关逻辑，
Unix 侧行为不据此声称；34 条线级用例在 Windows ①b 里显示 `running 0 tests`，那是 cfg
边界的正确结果而不是通过）：
①b **177** 条（Windows）/ **212** 条（Linux x86_64；`base/runtime` workspace 全量，含 rr-ipc 的 30 个
契约守护——13 条偏移/枚举对齐 + 帧层 8 条 + `protocol::body::tests` 的 5 条 payload 类型前缀
（M2 Task 7：前 2 字节小端 `Rr3PayloadType`，round-trip / 截断 / 未知值各一条）
+ `tests/envelope_meta.rs` 的 4 条**读 JSON**
的元数据守护（`x-byte-size` / `x-offset-sum` / `payload_len.minimum|maximum` / `x-schema-version`）；
rr-registry 的 39 条 = lib 15（schema 4 + 主题集 `topics::tests` 6 + 按文本入口
`validate::text_entry_tests` 5）+ 契约 24（M2 Task 6 给 `Rr3TopicSet` 加 `rr3_topic_set_kind`
访问器，实证并进了既有的 merge 用例，故条数不变）；
rrd 的 95 条（Windows）/96 条（Unix；差的那 1 条是 `cli::tests` 里 `#[cfg(unix)]` 的绝对路径幂等用例）
全在 lib 目标（`cli::tests` 13/14 + `topics_load::tests` 16 + `manifest::tests` 7 + 横幅 2
+ `registry::tests` 47 + `rpc::tests` 10；2026-09-29 修复轮新增 5 条：叠加层路径绝对化 2、`rr3-runtime/*` 冒充在装载处被拒 1、
形状错误标注来源 1、路径值不得以 `-` 开头 1；M2 Task 4 再补 `manifest::tests` 7 条；
M2 Task 5 补在册表/状态机/发布归属 29 条与合并后复验 1 条；
M2 Task 5 修复轮 1 再补 `registry::tests` 3 条：在册 owner ≠ 首段时的越权终判 1、
`watchdog_ms = 0` 入库即拦（仅 debug 构建）1、`--restart-base-ms 0` 的 `delay_ms` 现值 1；
M2 Task 6 补上行 RPC `rpc::tests` 10 条，同批 carry-forward 再补 4 条——契约 topic 的 kind
被 merge 改写时在装载处被拒 1、登记 owner 缺失时越权文案回显 `?` 1、三条拒绝门先于
watchdog 断言（仅 debug 构建）1、`rr3_reg_facts` 的取值回显 1；
Task 6 修复轮 1 再补 2 条——两处 `ForeignPrefix` 构造点的 owner 回显都为 `?`（前缀不符 /
owner 比对两支同测）1、被捕获的 watchdog panic 不得改动在册表（仅 debug 构建）1；
Task 7 的 carry-forward 再补 `cli::tests` 4 条——`--restart-base-ms` 下界 1（0 = 退避恒为 0 =
热重启循环）1、上界由退避表末档导出（`u64::MAX / 60`，过界即饱和成"永不重启"，须在启动时拒）1、
默认套接字取自**注入的表**而非进程环境（`ENV_LOCK` 随之一并删除）1、生产入口 `rr3_cli_parse`
仍委托进程环境读取（只读断言，不 `set_var`）1；CF-3 再补 `cli::tests` 2 条——待创建的套接字路径
**不存在**也必须解析成功（这条钉住 `rr3_socket_absolute_path` 用 `std::path::absolute` 而**不是**
`fs::canonicalize`：bind 之前套接字文件必然不存在，canonicalize 会以 ENOENT/os error 2 拒掉一个合法值；
把实现换成 canonicalize 该用例即实测变红）1、已经是绝对路径时原样返回（`#[cfg(unix)]`，幂等——它是
"横幅、`UnixListener::bind`、模块侧看到的 `RR3_BUS_PATH` 是同一个字符串"的前提）1；
CF-4 再补 `topics_load::tests` 1 条——契约路径进装载入口即被 `fs::canonicalize` 规范化，
坏契约的致命文案里不得漏出 `..`（`RR3_CONTRACT_TOPICS_PATH` 由 `CARGO_MANIFEST_DIR` 拼出、
字面就带 `..`，而 `main.rs` 只绝对化叠加层那一条；运维照报错里的 `..` 去查是按他自己的 CWD 展开的）；
这条与 CF-3 恰好用**相反**的工具，理由写进 `rr3_contract_resolved` 的注释：契约必须已存在，
ENOENT 正是该报的那一条，而 bind 前的套接字用 canonicalize 会误拒合法值。
同批既有 15 条 `topics_load` 用例的**逐字文案断言**全部原样通过（含两条全等断言的
`overlay_conflict_refuses_startup_and_names_the_entry` / `overlay_declaring_reserved_namespace_is_rejected`），
故规范化只改了契约那一条路径的来源，没有改写任何错误词汇。
**这后半句到 M2 Task 8 不再成立**：遗留 ③ 把**叠加层**那条路径也收进装载入口规范化，
`overlay_declaring_reserved_namespace_is_rejected` 的全等断言随之从"CLI 原词"改成
"canonicalize 后那一份"（用例本身没增删，仍是 16 条 `topics_load::tests`），并额外断言
文案整串不含 `..`；契约那一支的文案断言仍逐字未动。
Task 8 另补 `registry::tests` 1 条（遗留 ②）——`rr3_reg_add` 容量门里 `superseded.is_none() &&`
那半句**承重**：表满 + 同名条目已是 Failed 时必须放行重注册（旧条目即将腾出槽位），
删掉这半句它就变成 `Capacity` 拒。Task 5 的 17 组变异实验里这一支变红而全绿无人察觉，
因为当时没有任何用例把"表满"与"同名 Failed"两个条件同时凑齐；新用例
`full_table_still_allows_re_registering_a_failed_module` 就是那个组合，另带两条对照
（满表上的新名字仍被容量拒、满表上的同名非 Failed 仍回 `Duplicate`）。
M2 Task 10 补 `registry::tests` 1 条——`Rr3Expiry` 必须回显**判定用的那两个数**：`elapsed_ms` 是
`now - last_beat`（不是 `now - 注册时刻`）、`watchdog_ms` 是**该模块自己**的值（不是全局常量）。
线程侧那行判死日志与线级断言都从这两个数派生；表里少回显一个，线上便只能靠掐时间判对错，
而掐时间正是"随机红"的来源。同批补 `cli::tests` 1 条——`--watchdog-tick-ms` 默认 200、可配、
**拒 0 而不是夹 1**（0 = 每拍都扫 = 忙循环，那是另一种错法）；并把既有默认值用例扩成三值同钉
（`restart_base_ms` / `pre_register_timeout_ms` / `watchdog_tick_ms`）。
M2 Task 11a（崩溃记账收口）净增 `registry::tests` 8 条（39→47）：删掉旧的 `always_policy_backs_off_and_caps`
1 条、补 9 条把"唯一一本账"钉死——`crash_streak` 按 **name** 计、跨重注册连续不归零（这是旧 `Entry.restarts`
被同名替换归零、5 次上限今天撞不到的根因）、退避表 `RR3_RESTART_BACKOFF` × `restart_base_ms` 且饱和不回绕、
`RR3_RESTART_FAILURE_CAP = 5` 到顶即停、`rr3_charge` 是三条死亡路（conn EOF、watchdog 到期、子进程退出码）
唯一共用的记账入口（双路只记一次账）、活满一整条自己的窗口才归零、达上限停止重启。
**D-6**：`rr3_reg_beat` 返回值从 `bool` 改成三态 `Rr3Beat{Rejected, Kept, Promoted}`，只有 `Promoted`
那一次是 `registered → running` 的升级。**相位面（本轮边界）**：`running` 今天**只落到日志面**
（`conn.rs` 打 `{name} → running` 这一行钉住"唯一升级时刻"），`rr3-runtime/module/state` 上那条
`running` 广播**留给 Task 11b**——上线广播属"广播几条"的判据面，11a 不碰。**11b 已上线**：
`Rr3Beat::Promoted` 处 `rr3_state_frame(name, Rr3Phase::Running.as_str(), task)` + `rr3_reg_fanout`，
日志行 `{name} → running（广播到 {n} 个连接）`，判据在 `e2e_spawn_restart` 的 S1/S2/S3。
`Degraded` 分支归 `Kept`
（只续命不升级），结掉"一颗心跳抹平降级"的欠账。**D-8**：`Rr3Expiry` 新增 `restart: Option<Rr3RestartPlan>`
（到期那一路的记账结果），但 11a 的 daemon 侧 `let _ = ex.restart;` **收下不使用**、绝不据此打
"重启计划"日志——一旦打了就当场打死 `e2e_watchdog` 的 W4（判据是观察窗内 stdout 不得出现"重启计划"），
`supervisor` 派生重启归 11b。**本轮终态的欠账（degraded 死相位）**：degraded 是"有 enum 变体、无写入方"的
死相位——它的 setter 与 base 能力门（`depends.required` 的 `bus.publish`/`bus.subscribe`/`bus.rpc` 就位判定，
spec §5.3）都不在 11/11b，一个要新相位上线、一个要给 24 条生产主题建能力字典，均不阻塞 §9 验收；
这句话同时写在 `registry.rs` 文件头与本台账，不许被任何"已实现状态机"的措辞盖过去。
11a 对生产代码的**唯一**非 registry 改动是 `conn.rs` 的 `rr3_reg_beat` 调用点与 `rr3_reg_death` 返回类型适配
（`Rr3Death` 三态），四个 e2e 文件一条用例未改；`conn.rs`/`daemon.rs` 是 `#[cfg(unix)]`，故 Task 11a 的一处
同线程重入自死锁（把 `rr3_lock(..).rr3_reg_death(..)` 直接写进 `match` 被匹配表达式，守卫临时量延寿到整个
match 语句、`Broadcast` 分支再 `rr3_lock` 同一把锁）Windows L1 编不到、看不见，只有 L2 的 3 条 e2e 超时把它抓出——
`running 0 tests` 是盲区不是绿的又一记实证。
M2 Task 11b（supervisor：boot spawn + 按策略重启 + `running` 上线）净增 `cli::tests` 1 条
（`--boot-role` 默认 `resident`、原样透传不校验取值、空值与疑似开关值都拒）与新目标
`rrd/tests/e2e_spawn_restart.rs` 5 条。**S 组判据口径**：S1 DoD 主链（boot `spawn rr3-sima pid=A` 无
attempt 尾注 → 线上 `registered` → 线上 **`running`**（E-8 第一次上线；第二世的 `→ running（广播到 2 个连接）`
写数字断言，第一世不钉广播数——boot 队列在 accept 前投完，旁观者注册与第一颗心跳谁先谁后是两种合法时序，
如实记录）→ `子进程退出 status=signal: 6 → 广播 failed` → 线上 `failed` → `spawn … attempt=1` → 第二世
`registered`+`running`，pid/task 全部 `rr3_field_from_log` 抽取后 `assert_ne!`，不写死字面值（A-5①））；
S2 所有权对照（同一 `abort` role：owned 由退出码定 `failed`、foreign 只有 EOF → `unregistered` +
`跳过重启`，**这条是"崩溃检测按所有权分级"的唯一证据**）；S3 `never` 策略（一条 `failed` 广播 + 三条否定
`spawn rr3-simb`/`重启计划`/`跳过重启` 各钉一层，四文案互不重叠）；S4 watchdog 到期路（判死 →
`杀掉挂死的 rr3-sima pid=A` → `spawn … attempt=1` 新 pid → 第二世**又**被判死——后半否定钉"第一世的
fd/条目确实让位"）；S5 孤儿卫生（`drop(rrd)` 后有界轮询 `/proc/{pid}` 消失，`rr3_wait_until` 内容判据）。
**既有 28 条线级用例只改写简报点名的两条**（其余 26 条一字未动）：W4 改成正反一对（正面等
`跳过重启 rr3-sima` + 负面 300ms 窗内不得出现 `spawn rr3-sima`，`attempt=` 护栏保留）；
`killed_peer_is_only_an_eof_until_task_11_holds_the_child` 改名
`killed_foreign_peer_is_only_an_eof_and_its_plan_is_skipped` 并正面补 `跳过重启 rr3-sima`——没有这条正向，
`sup.rr3_restart(p)` 接线断了与接线在但被判外来分不开。**两处定向变异**（L2 实跑、每次只改一处、复跑后复原）：
①去掉 `rr3_reap` 的 `rr3_reg_child_exit`（只打日志不记账）→ S1/S2/S3 全红，红点在各文件"广播 failed"
断言行（`e2e_spawn_restart.rs` 的 106/148/190）。**与简报的偏差如实记录**：简报预期 S1 红在 `attempt=1` 行，
实际"不记账"同时拿掉 `failed` 广播，S1 判据链在更早的断言处断开；**S4 没有红——真实 finding**：S4 的重启
计划来自 watchdog 到期路（`rr3_reg_expire` 记账 → `Rr3Expiry.restart`），旧世子进程由 `rr3_boot_spawn` 的
kill+wait 当场收掉、从不经过 `rr3_reap`，故 S4 对"退出码那一路在记账"不承重，该判据的承重用例是 S1/S2/S3。
②去掉 E-1 的 `jobs` 成员判据（外来模块也起）→ W4 与 `killed_foreign_peer_…` 红（3s 等待超时的日志 dump 里
正多出 `spawn rr3-sima pid=… attempt=1` 一条），顺带暴露配套事实：外来子进程进了 `children` 后
`rr3_reap` 的 `expect("children 里的 name 必有 job")` panic、supervisor 线程死掉——E-1 破口的账在记账面同样有。**所有权边界一句话**：
只有本守护起的模块（`jobs` 由 `--manifest` 填）才会被 rrd 重启；外来自注册的照样记账、照样出计划，
执行时只有一行 `跳过重启 {name}：不是本守护起的进程`（spec §5.4"自动重启要求 rrd 是父进程"；代价是
外来永不可被 rrd 重启，见 E-1/E-2）。**conn.rs:216 同形残留**（11a 评审点名）本轮自加项修掉：
`rr3_reg_add` 不再写进 `match` 被匹配表达式，改先 `let added = …` 绑定。`e2e_spawn_restart` 在 L2
连跑 10 次 **10/10**（每次 5 passed，1.15–1.18s 无漂移）——本目标全项目唯一"真起子进程 + 真等退避"，
负载敏感性最高，m-8 的教训记在这里。
M2 Task 7 的 UDS 层没有新增 lib 用例（`conn.rs`/`daemon.rs` 是 `#[cfg(unix)]` 的线程与 IO，
纯逻辑仍在 `registry.rs`/`rpc.rs` 里已被钉住；Task 8 是唯一动到 lib 的那一支——上面那段
`registry::tests` 的容量门实证 1 条，其余全在测试目标里），它的 10 条（Task 8 之后 20 条）在两个独立测试目标
`rrd/tests/register_publish.rs`（顶层 `#![cfg(unix)]`，只在 Linux/aarch64 真跑：task 号与上线广播、
重复名 EBUSY、保留名 EPERM、坏 manifest 点名字段、首帧必须 `rr3.register`、`src_task` 伪造拒帧不断连、
上行只许 Request、未注册连接按读超时收掉、`rr3.unregister` 干净退场不重启，共 9 条；
修复轮 1 再补 3 条门禁线级用例——`conn.rs:149` 首帧 `dst_task` 非 rrd 时拒帧、
`conn.rs:262` 注册后 `dst_task=0` 上行帧按崩溃收尾不回 accepted、
`conn.rs:87` `rr3_reply_ok` 的应答 dst 点对点不广播，共 **12** 条；
M2 Task 8 的发布面再补 7 条，共 **19** 条——合法发布被转发给别的连接且**不回 ACK**
（头三项逐个断言：`src_task` 是 rrd 分配给发布者的号、`dst_task=0`、体原样）、
越权前缀 → EPERM 且点名 owner（两个变量：在册的 `rr3-simb/pong/state` 与既不在册、
首段也不是自己的 `rr3-simb/pong/typo`，后者才让三层顺序里"前缀"那一道在线上级承重）、
冒充 `rr3-runtime/*` 两支 → EPERM 且文案含"冒充"、在册但主题没登记 → EINVAL"未注册主题"、
**不带 `--topics-overlay`** 时夹具主题根本不在册而生产主题由 owner 挡住（K4：叠加层不是生产
放行口）、`Event` 体缺 `topic` 只回错误帧且同一条连接之后照常喂狗（裁定 R5：不按崩溃收尾）、
两条不同 seq 的转发在收侧**按内容逆序**挑帧再回头挑一条 Response（遗留 ⑤：Task 8 之后同一条
连接上同时有应答/状态广播/发布转发三类帧，顺序无关性得再证一型））与
`rrd/tests/socket_banner.rs`（CF-3 新增 1 条：子进程 CWD 设为 `/tmp`、`--socket` 只给文件名，
断言**版本横幅的 bus 字段与"监听 …"行都回显解析后的绝对路径**，并且按那个路径真能连上、注册得了——
只断言"字符串像绝对路径"抓不到"横幅与 bind 各说一套"这个原始缺陷）与
`rrd/tests/e2e_sim_peer.rs`（M2 Task 9 新增 5 条**跨进程**用例，客户端是另一个二进制 `sim_peer`
而不是测试自己：注册应答里的 `src_task` 与 rrd 日志里那个分配值逐字相等并连喂 3 次心跳被接受、
发布被转发给**早已在册**的旁观连接（体原样、类型仍是 Event、`src_task` 是发布者的分配值）、
主动 `rr3.unregister` → 恰好一条 `unregistered` 广播且 `never` 策略不产生重启计划、
**特征化**用例"被打死的 peer 今天只是一次 EOF"（`unregistered` + `重启计划 attempt=1 delay_ms=20`
+ 回显 manifest 里那条绝对 entry；夹具在 `abort` 前先把接收队列排空——队列里留着未读的广播帧时
内核会把这次关闭改成 RST，rrd 读到的就不是 EOF 而是 ECONNRESET、广播相位随之变成 `failed`，
实测不排空时该用例 10 次红 1 次；那一维量的是"进程有没有把 socket 读干净"，不是"有没有体面退场"，
FIN/RST 双路连同退出码一起归 Task 11。Task 11 拿到退出码后必须改写本条并在提交里点名）、
同一个二进制配两份 manifest → 两个身份而 `RR3_SIM_PEER_ROLE` 只改行为不改身份。
身份的唯一来源是 `RR3_MANIFEST` 指向的那份 `tests/fixtures/sim{a,b}/module.toml`，
两份的 `entry` 是占位符 `@SIM_PEER_ENTRY@`，交 rrd 之前才换成构建产物的绝对路径——仓里不留假路径）与
`rrd/tests/e2e_watchdog.rs`（M2 Task 10 新增 4 条**时间判定**用例，客户端同样是 `sim_peer` 真进程——
"不喂狗但 fd 还开着"这个形状只有真进程维持得住，手搓帧的用例一注册完就 EOF，watchdog 根本无从判活：
静默 peer 被 `watchdog 判死 rr3-sima task=…` 点名，并且把**日志里那两个数**读回来断言
`elapsed ≥ 500` 与"≥ 超时 500ms"（把"判得对不对"钉成内容判据，而不是掐时间）；
健康 peer 连喂 3 次、同一时刻静默 peer 已被点名，而 `watchdog 判死 rr3-simb` 在 300ms 观察窗内
**一条都没有**（`rr3_assert_no_log` 是否证写法，不是"sleep 之后还没死"那种猜时间），
且两条连接都还活着——判死改的是在册相位，不关你的 fd（spec §5.0 只说"超时即判 crash"，
断开连接是另一件事，本轮没做也不该被顺带做掉）；
**恰好一条** `failed`：判死之后再跨 3 拍（60ms，跨度由已知 tick 导出而非猜行为）按**内容**挑
`phase=="failed" && module=="rr3-sima"` 的 Notification 计数 `== 1`，日志侧同样不得出现第二条判死行；
判死后连喂 5 次心跳**全部被拒**且**不复活**——只判第 5 次那颗（那一刻已静默 ≥780ms，远超到期上界
520ms，"被拒"不是运气；前四次落在哪一拍是合法时序，不判），且**整个 300ms 观察窗内 rrd 一条
"重启计划"都没打**（`rr3_assert_no_log`，与 `register_publish` / `e2e_sim_peer` 同一条否证写法）：
到期路径今天不查 `restart_policy`、不产重启计划，这笔欠账显式留给 Task 11，钉在这里是为了让它不能被
悄悄宣布成"崩溃检测已完成"。判死行里不含 `attempt=` 只是**第二道**护栏（防"把计划塞进判死行"那种
写法）——它自己钉不住这件事：判死行的格式里本就没有那个字段，任何实现下都不可能红，而 Task 11
最自然的接线是另起一行打计划，那时只有窗口否证会红。**没有第 5 条**（brief 的 W5"CLI 的 tick 接到了线程"）：它与 support
末尾那句 `rr3_wait_log("watchdog 启动 tick=…ms")` 是同一条判据——本文件把它钉在 `20`、既有
19+1+5 条线级用例把它钉在 `3_600_000`，两个互不相同的值都只能由 CLI 传进来才对得上（线程里硬编码
200、或把 `pre_register_timeout_ms` 当 tick 传，会让这两批同时超时红），再加一条只断言
"套接字文件还在"的用例不新增任何判据，按"不许空转断言"就不写。四条的等待**全是内容判据**
（`rr3_wait_log` 3s / `SimPeer::rr3_wait` 5s），唯一的时长断言在第一条且是粗上限。
**时间能进判据邻域而不让整套用例变随机红**，靠的是两处口径各收在一处：
`RR3_QUIET_WATCHDOG_TICK_MS = 3_600_000` 让**既有**每一批线级用例里的 watchdog"起了但不扫"
（Task 10 的 diff 不得改变 Task 7/8/9 用例的可观察行为——真按生产 200ms 起跑会在它们中间插入
判死日志与 `failed` 广播），只有 `e2e_watchdog` 走 `rr3_start_with_tick` 显式要小 tick；
`E2E_TICK_MS = 20` 与夹具 manifest 的 `watchdog_ms = 500`（一起决定"到期上界 520ms"这条算式）
都放在 support 而不是测试文件里，两处各写一个数就是两处真值。
`rr3_collect_within` 本轮加 **5s 墙钟上限**（`COLLECT_WALL_CLOCK_CAP`）并在 panic 里点名已收帧的
kind：一条连接上有**两个写者**（RPC 应答由该连接的读线程写、状态广播由该连接的写线程写），
帧整体是原子的但**相对顺序无保证**，所以 brief 里 A-1 那种"先收一条 RPC 应答当栅栏帧、
再断言状态广播已到齐"的写法**不成立**、被否掉；替代口径只有"按内容挑帧 + 墙钟兜底"——
广播停不下来这类形状过去会卡在 64 帧的扫描预算里出不来（整个目标表现为超时），现在是一条点名帧流的
panic。四条判据的非空转由三处定向变异实证（本轮记录，逐条对应实跑）：去掉 `rr3_reg_expire`
的 skip-failed → 第三条红（并触发 `COLLECT_WALL_CLOCK_CAP` 的 panic，顺带把那条上限也实证了）；
watchdog 判死但不广播 → 第一/三条红；广播里的相位写错 → 第一/三条红。控制器另独立复跑第四处：
把 `rr3_reg_fanout` 重复调用一次（同一拍发两条 failed）→ 第三条红并报"3 条"，
即"恰好一条"这条判据抓得住重复回报。
（每次变异只改一处、复跑后即复原）`e2e_watchdog` 的时间余量由 fixture 的
`heartbeat_ms 100 / watchdog_timeout_ms 500` 与 `E2E_TICK_MS = 20` 共同决定
（最坏情况每 ~400ms 一次判死），高负载下是真实到期而非测试缺陷，Task 11 的 `task=` 号段断言
与 EBUSY 窗口复评时同批处理。）；
本任务使 `docs/status.md` 的"watchdog 线程还不存在 / 没有 tick 循环"与
`docs/reference/rrd-registry.md` 的 `rr3_lock` 定位（`conn.rs:61-64`）、`Rr3Expiry` 字段清单
三处断言失效：前者是在途文件（本轮归属纪律要求不动别人的文件），后者随 Task 13 文档收口一并改写，
本轮不据此声称已同步；
`src/main.rs` 自 M2 只剩"参数 + 装载 + 平台门 + 把主循环交棒 `daemon`"、其 `[[bin]]` 目标 0 条——纯逻辑放进 lib，
后续的 `rrd/tests/*.rs` 才引用得到（bin 里的模块对外不可见）；M2 Task 9 的第二个 `[[bin]]`
`src/bin/sim_peer.rs`（测试夹具二进制）同样 0 条，它的 5 条全在 `e2e_sim_peer` 目标里跨进程跑。
它落在 `rrd` 包内而不是 spec §8 影响面清单写的 `base/runtime/tests/sim-peer/`，唯一理由是测试要
`env!("CARGO_BIN_EXE_sim_peer")` 这个**只对同包 bin 生效**的绝对路径（独立 workspace 成员只能靠
`current_exe()` 往上猜 target 目录，那是一份假路径）；夹具的**数据文件**（两份 `module.toml` 与
`sim-topics.toml`）仍在 `base/runtime/tests/fixtures/` 下，spec R5 的"不进 modules/、
不进 contracts/topics.toml"不受影响；落点偏离的文字同步归 Task 13；
`contracts/README.md` 称那批用例是契约的执行入口，若 ① 只是普通 `cargo check`，
它们连编译都不做，更谈不上执行）；② **24** 条（含 `registry_shape_is_pinned` 的注册表形状
硬 pin：7 owner / 28 topic / kind 11-10-3-4 / 28 条三元组——0.0.4 批D 随 connect
wifi/scan|connect|forget append 同步（0.0.3 批C 曾随 `rr3-act/relay/state`）；`every_module_manifest_passes_contract`
钉住 modules/ 六模块集合且每份 module.toml 过契约；`every_owner_path_exists` 钉住 topics.toml
每条 owner.path 目录真实存在）、③ **5** 条（均为 ①b 的子集）、
②b **5** 条（`tools/mkmodule` 是独立包，不在 workspace 内，故 ①b 覆盖不到它）、
④ **9** 个独立工程（modules 五模块 + rr3-mkmod + rr3-ctl + rr3-sim + rr3-ota-ctl；
rr3-brain 为 Python，不参与 cargo 检查，其语法检查属人工检查点）。
① 用 `--all-targets` 而非 `cargo check`：普通 check 不编译 `#[cfg(test)]` 目标，
实测在测试代码里注入一个类型错误，`cargo check --workspace` 照样退 0。
M2 起随运行时语义落地再扩点名范围。

测试面如实登记（2026-10-01 M3 音频链收口复测。本轮 ①b 在 macOS x86_64（Unix）实跑 **249** 条，
逐目标相加：rr-bus 10（lib 4 + `client.rs` 新增 6——同步总线客户端的连接/注册/心跳/发布/收帧/应答）+
rr-hal 13（lib 5 + `sim.rs` 7 + `backend.rs` 1——`Rr3AudioSource`/`Rr3AudioSink` 源汇对称、
sim 三种波形、`RR3_HAL_BACKEND` 选择与 `rr3_block_on`）+ rr-ipc lib 38 + `envelope_meta` 4 +
rr-ota 5 + rr-registry lib 15 + 契约 24 + rrd lib 101（`cli::tests` 14 + `topics_load::tests` 16 +
`manifest::tests` 8 + 横幅 2 + `registry::tests` 49 + `rpc::tests` 12）+ `src/main.rs` 0 +
`src/bin/sim_peer.rs` 0 + 线级 `e2e_audio_chain` 1（三真进程音频链）+ `module_call` 4（rr3.call 回路）+
`e2e_sim_peer` 5 + `e2e_spawn_restart` 5 + `e2e_watchdog` 4 + `register_publish` 19 + `socket_banner` 1
= 249）。**rr-ipc 台账勘误**：上一轮（M2 Task 11b）把 rr-ipc lib 记作 26，漏记了 `buf.rs` 6 条、
`uds.rs` 5 条与 `frame.rs` 的第 9 条——锚点 `c49b085` 的树上代码本就带 38 条（本轮实测与逐文件
`grep -c '#\[test\]'` 核对一致），故上一轮两平台实数应读作 Unix **224** / Windows **184**，
"212/177"是台账漏记而非代码回退。本轮两数：Unix **249**（224 + M3 平台无关新增 20：rr-bus 6 +
rr-hal 9 + rrd lib 5——`registry` 2 / `manifest` 1 / `rpc` 2——+ 线级 Unix 独有 5：`e2e_audio_chain` 1 +
`module_call` 4）；Windows **204**（未实跑，按 184 + 平台无关 20 推算）。差额 45 条全部可枚举：
旧 Unix 独有线级 34（`register_publish` 19 + `socket_banner` 1 + `e2e_sim_peer` 5 + `e2e_watchdog` 4 +
`e2e_spawn_restart` 5）+ `cli::tests` 里 1 条 `#[cfg(unix)]` + `uds.rs` 整模块 `#[cfg(unix)]` 的 5 条 +
本轮两个新测试目标整体挂 `#![cfg(unix)]` 门的 5 条。M3 新增的六个测试模块（`client.rs` / `backend.rs` /
`sim.rs` / `registry.rs` / `manifest.rs` / `rpc.rs` 的增量）无一挂平台门，Windows 上照常参与编译。
②/②b/③/④ 本轮无变化（仍是 ② 24 / ②b 5 / ③ 5 / ④ 9 个独立工程）。

测试面如实登记（2026-10-01 F1 依赖仲裁复测。rr-registry 净增 **10** 条，全部平台无关、
两平台同加：lib 15→20——`depends::tests` 5 条（事实提取与 `start_priority` 缺省 100、
缺名与非字符串项逐条点名、同名先行拒绝、层内 `(priority, name)` 确定性次键、
自环整批拒且无辜者不点名）+ 契约 24→29——`manifest_unknown_dependency_rejected`
随 ADR-0009 裁定 1 **迁移**为 `manifest_unknown_dependency_moved_to_resolution_layer`
（单件层放行、仲裁层点名拒"未知能力"，迁移非放宽，条数不变），另新增 5 条：链式
拓扑序与层内次键、环+环下游整批拒、base 能力无需模块提供者、多提供者全排前、
`every_module_dependency_set_is_satisfiable_and_acyclic`（CI 全集补位——单件校验
不再判可满足性后，modules/ 六模块的整体依赖面由这条在 CI 层兜住）。本轮 ①b 推算：
Unix **259**（249 + 10）/ Windows **214**（未实跑，按 204 + 10 推算）。
② **29** 条；③/②b/④ 无变化。语义裁定全文见 ADR-0009）。

测试面如实登记（2026-10-01 F2 依赖仲裁实装复测（rrd boot 接仲裁 + Degraded 相位接线，
语义裁定全文见 ADR-0009）。净增 **10** 条：rr-registry lib 20→23——`depends::tests` 3 条
（`plan_boot` 三分流：不可满足只拒点名模块、被拒模块的 provides 仍计入可满足性宇宙故
下游不连坐、环 fatal 不产出半死集合）+ rrd lib 101→105——`manifest::tests` 1 条
（`[provides].capabilities` 可选段且逐项类型检查）+ `registry::tests` 3 条（提供者全灭才降、
双提供者死一不降、恢复须**全部** required 就位且重注册换新 task）；线级新目标
`rrd/tests/e2e_depends.rs` **3** 条（整体 `#![cfg(unix)]`，Unix 独有）：D1 链式启动序
（dependent 的 `start_priority=10` 故意低于 provider 的 50，拓扑主键压过优先级——旧的全局
优先级排序此处是反的；spawn 行由唯一 supervisor 线程串行打出，按行内位置断言序，注册序因
两个子进程并行 exec 不可用作序证据）、D2 环拒 boot（直接 `Command` 起 rrd 读 stdout 到 EOF，
断言非 0 退出 + `拒绝 boot：`/`依赖环涉及` 点名环成员 + 一个子进程都没起）、D3 降级→恢复
（提供者被测试侧 SIGKILL——死亡时刻收进测试手里，判据无时间赌注，watchdog 全程静默；
依赖者线上注册手动喂到 Running → kill → `依赖断供 → degraded` 且**依赖者自己**收到那条
degraded 帧 + `重启计划` 否定针（裁定 4）→ always 自动重生 attempt=1 → `依赖恢复 → running`）。
**本轮实跑勘误与语义补裁**：① 实装时发现 `plan_boot` 首版把"剔除后重仲裁"做成 fatal——
仲裁引擎的可满足性循环没跳过已剔除模块，同一错误第二轮再报、剔除集合零增长走防御分支；
已修（跳过剔除者），两平台同修、无条数变化。② 降级/解除广播的排除项从依赖者自身 task
改为 `RR3_TASK_RRD`（不排除，E-9 同一理据：依赖者恰恰是最需要知道"我已降级"的一方，
且它没有别的途径查自己的相位），三处调用点（daemon/watchdog、supervisor/收尸、conn/解除）
统一，D3 的线上读帧钉住。③ 既有表级用例 `provider_recovery_undegrades_only_when_all_deps_satisfied`
首版期望写反（按"全灭才降"写）——降级是**断供即降级**的非对称设计（任一 required 能力
失去全部提供者即置位，解除才要求全部就位），已按实现与 ADR 改正断言并把"已 Degraded 不重复
传播"一并钉入。本轮 ①b：Unix **269**（实跑 = F1 的 259 + 10；逐目标：rr-bus 10 + rr-hal 13 +
rr-ipc lib 38 + `envelope_meta` 4 + rr-ota 5 + rr-registry lib 23 + 契约 29 + rrd lib 105 +
线级 `e2e_audio_chain` 1 + `e2e_depends` 3 + `e2e_sim_peer` 5 + `e2e_spawn_restart` 5 +
`e2e_watchdog` 4 + `module_call` 4 + `register_publish` 19 + `socket_banner` 1 = 269）/
Windows **221**（未实跑，按 214 + 平台无关 7 推算）。② 29 条无变化；③/②b/④ 无变化）。

测试面如实登记（2026-10-01 F3 控制通道收口复测（`rr3.ctl.*` 查询面 + rr3-ctl 实装，
语义裁定全文见 ADR-0010）。净增 **10** 条 + 独立工程 **12** 条：rr-registry lib 23→25——
`registry::tests` 2 条（`rr3_reg_health_snapshot` 的判定数回显：task 升序 + `since_beat_ms`
= `now - last_beat` 三数逐一对、`saturating_sub` 对 now < last_beat 夹 0 不回绕）+
rrd lib 105→108——`rpc::tests` 1 条（四个 ctl 方法过同一解析器且 `is_ctl` 判据与方法面一致、
模块四方法全部 `!is_ctl`）+ `registry::tests` 2 条并入上面 rr-registry 段、rrd 侧实为
`cli`/`manifest` 0 条；线级新目标 `rrd/tests/e2e_ctl.rs` **5** 条（整体 `#![cfg(unix)]`，
Unix 独有）：status 列出真注册模块且 **ctl 连接自己不得进快照**（裁定 2）+ 应答后连接被收掉
（一问一答即断）、health 回显条目自己的 heartbeat/watchdog 配置与 since_beat、logs 环里
必须有启动横幅（`rr3_log` 单点落环的线上实证）、ota 恒拒 `Permission(-22)` 且文案点 M6
（契约冻结集不自造 NotSupported）、已注册连接上行 ctl 被 `Permission` 拒但同一条连接
心跳照常被接受（一次笔误不掉线）。本轮 ①b：Unix **279**（实跑 = F2 的 269 + 平台无关 5 +
Unix 独有 5；逐目标：rr-bus 10 + rr-hal 13 + rr-ipc lib 38 + `envelope_meta` 4 + rr-ota 5 +
rr-registry lib 25 + 契约 29 + rrd lib 108 + 线级 `e2e_audio_chain` 1 + `e2e_ctl` 5 +
`e2e_depends` 3 + `e2e_sim_peer` 5 + `e2e_spawn_restart` 5 + `e2e_watchdog` 4 + `module_call` 4 +
`register_publish` 19 + `socket_banner` 1 = 279）/ Windows **226**（未实跑，按 221 + 平台无关 5
推算；`e2e_ctl` 整体挂 Unix 门不计入）。② 29 / ③ 5 / ②b 5 / ④ 9 个独立工程均无变化。
**②b 的姊妹注记**：rr3-ctl 的 12 条（含 2 条 fake-rrd 真 UDS 往返）属独立 workspace，
①/①b 打不到、④ 只 `cargo check` 编译不执行——与 ②b 把 mkmod 的产物喂回契约测试同一
精神，收口轮在本地实跑 `cargo test --manifest-path tools/rrctl/Cargo.toml`（本轮 12/12 绿）。
2026-10-01 F4（rr3-sim 完整化）同口径：`tools/sim` 3→**9** 条（drill 纯逻辑 7：参数解析 /
仓根标记搜索 / 缺二进制点名构建指引 / manifest 文本含绝对 entry 与真契约主题 /
status 表解析跳过非行 / 注入式轮询达成与超时点缺 / 演练身份在真契约里存在——
最后一条把"演练用真 owner 名"钉成机器可检查；devices 2 条原样），**base/runtime 不动，
①b 条数无变化**；编排（Unix-only）与纯逻辑分居两模块，9 条单测全平台可跑。
9/9 绿 + 全链路演练实跑 3/3 OK（见 tools/sim/README.md 五步脚本）。
2026-10-01 F6（shm transport 初版，ADR-0011）同口径：rr-ipc lib 38→**49**
（shm 11：核心 7——头布局常量自洽 / 单读者逐帧往返含大帧跨环折返 / 落后过半圈
重同步且 lost 计数 / 超半容量拒发且区域无损 / 多读者独立游标追读与停读分化 /
阻塞 recv_buf 等到迟到帧 / 坏头如实报 Header 不静默；POSIX 附着 4——cleanup 幂等 /
`O_EXCL` 未清理重复 bind 拒绝 / bind→accept→writer_conn 经真 `shm_open`+`mmap`
全链路往返含读者分化 / 附着缺失对象干净报错）。逐目标：Unix **290**（279 + 11）/
Windows **233**（226 + 核心 7 推算；POSIX 段 cfg 门控不参与编译，与 uds 的
cfg 边界同口径；无 Windows 目标未实跑）。② 29 / ③ 5 / ②b 5 / ④ 9 个独立工程
均无变化。依赖预算零增量：POSIX 附着为手写 6 函数 extern 面，不引 libc crate
（ADR-0011 裁定 5）。
2026-10-01 M4（rr3-brain 实装与四真进程对话闭环，ADR-0014）同口径：①b rr-hal
sim.rs 7→**8**（burst 波形：解析入取值集 / 偶秒门控确定性 / 第 3 秒复现第 1 秒且
有声段与 Sine 同序号逐点相等——恒有声的 sine 全程只发一次 onset，晚注册的 brain
赶不上；burst 1s 说/1s 停让 VAD 周期性发 onset/offset），Unix **291**（290 + 1）/
Windows **234**（233 + 1 推算，sim.rs 平台无关；未实跑）。② 29 / ③ 5 / ②b 5 无变化；
④ tools/sim 9→**12**（brain 演练 3 条：三份 wire manifest 全量形状与绝对 entry /
shim 脚本与演练 config 形状 / brain 演练身份与主题在真契约里存在；
`rr3_require_bins_ext` 并入既有"缺二进制点名构建指引"用例）；rr3-brain（Python
工程，stdlib unittest，本地实跑）**26** 条（test_bus 10 + test_cloud 9 +
test_dialog 7）。e2e：`rr3-sim --brain` 四真进程全语音对话闭环（rrd +
rr3-sense(sim burst) + rr3-brain + rr3-interact，云端 mock——唯一假的一跳是云端
socket）实跑 2/2 OK，判据：三模块 running + brain 日志「对话轮完成」+ interact
日志「tts:」+ `denied_publish = 0`。
2026-10-01 T1（能力格坐标系落盘，ADR-0012）同口径：rr-registry lib 23→**26**（slots 3：
契约解析形状 20 格/层分布 5-11-4 / 坏文本给空表且空表报错 / 形状检查复用 capability
规则函数抓非法名与非法层），①b Unix **294**（291 + 3）/ Windows **237**（234 + 3
推算，slots.rs 平台无关；未实跑）。② 29→**33**（查册门 4：基础层与
`base_capabilities` 钉死等价 / 格位表形状 pin 全部 20 条 `(name, layer)` 在册且层分布
5/11/4 / manifest provides+depends 双侧查册 / default profile bindings-faces 装配自洽）。
③ 5 / ②b 5 / ④ 9 个独立工程均无变化。
2026-10-04 T3（rrd 装配链——`--profile` 装机正门 / faces 决定起谁 / 查册门 / 缺格 fail-closed，
ADR-0009/0012 与 0.0.2 方案 Q3 裁定）同口径：rrd lib 108→**122**——新模块 `profile.rs` 11 条
（解析形状错误全收集逐条点名 / 缺段与空 faces / 坏 TOML 单错 / 满配 manifest 按 modules-root
解析且升序（比对基准用规范化后根目录：macOS `/tmp` 是符号链接，拿原始路径比会红在系统布局上）/
缺 binding 只缺点名模块不连坐 / binding 被他人认领=不自洽致命 / 基础层进 bindings 致命 /
faces 引未在册格位致命点名 / 基础层进 faces 只记账不缺格 / 装配单文件不存在的绝对化文案）
+ 新模块 `probe.rs` 1 条（sim 后端对任何格位回在位——缺格只由装配单缺 binding 表达）
+ `cli.rs` 14→16（`--profile` 默认 None 且与叠加层并存 1、与 `--manifest` 两种顺序同给
都在 parse 处互斥拒 1）。新线级目标 `rrd/tests/e2e_profile.rs` **3** 条（整体 `#![cfg(unix)]`，
Unix 独有）：P1 满配（装配单凭证行 + 三条 spawn 行回显夹具 entry + ctl.status 轮询三模块全
running）/ P2 缺格（点名 `拒绝启动 rr3-pb：缺格 sensor.env` + pa/pc 照常 spawn +
300ms 窗否定断言 `spawn rr3-pb pid=` 不出现——缺格≠拒 boot）/ P3 非法装配单（faces 引
未在册格位 → bind 之前非 0 退出 + stderr 点名未在册 + 套接字无残迹；直接 `Command` 起 rrd，
管道收两端）。①b Unix **313**（macOS arm64 实跑；上轮 T1 登记 294，本批净增 17
（rrd lib 14 + e2e_profile 3），另 2 条为 T1 轮登记的历史口径差——按本轮逐目标复算上轮实数
应为 296，不追溯改史，以本轮实数为准）/ Windows **251**（237 + 14 推算，本批 lib 用例全部
平台无关；`e2e_profile` 挂 Unix 门不计入；未实跑）。② 33 / ③ 5 / ②b 5 / ④ 9 个独立工程
均无变化。
2026-10-05 T4a（rr3-act 实装——舵机/LED/继电器三事务面 + 进程内反射环宿主，
ADR-0007/0012 与 0.0.2 方案 Q1 裁定）同口径：rr-hal lib 14→**17**（+3：sim 舵机
记录末次落角不判行程 / sim 数字输出电平 round-trip / act 设备选择入口回合法后端
对象）；新线级目标 `rrd/tests/e2e_act.rs` **1** 条（整体 `#![cfg(unix)]`，Unix 独有）：
装配单正门起**真 rr3-act**——夹具用仓内真 module.toml 渲染绝对 entry 并**同树写
config.toml**（rr3_config_load 缺省取 manifest 同目录的 config.toml，缺它模块开机
exit(1)，那是夹具烂不是被测行为），判据链：spawn 行回显绝对 entry → 注册 → running →
旁观连接 rr3.call 三事务面（servo 合法角应答 angle_deg/move_ms=800（缺 move_ms 取
config 上限）+ servo/state 限速流 seq=1、src_task=分配值——先读 state 后读应答，
同 socket 上应答与广播不保序，read_call_reply 不回存队列故顺序不可反；越界 200 /
缺参 / 非布尔 `{"on":1}` 折 result.error 不断链；led blink 应答 effect/previous；
relay 吸合回显真值）。真模块夹具 5 函数（repo_root / build_module_bin /
module_manifest_text / connect_observer / read_call_reply）自 e2e_audio_chain 上收
`tests/support`，后者断言一字未动仅换 helper 来源。①b Unix **317**（macOS arm64
实跑；上轮 T3 登记 313，本批净增 4（rr-hal 3 + e2e_act 1））/ Windows **254**
（251 + 3 推算，rr-hal 新用例平台无关；`e2e_act` 挂 Unix 门不计入；未实跑）。
② 33 / ③ 5 / ②b 5 无变化；④ 9 个独立工程均无变化。rr3-act 独立工程单测（④ 步
只 check 不跑测，按 rr3-ctl 先例单独登记）**18** 条：servo 5（参数解析正例含缺
move_ms 取上限 / 缺参·类型·越界点名 / move_ms 超上限与 0 拒收 / 限速门首放-窗内挡-
窗外开（now 合成注入）/ state 体 seq+angle）+ led 5（白名单解析含垃圾拒 / 缺参·
非串点名 / apply 记账 current 与 previous / on·off 即时生效（有界轮询）/
blink 翻转与 off 停翻（Arc 克隆观察 worker 写面））+ reflex 4（窗计划推导 /
episode 恰报一次 / need 下界为 1 / 孤立饱和窗不触发）+ relay 2（只认真布尔 /
apply 落电平且应答回显）+ main config 解析 2（仓形同构可解析 / 缺段缺字段点名）。
2026-10-05 T4b（rr3-power 实装——battery/state 周期流 + battery/low 边沿事件 +
charger/state，rrd 低电兜底四件动作面，0.0.2 方案 Q4 裁定）同口径：rr-hal lib
17→**19**（+2：sim 电池三元组读回与电压曲线两端钉死（3200mV + 10mV/1%）/ 电池
env 取值裁决纯函数（percent 0–100、charging 只认 1/0，未设缺省 80/未充电））；rrd
lib +2：registry 全局低电旗（上升沿恰一次、只升不降——与 denied_publish 同款
"conn 与 supervisor 两路共见"守护态）+ daemon 低电点名帧形状钉死（Notification /
src=rrd / dst=广播 / 体恰四字段——e2e 旁观者接入时首拍已过，帧形状只有单测能钉，
两半拼成完整证据链）；新线级目标 `rrd/tests/e2e_power.rs` **1** 条（整体
`#![cfg(unix)]`）：装配单正门起**真 rr3-power**，env 经夹具新通道
`rr3_start_profile_env` 注入（`RR3_HAL_BACKEND=sim` 显式不赌平台默认 +
`RR3_HAL_SIM_BATTERY_PERCENT=5` → 3250mV ≤ 3300mV 首拍越阈），判据链：兜底触发行
含透传值与点名广播（服务侧真相，conn.rs 钩子先扇出后打日志，该行即"广播已发"
界标）→ 旁观者收 battery/state（percent=5 / voltage_mv=3250 / src=分配值 / seq
严格递增）与 charger/state（charging=false 缺省语义）→ pkill 真模块 →
`低电兜底：拒绝 spawn` 日志 + 1500ms 窗否定断言 `spawn rr3-power pid=`（拒绝行
不含 pid= 不误触）→ 末尾破坏性收尾计数：battery/low 与 system/power 各恰 **0** 帧
新增（"同段恰一次"模块侧边沿机与 rrd 侧上升沿的双重保险，重启真发生了则新实例
首拍再发、现形于此）。①b Unix **322**（macOS arm64 实跑；上轮 T4a 登记 317，
本批净增 5（rr-hal 2 + rrd lib 2 + e2e_power 1））/ Windows **258**（254 + 4 推算，
rr-hal 与 rrd lib 新用例平台无关；`e2e_power` 挂 Unix 门不计入；未实跑）。
② 33 / ③ 5 / ②b 5 / ④ 9 个独立工程均无变化。rr3-power 独立工程单测（④ 步只
check 不跑测，按 rr3-act 先例单独登记）**7** 条：低电边沿 2（越阈首拍恰报一次 /
严格大于才重新武装且武装本身不触发——武装若回 true 就会发出电压高于阈值的"低电"
事件）+ 事件体 3（state 全摘要带自流 seq / low 体恰三字段对齐 schema
additionalProperties=false / charger state 恰两字段）+ config 解析 2（仓形同构
可解析 / 缺段缺字段点名）。
2026-10-05 T4c（rr3-connect 实装——net/link 状态机周期流 + cloud/session 应答面 +
HAL Rr3Link sim 后端，0.0.2 方案 D7）同口径：rr-hal lib 19→**21**（+2：sim 链路
三元组读回且断链时设备面强制抹掉 ssid/rssi——模块侧不许从死链里读出网络信息 /
链路 env 取值裁决纯函数（up 只认 1/0、rssi 只认 -100..=-1，未设缺省在线 / rock-sim /
-50））；新线级目标 `rrd/tests/e2e_connect.rs` **2** 条（整体 `#![cfg(unix)]`，
Unix 独有）：装配单正门起**真 rr3-connect** 的两场景对偶——在线（sim 缺省不注入
UP 键钉缺省语义：net/link 报 up=true / ssid=rock-sim / rssi=-50，seq 严格递增的
周期流；cloud/session open 回 mock 会话应答回显 ssid——云侧凭据联调范围外，
`RR3_BRAIN_CLOUD=mock` 注入点维持）与断链（`RR3_HAL_SIM_LINK_UP=0` 注入：net/link
报 up=false 且 ssid/rssi 为 null——设备面抹掉语义的集成面；session open 折
result.error 点名"链路"，业务性失败不断链）。sim 后端状态 env 定死、运行期不可
跨进程翻转，翻转分支只有 wifi.rs 纯函数单测能钉（e2e_power 的时序前提同款分工）。
①b Unix **326**（macOS arm64 实跑；上轮 T4b 登记 322，本批净增 4（rr-hal 2 +
e2e_connect 2））/ Windows **260**（258 + 2 推算，rr-hal 新用例平台无关；
`e2e_connect` 挂 Unix 门不计入；未实跑）。② 33 / ③ 5 / ②b 5 / ④ 9 个独立工程
均无变化（rr3-connect 由骨架变实装属 ④ 步既有目录，check 口径不变）。rr3-connect
独立工程单测（④ 步只 check 不跑测，按 rr3-act 先例单独登记）**6** 条：链路状态机 2
（首拍基线必发-同态不发-翻转必发的主语义链 / 事件体全状态带自流 seq 且断链时
ssid/rssi 必须是 null）+ 会话面 2（args 只认 action=open 且缺参-类型错-未知值点名 /
result 跟随链路 up 回 mock 会话、down 折 error 点名判据词）+ config 解析 2（仓形
同构可解析 / 缺段缺字段点名）。
2026-10-05 T5（发行版快路轨 deb——tools/rr3-pack 打包器 + units/rrd.service +
profiles/distro.toml + CI distro-smoke，0.0.2 方案 D8 / ADR-0013 裁定 5 首证）同口径：
rrd lib 124→**125**（+1：`--contracts` 运行期契约目录入口——deb 装机后编译期仓内路径
不存在，CLI 缺省 None 保持仓内开发零变化，topics/slots 共用 `rr3_contracts_dir` 解析、
`rr3_profile_boot_inputs` 签名参数化 slots 路径；parse 门四态：缺省 None / 显式值 /
缺参数带用法 / `-` 开头值拒）。①b Unix **328**（macOS arm64 实跑；上轮 T4c 登记 326，
本批净增 2：rrd cli contracts 用例 + rr-registry 契约测试 distro 装配单对账用例）/
Windows **262**（260 + 2 推算，两条新用例均平台无关——contract.rs 读仓内文件与 cli
纯解析）。② 33→**34**（+1：`distro_profile_binds_exactly_the_body_layer`——机型集合
钉住断言同步扩为 default+distro，distro 机对账三条腿：绑定面查册/provides 对账同
default 门、未绑非 base 格位**恰好等于** rr3-brain provides 全集（缺格边界一格不多
不少）、faces 仍覆盖全部在册模块含 brain 缺格声明）/ ③ 5 / ②b 5 无变化；④ **9→10** 个独立工程（新增 tools/rr3-pack，依赖 rrd lib 只为 manifest 解析
——解析真值只有 rrd 运行期那一册，打包器不带第二份读取逻辑）。rr3-pack 独立工程单测
（④ 步只 check 不跑测，按 rr3-act 先例单独登记）**5** 条：staging 全布局落位 + entry
装机改写字节级钉死 1（manifest 除 entry 引号内一个值外与注入原件逐字节一致；control /
postinst / 单元文件 / 装配单 / 契约内容同批断言）+ 注入产物与仓内真值缺件点名 1
（rrd 缺席 / 模块缺 bin / 无模块 / 缺 distro.toml / 缺 config.toml——模块开机即读
manifest 同目录 config，缺件必重启环，随包必带）+ entry 不可装机拒 1（绝对路径 /
`..` 逃逸段）+ 目录名与 manifest.name 不一致拒 1 + CLI parse 1。**entry 装机改写是
本批关键裁定落地**：线上注册门禁（conn.rs）要求 manifest.entry 绝对路径，而模块开机
读的就是装机 manifest 原文上报，相对值必被 AIRY_EINVAL 拒——打包器只改 entry 一个值
（modules/README.md"绝对 entry 由装机环节写入"的实体落点；装配单磁盘装载两侧绝对值
两头都过）。保底轨 post-build.sh 现状不满足该门（manifest 平铺 /usr/lib/rr3/manifest/
且零改写、bin 落 modules/<name>/bin 与相对解析错位）——保底轨"脚本就绪、实测=板上"，
改写留保底轨批次收口，不在本批越界。CI：`distro-smoke` job 新增（ubuntu-latest 单步
`docker run rust:bookworm`，glibc 自洽、本机产物装本机 deb）——构建→rr3-pack 单测→
组装注入产物（两轨同源 rr3-modules/<name>/{bin,manifest.toml}）→出 deb→
`dpkg -i`→装机位断言→rrd 装配单正门冒烟（spawn 5 模块 + rr3-brain 缺格点名（P2/D2
语义首证：快路 payload 无 Python brain，faces 保留声明、cognition.* 不绑定）+
0 拒 boot + `rr3-ctl status` denied_publish=0）。macOS 本机无法真装 deb（方案 P1），
真装只认该 job 远端回执；本地装配演练走 `rr3-pack --stage-only`。
2026-10-05 保底轨修复批（T5 段点名的保底轨缺口收口——entry 装机改写同一裁定落到
buildroot 轨，ADR-0013 裁定 5 两轨同源的第二面）改 `base/os/buildroot/board/radxa-zero3/post-build.sh`：
① manifest 不再平铺 `/usr/lib/rr3/manifest/<name>.toml`——归位
`/usr/lib/rr3/modules/<name>/module.toml`（旧布局下模块 rr3_config_load 读 manifest 同目录
config 必撞同名文件，且 bin 的相对 entry 解析到 manifest 所在目录必错位）；② entry 装机
绝对化（与 rr3-pack `rr3_manifest_install_rewrite` 同语义同三道门：entry 行恰一条
`grep -c`、值可装机 `case` 拒绝对路径 / `.` 与 `..` 段 / 含 `|` 分隔符、改写后 sed 重提
复核）——只改引号内一个值，其余字节原样；③ config 从仓源 `modules/<name>/config.toml`
随装（env `RR3_REPO_ROOT` 可覆盖仓根，缺省按脚本位置上溯五级）；bin 改按 entry 落位
（注入格式固定名 `bin` → entry 指向的装机相对路径）；S99rrd 的 manifest glob 同步改
`/usr/lib/rr3/modules/*/module.toml`。**缺件口径收紧一处**：缺 bin / 缺 manifest / entry
不可装机 = 警告 + **整模块跳过**（manifest 一并不装）——"缺席只警告不阻断"仍指镜像结构
可构建，但缺件还装 manifest 会让 rrd spawn 一个必退出的模块（重启环）；跳过的是模块，
不是镜像。config 缺席仍只警告（镜像可构建，板上收口）。CI：`buildroot-assembly` job 新增
（ubuntu-latest 单步，纯 bash + 系统工具，夹具侧模拟 Buildroot 目标根的 etc/init.d，
不需要 Docker）——`tools/ci/buildroot-assembly-smoke.sh` 断言装机布局 / entry 绝对化且
除 entry 行外与仓源逐字节一致 / bin 落 entry 名可执行 / config 与仓源一致 /
旧平铺位不存在 / S99rrd 新 glob / 两类坏模块（缺 bin、无 entry 行）警告且整模块未装。
本批零 Rust 改动：①b / ② / ②b / ③ / ④ 全不变。
2026-10-05 D9 装配演练批（0.0.2 T6 前半——rr3-sim `--profile` 装配演练模式）：
`tools/sim` 新增 `assemble.rs`，装配单驱动 rrd 装配链正门（`--profile` + `--contracts`，
与 rrd.service 的 ExecStart 同形）boot 全部 face 模块，演练 modules-root 是**装机布局**——
仓源 manifest 原文只把 entry 改写为产物绝对路径（rr3-pack `rr3_manifest_install_rewrite`
同语义同三道门：装机值可装机 / entry 行恰一条 / 旧值与 facts 交叉核验 / 改写后重解析
复核，"entry 在仓相对、装机绝对"的第三面）+ config 同目录就位（Rust 模块仓源 config，
brain 用 M4 先例 mock 演练 config）；python 模块产物 = shim（`python3 -m`，包名 = 目录名
连字符转下划线）。配套：① rrd `Rr3ManifestFacts` 加 `lang` 字段（演练产物推导要它，
解析真值保持一册——不逼调用方另开 TOML 读取；req_str 缺失即点名）；② `which_python3`
从"PATH 第一个"改为**能力验证**（`import tomllib` 成败 = brain 运行门槛的直接证据，
不满足的候选继续向后找、全不满足才报错点名——macOS 本机 3.9 从"60s 超时红"变
"当场点名红"，--brain 同益）；③ sim 依赖 rrd（path，借两处解析真值，rr3-pack 同款先例）；
④ `--brain` 与 `--profile` 同给即拒（Q3 同款）。CI：`sim-assemble` job 新增
（`tools/ci/sim-assemble.sh` 构建 + 运行同一脚本）——**D9 六模块判据（含 brain）归
该 job 远端回执**：macOS 本机 python3 3.9 不过 tomllib 门（无 brew / uv / pyenv 可装），
本机实跑记录：无 brain 子集装配单（5 Rust 模块）装配演练全链绿（装配 boot → 全
running → `denied_publish = 0` → 四命令巡检 ota 预期拒，exit 0）+ default 满配单在
brain 门槛处 fail-fast 点名；六模块真进程判据与 P1 同理（distro-smoke 先例）不据本机
声称。测试面：sim 独立工程单测 12→**16**（+4：entry 改写钉字节 + 三道门拒绝形状 +
夹具布局装配 + **仓内 default faces 装配就绪对账**（六模块 manifest 可解析 / name ==
目录名 / entry 仓相对无 `..` / config 在——装配单↔模块目录漂移让单测红，不等演练跑
一半红）；rrd 无新增用例（lang 是 facts 解析面的自然扩展，既有 manifest::tests /
registry::tests 的文本夹具全带 lang 字段照过）。①b Unix **328** / Windows **262**
不变；② 34 / ③ 5 / ②b 5 / ④ 10 个独立工程不变（tools/sim 加依赖不改工程数）。
2026-10-05 D10 文档收口批（0.0.2 T6 后半——status.md 重锚 + reference 三件与代码同批，
方案 P4"只汇总重锚、不补账"）**零 Rust 改动**：①b / ② / ②b / ③ / ④ 全不变；⑤ 因新增
互链重跑全绿。两件事：
① [status.md](../status.md) 整篇重写（锚点 `540ec0f` → `363e4ed`）——T3 装配链接线、
T4a/b/c 三模块实装、T4b 低电兜底、T5 deb 打包链、保底轨修复批、D9 装配演练全部按锚点树
实况计入能力表（"能力格坐标系"行并入装配链 e2e 三条、"Buildroot OS 层"行改写 post-build.sh
装机布局、新增"发行版快路轨 deb 打包链"与"rrd 低电兜底"两行）；M5 行按 Q2 口径如实改写
（shm transport 达成、三模块 sim 侧实装、"语音控继电器"最后一跳 = 意图→动作映射属策略层
未做 + 板上证据零 = 未收口两条敞口）；"本机无法收口"表第 1/5 条随双远端与 0.0.2 条数更新、
新增第 8 条（deb 真装只认 distro-smoke 回执）与第 9 条（六模块真进程只认 sim-assemble 回执
——本机 python3 3.9 不过 tomllib 门）；在制品段枚举 `540ec0f..363e4ed` 全部 12 提交。
② reference 三件同批：[rrd-cli.md](../reference/rrd-cli.md) 补 `--contracts` 四处
（§1 USAGE 逐字含说明句 / §2 参数表新行 + `RrdConfig` 字段列 / §4 路径绝对化分工新增
"例外面"行——目录不单独规范化、存在性由 join 出的 topics/slots 各自 canonicalize 把关 /
§5 启动序第 5 步契约定位与第 7 步致命面补"格位契约读不到"）；
[rrd-registry.md](../reference/rrd-registry.md) §1 `RrdRegistry` 字段列补 `low_power`、
§10 补 `rr3_reg_low_power_raise` / `rr3_reg_low_power_is_set` 行（上升沿恰一次、只升不降
不设解除口、spawn 漏斗唯一消费者）；[rrd-rpc.md](../reference/rrd-rpc.md) §7 发布处理序
补低电兜底钩子段（挂转发点之后、兜底是额外动作不截断事件流、两段日志逐字）。
同批订正 [.github/README.md](../../.github/README.md) 的实况漂移（四 job → 七 job 清单、
"主远端已迁 gitcode" → 双远端口径）——T5/保底轨/D9 三批挂 job 时该文件漏同步，此处结清。
2026-10-05 0.0.3 批A（0.0.3 收口方案落档 + legacy 处置裁定入规约，Q1–Q4 裁定记录随
方案 §2 落档）**零代码改动，纯文档批**：新增
`docs/process/dev/0.0.3版本洛克三号收口方案.md`（M5 最后一跳 + M6 OTA 写侧主机面 +
legacy 盘点三交付，WBS T1–T5）；[§5](#5-文档纪律) 增补 legacy 处置三阶段口径
（只读消化 → 融合完成 → 退役，替代旧"冻结区不动它"；`docs/process/legacy-digest.md`
在 T2 落档时此处挂链）；docs/README.md 索引切换（0.0.2 行退历史存档、0.0.3 行
为当前版本，两处表同批）。①b / ② / ②b / ③ / ④ 全不变；⑤ 因新增文档与互链重跑全绿。
2026-10-05 0.0.3 批B（D2 legacy 语义盘点落档，Q4 裁定的台账面）**零代码改动，纯文档批**：
新增 [legacy-digest.md](legacy-digest.md)——legacy/core 九组件 + 三棵试验树逐项三分类
（已融合 7 / 待融合 3：data 计数、panel 面板、btctl 蓝牙链路，去向与排期 0.0.4 消化批 /
退役 5：firmware-c 宿主、firmware-rs、rust、link/eth、link/nearlink），每行带证据路径；
退役触发条件与退役批四件收尾（⑤ 门排除撤除、architecture §8 标注、status 已知偏差注销、
本表归零）在册。同批：§5 挂真链（替换批A 的占位句）、architecture §8 尾注挂链、
docs/README.md 两处表挂导航。①b / ② / ②b / ③ / ④ 全不变；⑤ 因新增文档与互链重跑全绿。
2026-10-05 0.0.3 批C（D3 M5 最后一跳，T3 补裁定随方案 §2 落档）：
**契约 append 三件**（`contracts/topics.toml` 第 25 条 `rr3-act/relay/state` event L2——
0.0.2 Q1 "留真实需要时再 append" 的真实需要此刻成立：模块 stdout 接 `Stdio::null()`，无状态
主题则意图执行效果在 e2e 层不可观测；`events/brain-intent-result.schema.json`；
`events/act-relay_state.schema.json`），形状 pin 三处同批（registry `tests/contract.rs`
25 条三元组 / kind 8-10-3-4、registry `src/topics.rs` 25、rrd `src/topics_load.rs` 25+28）；
**brain 侧**：新增 `rr3_brain/intent.py`（user_text → actions[]，精确词表 + 两字符否定护栏，
3.9 兼容语法）+ `tests/test_intent.py` 6 条，`__main__.py` 发布载荷并 `actions`；本机
python 3.9.6 实测 23 条中 22 过（`test_bus` 一条 import error：tomllib 需 py≥3.11，
D9 已登记门槛非本批引入），CI brain-unittest 钉 3.12 跑全量 23 条；**act 侧**：新增
`src/intent.rs`（两道归属门：exposes 归属 + 意图面开放，拒执行折日志不断链）5 条单测、
`relay.rs` 增 state 主题常量与摘要体（形状对齐 schema）1 条单测、`main.rs` RPC 与 intent
两条驱动路接线共用 `relay_seq` 流序、`module.toml` publishes 增 `rr3-act/relay/state`——
模块单测 24 条全绿（增 6）；**e2e**：`rrd/tests/e2e_act.rs` 扩 RPC 路状态回读（先读 state
后读应答，`rr3_read_call_reply` 不回存）+ brain 在册身份发 intent 事件 → relay 落电平 →
`relay/state` 回读（seq 跨两路连贯）+ 越权动作有界窗口否定断言；同批文档：方案三处回改
（§1.2 D3 行、§4 契约句"零改动"→"一条 append"、§6 DoD）、architecture §6.3 对账注记与
§7.2 草图注订正（relay/state 已在册）、act/brain README 目录树与总线面表同步。
**本批门禁增量钉子**：registry lib 单测与 rrd lib 单测各含一份与 `tests/contract.rs`
独立的形状 pin（`include_str!` 直读契约），append 主题必须三处同批改——批C 首跑红两条
（24≠25）即为该钉子咬合，同批结清；批B 遗留断链一处（legacy-digest.md 的
vad_start schema 链少一级 `../`）以独立 docs fix 提交先于本批落库。①a–⑤ 全绿重跑；
brain-unittest 与 lint 归 CI 侧。
2026-10-05 0.0.3 批D（D4 M6 OTA 写侧主机面，Q3 裁定落地）：
**rr-ota 增 `bootconf` 模块**（可测逻辑归 base/runtime workspace 随 ③ 全量跑测试；
CLI 薄壳）——`switch`/`factory` 判定与改写（只动生效行 root=LABEL 标签、注释与其余
字节一字不动；同槽幂等拒 / rr_factory 目标拒走 factory / 缺·歧义·杂牌 token 报形状
漂移拒）、原子落盘（读 → 改写 → 同目录临时文件 → rename）、cmdline 当前槽解析
（只认 `root=LABEL=rr_`，裸设备形态报"板上核对项"不覆盖）；`Rr3Slot::rr3_slot_from_label`
反查 + roundtrip 测试；真工件 `include_str` 字节级对拍（extlinux.conf 可逆律 / 对合律 /
长度差恰为标签差），单测 8 条新增、crate 13 条全绿。**首跑红两处即钉子咬合同批结清**：
①rr3_root_spans 初版把 extlinux.conf 注释行里的 `root=LABEL=rr_a` 也扫进来（2 处→歧义
拒）——改为逐行扫并跳过 `#` 注释行，span 收窄为标签字节区间（rewrite 不再重造
`root=LABEL=` 前缀，初版丢前缀缺陷一并消除）；②两条测试断言同步"注释一字不动"语义。
**rr3-ota-ctl 重写**（恒拒骨架 → 实装）：slots / switch / factory 三子命令，
`--bootconf` 必传不缺省碰任何路径，退出码 0 成功 / 1 业务拒 / 2 用法错；恒拒用例
`switch_is_always_refused_until_m6` 改写为实装语义三件（slot_arg 在册映射+杂牌拒、
CLI 壳临时文件端到端+同槽业务拒不落盘、bootconf_arg 硬门槛），4 条全绿；CLI 实跑冒烟
（slots / switch 落盘 / 同槽拒 / factory 目标拒 / factory 落盘 / 缺参拒 / 临时文件零残留）
逐项对。**文档三件同批**：partition-layout.md 增补"切换与回滚链"段（切换语义、U-Boot
bootcount/bootlimit/altbootcmd/upgrade_available 标准语义表、断电安全序七步、板上核对项
清单七条——首条即 vfat rename 原子性弱于 unix 文件系统的诚实敞口）；extlinux.conf 注释
订正（"由 rr3-ota-ctl 改 U-Boot 环境变量"→实装口径"改写本文件 append 行标签"）；
base/ota/README.md switch 恒拒段改实装口径 + 边界声明（本批交付切槽"翻标志"一步；
槽位镜像写入随升级下载链另批；回滚链与块设备探测属板上件不 claim）；rr3-ota-ctl
Cargo.toml description 同步。①a–⑤ 全绿重跑。
2026-10-05 0.0.3 批E（0.0.3 收口汇报批，纯文档）**零代码改动**：docs/status.md 整篇重写重锚
`363e4ed` → `68220e4`（M5 行改"已收口（sim 侧；板上敞口同 M3）"——批C 意图链闭环计入；
M6 行改"主机面已实装，板上件与升级链未做"；A/B OTA 行骨架 → 已实装（主机面）；schema 行
1 → 3 件；"本机无法收口"新增第 10 条 M6 板上件、第 7 条并 M3/M5 板上证据、第 1 条指针
双端核对口径；在制品段补批A–D 五提交；legacy 已知偏差措辞随 §5 三阶段口径更新——"冻结区
不动它" → "只读消化阶段不改本体，退役批处置"）；本条登记自身。①b / ② / ②b / ③ / ④
全不变；⑤ 因 status/本文件互链重跑全绿。0.0.3 五批次（批A 方案落档 / 批B legacy 盘点 /
批C M5 最后一跳 / 批D M6 写侧主机面 / 批E 重锚汇报）至此全部落库。
2026-10-05 0.0.4 批A（0.0.4 消化方案落档，Q1–Q6 裁定项随方案 §2 在册待裁）**零代码改动，纯文档批**：
`docs/process/dev/0.0.4版本洛克三号消化方案.md`（legacy 待融合三项收敛 + 退役批：D2 data
计数/持久化、D3 panel 本体、D4 配网面、D5 btctl 骨架、D6 整树退役删除（Q6 显式裁定前置）、
D7 收口；WBS T1–T7 与版本 DoD 随文）；docs/README.md 索引切换（0.0.3 行退历史存档、
0.0.4 行进当前）。①a–⑤ 全绿重跑。
2026-10-05 0.0.4 批B（data 消化——心跳 counters 双端镜像 + rrd 聚合持久化 + `rr3.ctl.counters`，
Q1/Q2 裁定落地）**新增 5 条单测 + 2 条 e2e**：rr-bus client（`client.rs`）心跳每拍快照
`rr3_counters_snapshot`、`rr3_counter_bump` 记账，非空才带 `counters`（新用例
`heartbeat_carries_counters_only_after_bump`，lib 实测 11 条含新用例）；rrd 新模块 `counters.rs`
（路径三态 + 原子写序 tmp→fsync→rename，5 单测）、`registry.rs` 加
`rr3_reg_counters_replace/bump/merge/snapshot` 四方法
（merge 按 `模块名/键` 前缀归并、饱和加法）、`rpc.rs` 加 `CtlCounters` 变体 +
`rr3_rpc_heartbeat_counters`（脏值逐对静默跳过）——**九方法白名单**（控制四→五）；
`conn.rs` 心跳分支聚合+write-through（锁内归并锁外写盘，Rejected 之后也聚合），
`daemon.rs` 启动装载 + boots 自增回写（`RR3_DATA_DIR` 优先 → linux 缺省 `/data/rr3` →
皆无不持久化只点名）；brain `bus.py` 逐语义镜像（`_heartbeat_params` 独立成法供测试直调，
manifest 1h 心跳拍等不来真拍）+ `__main__.py` 四计数点，act `relay.rs` 键名唯一落点 +
`main.rs` 两条驱动路记账。文档：rrd-rpc.md（§1/§2/§3 九方法口径、心跳 counters 可选字段、
ctl.counters 行）、rrd-cli.md §2.1（RR3_DATA_DIR）、brain/act README 计数键归属表。
①b/②/②b/③/④ 全绿重跑（e2e_ctl 7 条含新 2 条：聚合与跨重启持久化）；brain Python 本机
3.9 无 tomllib 起不了（既有现状，CI brain-unittest job 覆盖），py_compile 语法验证过。

2026-10-05 0.0.4 批C（panel 本体——interact std 手写 HTTP 九端点 + 单文件静态页 +
`rr3.ctl.reboot`，Q3 裁定落地）**rrd 侧新增 `CtlReboot` 变体**（`rpc.rs`，**十方法
白名单**：控制五→六；`is_ctl` + 显式 OR 臂两处共同看护不变），`conn.rs` 服务臂执行面
**双重门**（`cfg(target_os = "linux")` 且 env `RR3_REBOOT_OK=1` 才真跑 `systemctl reboot`，
缺省恒拒）——e2e 只钉拒绝路径（`ctl_reboot_is_refused_by_default_double_gate`），
解锁面留装机环境由 rr3.service 注入，ubuntu CI 编译 linux 面也拿不到真重启能力；
e2e_ctl 7→8 条。interact 侧：`panel/server.rs` std 手写 HTTP/1.1 子集（GET/POST +
Content-Length，**零新 crates.io 依赖**），九端点路由纯函数 + `Rr3PanelDispatch`
**无 Publish 变体**（面板不当发布者的类型面形态，单测
`hardware_control_is_bus_call_only_never_publish` 钉死）+ ctl 裸 UDS 短连（src=0 与
运维探针同权）+ HTTP 读面（半包/超时折 Err，头区 8KB / body 4KB 上界）+ 连接上限
8（超限 503）+ 监听面 env `RR3_PANEL_ADDR` 优先缺省回环（Q3）；beep/face 经 mpsc
动作通道由主循环串行代执行（sink/face 所有权不出主循环，与 TTS 点名天然互斥无锁），
selftest 并列三调各答各展示（无状态机不算编排，ADR-0007）；静态页 `include_str!`
内嵌四页签单文件（状态/硬件/系统/WiFi 批D 占位），`main.rs` 面板线程 + net/link
事件缓存，`module.toml` subscribes 加 `rr3-connect/net/link`（发布方批D 上线）。
interact 测试 12→17 条（九端点路由、越权静态断言、404/405/400、全量/半包读、
ctl 回路+错误信封）。文档：interact README（九端点表、监听面、局域网可信面声明）、
rrd-rpc.md（§1/§2/§3/§4 九→十方法口径、ctl.reboot 行、Permission 行）。
①b/②/②b/③/④ 全绿（rrd lib 15 条、e2e_ctl 8 条、interact 17 条），fmt/clippy 清零。

2026-10-05 0.0.4 批D（配网面——connect 三 RPC + AP 配网状态机 + 面板 WiFi 页，
D4/Q3 裁定落地）**契约 append 25→28 条**（`rr3-connect/wifi/scan` · `wifi/connect` ·
`wifi/forget` request L1，legacy/core/panel 配网面迁移承接：scan sim 模拟列表 +
`backend:"sim"` 诚实标注、connect 沿用 legacy `ssid`/`pass` 字段名与密码≥8 位语义、
forget 承接 `/api/factory_reset` 的 WiFi 语义且 ≠ M6 整机恢复出厂），topics.toml /
`module.toml` exposes / rr-registry 形状 pin（28 条三元组、kind 11-10-3-4）三处同批改
咬合（契约 34 条含 pin 门全绿；同批订正 rr-registry lib `topics.rs` 2 处与 rrd
`topics_load.rs` 3 处的同源计数断言——两档测试直读契约，25 的旧数字一处不改就红）；
`net/link` 事件体 append `mode` 字段（ap|station，
负载只增不改）。connect 侧：`wifi.rs` 新增 `Rr3Provision` 配网状态机（凭据内存
`Option<(ssid,pass)>`，无凭据=ap 有凭据=station，**模块内策略不进 HAL**——sim 是
设备不是策略，up/ssid/rssi 仍 HAL 如实报数；真凭据持久化板上后置重启即忘，
D4 判据如实标注）+ 三 RPC 参数/结果裁决（session.rs parse_args/result 分离范式，
"AP 态可模拟进出"由纯函数单测 `provision_roundtrips_ap_station` 钉死），
`rr3_link_body` 加 mode 参数；`main.rs` 三常量 + match 四臂（session + 三 wifi），
业务性失败折 error 应答不变。interact 侧：`server.rs` route 三臂（GET scan /
POST connect / POST forget，全部 BusCall 透传——参数裁决在 owner 模块，connect
body 原样透传零翻译），九→十二端点；静态页 WiFi 页签实装（扫描表 + 连接表单 +
忘记卡）+ 系统页"恢复出厂 WiFi"卡（两语义不合并文案点名）；connect 测试 6→10 条、
interact 测试 17 条（路由用例更新为十二端点逐一点名）。文档：contracts/README.md
演进登记（25→28）、connect/interact 两 README、architecture §6.3/§7.2 对账注记。
①b/②/②b/③/④ 全绿（rr-registry lib 26 条、契约 34 条、connect 10 条、
interact 17 条），fmt/clippy 清零。
2026-10-05 0.0.4 批E（btctl 骨架——connect 蓝牙链路状态机 + 退役声明，D5/Q5 裁定落地）：
`bt.rs` 从 3 行占位升级为状态机骨架——`Rr3BtPhase` 三态（Off/Discoverable/Connected）
+ `Rr3BtState` 转移函数（power_on/link_up/link_down/power_off，非法转移返回 false
且状态不动，转移本体与后端无关、纯函数单测 `bt_phase_roundtrips_full_chain` /
`bt_illegal_transitions_are_rejected_without_moving` 钉死）+ sim 后端开机初值
env 注入（`RR3_HAL_SIM_BT_UP=1` 开机即上电进 Discoverable，缺省关=出厂不自动开，
取值不认一律按关——env 是 sim 注入口不是配置面）；BlueZ 真后端板上后置（cfg 门，
方案 §1.3，本骨架不 claim 真射频链路）。**主题不登记**（方案 D5 显式裁定）：
btctl 的日志回放/远程命令语义已被 rrd 日志环 + `rr3.ctl.logs` 与 rr3-ctl 承接
（digest btctl 行在案），蓝牙链路暂无可发布事件——按"不预先占位"纪律（§3），
启用事件面前须先改契约。`main.rs` 开机 sim 初值接入 + 相位自述打印（蓝牙面
phase=off|discoverable）；`net.bt` 格位绑定核对在册（default.toml / distro.toml
两装配单 [bindings]/[faces] 均已绑定，本批零改）。digest btctl 行"待融合（低
优先级）"→"已收敛（0.0.4 批E）"+ Q5 退役声明（BlueZ 真射频随硬件采购后置板上
批次；data/panel 两行不动，三分类表清零归退役批 D6）；connect README bt 段落
同步。契约与形状 pin 不动（28 条）。①b/②/②b/③/④ 全绿（connect 12 条），
fmt/clippy 清零。
2026-10-05 0.0.4 批F（legacy 退役批——整树删除 + 四件收尾，D6/Q6 裁定落地）**零代码
改动，纯退役批**：`legacy/`（ESP32 冻结区，100 文件）整树删除——待融合清零确认在案
（panel 0.0.4 批C+D 实装、data 批B 实装、btctl 批E 骨架+Q5 退役声明；digest 三分类表
归零归档：已融合 10 / 待融合 0 / 退役 5）；删除前提满足（批E 提交 `586641d` 双远端
`ls-remote` 核对一致在案；git 历史 + tag `v0-pre-legacy` 双锚点回溯）。四件收尾同批：
① ⑤ 门 legacy 排除条款撤除（`check_docs` 的 `grep -v '^legacy/'` 与"为什么排除"注释
段——冻结区不存在后全集进门）；② architecture §8 标注消化完成（表尾注记改写，
行内 `legacy/...` 路径自此指 git 历史位置）；③ status.md"已知偏差"注销 legacy/README
断链条目；④ digest 头部/只读约束/汇总表/退役触发条件/消费指引归档化 + data/panel
两行补证据转"已融合"（data：批B 心跳 counters 双端镜像 + rrd 持久化原子写 +
`rr3.ctl.counters` 读面；panel：批C std 手写 HTTP 面板 + 批D 配网合流，两语义不合并
文案在册）。断链面同批清理：根 README 双语的 legacy 行改指 digest 归档台账、
docs/README 库地图三处描述更新。①b/②/②b/③/④ 全绿（测试条数全不变），⑤ 门以
删除后的 md 全集重跑；fmt/clippy 不涉及。
2026-10-05 0.0.4 批G（status.md 重锚 `68d0da9` + conventions 汇总收口，D7）**零代码
改动，纯文档重锚批**：status.md 按"收口时整篇重写"规矩重锚——锚点行 `68220e4`→`68d0da9`；
RPC 行"四方法"→"十方法（模块四 + 控制六）"（批B `rr3.ctl.counters` / 批C `rr3.ctl.reboot`
append）；主题注册表行补批D 25→28 append；**新增"运维计数面"行**（批B 全证据链：
client.rs 心跳镜像 + bus.py 同语义镜像 + rpc.rs 形状裁决 + registry.rs 聚合 +
counters.rs 原子写持久化）；sense/interact 行补面板证据（批C+D 十二端点 + 四页签）；
connect 行补配网面三 RPC（批D）与蓝牙骨架（批E，主题不登记理由在册）；rr3-ctl 行
改六方法口径（reboot 双重门 cfg(linux)+`RR3_REBOOT_OK=1`、rrctl CLI 子命令面仍四条
如实标注——两新方法的读写面走面板透传）；里程碑表头锚点随改 + M5 行补批D 配网面
收口；在制品段列 0.0.3 批E 与 0.0.4 批A–F 八提交（标题逐一照 git log 抄录）；
无法收口表：第 1 条指针随重锚更新、第 2 条 idf.py 密钥门**随 legacy 删除注销**
（验证对象不在工作树，固件线复活再重开）、第 3 条 key 轮换措辞订正（批F 只清
工作树，历史提交对象里 key 依旧在，提供方侧敞口不闭）。contracts/README 演进登记
批G 零契约改动（25→28 已随批D 登记），无需新改。①b/②/②b/③/④ 全绿（测试条数
全不变），⑤ 门全仓 md 全集重跑；fmt/clippy 不涉及。
2026-10-05 0.0.5 批A（0.0.5 对齐与裁定方案落档，Q1–Q4 裁定项随方案 §2 在册待裁）**零代码
改动，纯文档批**：`docs/process/dev/0.0.5版本洛克三号对齐与裁定方案.md`（0.0.4 消化收官后的
对齐批：D2 rrctl 四子命令→六子命令运维面对齐、D3 brain Rust 迁移设计裁定落档、D4 pcm
STREAM schema 与 shm 接线裁定登记、D5 收口；范围外显式列 rrd ctl.ota 维持恒拒 / pcm 不
冻结 / brain 实施批次不入 0.0.5；WBS T1–T5 与版本 DoD 随文）；docs/README.md 索引切换
（0.0.4 行退历史存档、0.0.5 行进当前，四档落点表 + 库地图两处）；根 README.md /
README_zh.md"当前版本"行订正（原指 0.0.2 属漂移，同批改指 0.0.5）。①a–⑤ 全绿重跑。
2026-10-05 0.0.5 裁定登记（Q1–Q4 逐项裁定，用户授权按 ADR 体系裁定，四问全按建议口径
通过；裁定依据逐条引 ADR 原文，全文见 0.0.5 方案 §2 裁定记录）**零代码改动，纯文档批**：
Q1 按建议——rrctl append `counters`/`reboot` 两子命令（ADR-0010 裁定 2/4/6：一次性查询
连接、已有数据源零新增状态、人读表渲染）；Q2 按建议且定死**新立 ADR-0016**（不增补
0014——决策不可变、新决策新编号）——brain 迁移 0.0.5 只落设计裁定，实施归 0.0.6 专项，
首件 bus.py → rr-bus 复用使 ADR-0014 裁定 1 镜像纪律自然退役；Q3 按建议——pcm STREAM
schema 维持空缺不冻结，冻结与 rrd shm 接线绑定同一份后续 ADR（ADR-0008 裁定 4"不写
半成品"）；Q4 按建议——远端 CI 宿主核实是用户侧唯一悬项，0.0.5 不排批，回执到手后立
结项批。`docs/process/dev/0.0.5版本洛克三号对齐与裁定方案.md` §2 裁定记录段落档。
①a–⑤ 全绿重跑。裁定后 T2–T5 依序解锁。
2026-10-05 0.0.5 批B（T2 rrctl 补 `counters`/`reboot` 两子命令——CLI 四→六与 rrd ctl
白名单对齐，Q1 裁定落地）**纯 rrctl 面改动**：`tools/rrctl/src/commands.rs` COMMANDS
4→6（`counters`/`reboot` append，与 `rr3.ctl.*` 白名单六方法逐一同名对齐，配套单测
钉死方法名不漂移）；`format_counters` 按模块前缀分组成 MODULE/KEY/VALUE 人读表
（`模块名/键` 拆前缀，rrd 自有键如 `boots` 无前缀归 `rrd` 组，空表打"无计数"，
计数非 u64 报错不渲染 `?`——与 status 的 cell 容错口径有意分开）；`format_reboot`
回执一行确认；**CLI 不加本地门**——reboot 的 rrd 执行面双重门（cfg(linux) +
`RR3_REBOOT_OK=1`）是唯一真值点，CLI 加门即第二真值，拒绝文案经错误信封统一
回显非 0 退出；`main.rs` 用法文案与模块头注释同步六命令口径。单测 12→19（新增
7 条：纯函数 5——方法名白名单钉死 / counters 分组渲染 / 空表 / 非 u64 报错 /
reboot 回执，fake-rrd 真 UDS 往返 2——counters 实答渲染、reboot 恒拒回显；往返
共 4 条含既有 status/ota）；
`tools/rrctl/README.md` 命令面与代码逐字对齐（六子命令 + counters/reboot 语义 +
"单测含四条真 UDS 往返用例"）。rrctl 独立工程不在七步门 ①/①b 作用域，④ 步
`cargo check` 覆盖编译；本地实跑 `cargo test --manifest-path tools/rrctl/Cargo.toml`
19 全绿 + fmt/clippy 清零（conventions §2 备注）后提交；七步 ①a–⑤ 全绿。
2026-10-05 0.0.5 批C（T3 brain 迁移设计裁定落档——新立 ADR-0016，Q2 裁定落地）**零代码
改动，纯文档批**：新立 [docs/process/adr/ADR-0016-brain迁移路线-python转Rust渐进首裁定.md](adr/ADR-0016-brain迁移路线-python转Rust渐进首裁定.md)
（**不增补 0014**——决策不可变、新决策新编号；0016 引用并收缩 0014 适用范围：
裁定 1 的 bus.py 逐语义镜像纪律随首件迁移退役，裁定 2/3/4 的云 API 形状 / 对话
回路归属 / e2e 编排语义继续有效）。四裁定：①切分 = 语义件渐进（首件 bus.py →
rr-bus 复用零协议代码，次件 cloud 三家，末件对话状态机；迁移件与 Python 对应物
同批一进一退，仓库任何时刻不存在双语言真值）；②功能语义不变量四条（对话闭环
与 mock 注入点 / 失败语义同契约 / 门禁契约校验不减 / config 键值不变形）；
③部署形态目标 = payload 去 python3 运行时（`lang=rust`、≥3.11 门槛消失、
`which_python3` 与 brain-unittest 随最后一件退役、OCI 分发格式裁定 0013 裁定 4
维持不动）；④实施归 **0.0.6 专项**（打包链/装配单/CI 三面联动属独立版本体量，
0.0.5 只落设计裁定，本裁定不等于实施解锁）。conventions §3 主语言条目加 0016
指针；docs/README.md 两处 ADR 区间 0001~0015 → 0001~0016（标题表补 brain 迁移
路线）。①a–⑤ 全绿重跑。
2026-10-05 0.0.5 批D（T4 pcm 传输裁定登记——contracts 演进段口径钉死，Q3 裁定落地）
**零代码改动，零契约改动，纯文档批**：`contracts/README.md` 演进登记 append
2026-10-05 条目——`rr3-sense/audio/pcm` 的 STREAM 负载 schema **维持空缺不冻结**，
原"冻结时机属 M5 shm 迁移裁定"表述升级为 **"schema 冻结与 rrd shm 接线绑定同一份
后续 ADR"**（依据 ADR-0008 裁定 4"不写半成品"：pcm_b64 JSON 是 UDS 承载期编码，
shm 零拷贝后大概率改环形直读，给注定换承载的形状写冻结件即半成品）。topics.toml /
events schema / slots.toml 一字未动（只增不改维持）；接线实施属多模块工程批届时
另立专项，冻结动作在专项内完成。status.md 能力表末行口径同步归 T5 重锚批（收口时
整篇重写，批内不动 status.md）。①a–⑤ 全绿重跑。
2026-10-05 0.0.5 批E（T5 文档收口——status.md 重锚 `c7b91e7` + conventions 汇总，
D5）**零代码改动，纯文档重锚批**：status.md 按"收口时整篇重写"规矩重锚——锚点行
`68d0da9`→`c7b91e7`；rr3-ctl 行 rrctl 侧改六子命令口径（0.0.5 批B `25e90b7` 证据
链：CLI 与 ctl 白名单六方法逐一同名对齐、counters 模块前缀分组渲染、reboot CLI
不加本地门）——原"CLI 子命令面仍四条 / 补子命令属演进项不阻塞"**注销**；能力表
末行 pcm 口径同步批D 裁定（维持空缺不冻结、冻结与 shm 接线绑定同一 ADR）；
`rr3-brain` 行规约注记补 ADR-0016 指针（实施归 0.0.6 专项）；里程碑表头锚点随改 +
M0 行"ADR 已写到 0016"；无法收口表：第 1 条指针随重锚更新、第 5 条补 0.0.5 批A–D
同口径；在制品段列 0.0.4 批G 载体与 0.0.5 批A–D 六提交（标题逐一照 git log 抄录）。
本版本 DoD 对账（0.0.5 方案 §6）：check.sh 七步全绿逐批达成、rrctl 六子命令一一
对应且有 fake-rrd 往返单测、裁定落档四问齐备（Q1–Q4 方案 §2 + 批B/C/D 落地）、
索引一致（docs/README 四档落点表 + 库地图 + 根 README 双语均指 0.0.5）、status.md
重锚完成——本批即 0.0.5 收口汇报，双远端指针一致随本批推送核对。①a–⑤ 全绿重跑。

其余规约（命名、分层、职责边界）靠评审；命名唯一权威是 [naming.md](naming.md)。

**七步之外的 CI 侧步骤**（`.github/workflows/ci.yml` 拆步 job，不进七步门，本地可跑）：
`lint`——rustfmt `--check` 与 clippy `-D warnings` 各自独立失败，覆盖 base/runtime 与
全部独立工程（④ 同一 find 口径）；绿色基线是 2026-10-01 的 rustfmt 全仓归一 + clippy
清零提交，此后 fmt/clippy 回退即 CI 红。`brain-unittest`——rr3-brain 26 条 stdlib
unittest（`python3 -m unittest discover -s tests`），ubuntu + macos 双矩阵；brain 不在
Rust workspace 里，①b/②b/④ 覆盖不到它，只有这一步跑。四真进程 e2e 仍属本地实跑
环节（同下节平台条件脚本的理由）。

**七步之外的平台条件脚本**：`bash tools/ci/ipct003.sh [--strict]`（IPC-T-003 跨架构对拍，
ADR-0006 后果项）**不在七步门内**——它依赖平台条件（Docker / aarch64 交叉目标），
塞进门禁等于让无容器环境的机器永远红。它做三件事：①宿主跑 `rr-ipc` 的
`src/bin/ipct003_dump.rs`（golden 头 + 五种 payload 类型各一整帧的 hex，全部编译期字面量），
header 行对 `contracts/golden/event-header-128.hex`；②已装 `aarch64-unknown-linux-gnu`
目标则交叉 `cargo check`，header.rs 的编译期 const 断言（偏移连续 / 头长 128 /
magic LE 字节 31 45 52 41 / payload 枚举值）随之在该目标求值；③有 Docker 则
`--platform linux/arm64` 容器真跑并与宿主输出逐行 diff。缺环境打 `SKIP` 并明说，
不静默当作已验证；`--strict` 供有容器环境的一方在 SKIP 时拒绝放行（退 2）。
双架构真跑结果何时回填，见 [../status.md](../status.md)。
2026-10-01 T4：`--strict` 模式已接 CI——`.github/workflows/ci.yml` 新增 `ipct003` job
（ubuntu-latest：x86_64 宿主 + `aarch64-unknown-linux-gnu` 交叉目标 + Docker
`rust:1-slim` 容器经 QEMU `--platform linux/arm64` 真跑），四步齐备无 SKIP 才绿；
远端跑绿后把 envelope.schema.json 的 IPC-T-003 移入 coverage 并回填 status.md。
本机（无 Docker）仍按宽容模式跑 ①②，③SKIP 照登。
2026-10-01 F5 本机实况：Intel Mac（macOS 13，无 Docker）跑出 ①PASS（宿主 x86_64，
dump == golden）②PASS（`aarch64-unknown-linux-gnu` 目标已装，交叉 check 通过，const
布局断言已在 aarch64 目标求值）③SKIP（本机无容器环境）。三方对拍的第三路真跑待
换机后补跑：`rustup target add aarch64-unknown-linux-gnu` + Docker 就绪的环境上
`bash tools/ci/ipct003.sh --strict`，全绿后在 [../status.md](../status.md) 回填。

## 3 结构纪律

- **主语言 Rust（2026-10-04 立规）**：仓内一切新实现一律 Rust；存量非 Rust 面按
  **功能语义不变**原则渐进迁移为 Rust，不设豁免通道——当前非 Rust 面为
  `rr3-brain`（Python，ADR-0014）与 CI 胶水脚本（bash，可最后迁）。brain 的迁移
  路线已由 [ADR-0016](adr/ADR-0016-brain迁移路线-python转Rust渐进首裁定.md)
  落设计裁定（首件 bus.py → rr-bus 复用、实施归 0.0.6 专项）；迁移本身走正常批次
  （生成器产出、契约校验、七步门禁一样不少），`module.toml` 的 `lang` 字段随迁移改为
  `rust`，原"Python 语法检查属人工检查点"的面随之收敛为 cargo 检查
- 新模块一律由生成器产出：`cargo run --manifest-path tools/mkmodule/Cargo.toml --bin rr3-mkmod -- rr3-<名>`
  （模块名形如 `rr3-arm`，第二参数是输出根，默认 `./modules`）。生成器落盘**五个文件**：
  `module.toml`、`config.toml`、`Cargo.toml`、`src/main.rs`、`README.md`——[naming.md](naming.md) §4
  所称"四件套"指四类结构角色（manifest / 运行参数 / 源码 / 说明），`Cargo.toml` 属于源码那件的
  crate 半边。五件齐备且 manifest 过契约校验才可合入 `modules/`；校验不过则整体不落盘，
  不留半成品模块
- 新增/变更主题必须先改 `contracts/topics.toml`（改契约 = 走 review），再在 manifest 里引用
- `contracts/` 只增不改；破坏性变更升 `contracts/envelope.schema.json` 的顶层元数据键
  `x-schema-version`（Rust 侧同名常量 `RR3_CONTRACT_SCHEMA_VERSION`，由 `cargo test -p rr-ipc
  --test envelope_meta` 逐值钉住）并在 `contracts/README.md` 的"演进登记"留痕；
  `contracts/topics.toml` 与 `module.schema.toml` 用的是另一个键名 `schema_version`，别混写
- 模块 crate 之间**不得**互相依赖，只能经 `rr-ipc`/`rr-bus`（一票否决项）
- 密钥、token 不进仓库：构建期环境变量注入（`SILICONFLOW_API_KEY`，见 `legacy/core/README.md`）
- 反射环（1k–10kHz 闭环）留在模块进程内，永不进总线（ADR-0007 裁定 1）
- `contracts/slots.toml` 能力格总表（ADR-0012）：与 topics.toml
  同级同纪律——只增不改，破坏性变更升 `schema_version`；manifest 的
  `provides`/`depends` 只许引用表内格位，查册门接入 CI 全集层（与仲裁共用真值）
- 装配单（ADR-0012）：`profiles/<机型>.toml` 是纯数据（`[os]` / `[bindings]` /
  `[faces]`），不得包含逻辑；新增机型 = 新增文件，不改代码；F⊗F 非法（brain 不许运行期
  决定能力归属，rrd 不许运行期重排装配）

## 4 Definition of Done

- [ ] `bash tools/ci/check.sh` 本地 exit=0
- [ ] 涉及 `contracts/` 的变更附带演进说明（只增不改 / major 升级）
- [ ] 涉及架构语义的变更已更新或新增 ADR
- [ ] 新模块由 rr3-mkmod 生成并通过 schema 校验
- [ ] 无新增 warning（`cargo check` 干净，不留 `#[allow]` 兜底）

## 5 文档纪律

- **legacy/ 处置三阶段（2026-10-05 用户裁定落规约，替代旧"冻结区不动它"口径）**：
  ①**只读消化期**——从 legacy 提语义进新架构（迁移映射见 architecture §8、逐组件盘点与
  排期唯一落点 [legacy-digest.md](legacy-digest.md)），**不回写
  legacy/ 内任何文件**（含其 README 已知断链——退役批才注销）；②**融合完成期**——盘点
  三分类中"待融合"清零，全部收敛为已融合/退役两类；③**退役期**——整树删除 + ⑤ 门
  legacy 排除条款撤除 + architecture §8 标注消化完成 + status 已知偏差注销，删除动作由
  退役批单独一提交执行。退役前的中间批次一律维持只读，不因"顺手"在 legacy 内改文件
- 状态与进度只写 `docs/status.md`；ADR 只在现状注记里记"决策落地到哪一步"的一句话指针；不写散落的周报式文档
- `docs/architecture/architecture.md` 是唯一架构图；目录树变更必须同步其 §3
- `docs/reference/` 三件的代码定位一律用**符号名锚定**（如 `registry.rs 的 rr3_reg_beat`、
  `Rr3Envelope 的 seq 字段`），**不写行号**（行号随代码漂移，是系统性风险源）；函数体内某段用
  "外层函数名 + 段描述"。`conventions.md` 正文里的历史叙述（如某轮评审点名的 `conn.rs:NNN`）
  属"当时发生了什么"的实录，不回改。
