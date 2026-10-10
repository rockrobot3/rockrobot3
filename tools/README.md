# tools —— 主机侧工具

| 目录 | bin | 用途 |
|---|---|---|
| `mkmodule/` | `rr3-mkmod` | 生成合规模块骨架，落盘**五个文件**（`module.toml` / `config.toml` / `Cargo.toml` / `src/main.rs` / `README.md`），产出物自动过 schema 校验 |
| `ci/` | — | `check.sh`：规约检查的唯一实现与唯一入口，**十步**（① workspace 编译含测试目标 / ①b workspace 全量测试 / ② manifest schema 与注册表形状 / ②b mkmod 产物即契约 / ③ 主题前缀归属 / ④ 独立 Rust 工程编译 / ⑤ 文档门：已跟踪 md 非空、相对链接不断 / ⑥ 独立工程测试接线 / ⑦ §13 静态断言四件 / ⑧ 件级判据三门：对拍计数·依赖闭包·注入面校验），本地与 CI 跑同一命令；标题沿用方案 D7 的"三检查"语义命名。步序、命令与每步条数只登记在 [`docs/process/conventions.md`](../docs/process/conventions.md) §2 |
| `rrctl/` | `rr3-ctl` | 订阅总线查状态/日志/触发 OTA 的运维 CLI（命令面与参数见其 README） |
| `sim/` | `rr3-sim` | mock HAL 设备源 + 假模块，x86 全链路联调 |

`rr3-mkmod` 是**独立工程**（`tools/mkmodule/Cargo.toml` 自带空 `[workspace]`），不在 `base/runtime` 的 workspace 内：它以路径依赖消费 `rr-registry`，把自己挂进 workspace 会让"生成器"变成"运行时的一部分"。测试用 `cargo test --manifest-path tools/mkmodule/Cargo.toml` 单独跑，CI 里单列为 ②b。
mkmod 是规约的执行器：命名、模块骨架的五件结构、主题前缀规则都在生成器里强制，而不是靠人记
[`docs/process/naming.md`](../docs/process/naming.md)。
