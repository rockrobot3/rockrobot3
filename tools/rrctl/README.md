# tools/rrctl —— `rr3-ctl` 总线侧运维 CLI

规划能力（architecture.md §3）：查在册模块、看健康状态、取日志、触发 OTA。
**v0.0.1 只交付命令面**：参数解析、套接字路径解析与输出形状先定下来，真正的
UDS 往返随 M2 —— rrd 还没上线时无处可问，先写死一条假链路只会骗过测试。

    rr3-ctl [--bus-path <path>] <status|health|logs|ota>

套接字路径优先级：`--bus-path` > 环境变量 `RR3_BUS_PATH` > 契约默认值
`/var/run/rock3-ipc.sock`。默认值与环境变量名都从 `rr-ipc` 引入，不在本文件重抄，
所以 M2 改动契约时 CLI 不会留在旧值上。

独立工程：自带空 `[workspace]`，**不在** `base/runtime` 的 workspace 内（与 rr3-mkmod 同构）。
路径依赖 `rr-bus` + `rr-ipc`，因此命令面受同一套契约类型约束。

    cargo run -q --manifest-path tools/rrctl/Cargo.toml --bin rr3-ctl -- status
    cargo test --manifest-path tools/rrctl/Cargo.toml

编译由 `tools/ci/check.sh` 第 ④ 步覆盖（独立 workspace 不在 ①/①b 的作用域内）。
