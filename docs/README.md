# docs —— 文档索引（四档）

docs/ 按**变更频率与读者动作**分四档：`architecture/` 半年一变，读者要的是"为什么这样切"；
`reference/` 随代码同批变，读者要的是"照着写、照着调"；`process/` 是协作契约，读者要的是
"提交前对什么"；`status.md` 是快照，每次里程碑收口整篇重写。

**每类内容只有一个权威落点**，其余文档引用它、不复述它——复述是漂移的源头（架构文档 §5
曾因把设计期草图当既成事实书写而整段重写）。

## 四档落点

| 档 / 路径 | 内容 | 谁在它上面兜底 |
|---|---|---|
| [`architecture/architecture.md`](architecture/architecture.md) | 唯一架构图：定位、分层、目录树、原子功能模块模型（格/件/组合件/面）、HAL、IPC、数据流、迁移映射、风险登记、跨操作系统移植与部署（§12，含 v0.8 三档承载、v0.11 七位供给、v0.12 第四条增长律（机制核零部署缺省）与 §12.8 **四处窄缝**）、门禁与验收（§13）、速查（§14）（§10 只留一行指向 `status.md`，本档不写进度） | 目录树变更必须同步其 §3（`process/conventions.md` §5 文档纪律） |
| [`design/`](design/模块设计索引.md) | **模块级细化设计集**（architecture 的下一层）：[框架 v1.3](design/模块设计框架.md)（十五节固定模板：身份证 / 件契约表 / 状态机 / 注入面 / 时序预算 / 主题契约 / 降级矩阵 / 安全核接入 / 两腿对拍用例设计 / 不变量断言 / 改造工单 / 敞口 / **跨档动作** / **策略字面量清单** / **基线对表**）+ 索引（系统切分 / 实然契约图 / **四处窄缝在各单元的落点 §3** / 对话实然序 / 待裁/已裁汇总 D1–D15（**2026-10-14 除 D9 外全部落定，细则唯一落点在增补轮六全案 §12**） / **本集裁定 R-1…R-6** / **形态裁剪闭环 F-A·F-B·F-C（v0.11 改两轴读法）** / **逻辑闭环审计 L-1…L-21** / **可开发性审查 §12（五件无主项指派、四处口径互斥消解、五条可证伪命题、逐批开工判定）** / **§13 基线对表（代码落库后的作废清单与自决 R-7…R-13）** / **§14 版本边界与余数归属总表（一个余数一个落点，R-14…R-16；14.6 = 增补轮六批N1–N5）**）+ 六单元各一册（职责 · 事实面 · 件级闭包 · 四面 · 时序 · 降级 · 红线 · 敞口 · §12 跨档 · §13 字面量 · §14 基线对表）。规矩：现状断言一律以符号名回指文件、用例存在性点着用例如名；**推测值只准待在 §11**，§4/§6 的"未实测/后端未实装"是缺口标记且须指回 §11；设计与事实分节不混排 | 设计评审；与 architecture / ADR 冲突时以上者为准并回改本集 |
| [`reference/rrd-cli.md`](reference/rrd-cli.md) · [`reference/rrd-rpc.md`](reference/rrd-rpc.md) · [`reference/rrd-registry.md`](reference/rrd-registry.md) | 运行期接口的可照抄事实：启动参数与默认值、RPC 方法与错误映射、在册表阶段与退避上限 | 与代码同批变更；引用须与代码逐字一致；代码定位一律用**符号名锚定**（如 `registry.rs 的 rr3_reg_beat`），不写行号（行号随代码漂移） |
| [`process/conventions.md`](process/conventions.md) | 开发规约：分支 / 提交 / CI 门禁 / DoD | 门禁唯一入口 `bash tools/ci/check.sh`（其 ⑤ 文档门对仓内每个已跟踪 md 兜底：不得为空、相对链接不得断；仓内每份 README 还必须出现在下文库地图里，漏链即红。本索引自身也在覆盖内）；**步序与条数只登记在其 §2**，本索引不抄数字 |
| [`process/naming.md`](process/naming.md) | 命名唯一权威（`rr3-` 前缀矩阵、模块目录四件套的四类结构角色） | `rr3-mkmod` 生成自检 + `rr-registry` 契约测试强制，不靠人记 |
| [`process/adr/`](process/adr/) | 决策记录 ADR-0001 ~ ADR-0022（ADR-0018/0019 各含修订 1–4，2026-10-09；ADR-0020 版本边界三窗分离，2026-10-12；ADR-0021 三独立形态与资源供给面——能力原子化补课，2026-10-14，含修订 2（2026-10-15 证伪审查轮的六处数据结构改判）；ADR-0022 代码事实复盘——机制核纯净度补漏·申报面逃逸封死·安全语义须有身体，2026-10-15；平台选型 / 模块模型 / IPC 选型 / Airymax 接入 / 命名规约 / AiryIPC 线格式 / 控制面分层 / 传输层 trait / 依赖仲裁 / 控制通道与查询面 / shm 传输 / 能力格坐标系与装配单 / OS 双轨 / Python 模块接入与 M4 对话闭环 / 板卡策略 / brain 迁移路线 / 模块内部分层 / 跨操作系统移植——机制核与主承载层分离 / 原子功能模块模型——格件面与组合代数 / 版本边界——开发收口与实机验证与验证后整理 / 三独立形态与资源供给面——能源域·角色绑定·执行件档案·面板覆盖率） | 架构文档每节顶部回链对应 ADR |
| [`process/dev/0.0.2版本洛克三号开发方案.md`](process/dev/0.0.2版本洛克三号开发方案.md) | 历史存档（M5 主战场，已收口；同目录 v0.0.1 是 M0 骨架存档） | 收口对账已由 status.md 锚点承接 |
| [`process/dev/0.0.3版本洛克三号收口方案.md`](process/dev/0.0.3版本洛克三号收口方案.md) | 历史存档（M5 最后一跳 + M6 OTA 写侧，已收口） | 收口对账已由 status.md 锚点承接 |
| [`process/dev/0.0.4版本洛克三号消化方案.md`](process/dev/0.0.4版本洛克三号消化方案.md) | 历史存档（legacy 待融合三项 + 退役批，已收口） | 收口对账已由 status.md 锚点承接 |
| [`process/dev/0.0.5版本洛克三号对齐与裁定方案.md`](process/dev/0.0.5版本洛克三号对齐与裁定方案.md) | **核心版本 = 软件开发收口版**（四轮并档：§1–§6 第 I 轮"对齐与裁定"**存档不改写**，R1–R29 已裁定；**第 II 轮 §7–§12 = 原子功能模块重铺施工**（已收官存档，承 [ADR-0019](process/adr/ADR-0019-原子功能模块模型-格件面与组合代数.md)）；**第 III 轮 §13–§17 = 跨 OS 三处窄缝与极致分离落地**（批I–K，**已收口**，余数另立批L）；**第 IV 轮 §18–§22 = 软件开发收全**（批M，MC-0…MC-11，**已立项未开工**，承 [ADR-0020](process/adr/ADR-0020-版本边界-开发收口与实机验证与验证后整理.md)；§2.7 = R32–R37 裁定记录；§18.1 功能验收清单 F-1…F-8 是三版共用的唯一真值） | 收口对账由 [status.md](status.md) 锚点承接；各轮 DoD 见 §6 / §11 / §17 / §22 |
| [`process/dev/0.0.5增补轮四——板载开发前技术落地全案.md`](process/dev/0.0.5增补轮四——板载开发前技术落地全案.md) | 增补轮四全案（**已裁定执行收官**，批W0–W7：CI 单真源 / counters 节流 / 真件手册 / espidf std 冒烟 / 桥设计 / e2e 判据 / S3 预案 / 收口对账） | 执行对账落 [0.0.5 方案 §2.5](process/dev/0.0.5版本洛克三号对齐与裁定方案.md)；敞口余量见全案 §7 |
| [`process/dev/0.0.5增补轮五——交互功能落地全案.md`](process/dev/0.0.5增补轮五——交互功能落地全案.md) | **当前执行轮**（R31 已裁执行中：OLED 显示·舵机转向·语音对话三功能——R31 版本制"功能归 0.0.5 轮次制，0.0.6/0.0.7 为验证版"；Q1=(a) 921600 双通道过桥 / Q2=四件全挂 ESP32 / Q3=vosk+Piper / Q4=0.0.6 BOM 第 10 行；批A–批F 依序，批E gated） | 裁定记录唯一落点 [0.0.5 方案 §2.6](process/dev/0.0.5版本洛克三号对齐与裁定方案.md)；每批四件套见全案 §4；**与第 IV 轮批M 的分账见 [0.0.5 §18.4](process/dev/0.0.5版本洛克三号对齐与裁定方案.md)**（施工归增补轮五、判据与站替清零归批M） |
| [`process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md`](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) | **已立项，未开工**（2026-10-14，开发窗）：承 [ADR-0021](process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md)——把增补轮五在 ESP32 实机踩到的四类抽象失败（引脚冲突烧舵机 / 轨压无供给位 / 关节只抽象成"角度" / 面板与配置面无对应关系）**逐部件**升格为契约数据结构、能力面与门禁。含 §1 十六条坑→五处落点对表、§3 十一问模板的逐部件盘点（能源/执行机械/声/显示/感知/链路/承载/运维认知）、§4 字段级数据结构（`roles.toml`/**`devices.toml`**/`[pins]`/`wiring.toml`/执行件档案）、§5 装载期**十条校验 / 24 个判定位**（每条带**缺失语义**与**后果三态**）、§6 标定四路线、§8 **门 a–o**（conventions 口径 **十七把待建**）、§9 批N1–N5、**§12 裁定记录（D10–D15 全裁 + 顺带结 D3/D5–D8）**、**§15 主线一致性二轮审查（十条无主项逐条指派 + 门禁自身的三处自指失败 L-14/L-15/L-16）**、**§16 全代码复盘基线与自核记录（L-17…L-21 的证据台账 + 我撤回或收窄的转述项）+ §16.4 第三轮计数口径**、**§17 批N5 生产与运维闭环（可达终点表 / 烧录 / 自检逐腿 / 回滚）**、**§18 逐项证伪审查（32 条命中：每条设计配一条"能推翻它的观测"；三类新失败 L-22/L-23/L-24；§18.7 审过成立十条、§18.8 撤回七处）**、**§19 跨文档一致性 + 运行期语义审查（X 类口径传播 5 条：撞名的"十项"、两个待建门分母、architecture 正文留着旧分母 9 处、"七位供给"今天只有 2 位可机判；Y 类掉电/重启/并发 8 条：面板写阈值无通路、面板落盘非原子、闸门复位即开闸、判定用读数无龄期、经桥两侧三种做法、rtos 档档案无处可落；纪律 2 条）** | 结论行见 [模块设计索引 §8](design/模块设计索引.md)；施工判据落本全案 §10 DoD（**本全案不产任何平台级点验结论**） |
| [`process/dev/0.0.6版本洛克三号树莓派验证方案.md`](process/dev/0.0.6版本洛克三号树莓派验证方案.md) | **纯验证版（全承载档 host）**——2026-10-12 修订版 13 按 [ADR-0020](process/adr/ADR-0020-版本边界-开发收口与实机验证与验证后整理.md) 定为"只出**平台级点验**、本版不新增功能代码"（开发项按 §18.4 分账：显示/外设归增补轮五批B/批C，通路/禁动/凭据/探测/参数归 0.0.5 批M），§6 DoD 扩第 11 条 = F-1…F-7 板上实录（引 0.0.5 §18.1，不自造判据）+ "缺码"与"无载体"两种败法分开登记 + BOM 第 2/4 行"由 ESP32 协处理器承载替代"注记（其下：修订版 12 出包 target 改由 **E1 标准环境**容器承担、修订版 11 矩阵按 architecture v0.9 的**七列**登记、修订版 10 执行前置三件、修订版 9 定位重述 + 承接点验矩阵全承载档首行；两阶段 + 批F5a/F5b 重排 + 真件矩阵八行 + §5.4 BOM 定案 + §5.5 板上工程执行手册） | 收口评审按此逐条对账 |
| [`process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md`](process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md) | **纯验证版（轻承载档 rtos）· 执行中**——2026-10-12 修订版 15 把批F 那次"版本收口"的**范围订正**为"跨 OS 通路四层梯子 + 矩阵 esp32-mini 行（证据对象 = 诊断哨兵）"，**不含功能可达**；新开 **批H 平台级点验**（功能验收施工归增补轮五批B–批E）（其下：修订版 11 编译面改由 **E2/E3 容器**承担 + §9 runbook 加环境指纹；修订版 10 轻承载档行随 architecture v0.9 扩为**七格**；修订版 9 层③ 实现口径改判、修订版 8 执行宿主与 runbook 订正、修订版 7 定位订正为轻承载档 rtos + **新增批G 点验矩阵轻承载档行收口**；Q1–Q6 已随 R20 裁定回填；层③④升必达（R19），§9 四层真板执行手册 + §5.1 桥形状钉死 + S3 预案 + e2e 双档判据；批A/A'/B/C/D1/E/G/F 已绿并收官落档） | 四层梯子判据不动；批H 按 §4 新行执行，细则随 §9 手册 |
| [`process/dev/0.0.8版本洛克三号模块内部分层实施方案.md`](process/dev/0.0.8版本洛克三号模块内部分层实施方案.md) | **验证后整理过渡版（2026-10-12 恢复独立成批）**——[ADR-0020](process/adr/ADR-0020-版本边界-开发收口与实机验证与验证后整理.md) 裁定 2 作废"并入 0.0.5 第 II 轮"旧裁定（物证：`base/runtime/` 下至今无 `rr3mod` crate）；与 R31"0.0.8 不设功能版"同向。范围四组：W-A `rr3mod` 骨架 + 五模块 boot/rpc/dispatch 收敛、W-B 形状清理（批L L-3/L-9/L-10）、W-C brain Python→Rust（ADR-0016 同窗）、W-D 验证返工登记位（**此刻为空是设计不是遗漏**）。原修订版说明保留：四面是视图不拆四份文件、§2 签名定性为 **std 驱动器形状**而非跨 OS 接口 | 开工前置 = 0.0.6 或 0.0.7 至少一档完成平台级点验 **且** 0.0.5 批M 收口；开工后按 §5 依序执行、按 §6 四件套留痕 |
| [`process/dev/真件装配与标定手册.md`](process/dev/真件装配与标定手册.md) | 真件到货桌面级验收、装配与标定 runbook（0.0.5 增补轮四 批W5：USB 麦 UAC / SG90+UBEC / MAX7219 / TP4056+保护板 / INA219 两点标定 / ESP32 UART2，每件四件套；型号权威=0.0.6 §5.4 BOM 定案，接线权威=§5.5.2） | 器件到货即照单执行；型号差异先改手册与 BOM 表再装配 |
| [`process/dev/方案A——整板电压监测与分轴限幅立项评审.md`](process/dev/方案A——整板电压监测与分轴限幅立项评审.md) | **已裁定（Q1–Q6 采纳建议，施工未开工）**：整板/5V 轨电压观测 + 欠压分级降级（并行→先左右后上下串行）+ 分轴限幅下沉 config 数据面；§8 裁定记录 + §9 真板验收实测（2026-10-12，方案C 并行件） | Q4 形状变更须先过契约级评审；施工面见本件 §4、顺序见 §8 |
| [`process/legacy-digest.md`](process/legacy-digest.md) | legacy/ 逐组件消化盘点（已归档）：三分类台账、证据路径 | 查"某 legacy 语义在新架构哪"（`legacy/` 整树已随 0.0.4 退役批删除） |
| [`status.md`](status.md) | 唯一实况快照：能力 × 状态 × 证据路径、里程碑进度（M0–M6 当前实况）、本机无法收口的项、在制品 | 收口时整篇重写，不做增量修补；**测试条数一律不抄进来**（见其开篇规矩） |

