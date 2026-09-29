# base/os —— Buildroot 层

ADR-0001 落点：Buildroot 主路线 + Linux 6.6 mainline + Radxa ZERO 3（RK3566）。
A/B 分区 slotA/slotB/factory 与 maskrom 不拆机自烧录在本层实现。

本层要交出的文件：`configs/rock3_zero3_defconfig`、
`board/radxa-zero3/genimage.cfg`（含 ADR-0007 决策二要求的独立可写模块分区）、
`patches/`、`post-build.sh`。这些文件今天有没有落、算哪一档属实况，唯一落点
[`docs/status.md`](../../docs/status.md)，本文件不记进度。
