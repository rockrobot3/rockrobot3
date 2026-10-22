# OTA 分区布局（RK3566 / Radxa ZERO 3）

> **状态**：v0.0.1 只冻结**分区集合与标签**，不冻结容量。标签的唯一事实源是
> `base/runtime/rr-ota/src/lib.rs`（`Rr3Slot::rr3_slot_label` 与
> `RR3_OTA_SLOT_NAMES` / `RR3_OTA_STAGING_PARTITION`），由 `cargo test -p rr-ota`
> 的用例（含 include_str 把 genimage.cfg 拉进来逐字对拍）钉死；`rr3-ota-ctl slots` 打印的就是那份常量，不再抄一遍。
> 容量不在本目录冻结（本机无 Buildroot 与板子，凭想象写下的 size 是最贵的那种错误）。
> genimage.cfg 的**结构与分区标签**已落 `base/os/buildroot/board/radxa-zero3/genimage.cfg`
> （容量一律构建期占位符，`post-image.sh` 注入；标签与本目录上述常量由
> `rr-ota` 的 include_str 守护用例逐字钉死）；容量定型与实板验证仍属 M6 板上收口。

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

## 0.0.5 增补轮六 批N5 占位：模块级更新的装载真值集判据（M6 实装）

模块级更新（上方轨道 2）落位前，除 manifest 契约外还须过两道判据
（实-12 修法，判据冻结在此、载体随 M6 落地——当前 `rr3.ctl.ota` 恒拒，
rr3-ota-ctl 已不在仓，消费方尚无运行态）：

1. **同一清单**：staging 树的装载消费真值集成员与 rrd
   `rr3_runtime_truth_set` 判等——OTA 侧不得自带第二份清单，与
   rr3-pack 打包面同口径（增补轮六 基-25：清单真值只有 rrd 一份，
   消费方照单判等）。
2. **契约 schema 版本列**：manifest 在 `min_from_semver` 兼容面增加
   契约 schema 版本列；staging 装机位的契约 schema 版本与模块声明的
   该列**不等即拒装**（fail-closed），不做区间兼容猜测。

## 0.0.3 批D 增补：切换与回滚链（主机面已实装，板上件待验）

### 切换语义（已实装，主机可测）

`rr3-ota-ctl` 写侧的判定与改写逻辑在 `rr-ota::bootconf`（base/runtime
workspace 内，字节级单测钉死，含 extlinux.conf 真工件对拍）：

- **`switch <rr_a|rr_b> --bootconf P`**：改写 boot 分区
  `/extlinux/extlinux.conf` 的 `append root=LABEL=rr_X` 标签做**持久**切换。
  同槽幂等拒；rr_factory 目标拒（走 factory 子命令）。只动生效行那一个标签，
  注释与其余字节一字不动；落盘走"读 → 改写 → 同目录临时文件 → rename"。
- **`factory --bootconf P`**：把 `root=LABEL` 切到 `rr_factory` 做**一次性**
  出厂引导。rr_factory 分区本体任何代码路径不得写入（上方只读约束不破）。
  "一次性"的回切职责在 rr_factory 镜像自带的恢复流程（见下方回滚链第 7 条）。
- 退出码：0 成功 / 1 业务拒 / 2 用法错。`--bootconf` 必传，CLI 不缺省碰任何路径。
- 运行槽的权威判定来源是**内核 cmdline**（`/proc/cmdline`），不是
  extlinux.conf 文本——后者只是引导配置，真正生效的是启动时注入的命令行。
  `rr-ota::bootconf::rr3_cmdline_current_slot` 只认 `root=LABEL=rr_` 形态，
  裸设备形态（root=/dev/…）明确不覆盖、报"板上核对项"。

### 回滚链（U-Boot 标准语义；本版本未实装，属板上件）

A/B 切换的断电安全不靠"翻标志不失败"，靠**失败可回**。U-Boot 的标准机制：

