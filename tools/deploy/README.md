# RR3 板侧固件烧录（发布制品）

**部署态口径（2026-11-25 批47 裁定，见 [decisions §2](../../docs/process/decisions.md)）**：
MCU 的交付形态 = 只烧基本功能档（classic `safe`），全链档只作验证选项；
MCU 无 A/B 分区，升级一律线下重烧。每个芯片平台独立部署，没有主/副板。

本说明随 tag release 资产与 ghcr OCI 制品
`ghcr.io/<owner>/rockrobot3-firmware:<tag>` 一同发布；制品由流水线构建，
本地不编译。

## 平台 × 入口 × 判据（到货即执行）

每个芯片平台独立部署，没有主/副板；下表只登记**脚本入口与判据**，实板做到了哪一步的
唯一落点是 [`docs/status.md`](../../docs/status.md)（敞口 6/10/11）。"实板状态"列里
"结构就绪"= 脚本与文档可在仓内复跑，**未在实板执行过**。

| 平台 | 承载档 | 制品 | 入口 | 验收判据 |
|---|---|---|---|---|
| ESP32 classic（xtensa LX6） | rtos 轻承载 | `rr3-esp32-safe-merged.bin`（4MB 整片）或 ELF + bootloader + `rr3-partitions.csv` | 整片：`esptool.py --chip esp32 write_flash 0x0 rr3-esp32-safe-merged.bin`；单件：`flash-artifact.sh <elf> --bootloader <bin>` | 串口 115200 前两行 `surface=` 与 `console=uart0@<baud> secondary=none`（采集用 `console-log.py`，先清 DTR/RTS） |
| ESP32-S3（N16R8） | rtos 轻承载 | `rr3-esp32s3-full-merged.bin`（16MB） | 同上，`--chip esp32s3` | 同上，另有 `[spool]` 周期行；full 档缺 WiFi/API key 会显式点名 |
| 瑞芯微 RK3566（Radxa ZERO 3）· Buildroot 保底轨 | host 全承载 | `rock3-zero3.img`（`buildroot.yml` 全量构建产出） | 按 [`flash-maskrom.md`](../../base/os/buildroot/board/radxa-zero3/flash-maskrom.md) 走 maskrom（idbloader 32K / itb 6M / boot.vfat 16M） | 串口出 shell；分区标签 `rr_a`/`rr_b`/`rr_factory`/`rr_staging`/`rr_data` 与 `genimage.cfg` 一致 |
| 瑞芯微 RK3566 · distro 快路轨 | host 全承载 | 版本化 bundle（`pack-bundle.sh`） | `sudo bash tools/deploy/install-bundle.sh <bundle.tar.gz>` | `verify-board-config.sh` 装机面漂移门 + `rr3-ctl status` 六模块 running |
| 英特尔 / AMD（x86_64） | host 全承载 | `ghcr.io/<owner>/rockrobot3:<tag>-amd64`（含 provenance/SBOM） | `docker run` 该镜像，或 bundle 装机 | `image-smoke.sh file` + `image-smoke.sh run`（六模块真起、零拒绝）——流水线每次发布实跑 |
| 进迭时空（SpacemiT K1/K3，riscv64） | host 全承载 | `ghcr.io/<owner>/rockrobot3:<tag>-riscv64` | `bash tools/deploy/build-riscv-image.sh --push` | 同上两段冒烟；`platforms.yml` 的 `riscv-image` 腿每次 push 复跑 |
| 树莓派 4B（aarch64）· 参考真板 | host 全承载 | deb / bundle | `pack-arm64.sh`（交叉）或 `build-onboard.sh`（板上原生构建音频件） | 六模块全 running、`denied_publish = 0`、整机重启自愈（0.0.6 真板已验） |

**各自第一个失败点**（按这个顺序查，别从代码开始）：

