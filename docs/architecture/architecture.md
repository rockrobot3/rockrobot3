# rock3 总体架构文档

- **状态**：v0.7（融入能力格坐标系与装配单 [ADR-0012](../process/adr/ADR-0012-能力格坐标系与装配单.md)、
  OS 双轨 [ADR-0013](../process/adr/ADR-0013-OS双轨-发行版快路与Buildroot保底.md)、
  跨操作系统移植模型 [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md)、
  原子功能模块模型 [ADR-0019](../process/adr/ADR-0019-原子功能模块模型-格件面与组合代数.md)；
  只写设计语义，一切"做到哪"的句子移出本档）
- **日期**：2026-10-01
- **修订记录**：v0.7（2026-10-08）大修——引入**原子功能模块模型**（立格→造件→成面 + `⊗`/`⊙` 组合代数 +
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

**最高约束：跨操作系统快速部署**（§12 / ADR-0018）——同一机制核要能在带 Linux 与不带 Linux 的板上成立；
换平台 = 实现 port + 交描述符 + 过点验矩阵，不改产品码。**功能守恒**：任何重铺不得使对外功能消失，
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

底座因此切成两层——**机制核（与操作系统无关、能下沉到裸机、契约级冻结）与主承载层（有操作
系统才叠加）**——这条切线是 [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md)
的裁定，架构展开见本文 **§12**。下面的三层模型（[ADR-0013](../process/adr/ADR-0013-OS双轨-发行版快路与Buildroot保底.md)）
是"具备 Linux 的主承载形态"这一档的视图；不带 Linux 的平台如何承载同一机制核，见 §12。

**底座 = 机制核 + 主承载层**：rock3 的交付物不是某一块板的镜像，而是一份可安装的**底座**——
带 Linux 时表现为 `rrd` + `rr-hal` + `contracts/` + 安装器 + 装配单这层 payload；不带 Linux 时
表现为同一机制核的一个切片。板卡与 OS 差异由三层模型消化：

```
Layer 2  模块层     rr3-sense / rr3-act / … / rr3-brain（OCI 分发，rrd 编排）
Layer 1  底座 payload（rock3 本体）rrd + rr-hal + contracts + 装配单 + 安装器
Layer 0  板卡支持   白拿上游发行版（Debian/Armbian/Alpine）；Buildroot 保底轨
```

**部署到新平台的三步**（换一块板 / 一个形态的全部动作；Linux 与非 Linux 同形，见 §12
与 [ADR-0018](../process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md) 裁定 6）：

1. **提供平台 port**——该板向机制核供四类东西：时间基准、内存策略、事件循环、传输实现；
   有操作系统的板在此之上叠加主承载层（发行版快路 deb / Buildroot 保底镜像落 `rrd`、HAL
   后端与契约文件）；
2. **交描述符**——`profiles/<机型>.toml` 声明格位→件与启用格集（`enabled_slots`，ADR-0012；原 `[faces]`，见 §4.3），并声明每格对应的
   物理绑定（引脚/总线/设备，见 §12）；`rrd`（或裸机上的机制核装配段）按描述符 + 硬件探测匹配；
3. **过点验矩阵**——机械确认底座按描述符起来、缺格降级正确、字节契约对得上 golden 向量、
   安全核生效。

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
> | 平台 port 接口（时基 / 内存 / 事件循环 / 传输） | ADR-0018 裁定 3 | 设计落点；具体 crate 划分随实施批进本树同批补画 |
> | ~~描述符扩展硬件物理绑定 + `rr-hal` 板特定缺省收口~~ | ADR-0018 裁定 4 | **已落盘**（2026-10-09，批D④：`profiles/<机型>.toml` 新增 `[hardware]` 物理绑定段两份、rrd 装配链解析后 spawn 注入 `RR3_HAL_BINDINGS`、rr-hal `descriptor` 模块合成生效绑定——描述符为底/env 应急覆盖/无则 fail-closed；五选择函数接绑定、板缺省常量与构造删除，见 §12.3 注记） |

---

## 4. 原子功能模块模型（格 · 件 · 组合件 · 面）

> 本节是 [ADR-0019](../process/adr/ADR-0019-原子功能模块模型-格件面与组合代数.md) 的架构展开；
> 裁定原文（含备选否决、rock3 化判断的注记）见该 ADR。下面只写设计语义与接缝；某原子件是否已建、
> 接线到哪一步，一律见 [`../status.md`](../status.md)。

