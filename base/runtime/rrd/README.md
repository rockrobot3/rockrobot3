# rrd —— 基座主守护进程（装配层骨架）

装配 `rr-ipc` + `rr-registry`（Cargo path 依赖）的守护进程落点：
supervisor / health / config，事件总线的宿主（architecture.md §6、§7.1 启动链）。

v0.0.1 实况：`src/main.rs`（68 行）只做"握手横幅"——打印契约版本与总线路径，
证明 workspace 六个 crate 能互相引用。**待落地（M2）**：

- 加载 manifests → 依赖仲裁 → 按 `start_priority` 拉起模块
- 心跳监督 + 按 `restart_policy` 处理崩溃（rr-registry 状态机的运行时执行方）
- 主题前缀的运行时注册校验（当前校验入口只有 mkmod 自检 + rr-registry 契约测试）
- 安全兜底（急停/限位/低电保护）不经 brain 直接在 rrd 执行（§7.3）

运行：`cargo run -p rrd`；UDS 路径默认 `/var/run/rock3-ipc.sock`，
可被环境变量 `RR3_BUS_PATH` 覆盖。横幅格式由 `banner()` 的单元测试锁定。
