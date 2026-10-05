# base/ota —— OTA 布局文档与槽位 CLI

本目录只有两样东西：

    partition-layout.md   ← 分区集合、两条更新轨道、切换与回滚链、断电安全约束
    rr3-ota-ctl/          ← bin rr3-ota-ctl（独立 Rust 工程）

标签的**唯一事实源在代码里**：`base/runtime/rr-ota/src/lib.rs` 的
`Rr3Slot::rr3_slot_label` 与 `RR3_OTA_SLOT_NAMES` / `RR3_OTA_STAGING_PARTITION`。
文档与 CLI 都引用那份常量而不重抄，所以"文档说 rr_a、代码用 rr_x"这种分家
在结构上不可能发生；`cargo test -p rr-ota` 的用例把标签、对合律与
genimage.cfg / extlinux.conf 真工件逐字钉死（条数只登记在
[`docs/process/conventions.md`](../../docs/process/conventions.md) §2）。

    cargo run -q --manifest-path base/ota/rr3-ota-ctl/Cargo.toml --bin rr3-ota-ctl -- slots
    # rr_a peer=rr_b / rr_b peer=rr_a / rr_factory peer=rr_factory / rr_staging (模块级暂存，无对侧)

0.0.3 批D 起，写侧的**引导配置改写**主机面已实装（判定与改写逻辑在
`rr-ota::bootconf`，base/runtime workspace 内全量跑测试；CLI 只做参数解析
与落盘，`--bootconf` 必传，不缺省碰任何路径）：

    rr3-ota-ctl switch rr_b --bootconf /boot/extlinux/extlinux.conf
    # 引导配置已改写：root=LABEL=rr_a → rr_b（持久切换，重启后生效）
    rr3-ota-ctl factory --bootconf /boot/extlinux/extlinux.conf
    # 一次性出厂引导；rr_factory 分区本体不写

边界（诚实口径）：本批交付的是**切槽的"翻标志"一步**——改 extlinux.conf 的
root=LABEL 标签（原子落盘）。槽位镜像写入（往 rr_b 落整镜像）随升级下载链
另批落地；U-Boot 环境变量写侧（bootcount / bootlimit / altbootcmd 自动回滚链）
与块设备/挂载探测仍是**板上件**，未实装、不 claim——语义与板上核对项清单见
[partition-layout.md](partition-layout.md) 增补段。在实板验证通过之前，这条
命令链具备"切过去"的能力，尚不具备"失败自动回得来"的保障。

容量不在本目录冻结（本机无 Buildroot 与板子，凭想象写下的 size 是最贵的那种错误）。
