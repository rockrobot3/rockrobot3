# rrd 命令行与启动面参考

> **代码即文档**：本档的参数名、默认值、错误文案与横幅格式全部取自
> [`rrd/src/cli.rs`](../../base/runtime/rrd/src/cli.rs)、
> [`rrd/src/lib.rs`](../../base/runtime/rrd/src/lib.rs)、
> [`rrd/src/main.rs`](../../base/runtime/rrd/src/main.rs) 的字面量。
> 断言口径分三档，说清楚免得改字面量时踩空：启动横幅（`rr3_banner`）由
> `lib.rs 的 rr3_banner` 与 `socket_banner.rs 的 banner_and_listen_line_show_the_resolved_absolute_path 用例` 用 `contains` 片段钉住，**不是逐字**；
> `Rr3RegisterError` / `Rr3PublishDeny` 的 `Display` 文案在 `registry.rs` 的
> `register_error_display_is_verbatim` 与 `publish_deny_display_distinguishes_the_three_layers` 两条单元用例里逐字 `assert_eq` 钉住，但集成侧（`tests/register_publish.rs`）
> 按 `contains` 子串区分"被哪一层拒的"；错误码映射（`rr3_rpc_status_for_register`
> 与 `rr3_rpc_status_for_publish_deny`，见 [`rrd-rpc.md`](rrd-rpc.md) §5）在
> `rpc.rs 的 register_error_maps_to_status_code 与 publish_deny_maps_to_status_code 用例` 的单元用例里逐枚举断言判别值。**唯一例外**：非 Unix 退出文案
> `RRD_UNSUPPORTED` 本 HEAD 的 Rust 用例里**没有任何一条按字面断言**它——全仓只有
> `lib.rs 的 RRD_UNSUPPORTED 常量`（定义）、`main.rs 的 rrd::RRD_UNSUPPORTED 导入` 与 `main.rs 的 rr3_run`（非 Unix 分支打印）三处使用，
> `base/runtime/rrd/README.md` 复述过它——改这个字面量的话，这两处与运行期任何按
> 字面匹配的脚本要同批查。**改代码必须同批改本档的对应字面量**，否则文档与线上文案各说一套。
> "接到了哪一步"属实况，唯一落点是 [`../status.md`](../status.md)，本档只写接口契约。

调用方读这一份就够：`rrd` 的全部命令行事实。设计语义（为什么这样切层）在
[`../architecture/architecture.md`](../architecture/architecture.md) §6.2。

---

## 1 用法串（逐字）

`cli.rs` 的 `USAGE` 常量是所有报错的尾巴，逐字如下（换行 = `\n`）：

```text
用法: rrd [--socket <path>] [--topics-overlay <path>] [--profile <path>] [--manifest <path>]... [--restart-base-ms <n>] [--pre-register-timeout-ms <n>] [--watchdog-tick-ms <n>] [--boot-role <role>]
说明: --boot-role 的值原样注入 boot 子进程的 RR3_SIM_PEER_ROLE，取值由夹具解释，rrd 不校验（E-6）。--profile 与 --manifest 不得同给（Q3：profile 装机正门，--manifest 调试后门）。
```

`-h` / `--help` 打印的就是这一串（`rr3_cli_parse_with_env` 的 `-h`/`--help` 分支）。多数参数错误也在文案第二段拼上它
（`need_value`、`need_path_value`、`rr3_cli_parse_with_env`、`parse_restart_base` 各 `\n{USAGE}` 分支）——但**不是全部**：`--socket 不得为空`
（`rr3_cli_parse_with_env` 的空值校验分支）、`{flag} 需为非负整数毫秒`（`cli.rs 的 parse_ms`）与
`--watchdog-tick-ms` 拒 0 那一支（`rr3_cli_parse_with_env` 的拒 0 分支）这三支只报第一行、
**不带 USAGE**（见 §3 表）。出处 `cli.rs 的 USAGE 常量`。

## 2 参数表