## 库地图（按目录）

**本索引是文档的入口，不是目录树的事实源**：文件是否存在以
[`architecture/architecture.md`](architecture/architecture.md) §3 为准，模块名册以
[`../modules/README.md`](../modules/README.md) 为准，crate 职责以
[`../base/runtime/README.md`](../base/runtime/README.md) 为准。下表只回答"这个目录下有哪份
README、什么时候该点开它"；小节名以文档自身为准，本表只作导航。`legacy/` 已退役
（0.0.5 §2.4 R21——git 历史 + tag `v0-pre-legacy` 可回溯），不进本表，⑤ 门同样
不要求它出现在导航里（下文动作索引留了一条对照入口，那是有意的）。新增 README 不写进
本表即判红。

### 仓根 · docs/

| 路径 | 什么时候读它 |
|---|---|
| [`../README.md`](../README.md) · [`../README_zh.md`](../README_zh.md) | 双语门面，各自外挂一份文档索引表 |
| 本文件 | 找文档从这里开始；四档落点、事实归属、动作索引都在这里 |
| [`architecture/architecture.md`](architecture/architecture.md) | §1 定位与目标（1.0 总纲：机制与策略极致分离 / 立格→造件→成面 / 跨 OS 快部署最高约束；1.1 分层与部署三步）· §2 总体分层（原则 5 = 跨 OS）· **§3 仓库目录树**（文件级，全仓唯一目录树事实源，含规划落点注记）· §4 原子功能模块模型（格/件/组合件/面 + ⊗/⊙ 代数 + 职责四面 + 五件套；§4.3 能力格坐标系与装配单）· §5 HAL 分层 · §6 IPC 与事件总线 · §7 关键数据流 · §8 legacy 迁移映射 · §9 Airymax 接入路径 · §11 风险登记（v0.11 起新行用 `RK-` 前缀，与本文件 §12 批次裁定 `R#`、索引 `R-#` 三套编号消歧）· §12 跨操作系统移植与部署（ADR-0018 + ADR-0021：三档承载与**七位供给** §12.1/§12.2 · **描述符八段** §12.3 · 点验矩阵七列 §12.6 · 三独立形态两轴 §12.7 · **四处窄缝 §12.8**）· §13 门禁与验收（含六把 v0.11 拟增门，全标待建）· §14 速查（ADR-0019 原子模型） |
| [`architecture/板卡设计规格书.md`](architecture/板卡设计规格书.md) | ADR-0015 过桥阶段实施档：核心板选型核对单 · 电源树与工作待机 <1W 预算 · 引脚/格位预算 · 连接器与设计规则 · 首版验收计划 · 开放项表 |
| [`status.md`](status.md) | 能力实况 · 里程碑进度 · 本机无法收口的项 · 在制品 |
| [`process/conventions.md`](process/conventions.md) | §1 分支与提交 · **§2 机器可检查项**（门禁步序与条数的唯一落点）· §3 结构纪律 · §4 DoD · §5 文档纪律 |
| [`process/naming.md`](process/naming.md) | 命名矩阵与模块目录四件套的结构角色——对不上名字时 |
| [`reference/rrd-cli.md`](reference/rrd-cli.md) | §1 用法串 · §2 参数表 · §3 fail-closed 门禁文案 · §4 两条路径绝对化的分工 · §5 启动序列、输出流与退出码 |
| [`reference/rrd-rpc.md`](reference/rrd-rpc.md) | §1 帧层前提 · §2 JSON-RPC 信封约束 · §3 方法白名单 · §4 握手额外门禁 · **§5 错误码映射表** · §6 心跳方向 · §7 发布面 |
| [`reference/rrd-registry.md`](reference/rrd-registry.md) | §1 类型与共享方式 · §2 常量表 · §3 阶段与活边 · §4 时钟约定 · **§5 `rr3_reg_add` 门序** · §6 心跳与到期 · §7 判死与重启计划 · §8 发布归属四道门 · §9 fanout · §10 其余入口 |
| [`process/adr/`](process/adr/) | 决策原文 ADR-0001 ~ ADR-0022（0018/0019 含修订 1–4，0020 版本边界三窗，0021 三独立形态与资源供给面，0022 代码事实复盘与机制核纯净度补漏）；动架构或线格式前先读对应那篇 |
| [`process/dev/0.0.2版本洛克三号开发方案.md`](process/dev/0.0.2版本洛克三号开发方案.md) | 历史存档：上一版本（M5 主战场）交付了什么 |
| [`process/dev/0.0.3版本洛克三号收口方案.md`](process/dev/0.0.3版本洛克三号收口方案.md) | 历史存档：上一版本（M5 最后一跳 + M6 OTA 写侧）交付了什么 |
| [`process/dev/0.0.4版本洛克三号消化方案.md`](process/dev/0.0.4版本洛克三号消化方案.md) | 历史存档：上一版本（legacy 消化 + 退役批）交付了什么 |
| [`process/dev/0.0.5版本洛克三号对齐与裁定方案.md`](process/dev/0.0.5版本洛克三号对齐与裁定方案.md) | **核心版本 = 软件开发收口版**：第 I 轮（rrctl 对齐 + R1–R29 裁定，存档不改）；**第 II 轮 §7–§12 原子功能模块重铺施工**（批 A–E，承 ADR-0018/0019，已收官存档）；**第 III 轮 §13–§17 跨 OS 三处窄缝与极致分离落地**（批I–K，承两份 ADR 修订 1–4，**已收口**；§13.1 前置任务 C1–C17 + §13.1 结清实况；余数另立批L L-1…L-10）；**第 IV 轮 §18–§22 软件开发收全**（批M MC-0…MC-11，承 [ADR-0020](process/adr/ADR-0020-版本边界-开发收口与实机验证与验证后整理.md) 的 R32–R37（§2.7；§2.6 的 R31 是同一句用户口径的功能立项面 = 增补轮五批A–批F，两轮分账见 §18.4）；§18.1 = 功能验收清单 F-1…F-8（三版共用一张表）、§18.2 = 站替件台账；**已立项未开工**） |
| [`process/dev/0.0.5增补轮四——板载开发前技术落地全案.md`](process/dev/0.0.5增补轮四——板载开发前技术落地全案.md) | 增补轮四全案（已执行收官，批W0–W7）：两板开工前全部技术点落地实况——执行对账落 0.0.5 方案 §2.5，板上欠账与真件档为触发态（boards §10 / 真件手册） |
| [`process/dev/0.0.5增补轮五——交互功能落地全案.md`](process/dev/0.0.5增补轮五——交互功能落地全案.md) | 当前执行轮全案（R31 已裁执行中，批A–批F）：OLED 显示 / 舵机转向 / 语音对话三功能落地——Q1 921600 双通道过桥、Q2 四件全挂 ESP32、Q3 vosk+Piper、Q4 0.0.6 BOM 第 10 行；裁定记录落 0.0.5 方案 §2.6，与第 IV 轮批M 的分账落 §18.4 |
| [`process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md`](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) | 底座抽象补课全案（已立项未开工，开发窗）：逐部件能力原子化盘点 + 角色词表/`devices` 词表/`[pins]`/能源域/执行件档案五类数据结构定形 + 装载期十条校验（24 个判定位）+ 门 a–o + 批N1–N5 编排 + **§18 逐项证伪审查**；裁定原文在 [ADR-0021](process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md)（含修订 2 的六处数据结构改判） |
| [`process/dev/0.0.6版本洛克三号树莓派验证方案.md`](process/dev/0.0.6版本洛克三号树莓派验证方案.md) | **纯验证版（全承载档 host）**——怎么算完成（修订版 13：本版**不新增功能代码**、§6 DoD 扩 F-1…F-7 板上实录、"缺码"与"无载体"分开登记、BOM 第 2/4 行加"由 ESP32 协处理器承载替代"注记；修订版 12：出包 target 改由 E1 标准环境；修订版 11：矩阵按 v0.9 的七列登记（⑦ 时序达标、③④ 只算装载门）；修订版 9：新增承接**点验矩阵全承载档首行**对位；修订版 7/8 = 批F5a 实况登记与定位重述；树莓派两阶段 + 真件矩阵八行 + BOM 定案 + 板上工程执行手册 §5.5） |
| [`process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md`](process/dev/0.0.7版本洛克三号ESP32系统级验证方案.md) | **纯验证版（轻承载档 rtos）· 执行中**：ESP32 系统级验证四层验证梯子 + **批H 平台级点验**（修订版 15：批F 那次"版本收口"的**范围订正**为"通路四层 + 矩阵 esp32-mini 行（证据对象 = 诊断哨兵）"，**不含功能可达**；本版不新增功能码，外设与显示施工归增补轮五批B/批C；修订版 11：编译面走 E2/E3 容器 + 环境指纹；修订版 10：轻承载档行扩为七格（⑦ 时序达标归本版真板）；修订版 7：**定位订正为轻承载档 rtos** + **新增批G 点验矩阵轻承载档行**；修订版 5 Q1–Q6 已裁，层③④必达，§5.1 桥形状钉死 + 执行手册 §9 含 S3 预案与 e2e 双档判据） |
| [`process/dev/0.0.8版本洛克三号模块内部分层实施方案.md`](process/dev/0.0.8版本洛克三号模块内部分层实施方案.md) | 验证后整理过渡版（2026-10-12 恢复独立成批；"并入 0.0.5 第 II 轮"随 ADR-0020 裁定 2 作废——`rr3mod` 从未被任何批次认领；与 R31"0.0.8 不设功能版"同向）：W-A rr3mod 骨架与五模块 boot/rpc/dispatch、W-B 形状清理（L-3/L-9/L-10）、W-C brain Python→Rust、W-D 验证返工登记位；原签名级规格与迁移映射作施工参考件保留（随 ADR-0019 修订 1 定性：四面是视图不拆四份文件，§2 签名是 **std 驱动器形状**而非跨 OS 接口） |
| [`process/dev/真件装配与标定手册.md`](process/dev/真件装配与标定手册.md) | 真件到货做验收、装配与标定时照单执行（批W5 每件四件套 + 到货差异登记表） |
| [`process/dev/方案A——整板电压监测与分轴限幅立项评审.md`](process/dev/方案A——整板电压监测与分轴限幅立项评审.md) | 用户新需求（整板电压监测 + 分轴限幅 + 低电压串行降级）的立项评审——**已裁定** Q1–Q6（§8）+ 真板验收实测（§9），施工待契约评审 |
| [`process/legacy-digest.md`](process/legacy-digest.md) | legacy（ESP32 冻结区）逐组件消化到哪了（已归档：0.0.4 退役批整树删除，待融合清零） |

