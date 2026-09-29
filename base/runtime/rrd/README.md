# rrd —— 基座主守护进程（装配层）

装配 `rr-ipc` + `rr-registry` + `rr-bus`（Cargo path 依赖）的守护进程落点：
supervisor / health / config，事件总线的宿主（architecture.md §6、§7.1 启动链）。

M2 起拆成 **lib + bin**：纯逻辑在 `src/lib.rs`（`cli` 参数、`topics_load` 所有权表装载），
`src/main.rs` 只剩"解析 → 装载 → 打印横幅 → 平台门"。这样集成测试与 `src/bin/`（Task 9 的
`sim_peer`）才引用得到这些模块——bin 里的模块对外不可见。

v0.0.1 + M2 Task 3 实况：

- 所有权表装载 = `contracts/topics.toml` 全集（生产真相）+ 仅测试的 `--topics-overlay` 叠加层；
  叠加层与契约 topic 撞名 → **拒绝启动并点名条目**；叠加层声明内建命名空间 `rr3-runtime/*`
  的任一条目（`rr3_topics_is_reserved_topic` 命中）→ **同样拒绝启动并逐条点名**；契约缺失、
  解析后空表、形状不合（owner 未登记 / 不在 owner 命名空间内 / kind 非法）同样致命，且形状错误
  逐条标注来源（`主题契约 …` vs `主题叠加层 …`）。装载结果只有一条路：非空且自检通过。
- 叠加层路径在 `main` 里经 `rr3_topics_absolute_path`（`std::fs::canonicalize`）**一次**绝对化，
  装载与横幅共用同一个值，故"主题叠加层已加载"这行凭证与 CWD 无关
  （Windows 上 canonicalize 带 `\\?\` verbatim 前缀，仍是绝对路径；Linux 侧是干净的 `/abs/…`）。
- 非 Unix 平台启动即打印 `rrd::RRD_UNSUPPORTED` 并以退出码 1 结束，绝不假称可跑
  （Windows 本机实测 `EXIT=1`；UDS 分支在 `#[cfg(unix)]` 之后，本机连编译都不参与）。
- 命令行：`--socket`、`--topics-overlay`、可重复的 `--manifest`、`--restart-base-ms`、
  `--pre-register-timeout-ms`；未知选项/缺参数/非数字一律报错，不静默回落默认值。
  三个路径型开关的取值**不得以 `-` 开头**——否则 `rrd --socket --topics-overlay` 会把开关名
  当成套接字路径吞掉、静默丢掉叠加层。
  默认总线路径取 `RR3_BUS_PATH` 环境变量，无则 `RR3_UDS_PATH_DEFAULT`。

**待落地（M2 后续任务）**：

- 加载 manifests → 依赖仲裁 → 按 `start_priority` 拉起模块（Task 4/8）
- 在册表 + 状态机 + 发布归属 + 重启记账（Task 5）、上行 RPC 三方法（Task 6）
- 心跳监督 + 按 `restart_policy` 处理崩溃（Task 7/8 的 `conn`/`daemon`）
- 主题前缀的**运行时注册**校验（模块在 RPC 里声称 `rr3-runtime/*` 时的拒绝）：
  `rr3_topics_is_reserved_topic` 现在有两个消费点——装载处（`rr3_topics_load` 拒叠加层冒充）
  已落地，注册处随 Task 5/6 的在册表与 RPC 接上
- 安全兜底（急停/限位/低电保护）不经 brain 直接在 rrd 执行（§7.3）

运行：`cargo run -p rrd -- --socket <path> [--topics-overlay <path>] [--manifest <path>]...`；
UDS 路径默认 `/var/run/rock3-ipc.sock`，可被环境变量 `RR3_BUS_PATH` 覆盖。
横幅格式由 `rr3_banner` 的单元测试锁定，主题数取自装载结果（不是写死的 21）。