- **ESP32 任何档**：串口无 banner → 先查 USB 线与 DTR/RTS（裸 `cat` 会把芯片按在复位态，
  表现为"ROM 日志后全噪声"）；能读 ROM 读不到 `surface=` → 波特率或烧录档不匹配；烧录即
  失败 → 先确认 `rr3-partitions.csv` 含 `spool`（脚本 fail-closed 会先拒）。
- **RK3566 maskrom**：工具认不到设备 → 查 USB OTG 口、`rkdeveloptool ld` 与板子是否真在
  maskrom（按住 recovery 上电）；镜像写入成功但无 shell → 先对串口 1500000 8N1 与
  `extlinux.conf` 的 console 参数。
- **x86_64 / riscv64 镜像**：`docker run` 起不来 → 先看 `image-smoke.sh file` 是否过（装机树
  与可执行面），再看 `run` 段（六模块注册与拒绝计数）。

## 两条烧录链的现状（0.0.8 发布点，2026-11-27 批70 对表）

只登记**做到哪一步**与**命令从哪来**；实况的唯一落点仍是
[`docs/status.md`](../../docs/status.md)（敞口 6/10/11），本节不复述进度。

| 链 | 已跑（可回查的证据） | 待实板 | 命令来源 |
|---|---|---|---|
| ESP32 基本功能档（classic `safe`） | 烧录链本身跑过：S3 首烧（`espflash flash`，[三平台验证计划 §6.6](../../docs/process/dev/0.0.8/0.0.8版本通用底座三平台验证计划.md)）；classic 真板复烧后 60s 零复位（[证据 0014](../../docs/evidence/实测/0014.md)）；S3 侧此后 0018/0019/0021/0022 四份真板复验 | classic **safe 整片**（`rr3-esp32-safe-merged.bin`）单条命令的产线式实烧回执；S3 外设接线后的复验 | `tools/deploy/image-esp32.sh` 装配整片 → `esptool.py write_flash 0x0`；单件走 `tools/deploy/flash-artifact.sh`（校验 sha 后烧，缺 `spool` 分区即拒） |
| RK3566 maskrom（Buildroot 保底轨） | 构建面全绿：全量 make + 六分区/内容断言（run `44035094772`，详见 status 能力表"Buildroot OS 层"行）；操作序文档在册 | **全链未跑**：目标板 Radxa ZERO 3 未采购，`rkdeveloptool` 实烧与串口出 shell 均未发生 | [`flash-maskrom.md`](../../base/os/buildroot/board/radxa-zero3/flash-maskrom.md)（idbloader 32K / itb 6M / boot.vfat 16M）+ `buildroot.yml` 的 image job 产物 |

## 版本化部署包契约（A10，主机档）

发行版快路轨的部署包不是"随便打个 tar"：`pack-bundle.sh` 出包、`install-bundle.sh` 装机，
两边靠下面**三条**对齐（此前只写在两个脚本的注释里，仓内没有第二处可查——2026-11-27 批96 立此节）：

| 契约 | 规则 | 违约后果 |
|---|---|---|
| 包顶层目录名 | `rockrobot3-<commit>`（commit 取产物对应提交，不是 tag 或日期） | 装机侧按 `cut -d/ -f1` 取顶层，名字不合 → `非 A10 版本化包` 当场拒 |
| `BUNDLE_VERSION` | **必在包顶层**，键固定为 `commit=` / `built_at=` / `target=` / `components=` / `natives=`（natives 可为空） | 缺文件或不认识的行 = 不是本仓出的包；装机前先核存在性 |
| 安装序 | 新树落 `<parent>/.rockrobot3-new.*` → 启动自检 → 换入 → 失败按 `rollback()` 回旧树**并回旧二进制** | 只回树不回二进制会留下"旧树 + 新 `rrd`"（2026-11-09 真板实测过） |

实现单点：[`pack-bundle.sh`](pack-bundle.sh) 的 BUNDLE_VERSION 段、[`install-bundle.sh`](install-bundle.sh)
的顶层校验与 `rollback()`；改任一侧都要同批改本节。

