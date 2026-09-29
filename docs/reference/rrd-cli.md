# rrd 命令行与启动面参考

> **代码即文档**：本档的参数名、默认值、错误文案与横幅格式全部取自
> [`rrd/src/cli.rs`](../../base/runtime/rrd/src/cli.rs)、
> [`rrd/src/lib.rs`](../../base/runtime/rrd/src/lib.rs)、
> [`rrd/src/main.rs`](../../base/runtime/rrd/src/main.rs) 的字面量。
> 其中三类文字由用例**逐字**断言——启动横幅（`rr3_banner`）、非 Unix 退出文案
> （`RRD_UNSUPPORTED`）、错误码映射（`rr3_rpc_status_for_register` 与
> `rr3_rpc_status_for_publish_deny`，见 [`rrd-rpc.md`](rrd-rpc.md)）——
> **改代码必须同批改本档的对应字面量**，否则文档与线上文案各说一套。
> "接到了哪一步"属实况，唯一落点是 [`../status.md`](../status.md)，本档只写接口契约。

调用方读这一份就够：`rrd` 的全部命令行事实。设计语义（为什么这样切层）在
[`../architecture/architecture.md`](../architecture/architecture.md) §6.2。

---

## 1 用法串（逐字）

`cli.rs` 的 `USAGE` 常量是所有报错的尾巴，逐字如下（换行 = `\n`）：

```text
用法: rrd [--socket <path>] [--topics-overlay <path>] [--manifest <path>]... [--restart-base-ms <n>] [--pre-register-timeout-ms <n>]
```

所有 `-h` / `--help` 的输出与所有参数错误的第二段都是这一串（出处 `cli.rs:21`）。

## 2 参数表

解析入口 `rr3_cli_parse`（`cli.rs:53`），可测试的注入版本 `rr3_cli_parse_with_env`
（`cli.rs:68`）。生产默认值取自**进程环境**，测试默认值取自注入的表——同一个函数体，
两条默认值来源（`cli.rs:57-64` 的注释是这条设计的理由）。

| 开关 | 值类型 | 默认 | 语义 | 出处 |
|---|---|---|---|---|
| `--socket` | 路径 | `rr3_default_socket`：环境键 `RR3_BUS_PATH`（常量 `RR3_BUS_PATH_ENV`）优先，缺省回落 `RR3_UDS_PATH_DEFAULT`（值 `/var/run/rock3-ipc.sock`） | UDS 绑定路径；进横幅与 `UnixListener::bind` 前被绝对化 | `cli.rs:48-50`、`cli.rs:82-85`、`transport/mod.rs:14` |
| `--topics-overlay` | 路径 | `None`（不加载） | 主题叠加层，**仅测试用**；装载与横幅共用同一个 canonicalize 结果 | `cli.rs:86-89`、`main.rs:39-48` |
| `--manifest` | 路径，**可重复** | 空表，顺序原样保留 | 待启动模块的 manifest；约定为绝对路径 | `cli.rs:90-93` |
| `--restart-base-ms` | 整数毫秒 | `1000` | 重启退避基数：`delay_ms` = 表值 × base；下界 1、上界 `RR3_RESTART_BASE_MS_MAX` | `cli.rs:94-98`、`cli.rs:16` |
| `--pre-register-timeout-ms` | 整数毫秒 | `3000` | 连接 accept 后多久没完成注册判为失败；`conn.rs` 侧按 `.max(1)` 用作读超时 | `cli.rs:99-103`、`conn.rs:368` |
| `-h` / `--help` | 无值 | — | 向 stdout 打印 `USAGE`，**然后继续解析并继续启动**：解析器没有短路分支，退出码不由这条决定 | `cli.rs:104-107` |

`RrdConfig` 的字段名（供调用库版本的读者）：`socket`、`overlay`、`manifests`、
`restart_base_ms`、`pre_register_timeout_ms`（`cli.rs:9-19`）。