### contracts/ · base/ · modules/ · tools/ · .github/

| 路径 | 什么时候读它 |
|---|---|
| [`../contracts/README.md`](../contracts/README.md) | 字节级契约入口：内容 · topics.toml 裁定 · 演进规则 · 标准歧义登记 |
| [`../contracts/events/README.md`](../contracts/events/README.md) | 给某个事件负载登记 JSON Schema 时 |
| [`../contracts/golden/README.md`](../contracts/golden/README.md) | 对同机字节基准向量做编解码校验时 |
| [`../base/README.md`](../base/README.md) | 基座层为什么承载横切关注点、模块为什么不碰它 |
| [`../base/os/README.md`](../base/os/README.md) | OS 双轨层（ADR-0013）：buildroot 保底轨与 distro 快路轨的分工 |
| [`../base/os/buildroot/patches/README.md`](../base/os/buildroot/patches/README.md) | 建镜像前内核/U-Boot 的 dts 与 defconfig 核对清单、补丁回移流程、交叉产物注入方式 |
| [`../base/os/distro/README.md`](../base/os/distro/README.md) | 发行版快路：payload 打包、systemd 单元、已验证板卡笔记 |
| [`../base/ota/README.md`](../base/ota/README.md) | OTA 布局文档与槽位 CLI（不写实际槽位写入） |
| [`../base/runtime/README.md`](../base/runtime/README.md) | 基座 Rust workspace 的 crate 职责表与依赖预算 |
| [`../base/runtime/rr-ipc/README.md`](../base/runtime/rr-ipc/README.md) | 协议层 + 传输层：帧读写、契约版本再导出 |
| [`../base/runtime/rr-core/README.md`](../base/runtime/rr-core/README.md) | 机制核切片：总线语义 + 模块运行时执行环状态机（no_std） |
| [`../base/runtime/rr-bus/README.md`](../base/runtime/rr-bus/README.md) | 总线常量层，及订阅/QoS/回压的落点位置 |
| [`../base/runtime/rr-registry/README.md`](../base/runtime/rr-registry/README.md) | manifest 字段字典与主题注册表校验器 |
| [`../base/runtime/rr-hal-api/README.md`](../base/runtime/rr-hal-api/README.md) | 能力面契约层：7 trait 同步签名 + 值类型（no_std，rr-hal 的实现底座，0.0.5 批I） |
| [`../base/runtime/rr-hal/README.md`](../base/runtime/rr-hal/README.md) | HAL 的 trait 面、数据类型与后端规划 |
| [`../base/runtime/rr-ota/README.md`](../base/runtime/rr-ota/README.md) | 升级"双轨"的类型形状 |
| [`../base/runtime/rrd/README.md`](../base/runtime/rrd/README.md) | 守护进程装配层：supervisor / health / config / 在册表宿主 |
| [`../modules/README.md`](../modules/README.md) | 模块名册与固定五件（加模块第一站） |
| [`../modules/rr3-sense/README.md`](../modules/rr3-sense/README.md) | 感知：麦克风阵列、环境传感器、IMU |
| [`../modules/rr3-act/README.md`](../modules/rr3-act/README.md) | 执行：舵机、LED、反射环宿主 |
| [`../modules/rr3-interact/README.md`](../modules/rr3-interact/README.md) | 交互：表情屏、TTS 播放、Web 面板 |
| [`../modules/rr3-power/README.md`](../modules/rr3-power/README.md) | 电源：电池、充电、低压关断兜底 |
| [`../modules/rr3-connect/README.md`](../modules/rr3-connect/README.md) | 连接：WiFi 链路、云端会话、蓝牙配网 |
| [`../modules/rr3-brain/README.md`](../modules/rr3-brain/README.md) | 认知（Python）：ASR/LLM/TTS 编排与对话策略 |
| [`../tools/README.md`](../tools/README.md) | 主机侧工具名册：目录 × bin × 用途 |
| [`../tools/ci/README.md`](../tools/ci/README.md) | `check.sh` 每步跑什么、为什么这么跑 |
| [`../tools/mkmodule/README.md`](../tools/mkmodule/README.md) | 用 `rr3-mkmod` 生成模块骨架时 |
| [`../tools/espidf-std-hello/README.md`](../tools/espidf-std-hello/README.md) | espidf std 构建链冒烟件（批W1）：版本钉死登记对表（ESP-IDF/esp-idf-sys/esp-clang/ldproxy）与三坑复现必读 |
| [`../tools/espidf-sentinel/README.md`](../tools/espidf-sentinel/README.md) | 层③ 哨兵（0.0.7 批D1）：内跑 `rr-core::runtime` 的真板消费者，板上 runbook（双证/三点堆采样/批D2 触发判据） |
| [`../tools/esp32-periph-base/README.md`](../tools/esp32-periph-base/README.md) | 批B 外设底座档（0.0.5 增补轮五）：OLED+舵机独立固件接线表、烧录 runbook、验收判据四件套 |
| [`../tools/sim/README.md`](../tools/sim/README.md) | 没有板子要跑全链路时（`rr3-sim`） |
| [`../tools/rrctl/README.md`](../tools/rrctl/README.md) | 总线侧运维 CLI 的命令面（`rr3-ctl`） |
| [`../.github/README.md`](../.github/README.md) | GitHub 侧工作流声明与它的实际地位 |