### 4.0 架构语言：Slot / Atom / Composite / Facade

| 中文 | 英文 | 定义 | 落点 |
|---|---|---|---|
| **能力格** | **Slot** | 机器人能力的最小归类坐标，固定不变、只增不改 | `contracts/slots.toml`（现 21 格：base 5 / body 12 / cognition 4） |
| **原子件** | **Atom** | 填一个格、不可再分、只含机制的最小实现件；每个原子一个目录、板无关、经描述符注入句柄 | 源码目录（0.0.5 重铺） |
| **组合件** | **Composite** | 两个及以上原子件按 `⊗` 装配得到的组件，是执行单元的承载 | 运行期落为一个执行单元 |
| **形态面** | **Facade** | 按职责面暴露的组合件集合；= 声明(`.manifest`) + 生成(样板) + 组合件装配(Composite 承载) | 见下"职责四面" |

核心关系式（全档统一，不得改写语义）：

```
面 Facade = 声明（.manifest）+ 生成（样板）+ 组合件装配（Composite 承载）
格 Slot   ←固定→   件 Atom   ←可变→
功能 = 原子件 ⊗ 装配 → 组合件 ⊗ 再装配 → 形态面
```

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
解耦（§12.7）的结构基。

### 4.0.3 每格一件 · 不留空格 · 格格双达标

- **每格一件**：每个能力格落一个原子件，同类件塌缩归一；格是架构承诺，**不留空格**（空格 = 该能力靠各处私搭维持）。
- **格格双达标**：① 落点件有 tests 且接线 CI（承 `conventions.md` §2 七步门，"有测试未接线 = 未达标"）；
  ② 落点件非死件（有有效扇入：真实跨单元消费者；`#include`/伞形引用只作初筛不计扇入）。
- **扇入分档**（rock3 化，非全盘 ≥3）：`N ≥ 3` 是**新建/收编跨形态复用机制原子**的准入杠杆；单消费者的能力格
  （如 `power.battery`、`sensor.imu` 在某形态只一处用）沿用 [ADR-0012](../process/adr/ADR-0012-能力格坐标系与装配单.md)
  裁定 6 的成格判据（≥1 真实依赖者），两档不互覆盖。

### 4.0.4 五件套（形态面的物理形态）与准入三问

每个组合件/面按 `rr3-mkmod` 生成五件、缺一不可：① `.manifest`（含 `slots` 字段，缺失即门禁失败，发布即冻结）·
② 入口（只做装配与启动，**≤ 80 行**，沿用 0.0.8 雏形）· ③ 实现（机制主体）· ④ 装配清单（声明用了哪些原子件的 `⊗` 链）·
⑤ 唯一私有头（跨域只走公开头）。任何拟新增/保留的件过**准入三问**：① 是机制吗（不含策略/厂商/产品决策）？
② 是收敛型抽象吗（收编后总行数下降、抽象带语义而非仅签名）？③ 复用达标吗（按 4.0.3 分档）？三问全过才建件，否则就地内联。

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
`bus.rpc`/`net.online`/`power.battery`，即 `base_capabilities` 的表内化，格名随原能力名，
基座内建永远在位）、身体层（`act.servo`/`act.led`/`net.wifi`/`net.bt`/`ui.face`/
`audio.playback`/`audio.capture`/`audio.vad`/`sensor.env`/`sensor.imu`/`power.charger`）、
认知层（`cognition.dialog`/`cognition.asr`/`cognition.llm`/`cognition.tts`）。
格位命名两段式 `<域>.<格>`，与主题命名 `rr3-<模块>/<域>/<事件>`（§6.3）
不同轴。**格位成活判据**（ADR-0012 裁定 6，2026-10-03 口径修订）：新格立格须**双达标**
——≥1 个真实依赖者，或评审记录中明确的 ≥3 潜在场景；**CI 只执法"测试接线"那条，扇入数
不进 CI**（留在 review）；落盘 20 格按落盘日 grandfather 豁免、不回溯执法。不留空格。

**造件**：模块只含机制（怎么采 PCM、怎么驱动舵机），策略不进模块（§2 原则 4）；
模块对外的身份是"我 provides 哪些格位"。