关于 `--manifest` 的两条事实要分清：注释里"实际 spawn 序按 `start_priority` 重排"
（`cli.rs:12`）是**约定**，本 HEAD 的 `main.rs` / `daemon.rs` 没有任何一处读
`cfg.manifests`，也没有对它的值做绝对性检查（绝对性门禁 `rr3_manifest_entry_is_absolute`
管的是**注册报文里** `manifest.entry` 字段，不是这个命令行开关）。进度归
[`../status.md`](../status.md)。

## 3 fail-closed 门禁（错误文案逐字）

规则是"要么照你说的做，要么当场报错退出"，**没有任何一条会静默回落默认值**。
文案里的 `{flag}` / `{raw}` / `{other}` / `{v}` 是运行时替换位，其余字符逐字照抄。

| 触发条件 | 文案（逐字模板） | 出处 |
|---|---|---|
| 开关后面没值 | `{flag} 缺参数` + 换行 + `USAGE` | `cli.rs:32` |
| 路径型开关的值以 `-` 开头（含 `-h`） | `{flag} 缺参数：收到疑似开关的 {raw}（路径值不得以 - 开头）` + 换行 + `USAGE` | `cli.rs:41` |
| 未知选项 | `未知选项 {other}` + 换行 + `USAGE` | `cli.rs:108` |
| `--socket` 解析后为空 | `--socket 不得为空` | `cli.rs:112` |
| 数字型开关的值不是非负整数（含负数、`fast` 这类笔误） | `{flag} 需为非负整数毫秒，收到 {raw}` | `cli.rs:118` |
| `--restart-base-ms 0` | `--restart-base-ms 需为正整数毫秒：0 会让重启退避恒为 0（热重启循环，spec §5.3）` + 换行 + `USAGE` | `cli.rs:143-145` |
| `--restart-base-ms` 超上界 | `--restart-base-ms {v} 过大：退避末档乘上它会溢出 u64（饱和 = 永不重启），最大 {RR3_RESTART_BASE_MS_MAX}` + 换行 + `USAGE` | `cli.rs:148-150` |

两条设计裁定，改门禁前必读：

- **路径值不得以 `-` 开头**（`need_path_value`，`cli.rs:38-44`）：否则
  `rrd --socket --topics-overlay` 会把 `--topics-overlay` 当成套接字路径吞掉，
  叠加层静默丢失——那是这条链路上唯一的 fail-open 缝。数字型开关由 `parse_ms`
  拒负值，两侧口径一致。
- **`--restart-base-ms` 上界由退避表末档导出**：
  `RR3_RESTART_BASE_MS_MAX = u64::MAX / 60`（`cli.rs:29`），分母 60 即
  `RR3_RESTART_BACKOFF` 的末档（见 [`rrd-registry.md`](rrd-registry.md)）。
  改表不改这里会让溢出缝悄悄回来。`0` 与"末档乘溢出"都在 parse 处拒，而不是让
  `rr3_reg_death` 的饱和乘法默默兜住：饱和成 `u64::MAX` 的意思是"永不重启"，
  那是策略结论，运维必须在启动时听到（`cli.rs:23-29`、`cli.rs:133-138`）。

## 4 两条路径绝对化的分工（别互换）

| 路径 | 用的函数 | 为什么是它 | 失败文案 |
|---|---|---|---|
| `--socket` | `rr3_socket_absolute_path` → `std::path::absolute`（`cli.rs:128-131`） | `bind` 之前套接字文件**必然还不存在**，`canonicalize` 会以 ENOENT 拒掉一个完全合法的待创建路径；代价是不解析符号链接 | `套接字路径 {path} 无法解析为绝对路径: {e}`，`main.rs` 打印时前缀 `rrd: ` |
| `--topics-overlay` | `rr3_topics_absolute_path` → `std::fs::canonicalize`（`topics_load.rs:44-47`） | 叠加层必须**已经存在**才可装载；横幅那行是"叠加层已生效"的可见凭证，按 CLI 原词回显会让凭证内容依赖 CWD | `路径 {path} 无法解析为绝对路径: {e}`，`main.rs` 前缀为 `rrd: 主题叠加层参数不可用: ` |
| 契约主题路径 | `rr3_contract_resolved` → `std::fs::canonicalize`（`topics_load.rs:63-66`） | `RR3_CONTRACT_TOPICS_PATH` 由 `CARGO_MANIFEST_DIR` 拼出、字面就带 `..`；不规范化则致命文案里漏出 `..`，运维按**自己**的 CWD 展开会查错文件 | `读不到主题契约 {path}: {e}`，`main.rs` 前缀为 `rrd: 主题装载失败: ` |

