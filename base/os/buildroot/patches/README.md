# base/os/patches —— 内核 / U-Boot 回移补丁

本目录承载 Buildroot 全量构建（Linux/容器侧）所需的内核与 U-Boot 补丁。
**今天还没有任何补丁文件**——下面登记的是"落补丁前必须先核对"的清单，
核对动作与补丁本体都属板上/Linux 侧收口，本机（macOS）无法声称。

## 核对清单（建镜像前逐条过）

| # | 事项 | 核对方法 | 未命中的处置 |
|---|---|---|---|
| 1 | `rk3566-radxa-zero-3.dts` 是否在 Linux 6.6 的 `arch/arm64/boot/dts/rockchip/` 里 | 在 Linux/容器侧 clone v6.6 tag 后 `ls`；或查 mainline 历史看该 dts 的引入版本 | 不在则从引入它的 mainline commit 回移（backport），补丁落本目录，**在补丁头注释里记来源 commit 与版本**；defconfig 的 `BR2_LINUX_KERNEL_INTREE_DTS_NAME` 保持不动 |
| 2 | `radxa-zero-3-rk3566` 是否为 U-Boot `2024.10` 的有效 defconfig | `make O=… radxa-zero-3-rk3566_defconfig` 一次即知 | 改用 Radxa BSP/社区分支的 U-Boot，来源同样记入补丁头或本表 |
| 3 | extlinux 的 `console=ttyS2,1500000` 与 fdt 文件名 | 板上串口实测 | 属板上收口，见 `board/radxa-zero3/extlinux.conf` 尾注 |

## Buildroot 接法

补丁落本目录后，在 `configs/rock3_zero3_defconfig` 加：

```
BR2_LINUX_KERNEL_PATCH="board/radxa-zero3/patches"        # 或 defconfig 内相对路径按 Buildroot 语义写
```

（Buildroot 对 `board/<vendor>/<board>/patches` 有默认搜索路径；以构建期实测为准，
本条只登记意图，不预设语法细节。）

## 产物注入（与补丁无关，但同属"Buildroot 外的输入"）

rrd / 模块二进制**不在 Buildroot 里编译**：用本仓 workspace 做
`aarch64-unknown-linux-gnu` 交叉编译后，把产物放到 `$BINARIES_DIR/`
（rrd、rr3-ctl）与 `$BINARIES_DIR/rr3-modules/<name>/{bin,manifest.toml}`，
由 `post-build.sh` 装入目标根。缺席时 post-build 只警告不阻断。