解析入口 `rr3_cli_parse`（`cli.rs 的 rr3_cli_parse`），可测试的注入版本 `rr3_cli_parse_with_env`
（`cli.rs 的 rr3_cli_parse_with_env`）。生产默认值取自**进程环境**，测试默认值取自注入的表——同一个函数体，
两条默认值来源（`rr3_cli_parse_with_env` 的文档注释是这条设计的理由）。

| 开关 | 值类型 | 默认 | 语义 | 出处 |
|---|---|---|---|---|
| `--socket` | 路径 | `rr3_default_socket`：环境键 `RR3_BUS_PATH`（常量 `RR3_BUS_PATH_ENV`）优先，缺省回落 `RR3_UDS_PATH_DEFAULT`（值 `/var/run/rock3-ipc.sock`） | UDS 绑定路径；进横幅与 `UnixListener::bind` 前被绝对化 | `cli.rs 的 rr3_default_socket`、`rr3_cli_parse_with_env` 的 `RrdConfig` 初始化、`transport/mod.rs 的 RR3_UDS_PATH_DEFAULT 常量` |
| `--topics-overlay` | 路径 | `None`（不加载） | 主题叠加层，**仅测试用**；`main.rs` 与装载入口各规范化一次（横幅凭证取 `main.rs` 那次、致命文案取装载入口那次，见 §4） | `rr3_cli_parse_with_env` 的 `--topics-overlay` 分支、`main` 的叠加层绝对化段、`topics_load.rs 的 rr3_overlay_resolved` |
| `--profile` | 路径 | `None`（不装载） | **装配单**（0.0.2 Q3 的装机**正门**）：`[bindings]`/`[faces]` 决定起谁，装载/查册/缺格判定/manifest 解析全部在 bind 之前完成；缺格只逐条点名"拒绝启动"降级该模块，**不是**致命项。与 `--manifest` 同给当场拒（§3） | `rr3_cli_parse_with_env` 的 `--profile` 分支、`main` 的装配单绝对化段、`profile.rs 的 rr3_profile_boot_inputs` |
| `--manifest` | 路径，**可重复** | 空表，顺序原样保留 | 待启动模块的 manifest（调试**后门**，Q3）；约定为绝对路径 | `rr3_cli_parse_with_env` 的 `--manifest` 分支 |
| `--restart-base-ms` | 整数毫秒 | `1000` | 重启退避基数：`delay_ms` = 表值 × base；下界 1、上界 `RR3_RESTART_BASE_MS_MAX` | `rr3_cli_parse_with_env` 的 `--restart-base-ms` 分支、`cli.rs 的 RR3_RESTART_BASE_MS_MAX 常量` |
| `--pre-register-timeout-ms` | 整数毫秒 | `3000` | 连接 accept 后多久没完成注册判为失败；`conn.rs` 侧按 `.max(1)` 用作读超时 | `rr3_cli_parse_with_env` 的 `--pre-register-timeout-ms` 分支、`conn.rs 的 rr3_conn_serve` |
| `--watchdog-tick-ms` | 整数毫秒 | `200` | **唯一 watchdog 线程的扫描间隔**（tick）；`0` 在 parse 处**当场拒**、不 `.max(1)` 静默夹——0 是忙循环，不是"更快一点"。非数字走 `parse_ms` 的"需为非负整数毫秒"口径 | `rr3_cli_parse_with_env` 的 `--watchdog-tick-ms` 分支、`rr3_cli_parse_with_env` 的文档注释、`RrdConfig 的 watchdog_tick_ms 字段`、`rr3_cli_parse_with_env` 的拒 0 分支 |
| `--boot-role` | 字符串 | `resident` | **原样透传、rrd 不校验取值**（E-6）：值进 boot 子进程的环境键 `RR3_SIM_PEER_ROLE`，合法集是**夹具**（`sim_peer`）的约定，不是守护进程的契约——rrd 一旦校验就要跟着夹具新增的脚本改，那是第二处真值；真设备模块根本不读这个变量。空值与疑似开关的值在 parse 处当场拒（见 §3） | `rr3_cli_parse_with_env` 的 `--boot-role` 分支、`RrdConfig 的 boot_role 字段`、`rr3_cli_parse_with_env` 的 `RrdConfig` 初始化 |
| `-h` / `--help` | 无值 | — | 向 stdout 打印 `USAGE`，**然后继续解析并继续启动**：解析器没有短路分支，退出码不由这条决定 | `rr3_cli_parse_with_env` 的 `-h`/`--help` 分支 |