三处都不静默回落原词（`cli.rs:121-127`、`topics_load.rs:40-43`、`topics_load.rs:49-62`）。

## 5 启动序列、输出流与退出码

`main.rs:15-82` 的顺序，逐条可观察：

| 步 | 动作 | 失败时 | 输出流 |
|---|---|---|---|
| 1 | `rr3_cli_parse` | 文案原样上 stderr，退码 1（`ExitCode::FAILURE`） | stderr（`main.rs:17-23`） |
| 2 | 套接字路径绝对化（**在横幅之前**） | `rrd: ` + 文案，退码 1 | stderr（`main.rs:29-35`） |
| 3 | 叠加层路径绝对化（若给了） | `rrd: 主题叠加层参数不可用: ` + 文案，退码 1 | stderr（`main.rs:39-48`） |
| 4 | `rr3_topics_load`：契约 →（可选）叠加层合并 | 每条装载错误一行 `rrd: 主题装载失败: {e}`，退码 1。**两个平台都装**：契约坏了要当场看见，而不是"等不到 UDS" | stderr（`main.rs:51-62`） |
| 5 | 打印横幅（+ 叠加层凭证行） | — | stdout（`main.rs:64-79`） |
| 6 | `rr3_run`：Unix 交 `rr3_daemon_run`；非 Unix 打 `RRD_UNSUPPORTED` 退 1 | bind 失败文案上 stderr，退码 1 | — |

运行期日志走 `rr3_log`：**stdout**、行首固定 `rrd: `（`lib.rs:20-24`）。集成侧的可观察面
全靠这一行首前缀，别把它当噪声。

### 5.1 横幅（逐字）

```text
rrd 0.0.1 (contracts schema v{contract}, manifest schema v{manifest}, bus {bus}, {topics} topics)
```

- `{contract}` = `RR3_CONTRACT_SCHEMA_VERSION`，`{manifest}` = `RR3_MANIFEST_SCHEMA_VERSION`。
- `{bus}` 必须是**解析后的绝对路径**，与 `UnixListener::bind` 同一个值
  （`main.rs:25-35`；行为由 `tests/socket_banner.rs` 从线上钉住）。
- `{topics}` 来自装载结果，**不得写死**：叠加层生效时它是另一个数（`lib.rs:30-35`）。

给了叠加层时，横幅之后紧接：

```text
rrd: 主题叠加层已加载（仅测试用）: {绝对路径}
rrd: 叠加主题 {t}
```

逐条列出叠加层新增的主题名（`main.rs:73-79`）。

### 5.2 非 Unix 平台

`conn` / `daemon` 两个模块整体挂在 `#[cfg(unix)]` 下，非 Unix 根本不参与编译；
走完 1–5 步后执行 `eprintln!` 打出下面这行并退码 1（`main.rs:96-101`）：

```text
rrd: 本平台不支持 UDS 总线（需 Unix；Windows 侧只编译协议层与平台无关逻辑）
```

该串是 `rrd::RRD_UNSUPPORTED` 常量的唯一真值（`lib.rs:26-28`），走 **stderr**，
前面那句"非 Unix 也装载契约"意味着这行之前还会先打出一条横幅。
`lib.rs:26` 的注释称它被某步实证按字面断言；本 HEAD 的 Rust 用例里没有逐字断言它的
那一条，`base/runtime/rrd/README.md` 复述过它——改这个字面量的话，这两处与运行期
任何按字面匹配的脚本要同批查。

## 6 相关文档

- 上行 RPC 方法与错误码：[`rrd-rpc.md`](rrd-rpc.md)
- 在册表阶段机、退避与发布归属：[`rrd-registry.md`](rrd-registry.md)
- 主题事实源：[`../../contracts/topics.toml`](../../contracts/topics.toml)（本档不抄表）
- 命名规约：[`../process/naming.md`](../process/naming.md)
