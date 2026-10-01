# ADR-0001：平台选型 —— Linux 6.6 mainline + Buildroot + RK3566

- **状态**：已接受；**构建系统主路线已被 [ADR-0013](ADR-0013-OS双轨-发行版快路与Buildroot保底.md) 修订**（Buildroot 主路线 → 降为保底轨，发行版快路 + 同一 payload 双轨；SoC、内核 6.6 mainline 与 maskrom 自烧录硬性目标不变）
- **日期**：2026-09-28
- **关联**：[architecture.md](../../architecture/architecture.md)

## 背景

项目由 ESP32 固件（legacy/）演进为 Linux 平台桌面机器人（体积约束约 0.5L），需选定 SoC、内核与构建系统路线，且必须支持不拆机自烧录 OS。

## 决策

| 项 | 选择 | 说明 |
|---|---|---|
| SoC | Rockchip RK3566 | 先用 Radxa ZERO 3 成品板验证，自绘板后置 |
| 内核 | Linux 6.6 mainline | 不使用瑞芯微 vendor SDK（陈旧、mainline 化滞后） |
| 构建系统 | Buildroot 主路线 | 前期硬件验证可临时用 Armbian 垫层 |
| 烧录 | maskrom 自烧录 | 任意板不拆机烧录，属硬性目标 |
| 语言 | 基座与模块 Rust，brain Python | 具体边界见 ADR-0002/ADR-0005 |

## 备选方案否决理由

- **Yocto**：能力更强但构建体系复杂度过高，后期再评估
- **vendor SDK**：内核版本老、补丁难维护，与 mainline 目标冲突
- **Armbian 主路线**：适合验证期，但镜像定制深度不及 Buildroot，且不利于自烧录管控

## 后果

- 正面：构建链完全自持、镜像可控、A/B OTA 与 factory recovery 易实现
- 负面：RK3566 的 DTS/音频/视频补丁工作量集中在 M1（风险 R1）
- 每半年复查一次 Airymax distro 仓的 aarch64/headless 产物，若成熟可评估 OS 层切换（见 ADR-0004）
