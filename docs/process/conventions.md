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
①b 把 workspace 的 214 条（Windows，推算）/259 条（Unix）测试全部跑一遍，②/③ 只是其中被点名两批的定点诊断；
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
spec §5.3）都不在 11/11b，一个要新相位上线、一个要给 21 条生产主题建能力字典，均不阻塞 §9 验收；
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
硬 pin：7 owner / 21 topic / kind 7-8-3-3 / 21 条三元组；`every_module_manifest_passes_contract`
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

其余规约（命名、分层、职责边界）靠评审；命名唯一权威是 [naming.md](naming.md)。

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

## 3 结构纪律

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

## 4 Definition of Done

- [ ] `bash tools/ci/check.sh` 本地 exit=0
- [ ] 涉及 `contracts/` 的变更附带演进说明（只增不改 / major 升级）
- [ ] 涉及架构语义的变更已更新或新增 ADR
- [ ] 新模块由 rr3-mkmod 生成并通过 schema 校验
- [ ] 无新增 warning（`cargo check` 干净，不留 `#[allow]` 兜底）

## 5 文档纪律

- 状态与进度只写 `docs/status.md`；ADR 只在现状注记里记"决策落地到哪一步"的一句话指针；不写散落的周报式文档
- `docs/architecture/architecture.md` 是唯一架构图；目录树变更必须同步其 §3
