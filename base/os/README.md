# base/os —— OS 双轨层（ADR-0001 选型、ADR-0013 分轨）

OS 层分两轨，**同一 payload、两条轨道各出一份安装产物**（ADR-0013 裁定 3）：

## `buildroot/` —— 保底轨

Buildroot 主路线 + Linux 6.6 mainline + Radxa ZERO 3（RK3566），角色是
**量产冻结镜像**（逐字节可复现、无发行版变数）与 maskrom 不拆机自烧录的
最深控制层。要交出的文件：`configs/rock3_zero3_defconfig`、
`board/radxa-zero3/genimage.cfg`（含 ADR-0007 决策二要求的独立可写模块分区；
容量一律是 `@XXX_SIZE@` 构建期占位符，由 `board/radxa-zero3/post-image.sh`
注入——容量不在本层冻结，见 [`../ota/README.md`](../ota/README.md)）、
`board/radxa-zero3/extlinux.conf`、`board/radxa-zero3/post-build.sh`、
`board/radxa-zero3/flash-maskrom.md`、`patches/`。
全量 make 构建与 dts/defconfig 的核对清单见 [`buildroot/patches/README.md`](buildroot/patches/README.md)。

## [`distro/`](distro/README.md) —— 快路轨

现成发行版 + payload 层（.deb / OCI）：rrd 与模块交叉产物、systemd 单元、
profile 按包交付，不碰内核与引导链。开发与功能收口先行走这条轨。

---

这些文件今天有没有落、算哪一档属实况，唯一落点
[`docs/status.md`](../../docs/status.md)，本文件不记进度。
机型选型与装配（哪些格位、哪个 profile）不落在本层——那是
`profiles/` 的活（ADR-0012 装配单）。