**装配单**：一台机器人的选型与绑定收敛为一份 `profiles/<机型>.toml` 纯数据文件
（`[os]` 轨道 / `[bindings]` 格位→件 / `[enabled_slots]` 模块→本形态启用的格位集合）。
**注**：该字段原名 `[faces]`，为避免与 §4.0.2 的"职责面 Facade"撞名，**已随批A 改名 `[enabled_slots]`**（2026-10-08）——
它表达的是"启用哪些格"，不是职责面。`profiles/*.toml`、§3 目录树、`rr-registry` 查册门字段、
`rrd` 解析与既有引用已同批连带更新（ADR-0019 裁定 1 消歧、R16 兑现）；`[faces]` 仅存于历史存档节（不改写）。**rrd 启动装配链**：
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
> **消化已完成（2026-10-05 0.0.4 批F）**：逐组件语义全部收敛（panel/data 批C/B 实装、
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

底座切成两层，边界即"要不要操作系统"：

| 层 | 组件 | 与 OS 关系 | 稳定性 |
|---|---|---|---|
| **机制核** | 字节契约（128B 头 + payload 类型）· 传输抽象（transport trait）· 消息语义（topic / 寻址 / task 号 / trace_id）· 模块运行时（连接→注册→心跳→分发→节拍环）· 控制原语 · 安全核 | 无关，`no_std` 可达 | 契约级冻结，所有平台共享 |
| **主承载层** | `rrd` 跨进程监听与扇出 · 进程监督/重启/退避 · 完整 HAL（ALSA / sysfs）· cloud brain · deb/OCI 打包 · OTA 写侧 | 需要操作系统 | 有则叠加，非依赖 |

带 Linux 的板跑全部；不带 Linux 的板跑机制核的一个切片。§3 的三层模型（L0/L1/L2）是
"具备主承载层"这一档的视图，本节给它补上"无主承载层"那一档，两层用同一套机制核。

### 12.2 平台移植边界：四类供给 + 能力格部分实现

把 rock3 带到一块新板，该平台只需提供四类东西，不多不少：

1. **时间基准**——一个单调时钟（心跳到期、控制节拍用），不要求墙上时间；
2. **内存策略**——有堆用堆、无堆用静态/定长区，机制核不得假设全局分配器；
3. **事件循环**——一个能驱动"收帧→分发→节拍"的调度形态（Linux 线程 + 阻塞 socket / 裸机主循环 + 中断 / RTOS 任务 + 队列）；
4. **传输实现**——§6.2 那条 transport trait 的一个实现，落到该板的物理链路（Linux 本地 UDS，跨片或裸机 UART/SPI/I²C/BLE）。

能力格允许**部分实现**：一块板只声明并实现它物理上具备的格，缺的格按现有纪律置
`Degraded`/`Unsupported`、显式回错、fail-closed（§4.3、§5），不影响底座在该板起来。不为一块板
凑齐格而写假实现——这条与"能力缺失必须显式回错"是同一原则。

### 12.3 描述符：板 / 平台 / 形态的统一数据事实源

一台机器人在某块板上的全部平台特定信息，收进一份声明式描述符（扩展 `profiles/<机型>.toml`，
承 §4.3、ADR-0012）：格位→件绑定、启用格集（`enabled_slots`）、**每个格对应的物理绑定**（引脚 / 控制器编号 /
总线 / 设备名），以及裁定 3 四类供给的取值。运行期由 `rrd`（裸机上为机制核装配段）读描述符、
探测硬件、据此实例化 HAL 后端，并把就绪句柄注入模块——模块只见 trait、不见引脚。

**泛化欠账已收口（2026-10-09 批D④）**：此前板特定值散在环境变量与代码缺省里（例如 I²C 总线
缺省取某块板的那条、GPIO/PWM 缺省取某块板的引脚，见 §5），"换板不改代码"只在格位层成立、在
驱动缺省层不成立。现收口为：`profiles/<机型>.toml` 增 `[hardware]` 物理绑定段（引脚/控制器编号/
总线/设备名，键位与 `rr-hal` 后端一一对应），rrd 装配链解析后 spawn 注入 `RR3_HAL_BINDINGS`；
`rr-hal` `descriptor` 模块合成生效绑定——**物理绑定来自描述符，环境变量降为应急覆盖（同键覆盖
赢），代码缺省删除、"描述符没给就 fail-closed"**（五选择函数键缺失落占位后端、使用时显式回错，
不回落某板引脚）。**"板 = 数据"从这次收口开始成立。**

