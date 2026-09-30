# base/os —— Buildroot 层

ADR-0001 落点：Buildroot 主路线 + Linux 6.6 mainline + Radxa ZERO 3（RK3566）。
A/B 分区 slotA/slotB/factory 与 maskrom 不拆机自烧录在本层实现。

本层要交出的文件：`configs/rock3_zero3_defconfig`、
`board/radxa-zero3/genimage.cfg`（含 ADR-0007 决策二要求的独立可写模块分区；
容量一律是 `@XXX_SIZE@` 构建期占位符，由 `board/radxa-zero3/post-image.sh` 注入——
容量不在本层冻结，见 [`../ota/README.md`](../ota/README.md)）、
`board/radxa-zero3/extlinux.conf`、`board/radxa-zero3/post-build.sh`、
`board/radxa-zero3/flash-maskrom.md`、`patches/`。这些文件今天有没有落、算哪一档属实况，
唯一落点 [`docs/status.md`](../../docs/status.md)，本文件不记进度。
全量 make 构建与 dts/defconfig 的核对清单见 [`patches/README.md`](patches/README.md)。
