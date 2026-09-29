# modules —— rr3-* 模块（四件套）

每个模块固定四件套，由 `rr3-mkmod` 生成，缺件不合入：

    modules/rr3-<域>/
    ├── module.toml   ← manifest：身份/能力/事件/资源/健康
    │                    字段字典：base/runtime/rr-registry/src/module.schema.toml
    ├── config.toml   ← 运行参数（改参数不碰源码，环境变量 RR3_ 前缀）
    ├── src/          ← Rust crate 或 Python 包
    └── README.md     ← 能力表 + 事件表 + 配置说明

生成（CWD 必须是仓库根，默认输出目录是相对的 `modules/`）：

    cargo run -q --manifest-path tools/mkmodule/Cargo.toml --bin rr3-mkmod -- rr3-sense

规划模块（architecture.md §3）：rr3-sense / rr3-act / rr3-interact / rr3-power /
rr3-connect / rr3-brain。v0.0.1 不放任何模块实现（M3+）。

硬规约：模块之间零直连，只经 rr-ipc/rr-bus；依赖写 capabilities 能力名，不写模块名；
主题必须先在 contracts/topics.toml 登记，前缀必须等于 manifest.name。