`RrdConfig` 的字段名（供调用库版本的读者）：`socket`、`overlay`、`profile`、`manifests`、
`restart_base_ms`、`pre_register_timeout_ms`、`watchdog_tick_ms`、`boot_role`（`cli.rs 的 RrdConfig 结构体`）。
`watchdog_tick_ms` 有两个消费者：watchdog 线程的 `tick_ms` 与 supervisor 的扫尸节拍
（E-5 复用同一值，`daemon.rs` 的 `rr3_daemon_run` → `rr3_spawn_watchdog` / `rr3_spawn_supervisor`）；
`boot_role` 的唯一消费者是 supervisor
（`rr3_spawn_supervisor` 的 `boot_role` 参数，spawn 时作为 `RR3_SIM_PEER_ROLE` 注入），
接口形状见 [`rrd-registry.md`](rrd-registry.md) §7.1。

关于 `--manifest` 的两条事实要分清：`cfg.manifests` **已被消费**（Task 11b 落地）——
`daemon.rs` 的启动序先经 `supervisor.rs::rr3_boot_job_from_manifest` **全部装载**（entry 相对路径
按 **manifest 所在目录**解析，存在性 + 可执行位在 boot 那一刻查，被拒打 `拒绝启动` 且不吃预算），
再交给 rr-registry 仲裁，最后按裁定投递。CLI 序只是"同优先级之间的 tie-break"，不是 spawn 序本身。
`--profile` 路径下 boot 的 manifest 全集另有来源：装配单解析出 `<modules-root>/modules/<名>/module.toml`
（`profile.rs 的 rr3_profile_boot_inputs`），**其后的装载、仲裁、spawn 与后门路径共用同一段代码**——
Q3 裁定"两路径共用仲裁与 spawn，不双真值"指的就是这里。进度归
[`../status.md`](../status.md)。

## 3 fail-closed 门禁（错误文案逐字）

规则是"要么照你说的做，要么当场报错退出"，**没有任何一条会静默回落默认值**。
文案里的 `{flag}` / `{raw}` / `{other}` / `{v}` 是运行时替换位，其余字符逐字照抄。

