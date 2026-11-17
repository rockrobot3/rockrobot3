# base/os/buildroot/patches —— 内核 / U-Boot 回移补丁

本目录承载 Buildroot 全量构建（Linux/容器侧）所需的内核与 U-Boot 补丁。
**现存一件**：`0001-arm64-dts-rockchip-add-radxa-zero-3.patch`（v6.12 → 6.6 板级
dts 回移）。下方核对清单是"落补丁前必须先核对"的台账，逐条结论随实测回填；
本机（macOS）不承担构建验证，构建回执归 CI `buildroot` 流水线。

## 核对清单（建镜像前逐条过）

| # | 事项 | 核对方法 | 结论 / 未命中的处置 |
|---|---|---|---|
| 1 | `rk3566-radxa-zero-3*.dts` 是否在 Linux 6.6 的 `arch/arm64/boot/dts/rockchip/` 里 | mainline 目录逐 tag 实查 | **2026-11-16 已核对：v6.6 没有该板 dts**；主线只有 `-3e`（v6.11 起）与 `-3w`（v6.12 起）。已从 v6.12 逐字回移三份文件并在本目录 `0001-…patch` 注册两个 dtb；defconfig 用 `BR2_LINUX_KERNEL_PATCH` 应用 |
| 2 | `radxa-zero-3-rk3566` 是否为 U-Boot `2024.10` 的有效 defconfig | 源码树实查 + `make …_defconfig` | **2026-11-16 已核对存在**（`source.denx.de/u-boot/u-boot` v2024.10，`configs/radxa-zero-3-rk3566_defconfig` HTTP 200）；构建期一次 make 回执随全量构建 CI |
| 3 | extlinux 的 `console=ttyS2,1500000` 与 fdt 文件名 | 板上串口实测 | 属板上收口；fdt 默认 `rk3566-radxa-zero-3w.dtb`（3E 实板改 `-3e.dtb`），见 `board/radxa-zero3/extlinux.conf` 尾注 |
| 4 | RK3566 的 TPL/BL31 blob | rkbin 仓内实查 + binman 回执 | **2026-11-17 已核对**：`bin/rk35/rk3566_ddr_1056MHz_v1.18.bin` 与 `bin/rk35/rk3568_bl31_v1.43.elf` 在 Buildroot 2024.02.11 钉住的 rkbin 提交 `b4558da` 内；defconfig 已启用 `BR2_PACKAGE_ROCKCHIP_RKBIN` + `BR2_TARGET_UBOOT_NEEDS_ROCKCHIP_RKBIN`。缺任一时 binman 报 `.binman_stamp Error 103`（第 5 拍实证） |
| 5 | genimage 的宿主 vfat 工具 | 全量构建回执 | **2026-11-17 已核对**：host-genimage 只依赖 libconfuse，不带 `mkfs.vfat`/`mcopy`；defconfig 已补 `BR2_PACKAGE_HOST_DOSFSTOOLS` / `BR2_PACKAGE_HOST_MTOOLS`。缺任一时只报 `target-post-image Error 1`（第 6 拍实证） |
| 6 | post-image 参数符号 | Buildroot `system/Config.in` 实查 | **2026-11-17 已核对**：真符号是 `BR2_ROOTFS_POST_SCRIPT_ARGS`，不是 `BR2_ROOTFS_POST_IMAGE_SCRIPT_ARGS`；写错被 kconfig 静默忽略，`genimage.sh` 收不到 `-c`（第 7 拍实证） |
| 7 | genimage `label` 位置 | genimage 源码 + 全量回执 | **2026-11-17 已核对**：`label` 属于 `vfat`/`ext` 文件系统子段，不是 `image` 公共段；放错报 `no such option 'label'`（第 8 拍实证） |
| 8 | 空分区与 ext 生成器 | genimage 源码 + 全量回执 | **2026-11-17 已核对**：无 `mountpoint` 的 ext4 必须 `empty = true`（否则回落到根 rootpath）；ext4 段用 `use-mke2fs = true`（host-genimage 不带 genext2fs，mke2fs 由 host-e2fsprogs 提供） |
| 9 | ext4 内容源 `srcpath` | 全量回执 | **2026-11-17 已核对**：`genimage.sh` 的 `--rootpath` 是空临时目录，ext4 段必须 `srcpath = "@TARGET_DIR@"`（`post-image.sh` 注入真实 target）；否则生成空 rootfs（e2fsck/标签全过、内容为空，第 11 拍实证） |

## 本目录现有补丁

- `0001-arm64-dts-rockchip-add-radxa-zero-3.patch`：v6.12 的
  `rk3566-radxa-zero-3.dtsi` / `-3e.dts` / `-3w.dts` 逐字回移到 6.6，
  并在 `arch/arm64/boot/dts/rockchip/Makefile` 注册两个 dtb；补丁头记来源 tag。

## Buildroot 接法

`configs/rock3_zero3_defconfig` 已写：

```
BR2_LINUX_KERNEL_PATCH="board/radxa-zero3/patches"
BR2_LINUX_KERNEL_INTREE_DTS_NAME="rockchip/rk3566-radxa-zero-3w rockchip/rk3566-radxa-zero-3e"
```

路径按"把 `configs/` 与 `board/` 叠进 Buildroot 源码树"的既有用法解析。

## 产物注入（与补丁无关，但同属"Buildroot 外的输入"）

rrd / 模块二进制**不在 Buildroot 里编译**：用本仓 workspace 做
`aarch64-unknown-linux-musl` 静态交叉编译后，把产物放到 `$BINARIES_DIR/`
（`rrd`、`rr3-ctl`、`rr3-ota-ctl`）与 `$BINARIES_DIR/rr3-modules/<name>/{bin,manifest.toml}`，
由 `post-build.sh` 装入目标根（模块 manifest 归位 `module.toml`，entry 改写为
装机绝对路径）。缺席时 post-build 只警告不阻断；产物注入的真值单点是
[`tools/ci/cross-artifacts.sh`](../../../../tools/ci/cross-artifacts.sh) 与
`buildroot.yml` 的注入步，两处不得各写一份清单。
