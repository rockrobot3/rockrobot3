# tools/mkmodule —— `rr3-mkmod` 模块骨架生成器

生成**合规**的 `modules/rr3-<域>/` 骨架，落盘五件（缺一不合入）：

    module.toml · config.toml · Cargo.toml · src/main.rs · README.md

用法（CWD 必须是仓库根，默认输出目录是相对的 `modules/`）：

    cargo run -q --manifest-path tools/mkmodule/Cargo.toml --bin rr3-mkmod -- rr3-sense
    # 第二个参数可指定输出根：... -- rr3-arm out/try

生成器是规约的**执行器**，不是模板拷贝：命名规则（`^rr3-[a-z]+$`，词干只许小写字母）、
五件结构、主题前缀必须等于 `manifest.name` 都在代码里强制。写错名字直接拒绝，
已存在目录拒绝覆盖。产出后仍需人工把主题 append 进 `contracts/topics.toml`
（契约只增不改，改契约 = 走 review），否则运行时引用会被契约测试拒绝。

自检：`rr3_mkmod_generate` 落盘后立即调用 `rr-registry` 的
`rr3_registry_validate_manifest` 校验生成的 `module.toml`，不过即回滚报错——
"生成物天然合契约"，无需人再记一遍 naming.md。

独立工程：自带空 `[workspace]`，**不在** `base/runtime` 的 workspace 内。它以路径依赖
消费 `rr-registry`，若把自己挂进基座 workspace 会让"生成器"变成"运行时的一部分"。
测试单独跑（CI 里单列为 ②b）：

    cargo test --manifest-path tools/mkmodule/Cargo.toml
