# rr-ota —— 升级通道的类型形状（双轨）

本 crate 交出"双轨"的**类型形状**（ADR-0007 决策二：整镜像 A/B 与模块级更新各一条轨道），
slot 切换与断电安全动作要随 Buildroot 镜像一起落地——那部分属实况，唯一落点
[`docs/status.md`](../../../docs/status.md)，本文件不记进度。

| 类型/常量 | 含义 |
|---|---|
| `Rr3Slot { SlotA, SlotB, Factory }` + `rr3_slot_peer` / `rr3_slot_label` | 整镜像 A/B 与出厂槽位（Factory 的对侧仍是自己）；标签 `rr_a` / `rr_b` / `rr_factory` 由此函数单点产出 |
| `Rr3SlotState { Active, Staged, Idle, Corrupt }` | 槽位状态机；Corrupt = 校验失败的自动回滚候选 |
| `RR3_OTA_SLOT_NAMES = ["rr_a","rr_b"]` | 与 `base/os/buildroot/board/radxa-zero3/genimage.cfg` 的分区标签逐字一致（include_str 守护用例钉死，改动任一侧即红） |
| `RR3_OTA_STAGING_PARTITION = "rr_staging"` | 模块级更新的 staging 分区标签——布局现在即预留 |

标签的**唯一事实源在本 crate 的代码里**，`base/ota/` 的布局文档与 `rr3-ota-ctl` 都引用那份常量、
不重抄。测试：`cargo test -p rr-ota`（由 check.sh 的 ①b 覆盖），
条数只登记在 [`docs/process/conventions.md`](../../../docs/process/conventions.md) §2。
