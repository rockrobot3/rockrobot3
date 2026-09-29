# docs —— 文档索引

三份权威文档 + 决策记录 + 版本方案。**每类内容只有一个权威落点**，其余文档引用它、
不复述它——复述是漂移的源头（架构文档 §5 曾因把设计期草图当既成事实书写而整段重写）。

| 路径 | 内容 | 谁在它上面兜底 |
|---|---|---|
| [`architecture/architecture.md`](architecture/architecture.md) | 唯一架构图：定位、分层、目录树、模块模型、HAL、IPC、数据流、迁移映射、里程碑 M0–M6、风险登记 | 目录树变更必须同步其 §3（`conventions.md` §5 文档纪律） |
| [`architecture/conventions.md`](architecture/conventions.md) | 开发规约：分支/提交/CI 五步/DoD | 门禁唯一入口 `bash tools/ci/check.sh` |
| [`architecture/naming.md`](architecture/naming.md) | 命名唯一权威（`rr3-` 前缀矩阵、模块目录四件套的四类结构角色） | `rr3-mkmod` 生成自检 + `rr-registry` 契约测试强制，不靠人记 |
| [`adr/`](adr/) | 决策记录 ADR-0001 ~ ADR-0007（平台选型 / 模块模型 / IPC 选型 / Airymax 接入 / 命名规约 / AiryIPC 线格式 / 控制面分层） | 架构文档每节顶部回链对应 ADR |
| [`dev/0.0.1版本洛克三号开发方案.md`](dev/0.0.1版本洛克三号开发方案.md) | 当前版本改造方案与 Definition of Done | 收口评审按此逐条对账 |

## 阅读顺序

新人：`architecture/architecture.md` §1–§3 → `architecture/conventions.md`（提交前必读）→
`adr/ADR-0002` 与 `ADR-0006`（模块模型与线格式两根支柱）。

改契约/字节格式：先读 [`../contracts/README.md`](../contracts/README.md)（歧义裁定与演进规则），
再动 `contracts/` 本体。

## 实况与规划的边界

架构文档同时承载**目标架构**与**当前实况**：v0.0.1 阶段 `modules/` 尚无模块实现、
`base/runtime/` 六 crate 多为骨架，标注"规划（M2+…）"的内容以 §10 里程碑表为准。
写文档时不要把设计期草图当既成事实——这是本项目已发生过的最大一次订正。
