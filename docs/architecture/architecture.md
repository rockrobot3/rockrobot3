# rock3 总体架构文档

- **状态**：v0.9（融入能力格坐标系与装配单 [ADR-0012](../process/adr/ADR-0012-能力格坐标系与装配单.md)、
  OS 双轨 [ADR-0013](../process/adr/ADR-0013-OS双轨-发行版快路与Buildroot保底.md)、
  跨操作系统移植模型 [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md)（含修订 1/2/3）、
  原子功能模块模型 [ADR-0019](../process/adr/ADR-0019-原子功能模块模型-格件面与组合代数.md)（含修订 1/2）；
  只写设计语义，一切"做到哪"的句子移出本档）
- **日期**：2026-10-01
- **修订记录**：v0.9（2026-10-09）闭合第 III 轮开工前查出的**九处逻辑漏洞 + 十处施工阻塞**，与
  ADR-0018 / ADR-0019 的**修订 2** 同步落档：① 无主承载层形态（自足固件）的**寻址与生命周期**补齐——
  task 号由描述符静态分配、`Rr3ModuleRuntime` 有两个合法入口、`⊙` 分"经桥接"与"自足"两拓扑，且自足形态
  **不宣称发布主题**（§12.1/§12.4/§12.7）；② 能力面契约的**增长律与对象形状**定形——新执行器=新增
  trait（禁止给既有 trait 加方法）、`bare` 档禁 `dyn`、值类型禁把 alloc 写进签名（§12.8 接缝一）；
  ③ **时序预算位**入 `[supplies]`，并规定"逐拍一致"只保证数值语义、不保证实时性（§12.2/§12.5/§12.6）；
  ④ 件级对拍判据补**第二条腿**（期望值必须引自契约 schema/golden/裁定语义 + 边界覆盖），否则门会
  奖励空件（§4.0.3/§13）；⑤ 立 **retired（号位下线）**一档，解决"只增不改 + 不留空格"制度性制造空壳件
  的问题，并把 正常/占位/retired 三态的**唯一真值点**定在 `contracts/slots.toml` 标记位（§4.0.3/§4.3/§13）；
  ⑥ 点验矩阵列**重定性**——③供给自证与④描述符装载属"装载门"（证描述符没撒谎），只有 ①②⑤⑥⑦ 是行为
  证据；① 列必须记"实跑第几路"，且承载体在远端缺位时明确用本地 runner 替代（§12.6）；⑦ 矩阵与描述符
  的**参数真值合并规则**（profile 优先、留痕）与**形态身份可读面**补齐（§4.3/§7.1）；⑧ §12.8 末句
  "两件码 + 一份描述符 + 一行矩阵"加**限制条件**（只对既有 trait 已覆盖的板成立）；⑨ 门禁自身的
  **硬编码清单同步纪律**（新建机制核面必须同批进 ⑦-a 扫面与 xtensa 门），防止"文档宣称有门、门没扫"。
  十处施工阻塞（装机包不含件级声明、sim 演练按 `enabled_slots` spawn、②b 形状 pin 等）不属架构语义，
  全部登记为 0.0.5 第 III 轮的**前置任务**（方案 §13.1/§14）。同日二轮对表再落两份 ADR 的**修订 2/修订 3**（自足形态寻址与生命周期 · 能力面增长律与对象形状 · 时序预算位 · 对拍判据第二条腿 · retired 号位下线机制 · 依赖闭包 L0/L1 两级 · 门查门 · 段数订正为七段与 transport 层 cfg 门解除 · 执行宿主前置），前置任务扩为 **C1–C16**，批次代号改 **批I/J/K**（避开 0.0.6 的批F1/F5a 与 0.0.7 自己的批G）。**同日三轮（形态裁剪推理收口）**：拿 F-B 桥接微型与 F-C 自足
微型两个形态当检验样本对全套设计做一次前提推理，查出六处不闭合并落档——① 供给清单缺"平台看门狗"位
（§12.2 第六供给位，结论引用了没供给过的能力）；② 自足档必须交本地守护输出（否则"错了没人知道"）；
③ 经桥档的 task 号定**注册态**（§12.7，此前两态漏了中间那档）；④ base 层"恒在位"按档分族
（§4.3 base-supply / base-bus，ADR-0012 增补注记）；⑤ 件的**载荷组装上移承载层**、依赖闭包按**发布权**收窄
（ADR-0019 修订 4——"件必须无 `serde_json`"作为普适跨档前提是过头的，它要求一件没有消费者的编码器）；
⑥ 禁词执法面与运行期自述字符串不匹配（`BlueZ`/`NetworkManager` 今天出现在 SURFACE 与 println 这类**对外面**，
⑦-a 扫不到），作用域须三分。推理过程与逐条落点见 [模块设计索引](../design/模块设计索引.md) §10/§11。
编号与既有锚点继续全部保留。
  v0.8（2026-10-09）按两份 ADR 的**修订 1** 收口径——① 承载切线由"要不要操作系统"
  改判为**四类供给齐备度**，固定三档（全承载 host / 轻承载 rtos / 无承载 bare，§12.1–§12.2）；
  ② 机制核**外延定名**（`rr-ipc::protocol` + `rr-core` + 待建 `rr-hal-api`），原子件与组合件定性为
  "机制在承载层内的物理组织"（§4.0、§12.1）；③ 新增 **§12.8 跨 OS 的三处窄缝**（能力面契约 /
  总线字节流 / 注入数据面）——跨操作系统从"设计里存在"变成"可施工"的前提；④ 描述符定形**六段**
  （`[platform]`/`[supplies]`/`[slots]`/`[bindings]`/`[enabled_slots]`/`[hardware]`），把"芯片平台"
  与"底板走线"在数据面上分开（§4.3、§12.3）；⑤ 点验矩阵**定形六列**，0.0.6/0.0.7 改用它收口
  （§12.6）；⑥ 格格双达标判据②由"扇入"改判为**件级逐拍对拍 + 依赖闭包 + 字面量位点清单**（§4.0.3）；
  ⑦ Facade 降为**组合件的职责视图**，不建第四份 manifest（§4.0/§4.0.2/§13）；⑧ §13 门禁表补三门、
  撤一条"业务阈值 grep 即红"的过高宣称。编号与既有锚点全部保留（章节号被十余文档按 §N 引用）。
  v0.7（2026-10-08）大修——引入**原子功能模块模型**（立格→造件→成面 + `⊗`/`⊙` 组合代数 +
  固定职责四面 + 机制核零策略载荷），决策落
  [ADR-0019](../process/adr/ADR-0019-原子功能模块模型-格件面与组合代数.md)、架构展开落 §1 总纲与 §4；
  §4 从"模块模型"改写为"原子功能模块模型"（保留 §4.1/§4.2/§4.3 锚点不破坏入链），新增 §13 门禁与验收、
  §14 速查。`profiles` 的 `[faces]` 语义改名 `enabled_slots`（与新"职责面 Facade"消歧，连带改 ADR-0012/
  `rr-registry` 查册门，见 §4.3）。§3 目录树仍为全仓唯一目录树事实源、编号不动（章节号被十余文档按 §N 引用）。
  v0.6（2026-10-08）确立"跨操作系统快速部署"为底座核心价值，把底座切分为**机制核（与 OS 无关、`no_std` 可达、
  冻结）与主承载层（有 OS 才叠加）**两层；决策落
  [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md)、架构展开落 **§12 跨操作系统移植与部署**；
  据此改写 §1 定位（不再以"面向 Linux"为底座定义）、§3 目录树 ADR 范围、§6/§7 的"非 Unix 即不支持"承重句为
  "跨进程 daemon 属主承载层、机制核跨平台"。v0.5（2026-10-01）能力格坐标系与 OS 双轨。
- **实况出口**：本档不回答"某能力有没有实现"。凡此类问题一律指向 [`../status.md`](../status.md)；
  本档里出现进度性断言即视为待订正。
- **关联决策**：[ADR-0001](../process/adr/ADR-0001-平台选型-linux66-buildroot-rk3566.md)（构建系统主路线已被 ADR-0013 修订） · [ADR-0002](../process/adr/ADR-0002-模块模型-5加1-进程隔离.md) · [ADR-0003](../process/adr/ADR-0003-IPC选型-uds-jsonrpc-事件总线.md) · [ADR-0004](../process/adr/ADR-0004-Airymax接入路径-运行时层对接.md) · [ADR-0005](../process/adr/ADR-0005-命名规约-rr3前缀矩阵.md) · [ADR-0006](../process/adr/ADR-0006-采用AiryIPC线格式与rr-ipc分层.md) · [ADR-0007](../process/adr/ADR-0007-控制面分层与模块级升级.md) · [ADR-0008](../process/adr/ADR-0008-传输层trait定型与uds实现.md) · [ADR-0009](../process/adr/ADR-0009-依赖仲裁-启动序与降级传播.md) · [ADR-0010](../process/adr/ADR-0010-控制通道与查询面.md) · [ADR-0011](../process/adr/ADR-0011-shm传输-单写者多读者环形缓冲.md) · [ADR-0012](../process/adr/ADR-0012-能力格坐标系与装配单.md) · [ADR-0013](../process/adr/ADR-0013-OS双轨-发行版快路与Buildroot保底.md) · [ADR-0014](../process/adr/ADR-0014-python模块接入与M4对话闭环.md) · [ADR-0015](../process/adr/ADR-0015-板卡策略-rk3566核心板过桥与量产自定制.md) · [ADR-0016](../process/adr/ADR-0016-brain迁移路线-python转Rust渐进首裁定.md) · [ADR-0017](../process/adr/ADR-0017-模块内部分层-机制与策略分离.md) · [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md)（跨操作系统移植——本档 §12） · [ADR-0019](../process/adr/ADR-0019-原子功能模块模型-格件面与组合代数.md)（原子功能模块模型——本档 §1 总纲 / §4）
- **命名速查**：[naming.md](../process/naming.md)

---

## 1. 定位与目标

### 1.0 总纲

**一条纲领：机制与策略极致分离。** rock3 的一切代码只用一个问题裁定归属——这是机制，还是策略？
答「机制」→ 留机制核，原子化、填格、成面；答「策略」→ 让位，经 `ops` 函数表 + 描述符**数据化注入**，
或整体迁到形态/生态层。机制核**零策略载荷**：不含厂商名、产品名、业务阈值、`if (vendor == …)`——
凡出现即架构违例（§13 静态断言，fail-closed）。

**主线三步：立格（Slot）→ 造件（Atom）→ 成面（Facade）。** 固定的是格，变化的是件。讨论"要不要加一个
模块/件"时先问"它填哪个格"；答不出格的，就是不该造（§4、ADR-0019）。

**最高约束：跨操作系统快速部署**（§12 / ADR-0018）——同一机制核要在全承载档（Linux）、轻承载档
（RTOS，如 ESP32 + FreeRTOS）与无承载档（纯裸机）三档板卡上都成立；**换平台 = 写两件码
（一个能力面后端 + 一个字节流实现）+ 交一份七段描述符 + 填一行点验矩阵**（§12.8），不改产品码。
**功能守恒**：任何重铺不得使对外功能消失，
抽象靠三条减法达成，不靠删功能。

**三条减法托住抽象：**

| 减法 | 含义 | 通道 |
|---|---|---|
| 迁出 | 策略与体验面离开机制核（搬迁非删除，功能在别处继续提供） | 迁到 brain/形态配置/生态层 |
| 塌缩 | 同类件收敛为每格一件 | 每格一件 + 准入三问（§4） |
| 生成 | 五件套样板由生成器产出，不手写 | `rr3-mkmod`（§4.6） |

### 1.1 底座定位与分层

RockRobot3（rock3）是一个**具身机器人底座**，核心价值是**跨平台快速部署**：同一套底座机制要
能快速部署到各种板卡上——既包括带 Linux 的板卡（起点为 RK3566 桌面陪伴形态，体积约束约 0.5L），
也包括**不带 Linux 的板卡（ESP32 这类微控制器）**。本质上这是**跨操作系统**：换一次平台省下的，
是重做一遍的时间与成本。底座由 ESP32 固件项目（`legacy/`，已退役——git 历史 + tag
`v0-pre-legacy` 可回溯）演进而来，最终面向各种具身机器人形态发展。

底座因此切成两层——**机制核（与 OS 无关、`core + alloc` 可达、契约级冻结）与主承载层（有操作系统
才叠加）**——这条切线是 [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md)
的裁定，架构展开见本文 **§12**。**但"两层"不等于"两档"**：真正的分档判据是"四类供给齐不齐"
（§12.2），于是底座可落三档——全承载（host）/ 轻承载（rtos）/ 无承载（bare）。一块跑 FreeRTOS 的
MCU 有操作系统却没有主承载层，只按"OS 有无"画线会把它漏在档外；这条改判随 ADR-0018 修订 1 落。
下面的三层模型（[ADR-0013](../process/adr/ADR-0013-OS双轨-发行版快路与Buildroot保底.md)）
是"具备 Linux 的主承载形态"这一档的视图；另两档如何承载同一机制核，见 §12。

**底座 = 机制核 + 主承载层**：rock3 的交付物不是某一块板的镜像，而是一份可安装的**底座**——
全承载档表现为 `rrd` + `rr-hal` + `contracts/` + 安装器 + 装配单这层 payload；轻承载与无承载档
表现为同一机制核的一个切片（切片今天能切到哪一层，取决于 §12.8 三处窄缝落完没有——在此之前
MCU 上真跑过的只有字节契约）。板卡与 OS 差异由三层模型消化：

```
Layer 2  模块层     rr3-sense / rr3-act / … / rr3-brain（OCI 分发，rrd 编排）
Layer 1  底座 payload（rock3 本体）rrd + rr-hal + contracts + 装配单 + 安装器
Layer 0  板卡支持   白拿上游发行版（Debian/Armbian/Alpine）；Buildroot 保底轨
```

**部署到新平台的三步**（换一块板 / 一个形态的全部动作；三档同形，见 §12、§12.8
与 [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md) 裁定 6）：

1. **提供平台 port**——该板向机制核供四类东西：时间基准、内存策略、事件循环、传输实现；落到码上就是
   **两件实现**：一个能力面后端（实现 `rr-hal-api` 的同步 trait）+ 一个字节流实现
   （`Rr3ByteStream` 两方法，§12.8 接缝二）。全承载档在此之上叠加主承载层（发行版快路 deb /
   Buildroot 保底镜像落 `rrd`、HAL 后端与契约文件）；
2. **交描述符**——`profiles/<机型>.toml` 的**七段**（§4.3）：`[platform]` 芯片平台档与承载档、
   `[supplies]` 四类供给取值、`[slots].present` 本板在位格集、`[bindings]` 格位→件、
   `[enabled_slots]` 启用格集、`[hardware]` 底板走线；`rrd`（或无 OS 上的机制核装配段）按描述符 +
   硬件探测匹配，缺格显式降级；
3. **填一行点验矩阵**——七列机械检查（§12.6）：字节契约对 golden、机制核在目标三元组编译、
   **供给自证与 `[supplies]` 逐键核对**、描述符装载与缺格降级、安全核在目标上真跑逐拍一致、
   生命周期（无承载档只跑单固件环）、**时序达标（名义周期与偏差，板上取直方图，⑦ 列）**。跑不到的格登敞口与触发条件，不拿"设计已覆盖"充数。

任何"需要改 rock3 代码才能换板 / 换平台"的现状都视为待修的泛化欠账。

**硬性目标**：

| 目标 | 说明 |
|---|---|
| 自烧录 / 自更新 OS | 保底轨 maskrom 自烧录（不拆机，Rockchip 路线）；日常自更新由 A/B OTA 承担 |
| 模块化 5+1 | sense / act / interact / power / connect + brain，进程级隔离 |
| 基座能力 day-one | 模块注册、看门狗、A/B OTA、工厂恢复 |
| 待机功耗 | 第一版即含电源管理，待机 <1W |
| 可演进 | 保留接入 Airymax OS/RT 的目标（运行时层对接，见 ADR-0004） |