### 12.4 模块的一般化：从"操作系统进程"到"平台执行单元"

修订 §4 与 [ADR-0002](../process/adr/ADR-0002-模块模型-5加1-进程隔离.md) 的隐含前提：模块
不必是一个操作系统进程。模块 = 由平台提供的一个**执行单元**，落地随平台——Linux 独立进程
（崩溃隔离由进程提供）、RTOS 一个任务、裸机一个内联子系统。模块对外可见面（manifest 的
provides / subscribes / exposes、能力格绑定、§6 的消息语义）**不因落地形态改变**，变的只是
"谁来调度它的执行单元"。这要求机制核的模块运行时不把 `fork/exec`、进程间 socket 当唯一装配手段。

### 12.5 控制与安全核：纯函数 + 注入时钟，控制即组合策略

承 §7.4（ADR-0007 裁定 1：反射环与高频闭环不进总线、留本地）与 ADR-0017（机制/策略分离），
本节再加一条可移植性约束：控制原语与安全核必须是**不读墙上时钟、不睡眠、时钟与输入由外部
注入**的纯计算。于是同一张控制图在 Linux 实时环里跑、在裸机定时器里跑，行为逐拍一致。

切线：**执行这张图的定长、确定性调度 = 机制；开发者编排的"用一组稳定原语连成的控制图"
（数据 + 参数）= 策略。** 控制因此是策略而非写死的代码，且这组原语接口像契约一样只增不改、
要求"非常稳定"。安全核（急停、限位、过流、低电兜底，见 §7.3）不可被策略改写，永远包住任何
控制图；它在有 OS 的平台落 rrd/基座，在无 OS 的平台落机制核内的同一段纯逻辑。

### 12.6 部署到新平台 = 实现 port + 交描述符 + 过点验矩阵

"把 rock3 部署到一块新板"定义为三个动作，Linux 与非 Linux 同形（§1 的三步即此）：

1. 实现 §12.2 的四类供给 + 该平台提供的能力格后端；
2. 交一份该平台/形态的描述符（§12.3）；
3. 过一遍**点验矩阵**——一套机械检查，确认：底座按描述符起来了、能力协商与缺格降级正确、
   字节契约对得上 golden 向量（§6、`contracts/golden/`）、安全核生效、具备主承载层时模块
   生命周期正常。

点验矩阵是"跨平台快速部署"的**机械下限**：它把"新增一块板的验证成本"从人肉 bespoke 检查，
压成"跑同一套矩阵"。承载它的应是能真正执行的跨平台流水线（把门禁与跨架构用例接在可跑的
流水线上）；本机/本环境跑不到的项，作为带触发条件的敞口显式登记进 status.md，不以"设计已
覆盖"充当"已验证"。

### 12.7 形态谱系与放置

一个**形态** = 描述符选的能力格与启用格集（`enabled_slots`）+ 编排的控制图 + brain 的行为/对话策略；**平台**
（含"有没有操作系统"）是另一条正交的轴。正交带来两个方向都成立：同一形态可落到不同平台
（桌面陪伴从 RK3566 迁到别的 Linux SoC），同一平台可承载多形态。一个机器人落在某平台上时
**承载深度按该平台能力而定**，是一条谱——从"跑全量主承载的大机器人"，到"一枚 MCU 上跑
机制核切片、自足的极小机器人"，中间是经 **bridge**（transport 级连接多路器，不进 manifest
注册面）把云端/大模型等主承载能力**按需接入**：桥是能力升级，不是活着的依赖。最终目标
"以具身形态的机器人底座，裁剪出各种各样的微型机器人"，落实为"一套机制核 + 描述符谱系"。

---

## 13. 门禁与验收（原子模型的静态执法子集）

本节把 §1 纲领（机制与策略极致分离）与 §4 原子模型里**能被机器执法**的部分，接进 `tools/ci/check.sh`
的对应步（步序与条数的唯一落点见 [`../process/conventions.md`](../process/conventions.md) §2，本档不抄）。
只取静态可断言者；判不了semantic 的（如"是不是收敛型抽象"）留评审。ADR-0019 裁定 8 明确**不引入
外部模板式的体积总账 / 迁出账 / 阶梯门禁**——rock3 代码量未到临界，过早设总账是负收益。