**前置（base 层）**：本包是 A10 更新/重装 payload，**不含也不装 systemd unit**——
`install-bundle.sh` 从已装 `rrd.service` 读 `--socket`/`--profile` 开关。干净机首装必须
先走 deb（unit 随 deb 落位）；bundle-only 干净机上漂移门报 unit `MISS` 是如实提示，
不是误报。

### 装机后核验：`verify-board-config.sh`

装机树与仓内真值逐文件 sha256 判等，**覆盖六类**：模块 config（含 esp32/pi4b 档）、
`profiles/<name>.toml`、契约四册与平台事实表（`contracts/*.toml`，平台册按档位
`[platform].chip` 选一）、systemd unit（口径忽略 `ExecStart=` 行）、
`boards/<profile>/etc/**`、`hardware/<stem>/{wiring.toml,axis/*.toml}`（rrd 装载直接
消费，漂了即拒 boot）。缺件与内容不同同权计漂移。

```bash
# 本机装机树 / 远端板卡二选一；--profile 只比该机型（缺省扫全部档）
bash tools/deploy/verify-board-config.sh --repo . --local /usr/lib/rockrobot3 --profile pi4b-solo
bash tools/deploy/verify-board-config.sh --repo . --remote <user@host> --profile pi4b-solo
# 工具自检（gate ⑥b 同款；例数与范围以 --self-test 输出为准）
bash tools/deploy/verify-board-config.sh --self-test
```

退出码：`0` 全等 / `1` 有漂移或缺件 / `2` 用法错或档位面不自洽（装配单/芯片/平台册对不上）/ `3` self-test 失败。

## 制品 tag 与 `VERSION` 对表

**版本唯一真源是仓根 `VERSION`（现为 `0.0.8`）**，tag 与它同名且只打在 release 提交上；
判据单点在 `tools/ci/lib/release-tag.sh`（`publish.yml` 每个发布 job 的第一件就是调它，
不同源或非 release 提交即红；自检在 gate 的 scripts job）。
仓内 tag 只保留 **`v0.0.8`** 一个发布点，历史上的高号 tag 一律归档为
`archive/v0.0.x/<tag>`（只作回溯，不构成发布承诺）。

| 流水线 | 制品 | tag 形状 | 触发 |
|---|---|---|---|
| `publish.yml` | 运行镜像（rrd + 六模块 + rr3-ctl + 契约/装配单） | `ghcr.io/<owner>/rockrobot3:<tag>-amd64` · `-arm64` · `-riscv64`，合并清单 `<tag>`；`latest` 只跟 `vX.Y.Z` 且与 `VERSION` 同源 | `v*` tag、`main`（edge）、手动 |
| `publish.yml`（固件腿） | ESP32 固件包（整片 + ELF + bootloader + 分区表 + sha256） | `ghcr.io/<owner>/rockrobot3-firmware:<tag>`（`oras pull` 拉取） | 同上 |
| `buildroot.yml` | Buildroot 全量镜像 / OTA 包 | `rockrobot3-buildroot:v<VER>` · `rockrobot3-ota:v<VER>`（`VER` 取自 `VERSION`） | 提交触发（`[buildroot-full]`）与 tag |

**开发分支不产出制品**：`dev/*` 推送只跑门禁与各平台验证腿，发布面只认 tag 与 `main`。
所以"`VERSION = 0.0.8` + tag `v0.0.8`"这一对成立时才有 `:v0.0.8` 系列制品；
制品是否在库以 ghcr 包页为准，本仓不复制制品状态。

## 资产