**明确不做**：ROS 2（太重）、PyO3 嵌入（改用独立进程）、gRPC（单机 UDS 足够）、
Yocto（后期再评估，双轨不含第三轨）、容器编排层（OCI 只作模块分发格式，`rrd`
保持模块生命周期的唯一所有者，见 ADR-0013 裁定 4）。

---

## 2. 总体分层

```
┌─────────────────────────────────────────────────────┐
│  模块层 Layer 2（5+1，独立进程，崩溃隔离）              │
│  rr3-sense  rr3-act  rr3-interact  rr3-power        │
│  rr3-connect            rr3-brain (Python)          │
│  分发 = OCI 镜像（打包格式，非编排层，ADR-0013）        │
├─────────────────────────────────────────────────────┤
│  底座 payload Layer 1（rock3 本体）                    │
│  rrd 守护进程：事件总线 · 模块注册表 · HAL · 看门狗     │
│  rr-ipc / rr-bus / rr-registry / rr-hal / rr-ota     │
│  能力格坐标系 contracts/slots.toml · 装配单 profiles/  │
│  （ADR-0012）                                          │
├─────────────────────────────────────────────────────┤
│  OS 层 Layer 0（ADR-0013 双轨）                       │
│  快路：Debian / Armbian / Alpine + payload 安装包      │
│  保底：Buildroot + Linux 6.6（base/os，量产冻结）      │
│  A/B 分区 slotA/slotB/factory · maskrom 自烧录         │
└─────────────────────────────────────────────────────┘
```

> **读法（承 §12 / ADR-0018）**：上图是**具备 Linux 的主承载档**视图。框中"独立进程"指该档下执行单元的
> 落地形态；不带 Linux 的平台，同一"模块"落为任务或内联子系统（§12.4、[ADR-0019](../process/adr/ADR-0019-原子功能模块模型-格件面与组合代数.md) 裁定 1）。
> 机制核（契约 / 传输抽象 / 消息语义 / 模块运行时 / 控制原语 / 安全核）跨档共享，不被 Layer 1/2 框住。

核心原则：

1. **基座承载横切关注点**：安全（急停/限位/低电保护）、日志、时间同步、OTA 全部在 base，不进模块——基座存活则安全兜底存活
2. **模块间零直连**：一切交互经事件总线/RPC，模块只依赖"能力名"不依赖具体模块名；能力名本身有坐标系（§4.3，ADR-0012）
3. **brain 是唯一编排者**：对话策略只存在于 rr3-brain（承接 legacy main.c 的"薄壳"哲学）
4. **机制与策略分离**：模块只含机制（怎么采 PCM、怎么驱动舵机）；装配（谁提供哪个格）归装配单、策略（何时说、说什么）归 brain、提供者选择归 rrd 仲裁（ADR-0012）。该原则在模块**内部结构**的落实见 [ADR-0017](../process/adr/ADR-0017-模块内部分层-机制与策略分离.md)：公共骨架 crate `rr3mod` 收敛五模块同构启动序列，main.rs 只装配、boot.rs 接入、rpc.rs 协议分发，策略不进模块——该实施已由 [ADR-0019](../process/adr/ADR-0019-原子功能模块模型-格件面与组合代数.md) 承接、并入 0.0.5 第 II 轮（模块内部拆分即 §4"原子功能模块模型"，0.0.8 不再独立成批）
5. **跨操作系统 = 机制核与主承载层分离**：把"机制与策略分离"再抬一个维度——机制核（契约、传输抽象、消息语义、模块运行时、控制原语、安全核）与操作系统无关、`no_std` 可达、冻结；主承载层（`rrd` 跨进程监督、完整 HAL、cloud brain、打包与 OTA 写侧）只在有操作系统的平台叠加。同一份机制核要能在带 Linux 与不带 Linux 的板上成立，这是 §12 与 [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md) 的主题

---

## 3. 仓库目录树（文件级）

本树只表达**结构与设计落点**：某个目录里放什么、归哪条 ADR 管。树里不写"某处有没有实现"——
目录存在不等于内容有进展，实况唯一落在 [`../status.md`](../status.md)。目录增删必须与本节
同批变更（`process/conventions.md` §5 文档纪律）。

```
rock3/
├── README.md · README_zh.md       ← 项目门面（不承担架构论证）
├── .gitignore                     ← 白名单式追踪：`*` 默认忽略，逐目录放开（密钥与再生件永不入库）
├── .github/
│   ├── README.md
│   └── workflows/ci.yml           ← 三 job 拆步：conventions（与本地同一入口
│                                      `bash tools/ci/check.sh`）· lint（fmt/clippy）·
│                                      brain-unittest（ubuntu+macos 双矩阵）
│
├── contracts/                     ← [SC] 字节级共享契约（Rust/Python 双方唯一事实源）
│   ├── README.md                  ← 歧义裁定与"契约只增不改"的演进规则
│   ├── envelope.schema.json       ← A-IPC 128B 消息头 + 5 种 payload 类型契约（ADR-0006）
│   ├── topics.toml                ← 主题所有权注册表（前缀归属与校验入口见 §6.3）
│   ├── slots.toml                 ← 能力格坐标系总表（查册门真值，§4.3、ADR-0012 裁定 1）
│   ├── golden/                    ← event-header-128.hex：一条真实头的 hex 向量（同机字节基准）
│   └── events/                    ← 各模块事件负载 JSON Schema 的落点（命名与登记规矩在其 README）
│
├── profiles/                      ← 机器人装配单（纯数据，一机型一文件，§4.3、ADR-0012 裁定 3）
│   └── default.toml               ← RK3566 参考机：[os] 轨道 · [bindings] 格位→件 · [enabled_slots] 模块→格位
│
├── base/                          ← 基座层（横切关注点，见 §2 原则 1）
│   ├── os/                        ← OS 双轨（ADR-0001 选型、ADR-0013 分轨）
│   │   ├── buildroot/             ← 保底轨：configs/（rock3_zero3_defconfig）·
│   │   │                             board/radxa-zero3/（genimage.cfg 容量=构建期占位符 ·
│   │   │                             extlinux.conf · post-build.sh（rrd 自启/fstab/产物安装位）·
│   │   │                             post-image.sh（容量参数注入）· flash-maskrom.md）·
│   │   │                             patches/（内核/U-Boot 回移补丁，核对清单见其 README）
│   │   └── distro/                ← 快路轨：payload 打包（.deb/OCI）· systemd units ·
│   │                                 boards/（发行版实板笔记）
│   ├── ota/                       ← README.md + partition-layout.md（A/B/factory 布局文档）
│   │   └── rr3-ota-ctl/           ← 独立 workspace，bin 名 `rr3-ota-ctl`（升级入口 CLI）
│   ├── runtime/                   ← Rust workspace：members = rr-ipc rr-core rr-registry rr-bus rr-hal rr-ota rrd
│   │   │                             （Cargo.lock 在 workspace 根，每 crate 各有一份 README.md）
│   │   ├── rr-ipc/                ← A-IPC 协议层 + 帧收发（§6.2）；协议层纯 core+alloc（no_std 可达）
│   │   │   ├── src/protocol/      ← header.rs（128B 头编解码）、body.rs（payload 类型前缀）、
│   │   │   │                          wire.rs（帧数据形状：Rr3Frame/编码，no_std 可达）、mod.rs
│   │   │   ├── src/transport/     ← frame.rs（整帧读写）、uds.rs（UDS 实现，`#[cfg(unix)]`）·
│   │   │   │                          shm.rs（单写者多读者环形缓冲）· buf.rs（整帧所有权缓冲）·
│   │   │   │                          mod.rs（传输分层 + 两个路径常量）
│   │   │   ├── src/bin/           ← ipct003_dump.rs：IPC-T-003 跨架构对拍的头 hex 转储（CI 平台条件下跑）
│   │   │   └── tests/             ← envelope_meta.rs：与 envelope.schema.json 逐字段对齐的守护测试
│   │   ├── rr-core/               ← 机制核切片（ADR-0018）：总线语义（常量/寻址/帧组装）
│   │   │                             + 模块运行时执行环状态机（runtime：连接态/心跳拍点/
│   │   │                             分发分类，时钟注入纯函数，no_std）
│   │   │                             + 控制原语与安全核（control：限速门/低电边沿/饱和
│   │   │                             检测/限位钳位，纯函数 + 注入时钟，no_std）
│   │   ├── rr-bus/                ← 总线常量真值在 rr-core（本 crate re-export）+ 模块侧同步客户端
│   │   │                             （client.rs：连接/注册/心跳/发布/rr3.call/收帧）
│   │   ├── rr-registry/           ← manifest 解析与校验、主题集（§4.1、§6.3）
│   │   │   ├── src/               ← schema.rs、validate.rs、topics.rs、slots.rs（能力格查册门）、
│   │   │   │                          depends.rs（依赖仲裁）、module.schema.toml（字段字典）
│   │   │   └── tests/             ← contract.rs（契约面用例）+ fixtures/rr3-sense.toml
│   │   ├── rr-hal/                ← HAL trait 与数据类型（§5）：lib.rs + backend.rs
│   │   ├── rr-ota/                ← 升级通道的类型形状（§8、ADR-0007 决策二）
│   │   ├── rrd/                   ← 主守护进程（装配层）
│   │   │   ├── src/               ← 十一个文件（十个模块文件 + `bin/sim_peer.rs`），逐个职责见下
│   │   │   │   ├── cli.rs         ← 命令行与总线路径解析（平台无关）
│   │   │   │   ├── main.rs        ← 启动序（§7.1）
│   │   │   │   ├── lib.rs         ← 平台无关模块图 + `#[cfg(unix)]` 门 + 横幅/日志出口
│   │   │   │   ├── manifest.rs    ← manifest 的运行期事实（`Rr3ManifestFacts`）
│   │   │   │   ├── registry.rs    ← 在册表与阶段机（§4.2）
│   │   │   │   ├── rpc.rs         ← 上行 JSON-RPC 方法的编解码
│   │   │   │   ├── topics_load.rs ← 契约主题表装载 + 仅测试的主题叠加层
│   │   │   │   ├── conn.rs        ← 单连接服务（reader + writer 线程）——`#[cfg(unix)]`
│   │   │   │   ├── daemon.rs      ← 监听 / accept / 连接线程编排——`#[cfg(unix)]`
│   │   │   │   ├── supervisor.rs  ← spawn / 收尸 / 重启执行（全 daemon 唯一执行线程）——`#[cfg(unix)]`
│   │   │   │   └── bin/
│   │   │   │       └── sim_peer.rs ← 跨进程 e2e 的对端夹具二进制：第二个真进程，走 rrd 的线上
│   │   │   │                           入口做注册→心跳→发布→退场；不是产品入口（只在测试里被起）
│   │   │   └── tests/             ← register_publish.rs、socket_banner.rs、e2e_sim_peer.rs、
│   │   │                             e2e_watchdog.rs（整体挂在 `#![cfg(unix)]` 门后：
│   │   │                             `e2e_watchdog.rs` 因头注更长落在第 20 行，其余在首行）
│   │   │                             + support/mod.rs（起真 rrd 子进程的线级助手）
│   │   └── tests/fixtures/        ← sim-topics.toml：仅测试的主题叠加层夹具（不吃契约 pin）
│   │                                 + sima/module.toml、simb/module.toml：e2e_sim_peer.rs 那两个
│   │                                   假模块的 manifest 夹具
│
├── modules/                       ← 统一内构（`rr3-mkmod` 落盘五件）：module.toml + config.toml +
│                                     Cargo.toml + src/main.rs + README.md；Rust 模块各带 Cargo.lock
│   ├── rr3-sense/                 ← src/audio/{capture,vad}.rs（+ mod.rs）、env.rs、imu.rs
│   ├── rr3-act/                   ← src/{servo,reflex,led}.rs——舵机/电机反射环的宿主进程
│   ├── rr3-interact/              ← src/player.rs、face.rs、panel/{mod.rs,server.rs,www/index.html}
│   ├── rr3-power/                 ← src/battery.rs、charger.rs
│   ├── rr3-connect/               ← src/wifi.rs、bt.rs、session.rs
│   └── rr3-brain/                 ← Python 包（无 Cargo.toml）：rr3_brain/{__main__.py,dialog.py,
│                                     cloud/{asr,llm,tts}.py} + config.toml + pyproject.toml
│
├── tools/                         ← 三个独立 workspace + 门禁脚本（各自 Cargo.lock）
│   ├── ci/check.sh                ← 规约检查唯一入口；步序与条数只登记在 conventions.md §2
│   ├── mkmodule/                  ← bin: rr3-mkmod，生成合规模块骨架并自检（tests/gen.rs 回灌契约）
│   ├── rrctl/                     ← bin: rr3-ctl，status / health / logs / ota 四个子命令
│   └── sim/                       ← bin: rr3-sim，虚拟设备与 PCM 形状，主机联调
│
└── docs/                          ← 四档结构与"事实归属"表见其 README.md
    ├── README.md                  ← 索引：哪类事实只有一个权威落点
    ├── architecture/architecture.md ← 本档（唯一架构图）
    ├── status.md                  ← 唯一实况快照
    └── process/                   ← conventions.md、naming.md、adr/（ADR-0001~0018）、dev/
