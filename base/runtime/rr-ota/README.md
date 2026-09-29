# rr-ota —— 升级通道骨架（双轨类型）

v0.0.1 只冻结"双轨"的**类型形状**（architecture.md §3、ADR-0007 决策二），
具体 slot 切换/断电安全实现随 Buildroot 镜像一起落地（**M6**）：

| 类型/常量 | 含义 |
|---|---|
| `Rr3Slot { SlotA, SlotB, Factory }` + `rr3_slot_peer` | 整镜像 A/B 与出厂槽位（Factory 无对侧） |
| `Rr3SlotState { Active, Staged, Idle, Corrupt }` | 槽位状态机；Corrupt = 校验失败的自动回滚候选 |
| `RR3_OTA_SLOT_NAMES = ["rr_a","rr_b"]` | 与 genimage.cfg 分区标签一致（M1/M2 建分区时核对） |
| `RR3_OTA_STAGING_PARTITION = "rr_staging"` | 模块级更新的 staging 分区标签——布局现在即预留 |

测试：`cargo test -p rr-ota`（check.sh ①b 覆盖）。整镜像 A/B → M6；模块级更新 → M6+。