| 触发条件 | 文案（逐字模板） | 出处 |
|---|---|---|
| 开关后面没值 | `{flag} 缺参数` + 换行 + `USAGE` | `cli.rs 的 need_value` |
| 路径型开关的值以 `-` 开头（含 `-h`） | `{flag} 缺参数：收到疑似开关的 {raw}（路径值不得以 - 开头）` + 换行 + `USAGE` | `cli.rs 的 need_path_value` |
| 未知选项 | `未知选项 {other}` + 换行 + `USAGE` | `rr3_cli_parse_with_env` 的未知选项分支 |
| `--socket` 解析后为空 | `--socket 不得为空` | `rr3_cli_parse_with_env` 的空值校验分支 |
| 数字型开关的值不是非负整数（含负数、`fast` 这类笔误） | `{flag} 需为非负整数毫秒，收到 {raw}` | `cli.rs 的 parse_ms` |
| `--watchdog-tick-ms 0` | `--watchdog-tick-ms 需为正整数毫秒（0 是忙循环，不是"更快一点"）`（文案里的引号是 ASCII `"`，**不带 USAGE**） | `rr3_cli_parse_with_env` 的拒 0 分支 |
| `--boot-role`（空值） | `--boot-role 不得为空（role 由夹具解释，空值只会得到一条查不出来源的"未知 RR3_SIM_PEER_ROLE"）` + 换行 + `USAGE`（文案里的引号是 ASCII `"`）。为什么拒空值而不是让子进程去报：`sim_peer` 会打 `未知 RR3_SIM_PEER_ROLE: ` 后跟一个空名字，那条失败既指不出参数也指不出用例 | `rr3_cli_parse_with_env` 的 `--boot-role` 空值分支 |
| `--boot-role` 的值以 `-` 开头（含 `--socket` 这类真开关） | `--boot-role 缺参数：收到疑似开关的 {raw}（role 不得以 - 开头）` + 换行 + `USAGE`。否则 `--boot-role --socket` 会把 `--socket` 当 role 吞下、下一个真开关丢失——与 `need_path_value` 同一形状的 fail-open 缝 | `rr3_cli_parse_with_env` 的 `--boot-role` 疑似开关分支 |
| `--restart-base-ms 0` | `--restart-base-ms 需为正整数毫秒：0 会让重启退避恒为 0（热重启循环，spec §5.3）` + 换行 + `USAGE` | `cli.rs 的 parse_restart_base` |
| `--restart-base-ms` 超上界 | `--restart-base-ms {v} 过大：退避末档乘上它会溢出 u64（饱和 = 永不重启），最大 {RR3_RESTART_BASE_MS_MAX}` + 换行 + `USAGE` | `cli.rs 的 parse_restart_base` |
| `--profile` 与 `--manifest` 同给（**两种顺序都拒**） | `--profile 与 --manifest 不得同给：profile 是装机正门（faces 决定起谁），--manifest 是调试后门（0.0.2 方案 Q3 裁定，两路径共用仲裁与 spawn，不双真值）` + 换行 + `USAGE` | `rr3_cli_parse_with_env` 的 `--profile`/`--manifest` 互斥分支（`cli.rs 的 PROFILE_MANIFEST_CONFLICT 常量`，两处分支共用同一句） |

四条设计裁定，改门禁前必读：

- **`--profile` 与 `--manifest` 在 parse 处互斥、当场拒**（`rr3_cli_parse_with_env` 的互斥分支）：
  不拒的后果不是报错晚，而是**双真值**——同一次 boot 里"faces 决定起谁"与"CLI 列表决定起谁"
  各说一套，装配单就失去了"装机正门"的资格。两条 boot 入口共用其后的仲裁与 spawn（§2），
  入口却必须唯一；这条与 `--watchdog-tick-ms` 拒 0 同一口径：**策略结论必须在启动时让运维听到**。

- **路径值不得以 `-` 开头**（`need_path_value`，`cli.rs 的 need_path_value`）：否则
  `rrd --socket --topics-overlay` 会把 `--topics-overlay` 当成套接字路径吞掉，
  叠加层静默丢失——那是这条链路上唯一的 fail-open 缝（`--boot-role` 落地后是**两条**：
  role 的同形拒绝见 §3 表）。数字型开关由 `parse_ms`
  拒负值，两侧口径一致。
- **`--restart-base-ms` 上界由退避表末档导出**：
  `RR3_RESTART_BASE_MS_MAX = u64::MAX / 60`（`cli.rs 的 RR3_RESTART_BASE_MS_MAX 常量`），分母 60 即
  `RR3_RESTART_BACKOFF` 的末档（见 [`rrd-registry.md`](rrd-registry.md)）。
  改表不改这里会让溢出缝悄悄回来。`0` 与"末档乘溢出"都在 parse 处拒，而不是让
  `rr3_charge` 的饱和乘法默默兜住：饱和成 `u64::MAX` 的意思是"永不重启"，
  那是策略结论，运维必须在启动时听到（`cli.rs 的 RR3_RESTART_BASE_MS_MAX 常量`、`cli.rs 的 parse_restart_base`）。
