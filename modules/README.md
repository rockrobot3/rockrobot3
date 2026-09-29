# modules —— rr3-* 模块（生成器落盘五个文件）

每个模块固定由 `rr3-mkmod` 生成下列**五个文件**，缺件不合入
（四类结构角色见 [naming.md](../docs/process/naming.md) §4，`Cargo.toml` 属"源码"那件的 crate 半边）：

    modules/rr3-<域>/
    ├── module.toml   ← manifest：身份/能力/事件/资源/健康
    │                    字段字典：base/runtime/rr-registry/src/module.schema.toml
    ├── config.toml   ← 运行参数（改参数不碰源码，环境变量 RR3_ 前缀）
    ├── Cargo.toml    ← 独立 crate（接总线时 path 依赖 rr-ipc / rr-bus）
    ├── src/main.rs   ← 入口骨架（Rust crate 或 Python 包）
    └── README.md     ← 能力表 + 事件表 + 配置说明

**`entry` 字段：在仓相对，装机绝对**。六份 `module.toml` 的 `entry` 写的是**相对**值
（如 `entry = "rr3-sense"`）——它们是**契约工件**，不是注册输入：供 ② 步的
`every_module_manifest_passes_contract` 校验 manifest 形状并钉住模块集合，同时是 `rr3-mkmod`
落盘的产物形态。真正的注册输入是设备上那份：rrd 的注册握手（`base/runtime/rrd/src/conn.rs`）
要求 `manifest.entry` 必须是绝对路径——注册帧带来的 manifest 从线上（wire）来，没有"所在目录"可依据，
相对 `entry` 会被 `AIRY_EINVAL` 拒掉；线级用例
`rrd/tests/register_publish.rs::bad_manifests_name_the_field_and_keep_rrd_alive`
把这条门禁与"文案必须点名绝对路径"钉着。所以：**不要为了让门禁放行而把仓内 `module.toml`
改成绝对路径**（那会把契约工件变成注册输入，等于用仓内夹具去定义生产语义）；绝对 `entry`
由装机环节写入——磁盘装载与打包时的路径改写今天做到哪一步属实况，见
[`docs/status.md`](../docs/status.md)。

**目录里有什么**：六个模块位都已建好 —— rr3-sense / rr3-act / rr3-interact /
rr3-power / rr3-connect（Rust，`rr3-mkmod` 生成的五件结构）+ rr3-brain（Python 包，`rr3_brain/`，
故无 `Cargo.toml`，其"源码半边"是 `pyproject.toml`）。每个模块的 `src/` 内构目录见各自
README 与 [`architecture.md`](../docs/architecture/architecture.md) §3。

机器把关：② 步校验六份 `module.toml` 并钉住模块集合，④ 步编译五个 Rust crate。
**这些入口今天做到哪一步**（只有自述、还是已接总线）属实况，唯一落点
[`docs/status.md`](../docs/status.md)；测试条数只登记在
[`docs/process/conventions.md`](../docs/process/conventions.md) §2，本文件都不抄。

生成新模块（CWD 必须是仓库根，默认输出目录是相对的 `modules/`）：

    cargo run -q --manifest-path tools/mkmodule/Cargo.toml --bin rr3-mkmod -- rr3-<新域名>

硬规约：模块之间零直连，只经 rr-ipc/rr-bus；依赖写 capabilities 能力名，不写模块名；
主题必须先在 contracts/topics.toml 登记，前缀必须等于 manifest.name。
