# tools/rrctl —— `rr3-ctl` 总线侧运维 CLI

四个子命令经 UDS **真连 rrd 的事务面**（F3 / [ADR-0010](../../docs/process/adr/ADR-0010-控制通道与查询面.md)）：
查在册模块、看健康状态、取日志；OTA 恒拒随 M6（[ADR-0007](../../docs/process/adr/ADR-0007-控制面分层与模块级升级.md)
决策二的模块级更新轨道）。

    rr3-ctl [--bus-path <path>] <status|health|logs|ota>

套接字路径优先级：`--bus-path` > 环境变量 `RR3_BUS_PATH` > 契约默认值
`/var/run/rock3-ipc.sock`。默认值与环境变量名都从 `rr-ipc` 引入，不在本文件重抄，
所以契约改动时 CLI 不会留在旧值上。

连接语义（ADR-0010 裁定 2/3）：每个子命令是一次**一次性控制连接**——同一套接字、
同一 ADR-0006 线格式，但**不注册、不进在册表、不受 watchdog 管**，一问一答即断。
`status` 列在册模块（NAME/TASK/PHASE + 拒发布的计数）、`health` 加心跳三列
（间隔 / 看门狗超时 / 距上次心跳）、`logs` 取 rrd 内存日志环（512 行，不落盘）、
`ota` 由 rrd 回错误（文案点 M6），CLI 以非 0 退出。

独立工程：自带空 `[workspace]`，**不在** `base/runtime` 的 workspace 内（与 rr3-mkmod 同构）。
路径依赖 `rr-bus` + `rr-ipc` + `serde_json`，命令面受同一套契约类型约束。

    cargo run -q --manifest-path tools/rrctl/Cargo.toml --bin rr3-ctl -- status
    cargo test --manifest-path tools/rrctl/Cargo.toml

编译由 `tools/ci/check.sh` 第 ④ 步覆盖（独立 workspace 不在 ①/①b 的作用域内）；
单测含两条真 UDS 往返用例（fake rrd），**不在七步门内**，收口时按 conventions §2
备注本地跑。