- **`--watchdog-tick-ms` 的 0 同样在 parse 处拒、不 `.max(1)` 静默夹**（`rr3_cli_parse_with_env` 的 `--watchdog-tick-ms` 分支）：
  0 的语义是"每毫秒全表扫一遍"，那是一条烧核的忙循环；静默夹成 1 等于横幅与实况各说一套。
  这条与上一条同一个口径——**策略结论必须在启动时让运维听到**（字段文档见 `RrdConfig 的 watchdog_tick_ms 字段`）。

## 4 路径绝对化的分工（别互换）

| 路径 | 用的函数 | 为什么是它 | 失败文案 |
|---|---|---|---|
| `--socket` | `rr3_socket_absolute_path` → `std::path::absolute`（`cli.rs 的 rr3_socket_absolute_path`） | `bind` 之前套接字文件**必然还不存在**，`canonicalize` 会以 ENOENT 拒掉一个完全合法的待创建路径；代价是不解析符号链接 | `套接字路径 {path} 无法解析为绝对路径: {e}`，`main.rs` 打印时前缀 `rrd: ` |
| `--topics-overlay` | `rr3_topics_absolute_path` → `std::fs::canonicalize`（`topics_load.rs 的 rr3_topics_absolute_path`） | 叠加层必须**已经存在**才可装载；横幅那行是"叠加层已生效"的可见凭证，按 CLI 原词回显会让凭证内容依赖 CWD。**注意 `2f3453a` 起分两次规范化**：`main.rs` 这次只给横幅凭证，装载入口 `rr3_overlay_resolved`（`topics_load.rs 的 rr3_overlay_resolved`）**独立**再 canonicalize 一次给致命文案——同工具、同结果，但**不是共用同一个返回值**（详见 [`../status.md`](../status.md)） | `路径 {path} 无法解析为绝对路径: {e}`，`main.rs` 前缀为 `rrd: 主题叠加层参数不可用: ` |
| `--profile` | `rr3_profile_absolute_path` → `std::fs::canonicalize`（`profile.rs 的 rr3_profile_absolute_path`） | 装配单必须**已经存在**才可装载（与主题契约/叠加层同一理据）；`main.rs` 那次给横幅凭证，装载入口 `rr3_profile_boot_inputs` **独立**再 canonicalize 一次给致命文案——与叠加层同形（同工具、同结果、不是同一份返回值）；modules-root 的定位也派生自这份规范化结果 | `路径 {path} 无法解析为绝对路径: {e}`，`main.rs` 前缀为 `rrd: 装配单参数不可用: ` |
| 契约主题路径 | `rr3_contract_resolved` → `std::fs::canonicalize`（`topics_load.rs 的 rr3_contract_resolved`） | `RR3_CONTRACT_TOPICS_PATH` 由 `CARGO_MANIFEST_DIR` 拼出、字面就带 `..`；不规范化则致命文案里漏出 `..`，运维按**自己**的 CWD 展开会查错文件 | `读不到主题契约 {path}: {e}`，`main.rs` 前缀为 `rrd: 主题装载失败: ` |

四处都不静默回落原词（`cli.rs 的 rr3_socket_absolute_path`、`topics_load.rs 的 rr3_topics_absolute_path`、`profile.rs 的 rr3_profile_absolute_path`、`topics_load.rs 的 rr3_contract_resolved`）；叠加层与装配单都在横幅凭证与装载入口各规范化一次（`topics_load.rs 的 rr3_overlay_resolved`、`profile.rs 的 rr3_profile_boot_inputs`），工具与结果一致，只是不是同一份返回值。

## 5 启动序列、输出流与退出码

`main.rs 的 main 函数` 的顺序，逐条可观察：