| 变量 | 语义 |
|---|---|
| `bootcmd` | 正常启动命令（distro boot 扫 extlinux.conf 属于此链） |
| `bootlimit` | 启动次数上限；`bootcount` 达限后 U-Boot 自动转走 `altbootcmd` |
| `altbootcmd` | 备援启动命令——回滚的落点（切回原槽） |
| `upgrade_available` | 置 1 时 bootcount 才计数；新槽启动成功后清零，回滚链解除 |

标准断电安全序（板上实装时不得违反，与上方"断电安全约束"同源）：

1. 写对侧槽镜像（任意时刻断电，原槽完好）；
2. 校验对侧（hash + manifest 契约）；
3. 翻引导标志——本批已实装的 extlinux 标签改写即此步；
4. `fw_setenv upgrade_available=1 bootcount=0 bootlimit=N altbootcmd=<回原槽>`；
5. 重启进新槽；
6. 启动成功（健康判定点）→ `bootcount=0 upgrade_available=0`，回滚链解除；
7. 启动失败 → bootcount 达 bootlimit → U-Boot 自动走 `altbootcmd` 回原槽；
   恢复出厂场景则由 rr_factory 镜像内恢复流程把标签切回（最近好槽或 rr_a）。

### 板上核对项清单（硬件未采购，以下全部未实测）

1. **boot 分区是 vfat**（genimage.cfg `image boot.vfat`）：vfat 无日志，
   rename 的原子性弱于 unix 文件系统，断电窗口最坏留下双目录项或零目录项。
   临时文件名 `.extlinux.conf.rr3-ota-tmp` 不在 U-Boot distro boot 的固定
   搜索路径上，不会被误读；最坏情形 = extlinux.conf 缺失，U-Boot 落回默认
   bootcmd 或需 maskrom 人工恢复。板上需实测 rename 行为，必要时改
   "写副本 + 验读 + 原位覆盖"序。
2. `rr3_bootconf_write_atomic` 落盘后补 **fsync（文件与目录）**——主机面
   未做，板上加固项。
3. U-Boot defconfig 是否启用 `CONFIG_BOOTCOUNT_LIMIT` 及其计数载体
   （env / RTC / I2C EEPROM）；无载体则 altbootcmd 链不生效，回滚退化为人工。
4. Buildroot 是否打包 `fw_setenv`/`fw_printenv`（`BR2_PACKAGE_UBOOT_TOOLS`），
   `/etc/fw_env.config` 的 env 存储偏移是否命中本板 U-Boot 布局。
5. `/proc/cmdline` 实际形态：确认 root 是 `LABEL=rr_X` 而非裸设备。
6. boot 分区挂载点与读写权限（`/boot` 或独立挂载），以及 rr3-ota-ctl 的
   运行身份与最小权限面。
7. rr_factory 镜像内的恢复流程：进厂环境后由谁把 extlinux 标签切回
   （rr_a 或最近好槽），是否叠加 bootcount 兜底——`rr3-ota-ctl factory`
   只保证"进得去"，"回得来"的职责在 rr_factory 镜像自带逻辑。

## 0.0.5 增补轮六 批N18 件二：ESP32 形态（rtos 档）升级语义定案

范围＝微型形态（ESP32，`tools/esp32-periph-base`，4MB flash）；上文 A/B 槽布局是
RK3566/树莓派 host 档的，本段是 rtos 档自己的明文结论（[全案 §17 批N5](../process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md)
「升级与回滚」行判据：表内"不能"一格必须带触发条件；"不可回滚"也是一句合格答案，
但必须写进表）。真值单点维持：`rr-ota` 常量与 `cargo test -p rr-ota` 只管 host 档
A/B 槽；本段事实为 2026-10-23 侦查实测，标签体系与 ESP32 侧零关系。

### 分区事实（手读 partition-table.bin，magic 0xAA50 逐项）

