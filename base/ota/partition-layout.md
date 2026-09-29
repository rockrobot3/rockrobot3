# OTA 分区布局（RK3566 / Radxa ZERO 3）

> **状态**：v0.0.1 只冻结**分区集合与标签**，不冻结容量。标签的唯一事实源是
> `base/runtime/rr-ota/src/lib.rs`（`Rr3Slot::rr3_slot_label` 与
> `RR3_OTA_SLOT_NAMES` / `RR3_OTA_STAGING_PARTITION`），由 `cargo test -p rr-ota`
> 的 4 条用例钉死；`rr3-ota-ctl slots` 打印的就是那份常量，不再抄一遍。
> 容量与 `genimage.cfg` 属 M6，须与 Buildroot 镜像一起实板验证，本机无环境。

## 分区集合

| 标签 | 角色 | 写入方 | 落地 |
|---|---|---|---|
| （boot） | SPL/ATF/U-Boot + extlinux | dd / maskrom | M6 |
| `rr_a` | A 槽 rootfs（当前运行） | rr3-ota-ctl | M6 |
| `rr_b` | B 槽 rootfs（对侧） | rr3-ota-ctl | M6 |
| `rr_factory` | 出厂恢复镜像，只读 | 产线烧录 | M6 |
| `rr_staging` | 模块级更新的暂存区 | rrd 拉包 → 校验 → 落位 | M6+ |
| （data） | 用户数据与配置，升级不动 | 各模块 | M2 起使用 |

## 两条更新轨道（ADR-0007 决策二）

1. **整镜像 A/B**：写对侧槽 → 校验 → 改引导标志 → 重启；启动失败回滚到原槽。
   槽位对合律（`peer(peer(s)) == s`）由 rr-ota 测试钉住，是回滚判定的前提。
2. **模块级更新**：只换 `rr_staging` 里的单个模块产物，经 manifest
   `min_from_semver` 兼容校验后原子替换，不动 A/B、不重启整机。

## 断电安全约束（M6 实现时不得违反）

- 引导标志的翻必须发生在**对侧校验通过之后**，否则断电即砖
- `rr_factory` 任何代码路径不得写入
- `rr_staging` 内容在落位前必须过 `rr-registry` 的 manifest 契约（同一套规则，
  不因来源是 OTA 而放宽）
