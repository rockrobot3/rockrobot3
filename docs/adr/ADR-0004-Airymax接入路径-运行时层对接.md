# ADR-0004：Airymax 接入路径 —— 运行时层对接，OS 层观察

- **状态**：已接受
- **日期**：2026-09-28
- **关联**：[architecture.md §9](../architecture.md) · ADR-0003

## 背景

用户决策：**先独立开发，保留接入 Airymax 的目标**。调研结论（代码级盘点，2026-09）：

| Airymax 仓库 | 现状 | 结论 |
|---|---|---|
| agentrt | 真实可用：约 35 万行 C，v0.1.18，含 gateway/protocols/daemons | 可对接其规约 |
| agent-linux | 骨架：内核自研仅 2600 行，distro 仅 x86_64+UKUI，无 aarch64 | 暂不可用，观察 |
| sdk-rust | Apache-2.0，reqwest+tokio | 可作 rr3-connect 试点 |
| memoryrovol | 商业 EULA | 不引入 |

许可证为 AGPL OR Apache-2.0 双许可，闭源走 Apache 路径。

## 决策

1. **接入点定在运行时层**：rock3 模块经 JSON-RPC 对接 AirymaxRT gateway；消息头格式已在 ADR-0003 对齐（定长 32B 头），字节级兼容预留
2. **OS 层不依赖**：Buildroot 主路线不变；每半年复查 agent-linux distro 仓是否出现 aarch64/headless 产物，成熟再评估 OS 层切换
3. **试点依赖**：sdk-rust 作为 rr3-connect 的候选依赖试用（Apache-2.0 无传染风险）
4. **规避项**：不引入任何 AGPL 路径代码（保持 Apache 路径），不引入 memoryrovol

## 后果

- 正面：独立开发不受制于外部仓库成熟度；运行时对接是低耦合点，切换成本低
- 负面：消息头兼容需在 contracts/envelope.schema.json 变更时人工核对 AirymaxRT 规约
- 复查节奏记入项目例行事项（每半年，下次 2027-03）