`nvs@0x9000/0x6000`（批N14/15 凭据源）＋ `phy_init@0xf000/0x1000` ＋
`factory@0x10000/0x100000`——**factory 单槽、无 ota_0/ota_1/otadata**。这是下方
两条语义裁定的物理前提。

如实登记（侥幸面）：debug 构建 app image 实测 1,130,224 B，**超 factory 槽上限
1,048,576 B 约 8.2%**——`espflash flash` 烧录不校验槽界，溢出区（0x110000 起）
无任何分区登记（分区表只到 factory 末尾）、bootloader 加载按 image 段表不按槽界，
故真板可跑（批N9 件三烧录实录在案）。release 构建 app image 实测 444,576 B
（2026-10-23 批N18 件三补测——件二时点未实测如实注记后同日补上），槽内 42.4%
不溢出：侥幸面仅 debug 构建所有，**产线交付一律走 `--release`**。

### 升级语义＝重烧即升级

本形态的升级路径＝`tools/deploy/flash-esp32.sh`（构建＋descriptor 三查＋烧录，
批N9 件二落）或其镜像装配件 `tools/deploy/image-esp32.sh`（批N18 件一：
save-image --merge 产整片 merged 镜像，布局三点 magic＋整片 4MB 自校验）重烧完整
固件。**NVS 凭据保留**：`espflash flash` 只写涉及分区，`nvs@0x9000/0x6000` 不在
擦除域——升级后免重配网（`cred_store = "nvs:rr3"`，批N14/15 读链消费）；仅
`erase-flash` 全片擦除才清凭据（产线首烧语义）。

### 真增量 OTA 不做（触发条件在案）

`ota_0/ota_1` 双槽＋`otadata`＋esp_ota 下载链**不做**，三理由：

1. 无双槽分区表：factory 单槽（上方手读事实），启用双槽须重裁 4MB flash 预算
   （当前 debug app image 已超 1MB 槽界，双槽各 ~640KB 更装不下——须先瘦固件）；
2. 无下载载体：形态无文件系统（NVS 除外）、配置腿 HTTP 无 TLS（批N13 裁定在案）、
   云分发面未立项；
3. 无接口语义：`rr3.ctl.ota` 恒拒面维持（解除需 M6 评审，全案 §17 判据原文）。

**触发条件**＝形态裁定启用双槽分区表（ota_0/ota_1＋otadata，契约级评审）＋云分发
面立项＋固件体积预算重裁。触发前，[可达终点表](../process/dev/0.0.5增补轮六——批N5首件——按形态分档可达终点表.md)
ESP32"升级"格记「能做到（重烧档）」。

### 回滚语义＝不可回滚（无自动回滚链）——终态裁定

[全案 §17](../process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md)
立法原文："无 FS 无 A/B 时'不可回滚'也是一句合格答案，但必须写进表。"本形态
终态裁定即此（2026-10-23 批N18 件三）：

- **factory 单槽**（上方手读事实，无 ota_0/ota_1/otadata）＋**bootloader rollback
  未启用**（生成 sdkconfig 实测 `# CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE is not
  set`）⇒ 自动回滚链为零：不存在"启动失败自动回原槽"的任何机制；
- **"回得来"的两半边如实登记**（与"自动回滚"是两种能力，不混称）：①重烧重试
  路径——USB 在手即 `flash-esp32.sh` 再烧（产线人工兜底）；②NVS 凭据保留——
  重烧不擦 NVS，配网成果不丢（上方升级语义同款事实）；
- **触发条件**＝形态启用双槽分区表（ota_0/ota_1＋otadata）＋
  `CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE`（U-Boot bootcount/altbootcmd 同语义的
  IDF 侧对应物）——分区表与 bootloader 配置变更走契约级评审。触发前
  [可达终点表](../process/dev/0.0.5增补轮六——批N5首件——按形态分档可达终点表.md)
  ESP32"回滚"格记「不能（终态裁定）」。