```

本树只画**已存在的结构**：`base/`、`modules/`、`tools/` 与每个 crate、每个模块目录还各有一份
就地自述的 `README.md`（职责写在那里，进度不写）。

> **规划落点**：以下落点由 [ADR-0012](../process/adr/ADR-0012-能力格坐标系与装配单.md)
> 与 [ADR-0013](../process/adr/ADR-0013-OS双轨-发行版快路与Buildroot保底.md) 定了设计落点；
> 已落盘的划线注销（§3 树已同批补画），仍未落盘的按"只画已存在的结构"纪律不进上面的树：
>
> | 落点 | 归属 | 语义 |
> |---|---|---|
> | ~~`contracts/slots.toml`~~ | ADR-0012 裁定 1 | **已落盘**（2026-10-01，见上方 §3 树；查册门接线于 rr-registry） |
> | ~~`profiles/`~~ | ADR-0012 裁定 3 | **已落盘**（2026-10-01，`default.toml` RK3566 参考机） |
> | ~~`base/os/buildroot/` + `base/os/distro/`~~ | ADR-0013 裁定 3 | **已落盘**（2026-10-01，见上方 §3 树） |
> | payload 打包器 + CI 双矩阵 | ADR-0013 裁定 2/5 | 同一 payload 两轨同批验证 |
> | ~~机制核 `no_std` 下沉——总线语义~~ | ADR-0018 裁定 1/5 | **已落盘**（2026-10-09，`rr-core`：常量/寻址/帧组装；帧形状下沉 `rr-ipc` protocol::wire；见上方 §3 树） |
> | ~~机制核 `no_std` 下沉——模块运行时~~ | ADR-0018 裁定 1/3 | **已落盘**（2026-10-09，`rr-core` runtime：执行环状态机 + 心跳拍点 + 分发分类，`rr-bus` client 改 std 驱动器消费；net.online 格机制核切片 = `Rr3LinkState`） |
> | ~~机制核 `no_std` 下沉——控制与安全核~~ | ADR-0018 裁定 1/5 | **已落盘**（2026-10-09，`rr-core` control：限速门/低电边沿/饱和检测三件自模块面迁入 + 限位钳位新原语，rr3-act/rr3-power re-export 保路径、RPC/intent 两路落角接安全核钳位；xtensa 双绿；见上方 §3 树） |
> | `base/runtime/rr-hal-api/`（能力面契约，no_std） | ADR-0018 修订 1 ③ 接缝一 · §12.8 | **待建**（0.0.5 第 III 轮批I）：7 trait + 值类型 + `Rr3HalError` 自 `rr-hal` 提出，同步签名 / 去 `Send + Sync` / 去 `String`；`rr-hal` 退成其 Linux/sim 实现层，对外路径 re-export 不变 |
> | `Rr3ByteStream`（总线字节流窄缝） | ADR-0018 修订 1 ③ 接缝二 · §12.8 | **待定形**（批J）：机制核内两方法缝，`Rr3TransportConn` 在其上 blanket impl，`std::io` 走 adapter——落点在 `rr-ipc`（传输抽象的家）；解开 `⊙` 在 rtos/bare 两档的可达性 |
> | 描述符 `[platform]` / `[supplies]` / `[slots].present` 三段 | ADR-0018 修订 1 ④ · §4.3 | **待建**（批J）：`contracts/` 不动，扩 `profiles/*.toml` schema + `rr-registry` 查册门 + 装配段校验 |
> | 注入面 `assemble.handles` / `assemble.ops` 被装配段读 | ADR-0019 修订 1 ③ · §12.8 接缝三 | **待建**（批J）：现为 17 份全空、无读者的声明；定形后同时成为缺格降级的判据来源 |
> | 平台 port 接口（时基 / 内存 / 事件循环 / 传输） | ADR-0018 裁定 3 | 设计落点；具体 crate 划分随实施批进本树同批补画 |
> | ~~描述符扩展硬件物理绑定 + `rr-hal` 板特定缺省收口~~ | ADR-0018 裁定 4 | **已落盘**（2026-10-09，批D④：`profiles/<机型>.toml` 新增 `[hardware]` 物理绑定段两份、rrd 装配链解析后 spawn 注入 `RR3_HAL_BINDINGS`、rr-hal `descriptor` 模块合成生效绑定——描述符为底/env 应急覆盖/无则 fail-closed；五选择函数接绑定、板缺省常量与构造删除，见 §12.3 注记） |

---

## 4. 原子功能模块模型（格 · 件 · 组合件 · 面）

> 本节是 [ADR-0019](../process/adr/ADR-0019-原子功能模块模型-格件面与组合代数.md) 的架构展开；
> **逐单元的模块级细化设计**（格 / 件 / 主题 / 句柄 / 时序 / 降级 / 红线 / 敞口八面）见
> [docs/design/ 模块设计集](../design/模块设计索引.md)——本档只留系统级语义，细节在那一层。
> 裁定原文（含备选否决、rock3 化判断的注记）见该 ADR。下面只写设计语义与接缝；某原子件是否已建、
> 接线到哪一步，一律见 [`../status.md`](../status.md)。

### 4.0 架构语言：Slot / Atom / Composite / Facade

| 中文 | 英文 | 定义 | 落点 |
|---|---|---|---|
| **能力格** | **Slot** | 机器人能力的最小归类坐标，固定不变、只增不改 | `contracts/slots.toml`（现 21 格：base 5 / body 12 / cognition 4） |
| **原子件** | **Atom** | 填一个格、不可再分、只含机制的最小实现件；每个原子一个目录、板无关、经描述符注入句柄 | 源码目录（0.0.5 重铺） |
| **组合件** | **Composite** | 两个及以上原子件按 `⊗` 装配得到的组件，是执行单元的承载 | 运行期落为一个执行单元 |
| **形态面** | **Facade** | 按职责面暴露的组合件集合；**组合件的职责视图，无独立声明物**（v0.8 修订，§4.0.2） | 见下"职责四面" |

核心关系式（全档统一，不得改写语义）：

```
面 Facade = 声明（.manifest）+ 生成（样板）+ 组合件装配（Composite 承载）
格 Slot   ←固定→   件 Atom   ←可变→
功能 = 原子件 ⊗ 装配 → 组合件 ⊗ 再装配 → 形态面
```

> **读法注记（v0.8 承 [ADR-0019](../process/adr/ADR-0019-原子功能模块模型-格件面与组合代数.md) 修订 1）**：
> 第一行右半的"声明 + 生成"两件事**只到组合件层为止**——一份 `composite.toml`（声明）加一份
> `src/main.rs` + `src/composite.rs`（生成产物）。第四层"面"不再产出第四份 manifest：面的内容
> （哪段职责、走哪些主题、装哪些件）全部由组合件声明与件的归属**推出**，另立声明物即第四真值、
> 四处可漂移。关系式的语义不变——面仍由组合件装配而得；变的是它**没有自己的文件**。

统一自检句：`ServoAngleSet 是 act.servo 格上的原子件，由 actuation 组合件 ⊗ 装配承载，经运行面暴露。`

### 4.0.1 组合代数 `⊗` 与 `⊙`

| 算子 | 时相 | 边界 | 类型 | 通道 |
|---|---|---|---|---|
| `⊗` 装配 | 装配期 | 执行单元内 | `A×A→C/F`、`C×A→C/F` | 函数调用 |
| `⊙` 编排 | 运行期 | 跨执行单元 | `F×F→F_sys` | **经 transport 抽象**（承 §6、ADR-0008）：Linux=UDS，无 Linux 平台=UART/SPI/BLE 等（承 §12、ADR-0018） |

- `⊗` 五性质：类型律、闭合律、结合律、幂等律，以及**地位律 `F⊗F` 非法**——面是执行单元边界上的职责视图，
  面之间只能 `⊙` 编排；允许面直接装配即等于"一个执行单元装下所有功能"，分层失效。`F⊗F` 违反 **fail-closed 拒绝**（§13）。
- **组合一律经组合件承载，禁薄胶水串联**：薄胶水（散落 if/转发调用直接串原子、无组合件实体）无契约、无测试、
  复杂度散落、无问责；装配契约（输入/输出/生命周期/错误传播）落在组合件代码内，组合件自身要有 tests 且接线 CI，
  入口只启动组合件不直连散原子。

### 4.0.2 职责四面（固定不变）

| 面 | 职责 | 典型 |
|---|---|---|
| **接入面 intake** | 对硬件/对外的入口：RPC 请求、传感器与事件入、下行指令入 | 收 `rr3-act/servo/command` |
| **运行面 operate** | 机制执行：采集、控制环、渲染、播放、编排原子 | 跑 servo 控制图 |
| **状态面 state** | 可观测：状态流、回读、计数、配置摘要 | 发 `rr3-act/servo/state` |
| **守护面 guard** | 治理：安全核、降级、看门狗、越权拒 | 限位/过流/低电兜底（§7.3） |

**组合件跨四面、不可拆**：一个组合件天然横跨这四面，四面是它的视图，不是四个可独立拆出的执行单元。原
`rr3-sense/act/interact/power/connect` 不再是"模块"这一等概念，**降为"组合件按能力域的一种分组"**；主轴变为
格→原子→组合件→面，能力域（感知/执行/…）与运行时职责面（四）两条轴正交——这正是"形态 / 平台 / 职责"三轴
解耦（§12.7）的结构基。**四面是读图与评审的切分，不是四份交付物**（§4.0 读法注记）：一个组合件交一份
`composite.toml` 与一对入口/实现，四面从中读出。

### 4.0.3 每格一件 · 不留空格 · 格格双达标

- **每格一件**：每个能力格落一个原子件，同类件塌缩归一；格是架构承诺，**不留空格**（空格 = 该能力靠各处私搭维持）。
- **格格双达标**（v0.8 承 ADR-0019 修订 1 改判）：
  ① 落点件**有 tests 且接线 CI**（承 `conventions.md` §2 九步门，"有测试未接线 = 未达标"）；
  ② 落点件**写得出件级对拍用例，且用例有两条腿**（v0.9 承 ADR-0019 修订 2）：
     **腿一·确定性**——给定合成输入序列与注入时钟，件的公开面产出确定输出序列，无 IO、无线程、无墙上
     时钟，同输入两跑逐拍相等（范式已立在 `rr-core/src/control.rs` 的
     `same_control_graph_beat_for_beat_identical`）；**腿二·正确性**——**期望值不得由件自己产出**，
     必须引自外部真值（`contracts/events/*.schema.json`、`contracts/golden/` 线格式、或裁定写死的语义：
     限位端点、低电阈值等号、饱和半拍线、`window_ms = 0` 往"不触发动作"方向收敛），并覆盖边界
     （下界/上界/等号/零值/畸形）。**只有腿一的用例记"未达标"**——否则恒输出 0 的空件照样过门，
     门就成了对空壳件的奖励。

  判据②原来是"扇入 / 非死件"，需要全链接图加人工判断，结果在第 II 轮**零执法**（⑥ 只断测试接线，⑦
  完全不查），于是两格由 2 行纯注释件名义占住。换成对拍之后这条判据可机器计数，而且它的诊断力更强：
  **写不出对拍用例的件，必然是里面藏了时钟、线程或平台形状**——正是 `atoms/led`（自持线程 + `mpsc` +
  `recv_timeout`）与 `atoms/bt`（件内读 env）这两件的病。扇入降为评审项，仍用于准入三问第③问。
- **依赖闭包**（与判据②同批执法）：件的外部依赖 ⊆ {`core`, `alloc`, `rr-ipc` protocol, `rr-core`,
  `rr-hal-api`（§12.8 待建）}；出现 `use std::` / `use rr_hal::` 即红。这是"件 = 机制"在源码上的可扫形式。
- **扇入分档**（rock3 化，非全盘 ≥3）：`N ≥ 3` 是**新建/收编跨形态复用机制原子**的准入杠杆；单消费者的能力格
  （如 `power.battery`、`sensor.imu` 在某形态只一处用）沿用 [ADR-0012](../process/adr/ADR-0012-能力格坐标系与装配单.md)
  裁定 6 的成格判据（≥1 真实依赖者），两档不互覆盖。
- **格的三态与唯一真值点**（v0.9 承 ADR-0019 修订 2）：`contracts/slots.toml` 每格除 `layer` 外带一个
  状态标记，取 **正常 / 占位 / retired** 三值，**门只读这一处**——不许散进 conventions 散文、脚本内
  `if`、模块 README（今天 ⑦-c 的 brain 豁免就是硬编码在步骤里的反例）：

  | 态 | 含义 | 对"每格一件 · 不留空格"的影响 | 出口 |
  |---|---|---|---|
  | 正常 | 有件、有机制、有两腿对拍用例 | 计入分母，须达标 | — |
  | 占位 | 先落形状、真件未到（现 `sensor.env` / `sensor.imu` / `power.charger`） | 计入分母但**不算达标**，须登记填实触发条件 | 填实转正常 |
  | retired | 承诺作废 | **号位保留、义务解除**——退出分母，随件同批删除或标归档件 | ADR 级评审置位 |

  这一档解决制度性反例：`slots.toml` / `topics.toml` 的纪律是只增不改，而过去**没有下线出口**，被证伪的格
  只能靠空壳件永远顶着；`rr3-interact` 申报却零发布的 `asr/text`、`dialog/turn` 两条主题因此被永久冻结
  在册。**冻结的是历史号位，不是永久义务**——不分清这两件事，格表就会读起来像已兑现。数据面（加标记位）
  归 0.0.5 第 III 轮批J，本节只定语义。

### 4.0.4 五件套（组合件的物理形态）与准入三问

每个**组合件**按 `rr3-mkmod` 生成五件、缺一不可：① `.manifest`（含 `slots` 字段，缺失即门禁失败，发布即冻结）·
② 入口（只做装配与启动，**≤ 80 行**，沿用 0.0.8 雏形）· ③ 实现（机制主体）· ④ 装配清单（声明用了哪些原子件的 `⊗` 链、
要哪些句柄与参数槽——v0.8 起这份清单**被装配段校验**，见 §12.8 注入接缝）·
⑤ 唯一私有头（跨域只走公开头）。任何拟新增/保留的件过**准入三问**：① 是机制吗（不含策略/厂商/产品决策）？
② 是收敛型抽象吗（收编后总行数下降、抽象带语义而非仅签名）？③ 复用达标吗（按 4.0.3 分档）？三问全过才建件，否则就地内联。
第①问现在有可答法：**写得出逐拍对拍用例 = 是机制**（§4.0.3 判据②）。

### 4.1 manifest（module.toml）

> **与 §4.0.4 三层 manifest 的关系**：下面这份 `module.toml` 是**组合件 / owner 级**契约——对外身份、
> 能力声明、主题归属，功能守恒下不动；§4.0.4 说的 Atom（原子件）与 Facade（职责面）层 manifest 由
> 0.0.5 批B 引入。本节先给现行 owner 级字段形状。

字段字典的唯一权威是 `base/runtime/rr-registry/src/module.schema.toml`（必填项、`kind`、`rule`
与取值白名单全在那份文件里，本文档不复列）。下面这份示例只演示**字段形状**：

```toml
[manifest]
schema_version = 1
name    = "rr3-sense"       # 必须与目录名/进程名/IPC 前缀一致（校验入口见 §6.3）
version = "0.1.0"
type    = "module"          # module | brain
lang    = "rust"            # rust | python
entry   = "rr3-sense"

[provides]
capabilities = ["audio.capture", "audio.vad"]   # 能力声明：依赖仲裁的唯一依据（ADR-0002）

[depends]
required = []               # 不可满足（无提供者）→ 拒启动本模块（ADR-0009）
optional = ["net.online"]   # v0.0.1 只作声明：不进启动序、不触发降级（ADR-0009）

[events]
publishes = ["rr3-sense/audio/vad_start", "rr3-sense/audio/asr_final"]
subscribes = []

[rpc]
exposes = ["rr3-sense/audio/set_mute"]

[resources]
restart_policy  = "on-failure"   # always | on-failure | never
mem_limit_mb    = 64
start_priority  = 50             # 数值小先启动

[health]
heartbeat_ms        = 1000
watchdog_timeout_ms = 5000
```

要点：

- **能力语义的分工**：`provides.capabilities` 是依赖仲裁的**唯一依据**（模块之间只认能力名，
  不认模块名，见 §2 原则 2）。校验分两层（ADR-0009 裁定 1）：校验层只保证**语法**——
  `capability` 的形状 `^[a-z]+(\.[a-z_]+)+$`（`provides` 与 `depends` 两侧同规则）；**可满足性**
  归跨 manifest 仲裁（`rr-registry` 的 `depends.rs`，rrd boot 与 CI 全集检查共用）——
  `depends.required` 的每一项必须命中 `base_capabilities`，或被本次 boot 全集内某个
  manifest 的 `provides.capabilities` 提供。仲裁的裁定：启动序 = 拓扑序为主键、
  `start_priority` 降为层内次键；环拒整个 boot，能力不可满足只拒该模块；提供者全灭 →
  下游置 `Degraded`、任一恢复 → 解除（降级不重启、不吃预算）。裁定全文见
  [ADR-0009](../process/adr/ADR-0009-依赖仲裁-启动序与降级传播.md)；接线到哪一步属实况，
  见 [`../status.md`](../status.md)。**能力名的坐标系收敛**：可引用的能力最终收敛到
  `contracts/slots.toml` 能力格总表（§4.3、ADR-0012 裁定 1/5）——manifest 只许引用表内格位，
  "未知能力"的查册由 CI 全集层与仲裁共用同一真值实现。
- 事件/RPC 主题必须以 `manifest.name` 为前缀，越权发布一律拒绝；三个校验入口见 §6.3
- **上面的示例只演示字段形状，其中 `audio/asr_final`、`audio/set_mute` 并未登记**：照抄会被
  契约测试以"未注册主题"拒。`modules/` 下六份真实 manifest 是可直接对照的样板
- 模块间依赖走 capabilities，未来更换传感器模块不影响 brain

### 4.2 生命周期（rrd 在册表阶段机）

阶段集是**四个名字**，来自 `base/runtime/rrd/src/registry.rs` 的
`enum Rr3Phase { Registered, Running, Degraded, Failed }`；`as_str()` 的回显
（`registered`/`running`/`degraded`/`failed`）就是下行广播
`rr3-runtime/module/state` 里的 `phase` 字面量，两边同一词表。本档旧版列过的 `resolved`、
`starting` 两个阶段不在这套词表里——"依赖满足与否"是仲裁环节的**结论**，不是模块的**生命周期阶段**，
把它画成状态会让重启策略与降级判定无处安放。以本枚举为准，不另立词表。

```
Registered ──── rr3_reg_beat（心跳，刷新到期基准）────► Running
    │                                                    │
    └───────────┬────────────────────────────────────────┘
                │ rr3_reg_expire：心跳超过 watchdog_ms
                │ rr3_reg_death：连接收尾（崩溃 / 退出）
                ▼
              Failed
                │
                │ rr3_reg_death 一并算出重启计划：always 总给、on-failure 只在不干净退出时给、
                │ never 不给；退避 = RR3_RESTART_BACKOFF 倍数 × restart_base_ms，
                │ 连续失败到 RR3_RESTART_FAILURE_CAP（值 5）即永久停手
                ▼
   同身份重注册 → 新条目落 Registered、换新 task 号（旧号作废、绝不回收，整体替换旧条目）
```

- **`Degraded`** 是词表里的第四项，语义为"依赖的 base 能力未就位、或整体降级"；进出它的两条边
  由 rrd 的**降级标注环节**承担（置位与持久化同批），这一环走到哪一步属实况，
  见 [`../status.md`](../status.md)。本档只锁一条设计约束：降级标注定型时 `rr3_reg_beat`
  必须同时改成"degraded 只续命、不升级"，否则一颗心跳就把降级标注抹平。
- 心跳超时基准是 `health.watchdog_timeout_ms`，判活用相对进程起点的单调毫秒
  （`rr3_now_ms`），不存 `Instant`：不可回溯的时钟没法测"刚好超窗"。
- `rr3.unregister`（模块主动退场）走 `rr3_reg_remove` 直接移出在册表，**不走** `restart_policy`
  ——那是崩溃语义，不是退场语义。它广播的 `phase` 字面量是 `unregistered`，不属于 `Rr3Phase`；
  一条连接的退出只发一条 `rr3-runtime/module/state`，不会"死亡一条 + 退场一条"。

三条不变式（前两条与 `registry.rs` 的头注同条；上限那条钉在 `RR3_REG_MAX_MODULES` 的注释里。
细节表留给 [`../reference/rrd-registry.md`](../reference/rrd-registry.md)，本档只留判据）：

1. **task 号只增不回收**：`0` 是广播、`1` 是 rrd（`RR3_BROADCAST_DST_TASK` / `RR3_TASK_RRD`），
   模块从 `RR3_TASK_FIRST_MODULE`（值 2）起单调分配；条目 failed 或被移除后旧号作废、绝不复用
   ——否则一条迟到的旧心跳就能顶替新连接。
2. **到期判定用显式单调毫秒**，且用 `saturating_sub`：时钟倒退宁可判"未到期"，也不回绕成
   巨大差值把健康模块集体判死。
3. **在册模块上限** `RR3_REG_MAX_MODULES`（值 16）：目标 5+1 模块留余量，超额注册直接拒；
   表满但同名条目是 `Failed` 属合法的替换式重注册，不被容量门拒。

### 4.3 能力格坐标系与装配单（ADR-0012）

> 本节是设计语义；裁定全文（含备选否决与不做清单）见
> [ADR-0012](../process/adr/ADR-0012-能力格坐标系与装配单.md)。`contracts/slots.toml` 与
> `profiles/` 已于 2026-10-01 落盘（§3 树），实体不得与本节冲突。

**立格**：能力格（slot）= 一个固定命名的能力位，只描述"这里需要/可以有什么"，不描述
"谁来填"。总表 `contracts/slots.toml` 分三层——基础层（`bus.publish`/`bus.subscribe`/
`bus.rpc`/`net.online`/`power.battery`，即 `base_capabilities` 的表内化，格名随原能力名。
**"永远在位"要按承载档分两族读**（v0.9 同日三轮补，依据 = [模块设计索引](../design/模块设计索引.md) §11 审计 L-10
与 [ADR-0012](../process/adr/ADR-0012-能力格坐标系与装配单.md) 增补注记）：**base-supply**（时基 / 内存 /
事件循环 / 传输 / 看门狗，真跨档恒在，由 `[supplies]` 供）与 **base-bus**（`bus.publish`/`bus.subscribe`/
`bus.rpc`/`net.online`，**仅在有主承载层的档在位**；自足 bare 无 rrd 即无供数方，该记"不适用"而非"缺格"））、身体层（`act.servo`/`act.led`/`net.wifi`/`net.bt`/`ui.face`/
`audio.playback`/`audio.capture`/`audio.vad`/`sensor.env`/`sensor.imu`/`power.charger`）、
认知层（`cognition.dialog`/`cognition.asr`/`cognition.llm`/`cognition.tts`）。
格位命名两段式 `<域>.<格>`，与主题命名 `rr3-<模块>/<域>/<事件>`（§6.3）
不同轴。**格位成活判据**（ADR-0012 裁定 6，2026-10-03 口径修订）：新格立格须**双达标**
——≥1 个真实依赖者，或评审记录中明确的 ≥3 潜在场景；**CI 执法"测试接线"与"件级逐拍对拍"两条
（§4.0.3），扇入数不进 CI**（留在 review）；落盘格按落盘日 grandfather 豁免、不回溯执法。不留空格。

**造件**：模块只含机制（怎么采 PCM、怎么驱动舵机），策略不进模块（§2 原则 4）；
模块对外的身份是"我 provides 哪些格位"。

**装配单 / 描述符**：一台机器人的选型与绑定收敛为一份 `profiles/<机型>.toml` 纯数据文件。
定形为**七段**（v0.9 订正：v0.8 写"六段"漏了既有的 `[os]`——`rrd/src/profile.rs` 与查册门测试都硬要
`[os].route`，段数与代码不符会让批J 第一步就撞门。承
[ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md) 修订 1/修订 3，§12.3），
每段各自可变，于是"换底板 / 换芯片 / 裁剪形态 / 换轨道"各只改一段：

| 段 | 内容 | 可变轴 |
|---|---|---|
| `[os]` | `route = distro \| buildroot`（ADR-0013 双轨） | 装机轨道 |
| `[platform]` | **芯片平台档**（SoC/MCU 型号档 + 承载档 host/rtos/bare）——决定后端族与供给形状 | 换芯片 |
| `[supplies]` | 四类供给的取值：时基 / 内存模型 / 事件循环形态 / 传输实现（§12.2） | 换平台 port |
| `[slots]` | `present` = 本板物理在位集——缺格从此是**声明**，不是由启用集反推 | 硬件在位性 |
| `[bindings]` | 格位 → 件（哪个原子件填哪个格） | 换实现件 |
| `[enabled_slots]` | 模块 → 本形态启用的格位集合 | 裁剪形态 |
| `[hardware]` | **底板走线**：引脚 / 控制器编号 / 总线 / 设备名（现 9 键） | 换底板 |

`[platform]` 与 `[hardware]` 必须分开：一颗芯片可以配若干底板，把两者混在一段里，换底板就要改平台档、
换平台就要重写引脚——"板 = 数据"的分工就散了。**注**：`enabled_slots` 原名 `[faces]`，为避免与
§4.0.2 的"职责面 Facade"撞名，已随批A 改名（2026-10-08）；它表达的是"启用哪些格"，不是职责面。
`profiles/*.toml`、§3 目录树、`rr-registry` 查册门字段、`rrd` 解析与既有引用已同批连带更新
（ADR-0019 裁定 1 消歧、R16 兑现）；`[faces]` 仅存于历史存档节（不改写）。**七段中现只落 `[os]` /
`[bindings]` / `[enabled_slots]` / `[hardware]` 四段**，`[platform]` `[supplies]` `[slots].present`
三段随 §12.8 注入接缝一起在第 III 轮批J 落（进度见 status.md）。

**机型集是钉死的（v0.9 写实，防止批K 撞门）**：查册门测试 `rr-registry/tests/contract.rs` 断言
`profiles/` 目录的 `.toml` 集合恰为 `[default.toml, distro.toml]`（"profiles/ 机型集合漂移"即红）。
所以新增任何机型文件（含第二形态样例 `esp32-mini.toml`）**必须同批改这条 pin**——它不是"数据新增"，
它是契约面变更。同理，`rrd/src/profile.rs` 的段类型校验与 `tools/rr3-pack` 的装机注入
（今天硬编码只读 `profiles/distro.toml`，无 `--profile` 参数）都在连带面内：**"换板只改装配单"这句话
今天还立不住**，因为出包链只认一份装配单。这一处列为批J 前置（方案 §13.1 C2）。

**参数真值的落点与合并规则（v0.9 新立）**：件与组合件的参数分落两文件——`modules/<mod>/config.toml`
（设备行为手感）与 `profiles/<机型>.toml`（板与形态事实）。注入面定形后 ops 槽会同时指向两处，冲突
必须有规则，否则"改参数改哪个文件"成为新的日常模糊带：

| 参数属谁 | 落点 | 判据 |
|---|---|---|
| 换这块**板**要改的（引脚 / 总线 / 设备名 / 时钟源 / 传输） | `[hardware]`、`[supplies]` | 与形态无关、与件无关 |
| 换这个**形态**要改的（启用哪些格、件用哪个句柄） | `[slots]`、`[bindings]`、`[enabled_slots]` + 件的 `assemble.toml` | 描述"这台机器该有什么" |
| 换**手感**要改的（阈值 / 周期 / 音量 / 灯效 / 对话人格） | `config.toml` 或 brain 策略配置 | 描述"这台机器怎么做" |

**同名键以 profile 为准并留痕**（profile 是装机事实源，config 是模块默认）；两处给了不同值时 boot 期
打一行"覆盖生效"的痕，不静默取其一。还有一条硬事实必须写明：**装机包今天只带 `module.toml` 与
`config.toml`**（`tools/rr3-pack` 的 staging 只复制这两件，`atoms/`、`composite.toml`、`assemble.toml`
全不进包）。所以"运行期由装配段校验件的句柄与参数槽"在真机上**无物可读**——件级注入声明的去向必须在
批J 开工前裁定（三选一：随包附生成的件清单 / 合并进 `module.toml` / 降级为纯构建期门），
否则 L3 只在仓内成立（方案 §13.1 C1，这是第 III 轮最硬的一条前置）。**rrd 启动装配链**：
读 profile + 硬件探测（`/proc/device-tree`、`/sys`、ALSA 枚举）→ 匹配 → 依赖仲裁
（§7.1、ADR-0009）→ **缺格置 Degraded**（fail-closed，不新增相位）。profile 只声明
"什么格位、由谁、给哪个形态"，不写启动序（那是仲裁输出）。
读 profile + 硬件探测（`/proc/device-tree`、`/sys`、ALSA 枚举）→ 匹配 → 依赖仲裁
（§7.1、ADR-0009）→ **缺格置 Degraded**（fail-closed，不新增相位）。profile 只声明
"什么格位、由谁、给哪个形态"，不写启动序（那是仲裁输出）。

**分界**：⊗装配（跨格位静态绑定，boot 前，profile + rrd）与 ⊙编排（运行期动态协调，
brain）分立；**F⊗F 非法**——brain 不许运行期决定能力归属，rrd 不许运行期重排装配。

**门禁三项**（fail-closed）：①模块互依赖成环拒整个 boot（ADR-0009）；②格位引用越表拒
（manifest 与 profile 双侧对 `slots.toml` 查册，与仲裁共用真值）；③机制/策略越界为
扫描点（模块互不依赖既有纪律 + F⊗F，v0.0.1 评审项 + lint 后置）。

---

## 5. HAL 分层（rr-hal）

本节只写**接口形状与设计裁定**。trait 名的权威落点是 `base/runtime/rr-hal/src/lib.rs`，
后端标识常量与"哪个后端承载什么"的表在 `base/runtime/rr-hal/src/backend.rs`；
两者之外的任何"某后端已经有实现/还没有实现"都不属于本节，见 [`../status.md`](../status.md)。

> **两层形状要分清（v0.8 承 ADR-0018 修订 1）**：下面这份 `#[async_trait]` + `Send + Sync` 的签名是
> **承载层实现形状**（Linux 侧要并发、要跨线程移交句柄，async 在 ADR-0008 裁定 1 里本来只许进模块侧，
> 这里就是模块侧）。机制核侧另有一张**能力面契约** `rr-hal-api`（§12.8 接缝一，待建）：同步签名、
> 去 `Send + Sync`、值类型去 `String`。原子件与无承载档只认后者——**件不许把 std 的调度形状当成机制**。
> 现况是原子件仍在直接消费本节的 async trait（`rr3_block_on` 自旋），这一层分离随第 III 轮批I 收口。

```rust
// [IND] 独立层：模块只见 trait，不见 Linux 细节。
// trait 名带 Rr3 前缀、方法名不带（naming.md §2），错误统一 Rr3HalError。
#[async_trait]
pub trait Rr3AudioSource: Send + Sync {            // 上行拾音
    async fn open(&self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError>;
    async fn read(&self, frames: usize) -> Result<Rr3PcmStream, Rr3HalError>;
}
#[async_trait]
pub trait Rr3AudioSink: Send + Sync {              // 下行播放（与 source 对称，M3 定型）
    async fn open(&self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError>;
    async fn write(&self, pcm: Rr3PcmStream) -> Result<(), Rr3HalError>;
}
#[async_trait]
pub trait Rr3GpioOut: Send + Sync {                // 数字输出（继电器、LED）
    async fn set(&self, level: bool) -> Result<(), Rr3HalError>;
    async fn level(&self) -> Result<bool, Rr3HalError>;
}
#[async_trait]
pub trait Rr3PwmServo: Send + Sync {               // PWM 舵机（闭环在 act 反射环内）
    async fn set_angle(&self, deg: u16) -> Result<(), Rr3HalError>;
    async fn angle(&self) -> Result<u16, Rr3HalError>;
}
#[async_trait]
pub trait Rr3Power: Send + Sync {                  // 电源读数
    async fn battery(&self) -> Result<Rr3BatteryState, Rr3HalError>;
}
#[async_trait]
pub trait Rr3Link: Send + Sync {                   // 网络链路读数
    async fn state(&self) -> Result<Rr3LinkState, Rr3HalError>;
}
```

配套数据类型也全部带 `Rr3` 前缀：`Rr3AudioCfg`（默认常量 `RR3_AUDIO_CFG_16K_MONO16` =
16k/单声道/16bit）、`Rr3PcmStream`（时长用 `rr3_pcm_duration_ms()`）、`Rr3BatteryState`、
`Rr3LinkState`，以及错误枚举 `Rr3HalError`（变体 `Unsupported(&'static str)` / `Busy` /
`InvalidParam(String)`）。占位实现 `Rr3UnavailableSource` 让六个 trait 的
每个方法都回 `Rr3HalError::Unsupported`——它承载的是设计裁定"**能力缺失必须显式回错，不许静默退化**"，
不是一时代替后端的临时件；object-safety 由 `lib.rs` 的 `dyn` 装箱用例在 `cargo test` 时机器校验
（注册表要持有异构设备集合，trait 对象装不进盒子就是接口设计错了）。

分层与落点（**只写位置，不写进度**）：

| 层 | 位置 | 承载 |
|---|---|---|
| traits + 数据类型 | `rr-hal/src/lib.rs` | 上面六个 trait 与配套类型，模块只见 trait、不见 Linux 细节；另有 `rr3_block_on`（同步面进 async 世界的唯一桥，ADR-0008 同步优先） |
| 后端标识常量 | `rr-hal/src/backend.rs` | `RR3_BACKEND_{LINUX,SIM,PLACEHOLDER}`，写入日志与诊断主题 `rr3-runtime/module/state` |
| linux 后端 | `rr-hal` 内 `linux` 后端 | 落点定在 `rr-hal`，承载面见 `backend.rs` 的表（那张表的词表本档不复述） |
| sim 后端（主机联调） | `rr-hal` 内 `sim` 后端 + `tools/sim` 的虚拟设备 | 让 CI 与无硬件开发共用同一套 trait |
| `shm` 加速路径 | `rr-ipc` 的传输层（不在 HAL） | 实时控制面的零拷贝通道，见 §7.4 与 ADR-0007 |

两条不可让的裁定：

1. **反射环不进 HAL**：1k–10kHz 的电机 FOC/PID 闭环留在 `rr3-act` 进程内，永不进 trait、
   更不进总线（ADR-0007 §裁定 1）。HAL 只承载慢速事件级接口。
2. **播放面与拾音面对称**：`Rr3AudioSink`（M3 定型，`open`/`write` 两方法，与 source 的
   `open`/`read` 对称）。同步模块代码经 `rr3_block_on` 进出 async trait；sim 后端写在内存
   尾窗（不落盘），linux 后端走 ALSA（feature 门，板上件）。

新增 trait/后端时本节与 `rr-hal` 源码同批更新。谁在本节写下"某后端已可用"，就是把实况塞进了
设计文档——那类句子一律落 [`../status.md`](../status.md)。

---

## 6. IPC 与事件总线（Airy IPC，ADR-0006）

### 6.1 协议：采用 Airy IPC 线格式

- **128B 定长消息头**（Layout C，L1 永久稳定）+ **5 种 payload 类型**：REQUEST/RESPONSE（RPC）、EVENT（事件总线）、STREAM（音频/运动数据流，有序投递）、NOTIFICATION（心跳/系统通知）。枚举是 `Rr3PayloadType`（`Request` = 0x0001 起连续取值），字节布局的唯一事实源是 `contracts/envelope.schema.json`
- 事务面 payload 默认 JSON-RPC 2.0；rock3 的 topic 字符串与 `phase` 置于 payload 内，不占用消息头 reserved 区。`phase` 的字面量就是 §4.2 那张词表（`registered`/`running`/`degraded`/`failed`），由 `Rr3Phase` 的 `as_str` 给出；模块主动退场那条广播额外用 `unregistered`
- RESPONSE.status 采用 AIRY_E* 子集（枚举 `Rr3StatusCode`）；`src_task` 由 rrd（受信运行时）在模块注册时分配并校验——用户态自设的 `src_task` 属协议违例，那一帧被拒但连接不断（AOS-STD-IPC-024，接线见 `conn.rs` 与 §4.2 的 task 号不变式）
- capability 令牌、内核强制等 AirymaxRT 运行时机制不引入（单机可信环境），具身化时再评估（ADR-0007）

### 6.2 rr-ipc 分层

```
rr-ipc/
├── protocol/     ← 128B 头与 payload 类型前缀的纯编解码，与传输无关（Rust/Python 对齐同一 schema）
│   ├── header.rs    ← `Rr3Header::from_bytes` / `to_bytes`、`MAGIC`、`RR3_IPC_HDR_SIZE` = 128
│   ├── body.rs      ← rr3_body_encode / rr3_body_split：前缀宽度 RR3_BODY_TYPE_PREFIX_LEN = 2（u16 小端）
│   └── mod.rs       ← 声明 header / body 两个子模块；对外名字由 crate 根 lib.rs 统一再导出
└── transport/    ← frame.rs：Rr3Frame 的整帧读写；mod.rs：传输分层注记与两个路径常量
```

- **protocol/**：平台无关到"用 `std::io::Cursor` 就能在任意主机跑满单元测试"的程度。
  payload 前缀值不在冻结枚举里即 `Rr3BodyError::UnknownType`——0x0006 起属未来追加，
  现在必须拒，而不是当成未知类型放行。
- **transport/frame.rs**：一帧 = 128B 头 + `payload_len` 字节 payload。
  读侧 `rr3_frame_read` 把五种失败分得清清楚楚：`Rr3FrameError::{Closed, TornHeader{got},
  TornPayload{want, got}, Header(Rr3HdrError), Io(String)}`——"一个字节都没读到（对端干净关闭）"
  与"头读了一半就 EOF"不是一回事，半帧永远不许被当成新帧起始；头本身的非法（magic / reserved
  非 0 / 超长）由 `Rr3HdrError::{TooShort, BadMagic, ReservedNotZero, PayloadTooLarge}` 上抛。
  已读字节数由 `read` 的计数给出而非扫零——payload 里合法地会出现 0 字节。
  写侧 `rr3_frame_to_bytes` **先**校验头声明的 `payload_len` 与实际 payload 等长
  （不符即 `Rr3FrameError::LengthMismatch{declares, actual}` 且一个字节都不发出去），
  再把头与体拼成整块交 `rr3_frame_write` 单次 `write_all` + `flush`——读侧因此不可能看到两半。
- **transport/mod.rs**：trait 三件（ADR-0008）——连接侧 `Rr3TransportConn`（同步阻塞、
  **所有权移交**：`rr3_send_buf` 消费整帧缓冲、幂等 seal 后单次 `write_all`；`rr3_recv_buf`
  返回所有权缓冲）、监听侧 `Rr3TransportListener`、端点 `Rr3TransportEndpoint`（清理残留 →
  bind）。零拷贝语义落在 `transport/buf.rs` 的 `Rr3FrameBuf`（单块内存 `[0..128)` 头预留区 +
  payload，原地编头）而不是方法签名上——M5 的 shm 在同一签名下把拷贝降为环形缓冲 grant，
  接口不再推翻（这就是 ADR-0007 §裁定 3 的落点）。裁定 1（ADR-0008）：传输层保持同步阻塞
  线程模型，不引 async；async 运行时只进模块侧。
  分层设计是 uds（事件/事务面，M2 唯一实现 `transport/uds.rs` 的 `Rr3UdsListener`）/
  shm（实时控制面，M5）/ tcp（多机，预留），反射环永远不进任何一层（ADR-0007 §裁定 1）；
  各层的实现进度见 [`../status.md`](../status.md)。**这三层都是同一条 transport trait 的
  实现**——不带 Linux 的平台以 UART/SPI/BLE 等实现同一 trait，跨操作系统口径见 §12 与
  [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md) 裁定 3。

套接字路径的三级决定：`--socket` > 环境变量 `RR3_BUS_PATH`（常量键 `RR3_BUS_PATH_ENV`）>
`RR3_UDS_PATH_DEFAULT`（自有路径常量，避免与真实 Airymax 运行时同机冲突）。
横幅回显的路径在打印**之前**就已绝对化，与 `UnixListener::bind` 取同一个值——两处各说一套比直接
报错更难查。参数与默认值的字面量归 [`../reference/rrd-cli.md`](../reference/rrd-cli.md)（本档不复述）。

### 6.3 主题命名

格式 `rr3-<模块>/<域>/<事件>`，全小写 snake_case。下面这份草图表达的是**命名域的切分**
（**示意，非注册表**）：

```
rr3-sense/audio/vad_start      rr3-sense/audio/asr_partial   rr3-sense/audio/asr_final
rr3-brain/dialog/response      rr3-brain/dialog/error
rr3-act/relay/set              rr3-act/relay/state          ← 示意：继电器面见下方对账
rr3-interact/audio/play_done
rr3-power/battery/state        rr3-power/battery/low         rr3-power/charger/event
rr3-connect/wifi/state         rr3-connect/bt/state
rr3-runtime/module/state       rr3-runtime/system/heartbeat
```

> **§6.3 对账注记（0.0.4 批D）**：上图 `rr3-connect/wifi/state` 仍属**示意**——
> 配网面实际登记为 `rr3-connect/wifi/scan|connect|forget` 三 request 主题，
> 配网态经 `net/link` 事件体的 `mode` 字段（ap|station）发布；`wifi/state` 单名
> 与 `rr3-connect/bt/state` 均未登记，使用前须先 append（§7.2 同注）。

> **裁定**：base 运行时主题命名空间用 `rr3-runtime/`（如 `rr3-runtime/module/state`、
> `rr3-runtime/system/heartbeat`），不用 `base/` 前缀——契约要求每段主题首段等于某个
> `[manifest].name`，而模块名规则是 `^rr3-[a-z]+$`。`rr3-runtime/*` 因此是**内建命名空间**：
> 它是 rrd 的下行广播，模块上行冒充必须被拒（常量 `RR3_RESERVED_OWNER`）。
> 全量已登记主题以 [`../../contracts/topics.toml`](../../contracts/topics.toml) 为准，
> 本档既不抄表也不抄计数。

**校验入口有三个，职责各不相同**：

1. **`rr3-mkmod` 生成自检**（`tools/mkmodule`）：骨架落盘前先自查前缀与形状，让不合规的模块
   生成不出来。
2. **`rr-registry` 契约测试**（`cargo test -p rr-registry`，用例面在
   `base/runtime/rr-registry/tests/contract.rs`）：按 `module.schema.toml` 逐字段与跨字段规则
   校验每份真实 manifest，含"引用了未注册主题即拒"。
3. **rrd 在册表判定**：`RrdRegistry::rr3_reg_publish_check(sender_name, topic, topics)`，
   返回 `Result<(), Rr3PublishDeny>`。它是**四道门按固定顺序**：
   ① `rr3-runtime/*` 冒充 → `ReservedNamespace`（最严重的定性必须拿得到，故排在最前）
   ② 主题首段 ≠ 发送方 → `ForeignPrefix`
   ③ 主题不在注册表 → `NotRegistered`
   ④ 在册 owner ≠ 发送方 → `ForeignPrefix`（首段相符只是**语法**事实，owner 相符才是**登记事实**；
   owner 取不到同样按越权拒，绝不因"查不到"而放行）。
   判定是关联函数、不碰在册表，所以 **`sender_name` 必须由调用方按这条连接持有的 rrd 在册身份取得**
   （注册握手时确立的登记名，而非从帧体现读），**绝不得取自帧体或注册报文里自称的 name 字段**——
   这条前提本函数替不了调用方保证。越权只计数不牵连：`rr3_reg_note_denied` 饱和加，既不改发布者生命周期
   状态也不影响其它连接的转发。返回值与错误映射的细节表留给
   [`../reference/rrd-rpc.md`](../reference/rrd-rpc.md)（在册表侧的门序与常量见
   [`../reference/rrd-registry.md`](../reference/rrd-registry.md)）。

入口 ③ 给的是**判定逻辑**；把它接到线上流量（UDS 受理路径上的发布分支）是 `conn.rs` 的职责，
"接到了哪一步"属实况，见 [`../status.md`](../status.md)。上面的代码块是**设计期草图**，
`contracts/topics.toml` 才是事实源，两者不一致时以注册表为准；注册表按"只增不改"演进，
各模块其余主题随里程碑逐条 append，不预先占位。登记之前，任何 manifest 的
`events.publishes`/`rpc.exposes` 引用未注册主题都会被入口 ② 拒绝。

> **`rr3-act` 的对账**：注册表里 `rr3-act` 的**已登记面是舵机、电机与继电器**
> （`rr3-act/servo/command` request、`rr3-act/servo/state` stream、`rr3-act/motor/reflex`
> notification、`rr3-act/led/effect` request、`rr3-act/relay/set` request、
> `rr3-act/relay/state` event，owner 角色写作"执行：舵机/电机、反射环宿主进程"）。
> ADR-0002 §1 把 rr3-act 的职责写作"继电器、未来运动控制（gpiod、PWM）"——继电器那一半已于
> 2026-10-04 按 0.0.2 方案 Q1 裁定立格（`act.relay`，立格判据真实依赖者 = M5 验收"语音控继电器"），
> `rr3-act/relay/set`（request）已同批 **APPEND** 进 `contracts/topics.toml`（形状 pin 同批更新）；
> 2026-10-05 又按 0.0.3 方案 T3 补裁定 **APPEND** `rr3-act/relay/state`（event，L2）——意图执行
> 效果的可观测面，RPC 与 intent actions 两条驱动路共用同一条状态流，本节草图与 §7.2 数据流图
> 里的 `relay/state` 已是在册主题（不再走"先 append 再引用"的待办）。
> ADR-0002 的决策正文不改（继电器本身没有被本批裁定否定），在册与否只查注册表。

---

## 7. 关键数据流

### 7.1 启动链

> 本节链条描述的是**具备操作系统的主承载层**的启动形态（`kernel → init → rrd → …`）。不带
> 操作系统的平台不经过 `init`/`rrd`，改由机制核自持启动——上电后 port 探测 + 描述符绑定 +
> 执行单元起来，见 §12 与 [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md) 裁定 2/3。
> 本节 ⑥ 的"非 Unix 退出"是 `rrd`（主承载层守护进程）当前的事实边界，不是整个底座的边界。

设计上的完整链条（目标形态；装配环由 rrd 的 profile 装载/探测/匹配环节承担，其余四环由
装载/仲裁/spawn/监督环节承担；装配环的设计落点见 §4.3 与 ADR-0012 裁定 3，实体尚未落盘）：

```
kernel → init → rrd → 读装配单 + 硬件探测（格位匹配，缺格 Degraded） →
加载 manifests → 依赖仲裁（能力格坐标系，拓扑序） → 拉起模块 → 心跳监督 → 事件总线就绪
```

`rrd` 自己这一段的契约是**定死的顺序**，`base/runtime/rrd/src/main.rs` 与
`tests/socket_banner.rs` 一起把它钉住——每一步的失败都当场以非 0 退出，绝不静默回落：

```
① 解析 CLI（`cli::rr3_cli_parse`）         失败 → stderr 打用法 + 非 0 退出
② 套接字路径绝对化                          失败 → 致命（横幅与 bind 必须共用同一个值）
③ 叠加层路径绝对化（若给了）                 失败 → 致命（横幅是"叠加层已生效"的可见凭证）
④ 装载 contracts/topics.toml + 叠加层       失败 → 逐条点名后非 0 退出
⑤ 打印横幅（契约/manifest schema 版本、总线绝对路径、装载到的主题数）
   主题数取自装载结果，不写死；给了叠加层就再打一行绝对路径与新增条目清单
⑥ 平台分岔：
   Unix     → `rr3_daemon_run`：清残留套接字 → `UnixListener::bind` → 起唯一的 watchdog
              监视线程 → accept 循环，
              每连接 2 条线程 + 2 个 fd（`daemon.rs` 在 accept 处 spawn reader，
              `conn.rs` 为同一条连接再 spawn writer 并自带一份 `try_clone`）
              （该环节永不返回，除非 bind 失败）
   非 Unix  → 往 stderr 打 `RRD_UNSUPPORTED` 并以非 0 退出
```

两条是设计约束、不是实现细节：

- **④ 在两个平台都做**。契约坏了要当场看见，而不是"等不到 UDS"——所以非 Unix 平台同样先装载、
  先打印横幅，之后才声明不支持。横幅因此是"装载成功"的凭证，不是"总线可用"的凭证。
- **②③ 的绝对化失败即致命**。按 CLI 原词回显会让运维按自己 shell 的 CWD 去连，而 daemon 绑的是
  rrd 的 CWD 相对路径——两处各说一套比直接报错更难查。

①–⑤ 与 ⑥ 的 Unix 分支属于 rrd 的**装配层**职责；目标链条里"加载 manifests → 依赖仲裁 →
按 `start_priority` 拉起 → 心跳监督"这四环落在 spawn/仲裁/监视环节上，本档不写它们到哪一步
（见 [`../status.md`](../status.md)）。`--manifest` 传入的路径在拉起时按依赖拓扑序重排
（层内按 `start_priority` 升序，[ADR-0009](../process/adr/ADR-0009-依赖仲裁-启动序与降级传播.md) 裁定 2）；
注册环节本身**只拒语法**——`entry` 的绝对性在握手处查（相对路径即 `Rr3StatusCode::Invalid`，
映射 `AIRY_EINVAL`，并点名字段），路径的存在性与可执行位留给 spawn 那一刻，在册表不替文件系统猜。

### 7.2 对话主闭环（legacy main.c 对话策略的分布式版）

> **草图（示意）**：下图只表达数据流向。其中的主题串**以 `contracts/topics.toml` 为准**——
> `rr3-act/relay/state`、`rr3-power/battery/low` 已登记（前者 0.0.3 方案 T3 补裁定 append），
> `rr3-brain/dialog/response`、`rr3-connect/wifi/state` 仍**未登记**（对账见 §6.3 的
> `rr3-act` 注记），使用前须先 append 契约。注：0.0.4 批D 起配网面以
> `rr3-connect/wifi/scan|connect|forget` 三 RPC + `net/link` 事件体的 `mode` 字段
> （ap|station）承接，`wifi/state` 这个单名依旧不在册（登记与否以 topics.toml 为准）。

```
                    ┌─────────────── rrd 事件总线 (UDS) ───────────────┐
  麦克风 ► rr3-sense ─┤ rr3-sense/audio/vad_start ──► rr3-brain (Python) │
  (ALSA)             │          asr_final               │ 云端 ASR/LLM/TTS
                     │                                  ├─ rr3-brain/dialog/response
  喇叭   ◄ rr3-interact ◄───────────────────────────────┤
  (ALSA)             │◄─ rr3-act/relay/state ─── rr3-act (GPIO 继电器)
                     │
  电池 ◄── rr3-power ─► rr3-power/battery/low ─► rrd（低电保护，横切）
  WiFi/BT ◄ rr3-connect ─► rr3-connect/wifi/state ─► 各模块（降级判断）
```

### 7.3 安全兜底（不依赖 brain）

急停、限位、低电保护逻辑在 rrd/rr-hal：即使 rr3-brain 崩溃或未注册，rrd 仍监听
`rr3-power/battery/low` 并直接执行保护动作。这是**设计意图层面的硬要求**——安全兜底不能挂在
"编排者活着"这个前提上（§2 原则 1：基座存活则安全兜底存活）。

三条落地约束：

- 保护动作**不进 brain、不进模块**：走 rrd 进程内的路径，最坏情况下模块全灭也要能执行。
- 反射环（限位这类 1kHz 闭环）永不进总线（§7.4、ADR-0007 §裁定 1），因此这一层的兜底与
  传输层可用性无关；跨进程的那一层才依赖 rrd 的下行广播（`rr3_state_frame` →
  `rr3-runtime/module/state`）与扇出（`rr3_reg_fanout`）这条通路。
- **主题串以 `contracts/topics.toml` 为准**：本句里的 `rr3-power/battery/low` 是草图用串，
  在册与否只查注册表；要用它做兜底触发，必须先按"契约只增不改"把它 append 进注册表、再让 rrd
  订阅——顺序反了就会被 §6.3 入口 ② 拒。

这条兜底的实装进度（哪一段通路已接通、哪一段还缺）见 [`../status.md`](../status.md)。

### 7.4 控制面三级分层（ADR-0007）

| 层 | 频率 | 承载 | 典型内容 |
|---|---|---|---|
| 反射环（进程内） | 1k–10kHz | 不进总线：act 进程内本地闭环 | 电机 FOC/PID、平衡控制 |
| 实时控制面 | 100Hz–1kHz | A-IPC over 共享内存 + `F_ZEROCOPY`/`F_URGENT` | 关节状态、力矩指令、音频 PCM 流 |
| 事务面 | <100Hz | A-IPC over UDS + JSON-RPC | 事件、RPC、对话、配置 |

反射环永远不出进程——安全闭环不依赖任何总线；总线只承载指令与状态摘要。

---

## 8. legacy 迁移映射

| legacy（已冻结） | 去向 | 方式 |
|---|---|---|
| `legacy/core/audio` | rr3-sense（采集+VAD）/ rr3-interact（播放） | HAL 换 ALSA，VAD 逻辑移植 |
| `legacy/core/asr` · `legacy/core/llm` · `legacy/core/tts` | rr3-brain/cloud/ | Python 直接移植 HTTP 逻辑（含 Content-Length 等踩坑） |
| `legacy/core/relay` | rr3-act | gpiod |
| `legacy/core/link/wifi` · `legacy/core/link/btctl` | rr3-connect | NetworkManager(D-Bus) / BlueZ |
| `legacy/core/panel` | rr3-interact/panel | HTML 资产直接搬入 |
| `legacy/core/applog` · `legacy/core/data` | base/runtime | 日志与计数是基座职责 |
| `legacy/firmware-c/main/config.h` | rr3-brain/config.toml | 对话策略参数平移 |
| `legacy/firmware-c/main/main.c` | rr3-brain/dialog.py | 编排逻辑移植 |

> 路径以迁移后（`e98a3d1`）的真实位置为准：`core/`、`firmware-c/` 已整体位于 `legacy/` 下。
> **消化已完成（2026-10-05 0.0.4 批I）**：逐组件语义全部收敛（panel/data 批C/B 实装、
> btctl 批E 骨架+退役声明，证据见 [`../process/legacy-digest.md`](../process/legacy-digest.md)），
> `legacy/` 整树随退役批删除（git 历史 + tag `v0-pre-legacy` 双锚点可回溯）。
> 本表保留"去向"设计语义，不随目录消失失效；行内 `legacy/...` 路径自此指 git 历史位置。

---

## 9. Airymax 接入路径（摘要，详见 ADR-0004）

- 接入点定在**运行时层**：rock3 已采用 Airy IPC 128B 线格式作为 [SC] 契约（ADR-0006），与 AirymaxRT 字节级同格式，经 JSON-RPC 对接 gateway 的成本进一步降低
- OS 层每半年复查 Airymax distro 仓的 aarch64/headless 产物，有则评估切换
- sdk-rust（Apache-2.0）可作 rr3-connect 试点依赖
- 许可证走 Apache-2.0 路径；memoryrovol（商业 EULA）不引入

---

## 10. 里程碑

里程碑的内容、验收标准与每条的实装状态合并登记在 [`../status.md`](../status.md) 的
"里程碑进度"一节；本档不另立一份表，也不复述验收标准。设计文档只负责回答"往哪切"，
"走到哪"只有一个落点。

---

## 11. 风险登记

| # | 风险 | 影响里程碑 | 缓解 |
|---|---|---|---|
| R1 | 保底轨 Buildroot + RK3566 DTS/音频补丁工作量超预期 | M1 / 保底轨 | 快路上线后保底轨冻结在 Radxa ZERO 3 参考板，自绘板后置；新板优先走发行版快路（ADR-0013） |
| R2 | ALSA 全双工延迟不达标 | M3 | legacy 闭环 2~5s 为基线，Linux 侧硬件换血预期更好；先实测再优化 |
| R3 | Python brain GIL/冷启动 | M4 | 策略预载；必要时 brain 拆子进程 |
| R4 | Rust↔Python 契约漂移 | M2+ | `contracts/` 目录 [SC] 唯一事实源；漂移由**三个入口**分层拦：`rr3-mkmod` 生成自检、`rr-registry` 契约测试、rrd 在册表的 `rr3_reg_publish_check`（三者职责与顺序见 §6.3）。前两个在提交前跑，第三个在线上判定；它接到线上流量的哪一段属实况，见 [`../status.md`](../status.md) |
| R5 | 0.5L 体积散热（RK3566 被动散热） | 硬件 | 选型时评估无风扇功耗档，后期实机温测 |
| R6 | A-IPC 自实现运行时与标准偏差 | M2 | IPC-T-001~003 必测；半年复查时对照标准版本变更 |
| R7 | 待机 <1W 需 suspend/电源状态机与 OS 深度协同 | M5 | rr3-power 与 rrd 电源状态机设计前置，模块声明 wake 权限 |
| R8 | 快路发行版变数（glibc/systemd/内核版本差异）导致 payload 行为漂移 | 双轨 CI | 同一 payload 双矩阵执法（ADR-0013 裁定 5）：契约与冒烟用例两轨同一套，不带病上板 |
| R9 | OCI 分发引入供应链面（镜像来源、依赖冻结后的漏洞积压） | Layer 2 分发 | 镜像来源白名单 + 版本 pin；编排权仍归 rrd（ADR-0013 裁定 4），容器不扩大故障域 |
| R10 | 机制核 `no_std` 下沉的工程量与内存模型约束（无全局分配器假设） | §12 / ADR-0018 | 先划清 `no_std` 可达线（字节层已下沉），总线语义/运行时/控制/安全分层批推进；静态/定长区先覆盖裸机路径 |
| R11 | 控制原语集为覆盖各种大小的机器人而膨胀，侵蚀"稳定"这一硬要求 | §12.5 / ADR-0017 | 原语集闭合、正交、只增不改；"更多轴/更大机器"应表现为组合增多而非新增原语；扩集走契约级评审 |
| R12 | 非 Linux 平台的实时与安全可移植性（定长调度、注入时钟）验证不足 | §12.5 | 控制与安全核做成纯函数 + 时钟注入，逐拍一致；跨平台点验矩阵把这点固化为机械下限 |
| R13 | "跨平台"被误读为"同一份 payload 装到任何板" | §12 | 反复申明：跨的是机制核，主承载层按平台能力叠加；非 Linux 跑机制核切片不是跑全量 rrd |
| R14 | `rr-hal` 板特定缺省（树莓派引脚/总线）不收口，则"板 = 数据"不成立 | §12.3 / ADR-0018 裁定 4 | 物理绑定移入描述符、env 降为应急、代码缺省改 fail-closed 不回落某板引脚；此项先于跨平台宣讲落地 |
| R15 | 原子化是一次全仓源码结构重铺（五模块 + brain → 原子目录），爆炸半径大 | §4 / ADR-0019 / 0.0.5 | 由 0.0.5 分批施工、每批带验收四件套 + 功能守恒核对；不一把梭；对外 topics/契约面零改动为红线 |
| R16 | `[faces]`→`enabled_slots` 改名触及 profile、`rr-registry` 查册门、既有 dev 方案与入链 | §4.3 / ADR-0012 | 改名与查册门、README 导航、dev 方案引用同批连带修；先改数据事实源再改引用，CI 查册门兜底防漏 |

---

## 12. 跨操作系统移植与部署

> 本节把 §1 与 §2 原则 5 的"跨操作系统快速部署"落到结构上。裁定原文见
> [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md)，这里是它的
> 架构叙述与与既有各节的接缝。本节只写设计语义；机制核下沉到哪个版本、哪一层已 `no_std`，
> 一律见 [`../status.md`](../status.md)。

### 12.1 两层切分：机制核 与 主承载层

底座切成两层。**切线的判据不是"要不要操作系统"，而是"四类供给齐不齐"（§12.2）**——RTOS 也是
操作系统，一块跑 FreeRTOS 的 MCU 有 OS 却没有主承载层，用 OS 有无画线会把它漏在档外。

| 层 | 组件（crate 级外延，v0.8 定名） | 属性 | 稳定性 |
|---|---|---|---|
| **机制核** | 字节契约 `rr-ipc::protocol`（128B 头 + payload 类型）· 消息语义与模块运行时 `rr-core`（topic / 寻址 / task 号 / trace_id + 连接→注册→心跳→分发→节拍环）· 控制与安全核 `rr-core::control` · 能力面契约 **`rr-hal-api`（待建，§12.8 接缝一）** · 传输抽象（transport trait，其无 std 窄面见 §12.8 接缝二） | 与 OS 无关，`no_std`（**core + alloc**）可达 | 契约级冻结，所有平台共享 |
| **主承载层** | `rrd` 跨进程监听与扇出 · 进程监督/重启/退避 · 完整 HAL 实现（ALSA / sysfs / chardev）· cloud brain · deb/OCI 打包 · OTA 写侧 | 有操作系统才叠加 | 有则叠加，非依赖 |

三条读法上的硬口径：

- **`no_std` 可达 ≠ 无堆可达。** 机制核以 `core + alloc` 达成（帧组装产 `Vec<u8>`、`Rr3FrameBuf` 分配）。
  "无堆则用静态/定长区"（ADR-0018 裁定 3 第 2 项）是**敞口**，等一块真无堆的板再裁，本节不写作已达成。
- **原子件与组合件不在机制核那一栏。** 它们是**机制在承载层内的物理组织**（§4.0、§4.0.4）：语义中
  可下沉的部分正被逐片提出进机制核（批D 已提出总线语义、运行时、控制与安全核四片），其余留在承载层。
  现况 `modules/**/atoms/**` 带 std 形状（线程、`mpsc`、env 读取、async HAL 句柄），把它写作"机制核"
  是名义；改到 no_std 可达才是事实——这正是批I/G 的内容。
- **§3 的三层模型（L0/L1/L2）是"具备主承载层"这一档的视图**；下表给它补上另两档，三档共用同一套机制核。

| 档 | 判据 | 承载内容 | 现役实例 |
|---|---|---|---|
| **全承载档 host** | 四类供给齐 + 进程模型 + 富文件系统 | 机制核 + 全部主承载层 | RK3566 参考机、树莓派（0.0.6） |
| **轻承载档 rtos** | 四类供给齐，无进程监督 / 无富文件系统 | 机制核 + 主承载层按需（经 `⊙` 桥接取云端与大模型） | ESP32 + FreeRTOS（0.0.7） |
| **无承载档 bare** | 仅时基 + 事件循环，无 OS | 机制核切片：单固件自足（**task 号由描述符静态分配、不经注册握手**，§12.4），或经桥上灌宿主 | 尚无（真机批触发） |

### 12.2 平台移植边界：四类供给 + 能力格部分实现

把 rock3 带到一块新板，该平台只需提供四类东西，不多不少；**三档的差别就是这四件的齐备度**：

| 供给 | 内容 | host | rtos | bare |
|---|---|---|---|---|
| 1. **时间基准** | 一个单调时钟（心跳到期、控制节拍、注入时钟用），不要求墙上时间 | `Instant` ms | OS tick / esp_timer | 定时器计数 |
| 2. **内存策略** | 有堆用堆、无堆用静态/定长区；机制核不得假设全局分配器 | 系统堆 | 系统堆 | **敞口（§12.1）** |
| 3. **事件循环** | 驱动"收帧→分发→节拍"的调度形态，由机制核运行环消费 | 线程 + 阻塞 socket | FreeRTOS 任务 + 队列 | 主循环 + 中断 |
| 4. **传输实现** | §12.8 接缝二那条窄缝的一个实现，落到该板物理链路 | UDS（+ shm） | UART | UART/SPI |

四类供给的**取值**写进描述符 `[supplies]` 段（§4.3、§12.3），由 port 交一份供给自证表核对。
**v0.9 补第五个供给位：时序预算**——名义周期与允许偏差、安全核调用间隔上界。理由很直白：裁定 5 用
"定长、确定性调度属于机制"定义控制环归属，但本设计从未对它提出**可测要求**（无周期上界、无抖动预算、
无安全核最坏延迟与心跳/看门狗的时序关系）。没有这一位，§12.5 的"逐拍一致"只在数值语义层成立，
在时序层是无证据的断言——所以本档明确它现在**只保证数值一致，不保证实时性**，实时性证据归 0.0.7
真板批的周期直方图实测（本机只能取"声明与代码常量一致"这一半）。

**v0.9 同日三轮补第六个供给位：平台看门狗（ADR-0018 修订 4 ②）**。§12.4 把自足形态的挂死处置交给"平台复位"，
而四类供给里从来没供给过"复位"这项能力——**结论引用的能力不在前提里**，这是前提集不闭合。现定：
`[supplies]` 增 `watchdog`（有无 / 超时上界 / 由哪一路喂），进点验矩阵 ③ 供给自证表与 ⑦ 时序达标；
**无看门狗的板不得归 bare 档**（只能归 rtos 且必须有外部监督）。配套第二条：自足档必须交**本地守护输出**
（UART/RTT 行日志：起动态、喂拍、安全核触发与拒动原因，格式由 `rr-core::runtime` 定形，随批K 定）——
一个"错了没人知道"的形态不是被裁剪过的机器人，是赌运气的固件（索引 §11 审计 L-1）。

**四类供给的可实现接口（v0.9 补——此前只有自然语言清单，移植者无从下手）**：一个 port 交付的是
下述四个形状的实现，缺一项即该档不成立（`bare` 档只需 1+3+4，2 走静态区=敞口）：

```rust
// 落点在机制核（rr-core::port 或 rr-hal-api 同级），全部 no_std + alloc 可达
pub trait Rr3TimeBase {                       // 1. 时间基准：单调，不要求墙上时间
    fn rr3_now_ms(&self) -> u64;              //    喂 rr-core::runtime 的拍点与 control 的注入时钟
}
pub trait Rr3EventLoop {                      // 3. 事件循环：驱动"收帧→分发→节拍"
    fn rr3_wait_ms(&mut self, timeout_ms: u64) -> bool;   // 有界阻塞；false = 超时无事
}
pub trait Rr3ByteStream {                     // 4. 传输实现（§12.8 接缝二，两方法窄缝）
    fn read_into(&mut self, buf: &mut [u8]) -> usize;
    fn write_all_bytes(&mut self, buf: &[u8]) -> usize;
}
// 2. 内存策略不是 trait：由"机制核签名里不出现 alloc 之外的堆假设"+ 目标三元组编译证明
//    （alloc 面已定形，无堆面按 ADR-0018 修订 1 ⑤ 记敞口）
```

配套三条硬约束，否则接口写出来也接不上：**①** `rr-core::runtime` 与 `rr-core::control` 的**调用方**
（今天的 `rr-bus` client 心跳线程、`rr3-act` 的 `rr3_now_ms`）必须改为从 `Rr3TimeBase` 取时基——
这一改顺带结掉"墙钟当单调时基"的债（`rr-bus/src/client.rs` 的 `rr3_now_ms` 注释把 `SystemTime`
称作"ADR-0018 裁定 3 的单调时基"，那是错的：墙钟会回拨，回拨时长内心跳停摆）。**②** 板侧的
`Rr3ByteStream` 实现必须能在 `target_os = "none"` 编译（前置是解除 `rr-ipc` transport 层的
`cfg(not(target_os = "none"))` 门，ADR-0018 修订 3 ①）。**③** 三 trait 的**存在与可编译**是点验矩阵
第③列（供给自证）的机械内容，不再是自由文本表。实施落点：批J（批K 的矩阵②③⑦列以其为前提）。

能力格允许**部分实现**：一块板只声明并实现它物理上具备的格（例如只有 PWM、GPIO 没有音频），写进
`[slots].present`；缺的格按现有纪律置 `Degraded`/`Unsupported`、显式回错、fail-closed（§4.3、§5），
不影响底座在该板起来。不为一块板凑齐格而写假实现——这条与"能力缺失必须显式回错"是同一原则。

### 12.3 描述符：板 / 平台 / 形态的统一数据事实源

一台机器人在某块板上的全部平台特定信息，收进一份声明式描述符（扩展 `profiles/<机型>.toml`，
**七段定形见 §4.3**）：芯片平台档、四类供给的取值、本板在位格集、格位→件绑定、启用格集
（`enabled_slots`）、每个格对应的物理绑定（引脚 / 控制器编号 / 总线 / 设备名）。运行期由 `rrd`
（无 OS 上为机制核装配段）读描述符、探测硬件、据此实例化 HAL 后端，并把就绪句柄与参数槽注入件——
**件只见句柄与 ops、不见引脚、不见厂商、不见产品决策**（§4.0.3 依赖闭包 + §12.8 注入接缝）。

**收口面分两块记账，不许合并宣称**：

- **已收口（2026-10-09 批D④）：物理绑定面。** 此前板特定值散在环境变量与代码缺省里（I²C 缺省取某块板的
  那条、GPIO/PWM 缺省取某块板的引脚，见 §5），"换板不改代码"只在格位层成立、在驱动缺省层不成立。
  现 `profiles/<机型>.toml` 有 `[hardware]` 物理绑定段（9 键，键位与 `rr-hal` 后端一一对应），rrd 装配链
  解析后 spawn 注入 `RR3_HAL_BINDINGS`；`rr-hal::descriptor` 合成生效绑定——**物理绑定来自描述符，
  环境变量降为应急覆盖（同键覆盖赢），代码缺省删除、"描述符没给就 fail-closed"**（选择函数键缺失落
  占位后端、使用时显式回错，不回落某板引脚）。**换底板从此只改这一段——这句成立。**
- **未收口（第 III 轮批J）：平台与供给面。** `[platform]` / `[supplies]` / `[slots].present` 三段还没有
  schema 与消费者，于是"换芯片平台"目前仍要动码（后端族选择靠环境变量、供给取值无处写、缺格只能由
  启用集反推）。**"板 = 数据"目前的成立范围是底板走线一层，不是平台移植那一层**——这句话在 v0.7 里
  写得过满，v0.8 收到实况。
- **同一批要顺手结清的一处静默**：描述符解析失败在 `rr-hal/src/backend.rs` 的四选择函数里被 `.ok()`
  丢弃（`[hardware]` 键名拼错 → 整份描述符作废 → 全部能力面落占位后端，开机横幅照绿、直到第一次
  指令才回 `Unsupported`，且错误文案不出处）。fail-closed 的"显式"两字要求这条错误当场可见。

### 12.4 模块的一般化：从"操作系统进程"到"平台执行单元"

修订 §4 与 [ADR-0002](../process/adr/ADR-0002-模块模型-5加1-进程隔离.md) 的隐含前提：模块
不必是一个操作系统进程。模块 = 由平台提供的一个**执行单元**，落地随平台——Linux 独立进程
（崩溃隔离由进程提供）、RTOS 一个任务、裸机一个内联子系统。模块对外可见面（manifest 的
provides / subscribes / exposes、能力格绑定、§6 的消息语义）**不因落地形态改变**，变的只是
"谁来调度它的执行单元"。这要求机制核的模块运行时不把 `fork/exec`、进程间 socket 当唯一装配手段。

**v0.9 补齐这句之下的空白（寻址与生命周期）**：模块运行时的第一步是**注册**，而 task 号今天由基座分配
（号空间 `0` 广播 / `1` 基座 / `≥2` 模块三段是 `rr-core` 的常量真值；`trace_id = 0` 的"非代理"语义也以
基座在场为前提）。一块自足的裸机固件没有发号者，于是"可见面不变"这句在自足形态里原先无解。现定三条
（裁定原文见 [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md) 修订 2 ①）：
① **号空间语义不变、分配者变**——无主承载层形态的 task 由**描述符静态分配**（`[platform]` 任务号位或
等价的构建期常量表），`rr3_task_kind` 的三段判定照旧成立；
② **运行环有两个合法入口**——有基座走 `Registering → Online{task} → Down` 的注册路径；自足形态走**同一
状态机的退化用法**（装配段直接预置 `Online` 与静态 task），机制核不另建第二套状态机；
③ **`Down` 的处置随承载档而变**——有基座交监督重启，无基座交平台复位（这是裁定 2 "裸机面 = 运行环复位"
的应有含义，与 §4.2 的生命周期表对齐）。

### 12.5 控制与安全核：纯函数 + 注入时钟，控制即组合策略

承 §7.4（ADR-0007 裁定 1：反射环与高频闭环不进总线、留本地）与 ADR-0017（机制/策略分离），
本节再加一条可移植性约束：控制原语与安全核必须是**不读墙上时钟、不睡眠、时钟与输入由外部
注入**的纯计算。于是同一张控制图在 Linux 实时环里跑、在裸机定时器里跑，行为逐拍一致。

切线：**执行这张图的定长、确定性调度 = 机制；开发者编排的"用一组稳定原语连成的控制图"
（数据 + 参数）= 策略。** 控制因此是策略而非写死的代码，且这组原语接口像契约一样只增不改、
要求"非常稳定"。安全核（急停、限位、过流、低电兜底，见 §7.3）不可被策略改写，永远包住任何
控制图；它在有 OS 的平台落 rrd/基座，在无 OS 的平台落机制核内的同一段纯逻辑。

**这句"逐拍一致"的适用范围（v0.9 明确）**：件级对拍用例证的是**数值语义一致**（同样的输入序列在 Linux
实时环与裸机定时器上产出同样的输出序列）。它**不证**时序层的东西——周期是否守住、抖动多大、安全核
最坏延迟与看门狗的关系如何。那些是另一类证据（`[supplies]` 的时序预算位 + §12.6 的时序达标项 + 真板
直方图实测）。把前者当后者，就是拿函数正确性冒充实时性保证。

### 12.6 部署到新平台 = 实现 port + 交描述符 + 过点验矩阵

"把 rock3 部署到一块新板"定义为三个动作，Linux 与非 Linux 同形（§1 的三步即此）：

1. 实现 §12.2 的四类供给 + 该平台提供的能力格后端；
2. 交一份该平台/形态的描述符（§12.3）；
3. 过一遍**点验矩阵**——一套机械检查，确认：底座按描述符起来了、能力协商与缺格降级正确、
   字节契约对得上 golden 向量（§6、`contracts/golden/`）、安全核生效、具备主承载层时模块
   生命周期正常。

**矩阵定形（v0.8；此前它是"有名字没有内容"的东西——十余处引用、零行列）**：行 = 一块板/一档，
列 = 固定七项（v0.9 在 v0.8 的六项之上补 ⑦ 时序达标），一项一格，跑不到的格写"敞口 + 触发条件"，不留白也不拿"设计已覆盖"充数。

| 列 | 检查项 | 判据 | 载体 |
|---|---|---|---|
| ① | 字节契约 | 128B 头 + payload 枚举 + magic 对 `contracts/golden/` 逐字节 | `tools/ci/ipct003.sh` 多路对拍（已有，随档扩目标） |
| ② | 机制核编译面 | 目标三元组下 `core + alloc` 编译通过；无 std 逃逸、无全局分配器假设 tripwire | `tools/ci/rr-core-xtensa-check.sh`（已有） |
| ③ | **供给自证**（装载门，非行为证据） | port 交一张表：时基分辨率与是否单调、内存模型、事件循环形态、传输是否保消息边界；与描述符 `[supplies]` 逐键核对，不一致即红 | **待建（批J）**。v0.9 定性：这一列只证"描述符没撒谎"，**不证 port 正确** |
| ④ | 描述符装载（装载门） | 七段全解析、未知键与坏值当场点名、缺格显式置 `Degraded`（不许静默落占位后端） | descriptor 错误上报 + 探测实装（**待修**，§12.3 末条） |
| ⑤ | 安全核生效 | 限位钳位 / 低电边沿 / 饱和通告在目标上**真跑**逐拍一致，且用例带**两条腿**（确定性 + 期望值来自契约，§4.0.3） | 件级对拍用例接目标 runner（批K） |
| ⑥ | 生命周期 | 注册→心跳→分发→节拍→断开→监督重启（host/rtos 档）；bare 自足档只跑单固件环 + 平台复位（§12.4） | `rrd` e2e + `tools/sim --chain` |
| ⑦ | **时序达标**（v0.9 新增） | 名义周期与偏差、安全核调用间隔上界：板上取周期直方图，本机只核"声明与代码常量一致" | 真板实测归 0.0.7；`[supplies]` 时序位归批J |

**两列是装载门、五列是证据**（v0.9）：把 ③④ 读成行为证据会得出"换板已验证"的假结论，这是本版要堵的
口径滑坡。另规定 **① 列必须记实跑路数**——`tools/ci/ipct003.sh` 在缺 docker 与缺 esp 工具链时打 SKIP
且默认退 0（只有 `--strict` 才拦），所以矩阵里不许写"① 已过"，只能写"实跑 N/4 路 + 其余 SKIP 原因"。

**承载体（v0.9 写实，原先无主语）**：矩阵假定"能真正执行的跨架构流水线"承载它，但远端 atomgit 的
Actions 从不运行（敞口见 status.md）。故规定：矩阵的**默认承载体是一组钉死版本的标准化环境，不是某台机器**（v0.9 同日四轮改判，D4 裁定，定形表见
[0.0.5 方案 §13.2](../process/dev/0.0.5版本洛克三号对齐与裁定方案.md)）：E1 host-x86_64（`rust:*-bookworm`
+ musl/arm64 target + qemu-user + docker）、E2 xtensa-none-elf（esp-rs fork v1.90.0.0 镜像，
`build-std=core,alloc`）、E3 espidf-std（`espressif/idf` + esp-clang）三个容器出 ①②③④⑤⑥ 六列，
本地 WSL2 与 GitHub-hosted runner 只是同一环境的两种载体；入口仍是 `tools/ci/matrix.sh`（随批K 建），
必须以 `--strict` 口径出数——**SKIP 记为未跑，不记为通过**。**两条硬限制写在这里防冒领**：
⑦ 时序达标的真值**只能来自 E4 真板**（云 runner 是共享 x86、qemu-user 是指令级模拟，两者的时间都不是
硬件时间）；E2/E3 出的是**编译面证据**，不能折算成"目标上跑过"。矩阵每行还必须带**环境指纹**
（镜像 digest + `rustc -Vv` + 目标三元组），指纹缺失或换代而不重跑的行按"无载体"计，不作 PASS。——**SKIP 记为未跑，不记为通过**。

**承载三档各交一行实录**：全承载档首行 = 树莓派（0.0.6 真板批，已发生的事按本表重新登记）；轻承载档
首行 = ESP32（0.0.7，四件供给 + ①②③⑤ 为硬项）；无承载档留空行，触发条件 = 一块真无 OS 的板。
"新增一块板要花多少"从此是**填一行矩阵的代价**——这个数就是本项目存在理由的量化口径。

点验矩阵是"跨平台快速部署"的**机械下限**：它把"新增一块板的验证成本"从人肉 bespoke 检查，
压成"跑同一套矩阵"。承载它的应是能真正执行的跨平台流水线（把门禁与跨架构用例接在可跑的
流水线上）；本机/本环境跑不到的项，作为带触发条件的敞口显式登记进 status.md，不以"设计已
覆盖"充当"已验证"。机制核切片的首件机械点验已落（2026-10-09 批D⑤）：
`tools/ci/rr-core-xtensa-check.sh`——同一份机制核源（`rr-core`：总线语义/运行时/控制/安全核）
在宿主 std 测试面与 xtensa-esp32-none-elf 无 OS 编译面**对拍**（无全局分配器/std 逃逸
tripwire + rr-ipc golden 头编译期断言在该目标重求值），工具链缺位 SKIP 明说、不静默当作已验证。

### 12.7 形态谱系与放置

一个**形态** = 描述符选的能力格与启用格集（`enabled_slots`）+ 编排的控制图 + brain 的行为/对话策略；**平台**
（含"有没有操作系统"）是另一条正交的轴。正交带来两个方向都成立：同一形态可落到不同平台
（桌面陪伴从 RK3566 迁到别的 Linux SoC），同一平台可承载多形态。一个机器人落在某平台上时
**承载深度按该平台能力而定**，是一条谱——从"跑全量主承载的大机器人"，到"一枚 MCU 上跑
机制核切片、自足的极小机器人"，中间是经 **bridge**（transport 级连接多路器，不进 manifest
注册面）把云端/大模型等主承载能力**按需接入**：桥是能力升级，不是活着的依赖。最终目标
"以具身形态的机器人底座，裁剪出各种各样的微型机器人"，落实为"一套机制核 + 描述符谱系"。

**谱上的两种 `⊙` 拓扑（v0.9 定口径，堵"自足形态也能发布"的误读）**：

| 拓扑 | 基座在场？ | 语义 | 能做什么 |
|---|---|---|---|
| **经桥接** | 有（对端 host 的 rrd） | 注册 / 心跳 / 前缀归属全按现有语义走；**task 号走注册态**（ADR-0018 修订 4 ③——桥只做 `⊙` 透传，不因为本地无 rrd 就改静态分配） | 完整可见面：发布主题、应答点名、被监督；桥断按 §12.4 `Down` 处置 |
| **自足** | 无 | 只跑单元内 `⊗` + 本地安全核；task 由描述符静态分配（§12.4） | 可以产帧、可以经桥上灌；**不宣称发布主题**——没有注册中心，"发布"无处投递 |

**形态身份（v0.9 补，裁剪谱系的可读面）**：形态 = 一份描述符，但消费者今天分不清"这台机器根本没有这个
能力"与"有但坏了"——缺格降级（`Degraded`）与"该主题无生产者"在契约面上同形。现定：基座在 boot 后把
本形态能力集（`[slots].present` ∩ `enabled_slots`，含三态标记）作为**可查面**暴露（走 `rr3.ctl.status`
扩展，不新增主题），并把"降级"与"不适用"写成两个不同文案。没有这一条，裁剪越多样态运维越盲。

### 12.8 跨 OS 的三处窄缝（第 III 轮实施前提）

§12.2 说"平台只需提供四类供给"，但按现有签名，平台**提供不了**——三条接缝的接口形状都是 std 的。
本节把这三条窄缝定形；它们是"跨操作系统"从设计变成可施工的全部内容，其余改动都是它们的收口。

**接缝一 能力面契约 `rr-hal-api`（件 ↔ 硬件）** · 批I
现况：`rr-hal` 七个 trait 全挂 `#[async_trait]` + `Send + Sync`，件侧经 `rr3_block_on`
（`std::thread::yield_now()` 自旋）驱动。一次件内调用 = 一个装箱 future + 一个 std 调度假设；
`Rr3LinkState.ssid: Option<String>`、`Rr3PcmStream.samples: Vec<i16>` 这类值形状又把平台 crate
焊进了件的签名。
定形：把 7 trait + 值类型 + `Rr3HalError` 提进 no_std crate `rr-hal-api`，做三处去形状——
**同步签名**（`fn set(&mut self, level: bool) -> Result<(), Rr3HalError>`，async 留模块侧，与
ADR-0008 裁定 1 同线）、**去 `Send + Sync`**（单核内联不需要；有 OS 侧由承载层包一层）、
**值类型去 `String`**（定长 + 长度）。`rr-hal` 退成它的 Linux/sim 实现层，既有 import 路径由
re-export 保持不破。
解开的是：件可以进 no_std 面、`§4.0.3` 依赖闭包门才有可能绿。

**接缝二 总线字节流 `Rr3ByteStream`（单元 ↔ 单元，即 `⊙`）** · 批J
现况：`Rr3TransportConn` 的唯一实现路径是 `impl<T: std::io::Read + Write + Send> Rr3TransportConn`
（`rr-ipc/src/transport/mod.rs`），而 `uart` 整模块 `cfg(any(target_os = "linux", target_os = "macos"))`
——它是**宿主侧开 `/dev/ttyUSB`**，不是板侧实现。所以无 std 的板今天**接不进总线**，`⊙` 在非 Linux
平台上是文字。这是本项目核心价值唯一的结构性阻塞点。
定形：机制核内一条两方法窄缝，帧层在它上面 blanket impl 一次，`std::io` 走适配：

```rust
// 机制核面：无错误类型、不引 std::io、不装箱
pub trait Rr3ByteStream {
    fn read_into(&mut self, buf: &mut [u8]) -> usize;      // 阻塞至少读到 1 字节
    fn write_all_bytes(&mut self, buf: &[u8]) -> usize;    // 返回实际写出字节数
}
// std 承载侧一个 adapter：impl Rr3ByteStream for T where T: Read + Write
```
**但有一条前置必须写在这里**：`rr-ipc/src/lib.rs` 今天把整层写成
`#[cfg(not(target_os = "none"))] pub mod transport`——裸机目标连 transport 模块都不编译。窄缝若不解除
这道门，等于不存在（裁定见 ADR-0018 修订 3 ①：把 `std::io` 隔离到 impl/adapter 侧，让窄缝本身在
`target_os = "none"` 下可编译）。做到之后：三实现的**行为与线上字节**零改动（签名面必然要加适配，
"零改动"这句 v0.8 写过了），板侧要接总线从"实现两个 std trait"降成"写两个 `usize` 方法"；解开的是
`⊙` 在 rtos/bare 两档的真正可达（ADR-0019 裁定 2 的跨 OS 那句）。

**接缝三 注入数据面 `handles` / `ops`（策略 → 件）** · 批J
现况：17 份 `assemble.toml` 全部 `handles = []` / `ops = []`，且全仓无代码、无门禁读它——件拿参数
的真实通道是组合件解析 `config.toml` 后把 cfg 结构体当形参传入。这确实是注入，但它**不可校验**，
也表达不了"哪些件要哪些句柄"，于是 §4.3 的缺格降级环只能靠探测空转。
定形：`[assemble.handles]` 声明"这件用哪个能力面 trait ← 绑描述符哪个键"、`[assemble.ops]` 声明
"这件暴露哪些参数槽 ← 取哪个 config/profile 键"；装配段（Linux 为 `rrd`，无 OS 为机制核装配段）
**校验缺项即该格 `Degraded`，点名到件与键**。件内字面阈值随位点清单迁出（§4.0.3、§13）。
解开的是：L3 由口号变实体（ADR-0019 裁定 5）、裁剪有了表达位置、降级环有了判据来源。

三缝落完，"部署到一块新板"的动作面收敛成一句话：**写两件码（一个能力面后端 + 一个 `Rr3ByteStream`
实现）、交一份七段描述符、填一行点验矩阵**。

**这句话的适用条件（v0.9 加，别把有条件的结论写成定义）**：它只对**既有能力面 trait 已覆盖的硬件**成立。
新执行器种类（步进、电调、气阀、多轴）、新反馈类型（IMU 矩阵、ToF、编码器等）不在"两件码"里——
它们要**新格 + 新 trait + 新件**，走契约级立格评审（§4.3、ADR-0012 裁定 6）。这就是本项目的诚实成本地板：
覆盖内换板近乎零改动，覆盖外有界且有主（§1 三减法压不掉这一层）。

**能力面契约的增长律与对象形状（v0.9，裁定见 ADR-0018 修订 2 ②）**：① 增长只走"新增 trait / 新增
值类型"，**禁止给既有 trait 加方法**（那要求所有实现者同批改，等于变更契约语义）；② 对象形状分档——
`bare` 档**禁用 trait 对象**（`dyn` 的 vtable 与潜在分配在单固件里不可预算，只允许泛型单态化或编译期
常量分派），`rtos`/`host` 档允许 `dyn` 句柄；③ 值类型一律定长或借用，**机制核允许 alloc，但能力面
签名里不许出现 alloc**。这三条不先定，接缝一会以"第一个裸机后端返工"的代价补回来。

---

## 13. 门禁与验收（原子模型的静态执法子集）

本节把 §1 纲领（机制与策略极致分离）与 §4 原子模型里**能被机器执法**的部分，接进 `tools/ci/check.sh`
的对应步（步序与条数的唯一落点见 [`../process/conventions.md`](../process/conventions.md) §2，本档不抄）。
只取静态可断言者；判不了semantic 的（如"是不是收敛型抽象"）留评审。ADR-0019 裁定 8 明确**不引入
外部模板式的体积总账 / 迁出账 / 阶梯门禁**——rock3 代码量未到临界，过早设总账是负收益。

| 门 | 断言 | 形态 | 处置 |
|---|---|---|---|
| 机制核零策略载荷 | 机制核面（`rr-core` 全部 + `rr-ipc` protocol + 传输抽象 + 待建 `rr-hal-api`）源码 grep 外部板卡厂商名 / 产品名 / 器件名 / `if (vendor == …)` 命中即红（自家标识不在禁词，见 §13 注） | 静态断言（`check.sh` ⑦-a，已接） | **fail-closed** |
| 件内字面阈值位点 | 原子件里的业务阈值与厂商/产品**注释**逐条列位点清单，随批迁出到 ops/config/描述符。**不宣称 grep 命中即红**——阈值字面量无可靠静态判据（注释里的版本号同为点分数值），v0.8 把这条从"静态门"降为"清单 + 评审"，免得文档口径高于门的实际断言 | 位点清单 + 评审 | 清单登记，评审收口 |
| 件级依赖闭包 | **分两级执法**（v0.9）：L0 机制核可达集 {`core`, `alloc`, `rr-ipc` protocol, `rr-core`, `rr-hal-api`}；L1 承载层机制件集 = L0 + `serde_json`（线格式载体，属承载层形状）。`modules/**/atoms/**` 的**非 test 面**出现 `use std::` / `use rr_hal::` 即红；`#[cfg(test)]` 面不扫（假后端夹具合法，归属由批I 定形）。`serde_json` 退场另列 L0 迁移清单，不在门里假装它已在机制核 | 静态断言（批I 新门） | **fail-closed** |
| 件级逐拍对拍 | 每件**两条腿**：① 确定性（合成输入序列 → 输出序列，无 IO / 无线程 / 无墙上时钟，同输入两跑逐拍相等，范式同 `rr-core::control` 的 `same_control_graph_beat_for_beat_identical`）；② 正确性（期望值引自 `contracts/events/*.schema.json` / `contracts/golden/` / 裁定写死的语义，并覆盖下界/上界/等号/零值/畸形）。**只有腿一记未达标**，门另需一条"恒输出 0 的件被拦下"的自证反例 | 计数断言（批K 新门，⑥/⑦ 接线） | **fail-closed** |
| 格状态标记 | 正常 / 占位 / retired 三态读 `contracts/slots.toml` 的格标记位——**豁免一律数据化**，门内不许有硬编码 `if`；占位件记"不适用"而非"通过"，retired 格退出"每格一件"分母 | 查册门 + 数据面校验（批J/K） | **fail-closed** |
| 门禁清单同步 | 新建机制核面 crate 必须**同批**出现在两处清单：`check.sh` ⑦-a 的禁词扫面枚举、`tools/ci/rr-core-xtensa-check.sh` 的目标 crate 列表。缺任一处即红——防止"§13 宣称有门、门实际没扫" | 静态断言（自指：门查门） | **fail-closed** |
| 每格一件 · 不留空格 | manifest/profile 引用的格必须命中 `slots.toml`；`enabled_slots` 声明的格有对应原子件；占位件登记显式豁免、不折算达标 | 查册门 + 仓扫描（§4.3 门禁②扩，已接） | **fail-closed** |
| 注入面校验 | `assemble.toml` 的 `handles`/`ops` 与描述符对表：件要的句柄/参数槽未给 → 该格 `Degraded` 并点名到件与键 | 数据面校验（批J 新门） | **fail-closed** |
| `F⊗F` 非法 | 面不得作为被装配方出现在装配清单里（面间只 `⊙`）；`composite.toml` 的 `⊗` 链成员必须是本模块原子件 | 静态断言（`check.sh` ⑦-b，已接） | **fail-closed** |
| 禁薄胶水 | 跨原子件的串联必须有组合件实体（`composite.toml` + 入口 + 实现），散落转发直连原子即红 | 静态（存在性，已接）+ 评审（"散落"判定） | fail-closed（存在性）|
| 五件套齐全 | 每个**组合件** ①`.manifest`+②入口+③实现+④装配清单+⑤唯一私有头 五件缺一即红（`rr3-mkmod` 产物回灌契约，承 ②b）。**面不计**——面是视图，无独立声明物（§4.0 读法注记） | 计数断言（⑦-c，已接） | **fail-closed** |
| 入口体量 | 装配入口（`main.rs`/生成入口）≤ 80 行 | 计数断言（`wc -l`，⑦-d，已接） | **fail-closed** |
| 格格双达标·运行 | 每格落点件有 tests 且接线 CI；"有测试未接线 = 未达标" | 承 check.sh 九步门的 ⑥（已接） | **fail-closed** |
| 契约冻结 | 128B 头/payload 枚举对 `envelope.schema.json` + golden 逐字节；主题/格只增不改 | `envelope_meta` + 形状 pin（§6、§4.3） | **fail-closed** |
| 层界单向 | 依赖只从上层指向下层，无反向、无模块 crate 互依赖 | 链接图断言（承 §2 原则、ADR-0002） | **fail-closed** |
| 准入三问留痕 | 新建/收编原子件的三问结论登记录（评审项，非静态门）；第①问的可答法见 §4.0.3 | 评审 | 记录留痕 |

注：**门禁的强度只等于脚本真做的事**——本表"形态"列区分"已接"与"批J/K 新门"，不把待建的门写成
现有强度。步序与条数的唯一落点仍是 [`../process/conventions.md`](../process/conventions.md) §2。

跨平台点验矩阵（§12.6 七列）与格格双达标的"运行正常"这半，靠能真正执行的跨架构流水线承载；本机跑不到、
需实板/实芯的项，作为带触发条件的敞口显式登记进 status.md，不以"设计已覆盖"充当"已验证"。

---

## 14. 架构速查

```
纲领      机制与策略极致分离（机制留核、原子化填格成面；策略经 ops+描述符注入或迁生态层）
核心价值  跨操作系统快速部署 ＝ 写两件码（能力面后端 + 字节流）+ 交七段描述符 + 填一行点验矩阵
承载三档  判据是"四类供给齐不齐"，不是"有没有 OS"：host 全承载 / rtos 轻承载 / bare 无承载
寻址两态  有基座：注册拿 task；自足 bare：task 由描述符静态分配、运行环预置 Online、**不宣称发布主题**（§12.4/§12.7）
形态三例  F-A 全承载具身主形态 / F-B 桥接微型（sense 2 格 + act.servo，认知在 host）/ F-C 自足微型（未成立，
          待 base 分族与看门狗位落地）——逐格得失与矩阵行见 [../design/模块设计索引.md](../design/模块设计索引.md) §10
供给六位  时基 / 内存 / 事件循环 / 传输 / **时序预算** / **平台看门狗**（后两位 v0.9 同日三轮补；无 WDT 不得归 bare）
格三态    正常 / 占位 / retired 记在 `contracts/slots.toml` 标记位；retired = 号位保留、义务解除（只增不改冻结的是历史，不是义务）
件级判据  两腿：确定性对拍 + 期望值引自契约（schema/golden/裁定语义），只有腿一记未达标；闭包 L0/L1 两级、test 面不扫
时序口径  逐拍一致 = 数值语义一致；实时性要 `[supplies]` 时序位 + 板上直方图，未测前不声称（§12.5/§12.6 ⑦）
主线      立格(Slot) → 造件(Atom) → 成面(Facade)；固定的是格，变化的是件
坐标系    contracts/slots.toml 能力格（现 21 格）· 每格一件 · 不留空格 · 双达标（①测试接线 ②件级对拍）
四层      格 Slot ← 件 Atom ← 组合件 Composite ← 面 Facade（面=组合件的职责视图，无第四份 manifest）
组合      ⊗ 装配（装配期 · 执行单元内 · 函数调用）· ⊙ 编排（运行期 · 跨单元经字节流窄缝）· F⊗F 非法
面        固定职责四面 接入/运行/状态/守护；组合件跨四面不可拆；能力域降为组合件分组
机制核    `rr-ipc::protocol` + `rr-core`（消息语义·运行时·控制·安全核）+ `rr-hal-api`（待建）—— core+alloc 可达、零策略载荷、冻结；无堆=敞口
承载分层  原子件与组合件 = 机制在承载层内的物理组织（不是机制核本体）；主承载层 = rrd 监督 · 全 HAL · brain · 打包 · OTA 写侧
三处窄缝  §12.8：① 能力面契约（去 async/去 Send+Sync/去 String）② 总线字节流 `Rr3ByteStream`（⊙ 的 no_std 通路）③ 注入面 `handles`/`ops` 被装配段校验
描述符    [platform] [supplies] [slots] [bindings] [enabled_slots] [hardware]——芯片平台与底板走线分开，裁剪只改数据
安全      反射环/控制图不进总线(§7.4)· 纯函数+注入时钟逐拍一致 · 安全核不可被策略改写(§7.3)
门禁      静态可执法子集见 §13（零策略载荷 / 依赖闭包 / 件级对拍 / 每格一件 / 注入面 / F⊗F / 五件套 / 双达标 / 契约冻结 / 层界单向）
```

自检句：

- 「这是机制，还是策略？」——答不出即违例。
- 「它填哪个格？」——答不出格的不该造。
- 「组合怎么承载的？」——答不出组合件实体的就是薄胶水。
- 「这个件写得出逐拍对拍用例吗？」——写不出就说明里面藏了时钟、线程或平台形状，它还不是机制。
- 「这块没 Linux 的板跑的是什么？」——今天的答案是"只有字节契约真跑过"；答"跑同一机制核切片"要等
  §12.8 三处窄缝落完（此前这是设计意图，不是事实）。
