# base/os —— Buildroot 层（M1）

ADR-0001 落点：Buildroot 主路线 + Linux 6.6 mainline + Radxa ZERO 3（RK3566）。
A/B 分区 slotA/slotB/factory 与 maskrom 不拆机自烧录在本层实现。

v0.0.1 只占位。M1 交付物：`configs/rock3_zero3_defconfig`、
`board/radxa-zero3/genimage.cfg`（含 ADR-0007 决策二要求的独立可写模块分区）、
`patches/`、`post-build.sh`。
