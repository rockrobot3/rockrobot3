# Rockrobot3 · 洛克三号机器人

走向具身，洛克三号是一个桌面陪伴机器人：能听、会说、可交互。

## 参与

项目处于早期阶段，架构（设计）见 [docs/architecture/architecture.md](docs/architecture/architecture.md)，
"代码今天到哪一步"见 [docs/status.md](docs/status.md)

欢迎讨论与共建

## 文档索引

| 文档 | 内容 |
|---|---|
| [docs/README.md](docs/README.md) | 文档库地图：仓内每份 README 各在何时该读，以及"我要做 X"该看哪篇 |
| [docs/architecture/architecture.md](docs/architecture/architecture.md) | 架构（设计）：分层、模块模型、HAL、IPC、数据流、风险登记 |
| [docs/status.md](docs/status.md) | 唯一实况快照：HEAD 上有什么、里程碑进度、本机无法收口的项 |
| [docs/reference/](docs/reference/) | 运行期接口的可照抄事实：rrd 启动参数 / 上行 RPC / 在册表规则 |
| [docs/process/dev/0.0.5版本洛克三号对齐与裁定方案.md](docs/process/dev/0.0.5版本洛克三号对齐与裁定方案.md) | **核心版本 = 软件开发收口版**：四轮并档（第 I 轮裁定存档 / 第 II 轮原子重铺已收官 / 第 III 轮跨 OS 三处窄缝已收口 / **第 IV 轮"软件开发收全"批M 已立项未开工**——功能验收清单 F-1…F-8 与站替件台账是三版共用的唯一真值） |
| [docs/process/dev/0.0.6版本洛克三号树莓派验证方案.md](docs/process/dev/0.0.6版本洛克三号树莓派验证方案.md) | **纯验证版（全承载档 host）· 执行中**：只出证据、本版不新增功能码（修订版 13）；树莓派两阶段 + 真件矩阵八行 + BOM 定案 + 板上工程执行手册 §5.5 |
| [docs/process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md](docs/process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md) | **纯验证版（轻承载档 rtos）· 执行中**：四层验证梯子已收官（修订版 15 把"收官"范围订正为通路四层，不含功能可达）+ 新开**批H 功能验收**（前置 = 0.0.5 MC-7） |
| [docs/process/dev/0.0.8版本洛克三号模块内部分层实施方案.md](docs/process/dev/0.0.8版本洛克三号模块内部分层实施方案.md) | **验证后整理过渡版（2026-10-12 恢复独立成批）**：rr3mod 骨架 + 形状清理 + brain Rust 化 + 验证返工登记位；开工前置 = 0.0.6/0.0.7 出 F 组实录且 0.0.5 批M 收口 |
| [docs/process/conventions.md](docs/process/conventions.md) | 开发规约（分支/提交/CI 门禁十步/DoD/**版本按交付物类型分窗**） |
| [docs/process/naming.md](docs/process/naming.md) | 命名唯一权威 |
| [contracts/README.md](contracts/README.md) | 字节级契约与歧义裁定 |
| [docs/process/adr/](docs/process/adr/) | 决策记录 ADR-0001 ~ ADR-0020 |
| [docs/process/legacy-digest.md](docs/process/legacy-digest.md) | legacy（ESP32 冻结区）消化台账（已归档：整树随 0.0.4 退役批删除） |
| [README.md](README.md) | 本门面的英文版 |

## 许可

许可协议筹备中，正式发布前请勿直接复用代码。
