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

## 接入一块新板：要交的六件（机制面零改动）

"换一块芯片就重做一遍"是本层要消掉的成本，所以把**换板动作**列成清单——每一件都有现役
实例可抄，每一件都有机器判据接手；清单之外的东西（机制核、契约四表、六模块、rrd）**不许改**，
那是功能守恒红线（[`architecture.md`](../../docs/architecture/architecture.md) §12.8 四处窄缝）。

| # | 层 | 交付物 | 现役实例 | 谁接住 |
|---|---|---|---|---|
| 1 | 芯片事实 | `contracts/platform/<芯片>.toml`（控制器词表、能力集、冲突矩阵、具名构造表） | `esp32` / `esp32s3` / `rk3566` / `raspberry-pi-4` 四份在册 | 装载门十条校验 + ⑨ 增量门；缺册即拒 boot |
| 2 | 装机描述符 | `profiles/<形态>.toml` 八段（`[os] [platform] [supplies] [slots] [bindings] [enabled_slots] [hardware] [pins]`） | `esp32-mini` / `esp32s3-mini` / `pi4b-*` / `distro` | profile 装载链（基-23 双缺即拒）+ ⑨-n「九枚物理键只许由 `[[pin]]` 派生」 |
| 3 | 角色→引脚构造（只有 MCU 档需要） | `tools/esp32-periph-base/src/board/pins_<芯片>.rs`——esp-idf-hal 要具名引脚类型，这处不对称消不掉 | `pins_s3.rs` / `pins_classic.rs` | `platforms` 的 `esp32-gates` 该芯片腿 |
| 4 | 保底轨（要量产冻结镜像 / maskrom 自烧录时） | `buildroot/configs/<板>_defconfig` + `board/<板>/{genimage.cfg,extlinux.conf,post-build.sh,post-image.sh,flash-maskrom.md}` + `patches/`（内核 dts 回移） | `rock3_zero3_defconfig` + `board/radxa-zero3/` | `buildroot.yml` 的 `dtb-backport` / `image` 腿 + 装配冒烟 |
| 5 | 快路轨（发行版上跑） | `distro/boards/<板>.md`（装机与排障笔记）+ 复用 `distro/units/rrd.service` | `rk3566-zero3.md` / `raspberry-pi-64.md` | `integration` 的 `distro-smoke`（deb 装机冒烟） |
| 6 | 制品与烧录入口 | `tools/deploy/` 对应脚本 + `platforms.yml` 一条矩阵腿 + [`ci-workflows.md` 覆盖矩阵](../../docs/process/ci-workflows.md)一行 | `flash-artifact.sh` / `image-esp32.sh`（MCU）、`build-riscv-image.sh` / `pack-*`（host） | 矩阵腿删除/改名即红（`workflows_audit`） |

**两轨的目录形状不同，且不该强行一致**：保底轨是 `board/<板>/` 一目录多件（Buildroot 按目录
取板级件），快路轨只有一页 `<板>.md`（发行版差异是运维笔记，不是构建输入）。把快路轨也摊成
目录，或把保底轨压成一页，都会让"这一件谁在读"变得不可判。
**核对（2026-11-27 批79）**：两轨现役文件无孤儿件——保底轨仅 `radxa-zero3`（多一份 `patches/`
做内核 dts 回移，快路轨没有对应物，这是分轨本意）；快路轨两页对应两块已验证板
（RK3566、树莓派 4B）。

---

这些文件今天有没有落、算哪一档属实况，唯一落点
[`docs/status.md`](../../docs/status.md)，本文件不记进度。
机型选型与装配（哪些格位、哪个 profile）不落在本层——那是
`profiles/` 的活（ADR-0012 装配单）。