## 我该读哪篇（按动作）

| 我要…… | 读这里 |
|---|---|
| 建立整体图（新人） | [`architecture/architecture.md`](architecture/architecture.md) §1–§3 → 下文"阅读顺序" |
| 加一个模块 | [`../modules/README.md`](../modules/README.md)（五件）+ [`../tools/mkmodule/README.md`](../tools/mkmodule/README.md) + [`process/naming.md`](process/naming.md) |
| 登记或改一个主题 | [`../contracts/README.md`](../contracts/README.md) 的 topics.toml 裁定，主题表本身在 [`../contracts/topics.toml`](../contracts/topics.toml) |
| 定义一个事件负载 | [`../contracts/events/README.md`](../contracts/events/README.md) |
| 查 128B 头 / payload 枚举的字节布局 | [`../contracts/envelope.schema.json`](../contracts/envelope.schema.json) + [`../contracts/golden/README.md`](../contracts/golden/README.md) |
| 起 rrd、查参数与默认值 | [`reference/rrd-cli.md`](reference/rrd-cli.md) §2、§5 |
| 调上行 RPC、对错误码 | [`reference/rrd-rpc.md`](reference/rrd-rpc.md) §5（帧层前提看 §1） |
| 查在册表的阶段、门序与退避 | [`reference/rrd-registry.md`](reference/rrd-registry.md) §3、§5、§7 |
| 提交前对门禁 | [`process/conventions.md`](process/conventions.md) §2 + [`../tools/ci/README.md`](../tools/ci/README.md) |
| 看进度实况与在制品 | [`status.md`](status.md) |
| 名字对不上（目录 / crate / bin / trait） | [`process/naming.md`](process/naming.md) |
| 改 Buildroot 镜像与分区 | [`../base/os/README.md`](../base/os/README.md) + [`../base/os/buildroot/board/radxa-zero3/flash-maskrom.md`](../base/os/buildroot/board/radxa-zero3/flash-maskrom.md) |
| 选核心板 / 画底板 / 整机硬件询价与验收 | [`architecture/板卡设计规格书.md`](architecture/板卡设计规格书.md)（决策依据在 [`process/adr/ADR-0015`](process/adr/ADR-0015-板卡策略-rk3566核心板过桥与量产自定制.md)） |
| **这条设计能被推翻吗 / 这条门会不会恒绿 / 判据有几个判定位** | [`process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md`](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §18（逐项证伪审查：每条设计配一条"能推翻它的观测"；§18.1 三类失败形态 L-22/L-23/L-24、§18.7 审过成立十条、§18.8 撤回七处）；判据口径见同档 §5（**十条 / 24 个判定位**）与 §8（门 a–o 对 `check.sh` 拟步的映射表） |
| **接一块新板 / 加一类外设 / 引脚与接线冲突 / 电机标定与复位 / 面板该看到什么** | [`process/adr/ADR-0021`](process/adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md)（裁定：能源是第七位供给、`[pins]` 角色绑定三层对象、九键闭集改判、执行件档案、面板覆盖率门；**修订 2** 六处数据结构改判：`[hardware]` 六物理键下移、`pin_id` 等式、`devices.toml` 新立、门 a 六表、校验缺失语义与后果三态、撤名 `Rr3PowerDomain`）+ [`process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md`](process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md)（逐部件十一问盘点、装载期十条校验、批N1–N5）；裁定记录 D10–D15 与结论行在 [`design/模块设计索引.md`](design/模块设计索引.md) §8、细则在全案 §12 |
| 动 OTA 槽位与双轨 | [`../base/ota/README.md`](../base/ota/README.md) + [`../base/runtime/rr-ota/README.md`](../base/runtime/rr-ota/README.md) |
| 无硬件跑通整条链路 | [`../tools/sim/README.md`](../tools/sim/README.md) |
| 器件到货做验收 / 装配 / 标定 | [`process/dev/真件装配与标定手册.md`](process/dev/真件装配与标定手册.md)（接线对照 [`process/dev/0.0.6版本洛克三号树莓派验证方案.md`](process/dev/0.0.6版本洛克三号树莓派验证方案.md) §5.5.2） |
| 对照 legacy ESP32 固件 | [`process/legacy-digest.md`](process/legacy-digest.md)（归档台账）+ [`architecture/architecture.md`](architecture/architecture.md) §8（`legacy/` 整树已随 0.0.4 退役批删除，源码在 git 历史） |

## 事实归属（改文档前先对这张表）

| 事实类型 | 唯一落点 | 其他文档的写法 |
|---|---|---|
| 门禁步序、测试条数 | [`process/conventions.md`](process/conventions.md) §2 | "步序与条数见 conventions §2"——`tools/ci/check.sh` 头部注释已立此规 |
| 已实装 / 未实装、里程碑进度 | [`status.md`](status.md) | 不写。`architecture/` 只写设计语义，不写"当前 HEAD 有没有" |
| 主题清单与 kind | [`../contracts/topics.toml`](../contracts/topics.toml) | 只写"如何登记、如何校验、前缀归属"，不抄表 |
| manifest 字段字典 | [`../base/runtime/rr-registry/src/module.schema.toml`](../base/runtime/rr-registry/src/module.schema.toml) | 只写字段语义与一份合规示例 |
| 字节布局（128B 头 / payload 枚举） | [`../contracts/envelope.schema.json`](../contracts/envelope.schema.json) + [`../contracts/golden/`](../contracts/golden/) | 只写语义与歧义裁定 |
| 运行期接口字面量（rrd 参数与默认值、RPC 方法与错误码、在册表常量与门序） | [`reference/`](reference/) 三件 | 架构文档只留判据与设计语义，指针指向 reference/；断言口径分档（`Display`/错误码逐字 `assert_eq`，横幅只 `contains`，`RRD_UNSUPPORTED` 无用例断言——见各文件顶部），**改代码须同批改 reference/** |

## 阅读顺序

新人：`architecture/architecture.md` §1–§3 → `process/conventions.md`（提交前必读）→
`process/adr/ADR-0002` 与 `ADR-0006`（模块模型与线格式两根支柱）。

改契约/字节格式：先读 [`../contracts/README.md`](../contracts/README.md)（歧义裁定与演进规则），
再动 `contracts/` 本体。

## 实况与规划的边界

**实况与规划分家**：架构文档只写目标架构与设计语义，不承载"某能力今天有没有"这类实况——
后者唯一落在 [`status.md`](status.md)（见上文"事实归属"表）。里程碑的**内容、验收标准与每条的
当前进度都在 `status.md` 的"里程碑进度"一节**，架构文档 §10 只留一行指向它：同一张表抄两份
必然漂移，所以只留一份。

写文档时不要把设计期草图当既成事实——这是本项目已发生过的最大一次订正。
