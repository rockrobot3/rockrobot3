# base/ota —— OTA 布局文档与槽位 CLI

本目录只有两样东西，且都**不做实际的槽位写入**：

    partition-layout.md   ← 分区集合、两条更新轨道、断电安全约束
    rr3-ota-ctl/          ← bin rr3-ota-ctl（独立 Rust 工程）

标签的**唯一事实源在代码里**：`base/runtime/rr-ota/src/lib.rs` 的
`Rr3Slot::rr3_slot_label` 与 `RR3_OTA_SLOT_NAMES` / `RR3_OTA_STAGING_PARTITION`。
文档与 CLI 都引用那份常量而不重抄，所以"文档说 rr_a、代码用 rr_x"这种分家
在结构上不可能发生；`cargo test -p rr-ota` 的用例把标签与 `peer()` 对合律钉死
（条数只登记在 [`docs/process/conventions.md`](../../docs/process/conventions.md) §2）。

    cargo run -q --manifest-path base/ota/rr3-ota-ctl/Cargo.toml --bin rr3-ota-ctl -- slots
    # rr_a peer=rr_b / rr_b peer=rr_a / rr_factory peer=rr_factory / rr_staging (模块级暂存，无对侧)

`switch` 一律**拒绝执行**（退出码 1）：对块设备改引导标志属不可逆操作，写侧要与
`base/os/buildroot/board/radxa-zero3/genimage.cfg` 一起落地并实板验证——那一步今天有没有做，
唯一落点是 [`docs/status.md`](../../docs/status.md)。在此之前提供半成品的
切换命令，等于提供一台可以砖掉的机器。

容量不在本目录冻结（本机无 Buildroot 与板子，凭想象写下的 size 是最贵的那种错误）。