| 步 | 动作 | 失败时 | 输出流 |
|---|---|---|---|
| 1 | `rr3_cli_parse` | 文案原样上 stderr，退码 1（`ExitCode::FAILURE`） | stderr（`main` 的 `rr3_cli_parse` 调用段） |
| 2 | 套接字路径绝对化（**在横幅之前**） | `rrd: ` + 文案，退码 1 | stderr（`main` 的套接字绝对化段） |
| 3 | 叠加层路径绝对化（若给了） | `rrd: 主题叠加层参数不可用: ` + 文案，退码 1 | stderr（`main` 的叠加层绝对化段） |
| 4 | 装配单路径绝对化（若给了） | `rrd: 装配单参数不可用: ` + 文案，退码 1 | stderr（`main` 的装配单绝对化段） |
| 5 | `rr3_topics_load`：契约 →（可选）叠加层合并 | 每条装载错误一行 `rrd: 主题装载失败: {e}`，退码 1。**两个平台都装**：契约坏了要当场看见，而不是"等不到 UDS" | stderr（`main` 的 `rr3_topics_load` 调用段） |
| 6 | 打印横幅（+ 叠加层凭证行、+ 装配单凭证行） | — | stdout（`main` 的横幅打印段） |
| 7 | `rr3_run`：Unix 交 `rr3_daemon_run`；非 Unix 打 `RRD_UNSUPPORTED` 退 1 | bind 失败、boot 仲裁 fatal 与装配链致命（装配单读不到 / 形状不合 / 查册未命中 / 结构不自洽；**缺格不是致命项**，只逐条点名）文案上 stderr，退码 1 | — |

运行期日志走 `rr3_log`：**stdout**、行首固定 `rrd: `（`lib.rs 的 rr3_log`）。集成侧的可观察面
全靠这一行首前缀，别把它当噪声。

### 5.1 横幅（逐字）

```text
rrd 0.0.1 (contracts schema v{contract}, manifest schema v{manifest}, bus {bus}, {topics} topics)
```

- `{contract}` = `RR3_CONTRACT_SCHEMA_VERSION`，`{manifest}` = `RR3_MANIFEST_SCHEMA_VERSION`。
- `{bus}` 必须是**解析后的绝对路径**，与 `UnixListener::bind` 同一个值
  （`main` 的套接字绝对化段；行为由 `tests/socket_banner.rs` 从线上钉住）。
- `{topics}` 来自装载结果，**不得写死**：叠加层生效时它是另一个数（`lib.rs 的 rr3_banner`）。

给了叠加层时，横幅之后紧接：

```text
rrd: 主题叠加层已加载（仅测试用）: {绝对路径}
rrd: 叠加主题 {t}
```

逐条列出叠加层新增的主题名（`main` 的叠加层凭证打印段）。

给了装配单时，横幅（及叠加层凭证行，若两者同给则按此顺序）之后紧接：

```text
rrd: 装配单已加载: {绝对路径}
```

装配单的可见凭证（0.0.2 Q3：装机正门）：绝对路径，与 CWD 无关（`main` 的装配单凭证打印段）。

### 5.2 非 Unix 平台

`conn` / `daemon` 两个模块整体挂在 `#[cfg(unix)]` 下，非 Unix 根本不参与编译；
走完 1–6 步后执行 `eprintln!` 打出下面这行并退码 1（`main.rs 的 rr3_run`（非 Unix 分支））：

```text
rrd: 本平台不支持 UDS 总线（需 Unix；Windows 侧只编译协议层与平台无关逻辑）
```

该串是 `rrd::RRD_UNSUPPORTED` 常量的唯一真值（`lib.rs 的 RRD_UNSUPPORTED 常量`），走 **stderr**，
前面那句"非 Unix 也装载契约"意味着这行之前还会先打出一条横幅。
`RRD_UNSUPPORTED 常量` 的注释称它被某步实证按字面断言；本 HEAD 的 Rust 用例里没有逐字断言它的
那一条，`base/runtime/rrd/README.md` 复述过它——改这个字面量的话，这两处与运行期
任何按字面匹配的脚本要同批查。

## 6 相关文档

- 上行 RPC 方法与错误码：[`rrd-rpc.md`](rrd-rpc.md)
- 在册表阶段机、退避与发布归属：[`rrd-registry.md`](rrd-registry.md)
- 主题事实源：[`../../contracts/topics.toml`](../../contracts/topics.toml)（本档不抄表）
- 命名规约：[`../process/naming.md`](../process/naming.md)
