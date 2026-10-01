# maskrom 烧录操作序（Radxa ZERO 3 / RK3566）

> **档位**：文档本机收口；**每一步都未经实板执行**（硬件未采购，见
> `docs/status.md`）。命令与偏移按 RK3566 通用流程书写，实板首次烧录时
> 逐条回填核对结果。分区标签的唯一事实源：
> `base/runtime/rr-ota/src/lib.rs`（`rr_a` / `rr_b` / `rr_factory` / `rr_staging`）。

## 1. 前置物

| 物件 | 说明 |
|---|---|
| `rock3-zero3.img` | Buildroot 全量构建产物（含 GPT 分区表，见 `genimage.cfg`）；生成入口 `post-image.sh` |
| `rk3566_loader.bin` | Rockchip maskrom 加载器（DDR init + USB 下载）；来源用 Rockchip 官方 rkbin 或 Radxa 发布页，**文件版本记入本页** |
| rkdeveloptool | 宿主侧工具（macOS/Linux 均可编译；Windows 另用 RkDevTool） |
| USB-A 转 C 线 | 宿主机 ↔ 板子 OTG 口 |

## 2. 进入 maskrom 模式

板上必须处于 maskrom（裸 ROM USB 等待）状态，三种进入方式：

1. **全新板 / 空存储**：上电即自动停在 maskrom；
2. **可启动系统的板**：按住板上的 MASKROM/BOOT 按键再上电复位；
3. **不拆机自烧录（软件方式）**：系统已在跑的前提下，宿主机执行
   `rkdeveloptool rd`（reboot to maskrom/rockusb）——板子经 OTG 复位进
   maskrom，全程不动外壳。此路依赖当前系统仍可执行软复位指令，
   **用于刷坏 rootfs 后自救时不成立**（系统起不来只能走方式 1/2）。

验证：宿主机 `rkdeveloptool ld` 列出 `MASKROM` 设备。

## 3. 烧录序列（宿主机）

```
rkdeveloptool ld                          # 确认设备在位
rkdeveloptool db rk3566_loader.bin        # 下载加载器进 RAM（不落盘）
rkdeveloptool ef                          # （可选，慎用）擦除整片存储
rkdeveloptool gpt rock3-zero3.img         # 写入 GPT 分区表
rkdeveloptool wl 0 rock3-zero3.img        # 从 LBA0 写入整盘镜像
rkdeveloptool rd                          # 复位，从新镜像引导
```

要点：

- `db` 之后设备进入"可写"状态；`wl 0` 从 0 扇区整写，`uboot-idbloader`（32KiB）
  与 `uboot-itb`（6MiB）两个裸区随镜像一起落位，无需单独 dd；
- 首次烧录**不要** `ef` 整擦（出厂预烧的 idbloader 兜底被擦掉后，只能
  依赖按键进 maskrom）；`ef` 留给需要彻底清盘的产线场景。

## 4. 不拆机自烧录（运行时自救）

目标：系统在跑、rootfs 已坏或需换代时，不动外壳完成重刷。

1. 板上系统经 OTG 口连宿主机（`g_ether`/`g_serial` 或 rockusb 内核驱动可见）；
2. 宿主机 `rkdeveloptool rd` 触发软复位进 maskrom；
3. 走第 3 节序列。

约束：板端需支持软复位进 rockusb（内核 config / udev 规则属板上收口项）。
若 rootfs 已坏到无法执行软复位，本路不成立——这正是 `rr_factory` 只读槽
存在的意义：从 factory 引导后再走本节。

## 5. 与 A/B 回滚链的关系

maskrom 是"任何软件手段都救不回来"之后的最后一道兜底，与 U-Boot 级
bootcount/altbootcmd 回滚（M6 写侧，见 `extlinux.conf` 尾注）互补：

```
正常启动（extlinux → rr_a）
  └─ 启动失败 → bootcount 超限 → altbootcmd → 对侧槽 / rr_factory   （M6）
       └─ factory 也起不来 → 软复位进 maskrom 重刷                 （本页）
            └─ 无 OTG/按键不可及 → 送修（拆机级，不在任何自动化路径内）
```
