# rrd —— 基座主守护进程（装配层）

装配 `rr-ipc` + `rr-registry` + `rr-bus`（Cargo path 依赖）的守护进程落点：supervisor /
health / config，事件总线的宿主（[`architecture.md`](../../../docs/architecture/architecture.md)
§6 与 §7.1 启动链；急停/限位/低电保护这类安全兜底**不经 brain 直接在 rrd 执行**，语义见同档 §7.3）。

拆成 **lib + bin**：纯逻辑全部在 `src/lib.rs` 声明的那几个模块里（bin 里的模块对外不可见，
集成测试引用不到），`src/main.rs` 只做"解析 → 装载 → 打印横幅 → 平台门 → 把主循环交棒 daemon"。

| 文件 | 内容 |
|---|---|
| `src/cli.rs` | `rr3_cli_parse` / `rr3_cli_parse_with_env`：命令行参数与 `RrdConfig` 的默认值注入 |
| `src/topics_load.rs` | 所有权表装载：`contracts/topics.toml` 全集 + 仅测试用的叠加层，两条路径都在装载入口内规范化（`rr3_topics_load` / `rr3_topics_absolute_path` / `rr3_topics_is_reserved_topic`） |
| `src/manifest.rs` | 运行期事实的读取与入口校验：`Rr3ManifestFacts`、`rr3_manifest_facts_from_text`、`rr3_manifest_entry_is_absolute` |
| `src/registry.rs` | 在册表 `RrdRegistry`：注册、心跳、到期判活、死亡记账与重启计划、扇出、快照，以及发布归属判定 |
| `src/rpc.rs` | 上行 JSON-RPC 的方法白名单、参数校验、应答形状与错误码映射 |
| `src/conn.rs` · `src/daemon.rs` | **整体 `#[cfg(unix)]`**：UDS 监听与 accept 循环、每连接一线程、注册握手、上行分发与发布转发 |
| `src/lib.rs` | 上述模块声明 + 两个字面量：`rr3_banner` 与 `RRD_UNSUPPORTED` |

**接口字面量不在本文件复述**：启动参数与默认值、横幅、退出码见
[`docs/reference/rrd-cli.md`](../../../docs/reference/rrd-cli.md)；上行三方法、错误码表与
发布面处理序见 [`docs/reference/rrd-rpc.md`](../../../docs/reference/rrd-rpc.md)；阶段机、
常量表、四道归属门与重启退避见 [`docs/reference/rrd-registry.md`](../../../docs/reference/rrd-registry.md)。
主题与 kind 的事实源是 [`contracts/topics.toml`](../../../contracts/topics.toml)。

**进度也不在本文件复述**：哪一条能力接到了线上、watchdog 线程与模块 spawn 还缺哪一手，
唯一落点 [`docs/status.md`](../../../docs/status.md)；测试条数只登记在
[`docs/process/conventions.md`](../../../docs/process/conventions.md) §2。
本 crate 真正跑起来只在 Unix 上；非 Unix 平台启动即打印 `RRD_UNSUPPORTED` 并以非 0 退出，
绝不假称可跑（退出码的字面值见 `docs/reference/rrd-cli.md`）。
