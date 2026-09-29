# tools —— 主机侧工具

| 目录 | bin | 用途 |
|---|---|---|
| `mkmodule/` | `rr3-mkmod` | 生成合规模块四件套，产出物自动过 schema 校验 |
| `rrctl/` | `rr3-ctl` | 订阅总线查状态/日志/触发 OTA（M2+） |
| `sim/` | `rr3-sim` | mock HAL + 假模块，x86 全链路联调（M2+） |

`rr3-mkmod` 是**独立工程**（`tools/mkmodule/Cargo.toml` 自带空 `[workspace]`），不在 `base/runtime` 的 workspace 内：它以路径依赖消费 `rr-registry`，把自己挂进 workspace 会让"生成器"变成"运行时的一部分"。测试用 `cargo test --manifest-path tools/mkmodule/Cargo.toml` 单独跑，CI 里单列为 ②b。
mkmod 是规约的执行器：命名、四件套结构、主题前缀规则都在生成器里强制，而不是靠人记 naming.md。