| 门 | 断言 | 形态 | 处置 |
|---|---|---|---|
| 机制核零策略载荷 | 机制件源码 grep 厂商名 / 产品名 / 业务阈值 / `if (vendor == …)` 命中即红 | 静态断言 | **fail-closed** |
| 每格一件 · 不留空格 | manifest/profile 引用的格必须命中 `slots.toml`；`enabled_slots` 声明的格有对应原子件 | 查册门（§4.3 门禁②扩） | **fail-closed** |
| `F⊗F` 非法 | 面不得作为被装配方出现在装配清单里（面间只 `⊙`） | 静态/链接图断言 | **fail-closed** |
| 禁薄胶水 | 跨原子件的串联必须有组合件实体（`.manifest` + 装配清单登记 `⊗` 链），散落转发直连原子即红 | 静态 + 评审 | fail-closed（存在性）|
| 五件套齐全 | 每个组合件/面 `.manifest`+入口+实现+装配清单+唯一私有头 五件缺一即红（`rr3-mkmod` 产物回灌契约，承 ②b） | 计数断言 | **fail-closed** |
| 入口体量 | 装配入口（`main.rs`/生成入口）≤ 80 行 | 计数断言（`wc -l`） | 告警→随批转 fail-closed |
| 格格双达标·运行 | 每格落点件有 tests 且接线 CI；"有测试未接线 = 未达标" | 承 check.sh 测试步（≥1 用例真跑） | **fail-closed** |
| 契约冻结 | 128B 头/payload 枚举对 `envelope.schema.json` + golden 逐字节；主题/格只增不改 | `envelope_meta` + 形状 pin（§6、§4.3） | **fail-closed** |
| 层界单向 | 依赖只从上层指向下层，无反向、无模块 crate 互依赖 | 链接图断言（承 §2 原则、ADR-0002） | **fail-closed** |
| 准入三问留痕 | 新建/收编原子件的三问结论登记录（评审项，非静态门） | 评审 | 记录留痕 |

跨平台点验矩阵（§12.6）与格格双达标的"运行正常"这半，靠能真正执行的跨架构流水线承载；本机跑不到、
需实板/实芯的项，作为带触发条件的敞口显式登记进 status.md，不以"设计已覆盖"充当"已验证"。

---

## 14. 架构速查

```
纲领      机制与策略极致分离（机制留核、原子化填格成面；策略经 ops+描述符注入或迁生态层）
核心价值  跨操作系统快速部署（带 Linux / 不带 Linux 同形）＝ 实现 port + 交描述符 + 过点验矩阵
主线      立格(Slot) → 造件(Atom) → 成面(Facade)；固定的是格，变化的是件
坐标系    contracts/slots.toml 能力格（现 21 格）· 每格一件 · 不留空格 · 双达标
四层      格 Slot ← 件 Atom ← 组合件 Composite ← 面 Facade（声明+生成+装配承载）
组合      ⊗ 装配（装配期 · 执行单元内 · 函数调用）· ⊙ 编排（运行期 · 跨单元经 transport）· F⊗F 非法
面        固定职责四面 接入/运行/状态/守护；组合件跨四面不可拆；能力域降为组合件分组
机制核    契约·传输·消息语义·模块运行时·控制原语·安全核 —— no_std 可达、零策略载荷、冻结
主承载层  rrd 跨进程监督 · 全 HAL · brain · deb/OCI · OTA 写侧 —— 有 OS 才叠加
安全      反射环/控制图不进总线(§7.4)· 纯函数+注入时钟逐拍一致 · 安全核不可被策略改写(§7.3)
门禁      静态可执法子集见 §13（零策略载荷 / 每格一件 / F⊗F / 五件套 / 双达标 / 契约冻结 / 层界单向）
```

自检句：

- 「这是机制，还是策略？」——答不出即违例。
- 「它填哪个格？」——答不出格的不该造。
- 「组合怎么承载的？」——答不出组合件实体的就是薄胶水。
- 「这块没 Linux 的板跑的是什么？」——是同一机制核的切片，不是全量 rrd；答"跑不了"即 ADR-0018 未落。