| 文件 | 说明 |
|---|---|
| `rr3-periph-base-esp32-safe-dev.elf` | ESP32 classic safe 档（默认 feature：外设底座 + 麦克风采集，不含云/播放） |
| `rr3-periph-base-esp32s3-full-dev.elf` | ESP32-S3 full 档（`esp32s3,autonomy,tts`：全链） |
| `rr3-esp32-safe-merged.bin` | **classic 整片镜像（4 MB）**：bootloader + 分区表 + app，直烧 `0x0` |
| `rr3-esp32s3-full-merged.bin` | **S3 整片镜像（16 MB）**：同上 |
| `rr3-bootloader-esp32-dev.bin` | classic bootloader，与 app 同一 IDF v5.2.2 构建树 |
| `rr3-bootloader-esp32s3-dev.bin` | S3 bootloader，同上 |
| `rr3-partitions.csv` | 分区表真值（factory 0x2F0000 + spool 1MB） |
| `firmware-sha256.txt` | 全文件 sha256 |

拉取：`oras pull ghcr.io/<owner>/rockrobot3-firmware:<tag>`（或从 release 资产直接下载）。
`-dev` 表示构建期 `RR3_ALLOW_DEV_ADMIN_TOKEN=1`（公开缺省管理令牌），
不冒充生产凭据。

## 烧录（推荐：仓内脚本，校验后烧）

```bash
bash tools/deploy/flash-artifact.sh ./rr3-periph-base-esp32-safe-dev.elf \
  --bootloader ./rr3-bootloader-esp32-dev.bin --monitor

bash tools/deploy/flash-artifact.sh ./rr3-periph-base-esp32s3-full-dev.elf \
  --chip esp32s3 --bootloader ./rr3-bootloader-esp32s3-dev.bin
```

## 整片直烧（basic 功能交付态，推荐产线/无工具链机器）

整片镜像已含 bootloader、分区表与 app，**一条命令即可**（`esptool`）：

```bash
esptool.py --chip esp32   write_flash 0x0 rr3-esp32-safe-merged.bin
esptool.py --chip esp32s3 write_flash 0x0 rr3-esp32s3-full-merged.bin
```

写入后按 §验收 读串口自证。镜像由 `tools/deploy/image-esp32.sh` 装配，
流水线每次推送都跑同一脚本（三点 magic + 整片尺寸 fail-closed）；
分区表仍以 `rr3-partitions.csv` 为真值，`nvs` 不落在整片覆盖之外时保留凭据
（整片写入会覆盖 nvs——需保留 WiFi 时改用上面的单件烧录）。

## 裸 espflash（不经脚本时三件必须显式给）

```bash
espflash flash --chip esp32 \
  --bootloader rr3-bootloader-esp32-dev.bin \
  --partition-table rr3-partitions.csv \
  rr3-periph-base-esp32-safe-dev.elf
```

## 为什么不能只给 ELF

- ESP-IDF ELF 只含 app 镜像，**不含** bootloader 与分区表。espflash 未给
  `--bootloader` 时用其自带引导镜像，IDF 版本与 app 不保证一致；本包随带同版
  bootloader，就是为了消掉这个错配。
- 未给 `--partition-table` 时 espflash 从当前目录猜 CSV。缺 `spool` 分区的表
  会让语音段静默降级——烧前确认 CSV 含 `spool` 行（脚本会 fail-closed 拒烧）。
- `nvs` 区（0x9000/0x6000）不在擦除范围，WiFi 凭据保留。

## 验收（串口 115200）

- **采集方式**：仓内 `tools/deploy/console-log.py`（raw 8N1，打开后**显式清
  DTR/RTS**）。不要用裸 `cat`/`stty`——打开 CH340 时 DTR/RTS 常停在置位态，
  芯片被按在复位/下载态，表现为"ROM 日志可读、随后全是噪声"，极易误判成
  固件控制台配错。自实现时同样必须先清两线再收流。
- boot 横幅前两行自证：`surface=...` 与 `console=uart0@<baud> secondary=none`
  ——能读到这两行，说明日志口与终端波特率都正确。
- 其余：boot 横幅点名芯片与 PSRAM；`[mem]` 给堆账，`[mic]` 给采样率（周期
  诊断行 10s 一行），full 档另有 `[spool]` 行。
- safe 档不连云；full 档需 NVS 已有 WiFi 与 API key，缺则显式点名。
