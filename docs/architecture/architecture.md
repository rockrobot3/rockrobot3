# rock3 总体架构文档

- **状态**：v0.4（只写设计语义：目录树、阶段机、传输层、启动链按代码真实形状订正，
  一切"做到哪"的句子移出本档）
- **日期**：2026-09-30
- **实况出口**：本档不回答"某能力有没有实现"。凡此类问题一律指向 [`../status.md`](../status.md)；
  本档里出现进度性断言即视为待订正。
- **关联决策**：[ADR-0001](../process/adr/ADR-0001-平台选型-linux66-buildroot-rk3566.md) · [ADR-0002](../process/adr/ADR-0002-模块模型-5加1-进程隔离.md) · [ADR-0003](../process/adr/ADR-0003-IPC选型-uds-jsonrpc-事件总线.md) · [ADR-0004](../process/adr/ADR-0004-Airymax接入路径-运行时层对接.md) · [ADR-0005](../process/adr/ADR-0005-命名规约-rr3前缀矩阵.md) · [ADR-0006](../process/adr/ADR-0006-采用AiryIPC线格式与rr-ipc分层.md) · [ADR-0007](../process/adr/ADR-0007-控制面分层与模块级升级.md)
- **命名速查**：[naming.md](../process/naming.md)

---

## 1. 定位与目标

RockRobot3（rock3）是面向 **RK3566 + Linux 6.6** 的桌面陪伴机器人项目（体积约束约 0.5L），由 ESP32 固件项目（`legacy/`）演进而来，最终面向具身机器人发展。

**硬性目标**：

| 目标 | 说明 |
|---|---|
| 自烧录 OS | 任意开发板不拆机烧录（maskrom 路线），换板不需要烧录器 |
| 模块化 5+1 | sense / act / interact / power / connect + brain，进程级隔离 |
| 基座能力 day-one | 模块注册、看门狗、A/B OTA、工厂恢复 |
| 待机功耗 | 第一版即含电源管理，待机 <1W |
| 可演进 | 保留接入 Airymax OS/RT 的目标（运行时层对接，见 ADR-0004） |

**明确不做**：ROS 2（太重）、PyO3 嵌入（改用独立进程）、gRPC（单机 UDS 足够）、Yocto（后期再评估）。

---

## 2. 总体分层

```
┌─────────────────────────────────────────────────────┐
│  模块层（5+1，独立进程，崩溃隔离）                      │
│  rr3-sense  rr3-act  rr3-interact  rr3-power        │
│  rr3-connect            rr3-brain (Python)          │
├─────────────────────────────────────────────────────┤
│  基座层 base/                                        │
│  rrd 守护进程：事件总线 · 模块注册表 · HAL · 看门狗     │
│  rr-ipc / rr-bus / rr-registry / rr-hal / rr-ota    │
├─────────────────────────────────────────────────────┤
│  OS 层 base/os/                                      │
│  Buildroot + Linux 6.6 (Rockchip RK3566)             │
│  A/B 分区 slotA/slotB/factory · maskrom 自烧录        │
└─────────────────────────────────────────────────────┘
```

核心原则：

1. **基座承载横切关注点**：安全（急停/限位/低电保护）、日志、时间同步、OTA 全部在 base，不进模块——基座存活则安全兜底存活
2. **模块间零直连**：一切交互经事件总线/RPC，模块只依赖"能力名"不依赖具体模块名
3. **brain 是唯一编排者**：对话策略只存在于 rr3-brain（承接 legacy main.c 的"薄壳"哲学）

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
│   └── workflows/ci.yml           ← 与本地同一入口：`bash tools/ci/check.sh`
│
├── contracts/                     ← [SC] 字节级共享契约（Rust/Python 双方唯一事实源）
│   ├── README.md                  ← 歧义裁定与"契约只增不改"的演进规则
│   ├── envelope.schema.json       ← A-IPC 128B 消息头 + 5 种 payload 类型契约（ADR-0006）
│   ├── topics.toml                ← 主题所有权注册表（前缀归属与校验入口见 §6.3）
│   ├── golden/                    ← event-header-128.hex：一条真实头的 hex 向量（同机字节基准）
│   └── events/                    ← 各模块事件负载 JSON Schema 的落点（命名与登记规矩在其 README）
│
├── base/                          ← 基座层（横切关注点，见 §2 原则 1）
│   ├── os/                        ← Buildroot 层（ADR-0001）：README.md 里定义了 configs/、board/、
│   │                                 patches/、package/ 四类交付物的落点
│   ├── ota/                       ← README.md + partition-layout.md（A/B/factory 布局文档）
│   │   └── rr3-ota-ctl/           ← 独立 workspace，bin 名 `rr3-ota-ctl`（升级入口 CLI）
│   ├── runtime/                   ← Rust workspace：members = rr-ipc rr-registry rr-bus rr-hal rr-ota rrd
│   │   │                             （Cargo.lock 在 workspace 根，每 crate 各有一份 README.md）
│   │   ├── rr-ipc/                ← A-IPC 协议层 + 帧收发（§6.2）
│   │   │   ├── src/protocol/      ← header.rs（128B 头编解码）、body.rs（payload 类型前缀）、mod.rs
│   │   │   ├── src/transport/     ← frame.rs（整帧读写）、mod.rs（传输分层 + 两个路径常量）
│   │   │   └── tests/             ← envelope_meta.rs：与 envelope.schema.json 逐字段对齐的守护测试
│   │   ├── rr-bus/                ← 总线客户端骨架：task 号与主题常量（订阅分发/QoS/回压的设计落点）
│   │   ├── rr-registry/           ← manifest 解析与校验、主题集（§4.1、§6.3）
│   │   │   ├── src/               ← schema.rs、validate.rs、topics.rs、module.schema.toml（字段字典）
│   │   │   └── tests/             ← contract.rs（契约面用例）+ fixtures/rr3-sense.toml
│   │   ├── rr-hal/                ← HAL trait 与数据类型（§5）：lib.rs + backend.rs
│   │   ├── rr-ota/                ← 升级通道的类型形状（§8、ADR-0007 决策二）
│   │   ├── rrd/                   ← 主守护进程（装配层）
│   │   │   ├── src/               ← 九个文件，逐个职责见下
│   │   │   │   ├── cli.rs         ← 命令行与总线路径解析（平台无关）
│   │   │   │   ├── main.rs        ← 启动序（§7.1）
│   │   │   │   ├── lib.rs         ← 平台无关模块图 + `#[cfg(unix)]` 门 + 横幅/日志出口
│   │   │   │   ├── manifest.rs    ← manifest 的运行期事实（`Rr3ManifestFacts`）
│   │   │   │   ├── registry.rs    ← 在册表与阶段机（§4.2）
│   │   │   │   ├── rpc.rs         ← 上行 JSON-RPC 方法的编解码
│   │   │   │   ├── topics_load.rs ← 契约主题表装载 + 仅测试的主题叠加层
│   │   │   │   ├── conn.rs        ← 单连接服务（reader + writer 线程）——`#[cfg(unix)]`
│   │   │   │   └── daemon.rs      ← 监听 / accept / 连接线程编排——`#[cfg(unix)]`
│   │   │   └── tests/             ← register_publish.rs、socket_banner.rs（均 `#![cfg(unix)]`）
│   │   │                             + support/mod.rs（起真 rrd 子进程的线级助手）
│   │   └── tests/fixtures/        ← sim-topics.toml：仅测试的主题叠加层夹具（不吃契约 pin）
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
├── docs/                          ← 四档结构与"事实归属"表见其 README.md
│   ├── README.md                  ← 索引：哪类事实只有一个权威落点
│   ├── architecture/architecture.md ← 本档（唯一架构图）
│   ├── status.md                  ← 唯一实况快照
│   └── process/                   ← conventions.md、naming.md、adr/（ADR-0001~0007）、dev/
│
└── legacy/                        ← 冻结四棵 ESP32 树：core/ + firmware-c/ + firmware-rs/ + rust/
                                     （只读参照，不再演进；组件计数见 legacy/README.md）
```

本树只画**已存在的结构**：`base/`、`modules/`、`tools/` 与每个 crate、每个模块目录还各有一份
就地自述的 `README.md`（职责写在那里，进度不写）。

---

## 4. 模块模型

### 4.1 manifest（module.toml）

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
required = []               # 缺失 → 模块不启动
optional = ["net.online"]   # 缺席 → 降级运行

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
  不认模块名，见 §2 原则 2）；仲裁这个**动作**归 rrd 的装载/拉起环节，不在校验层做。校验层只保证
  两条事实：一条**语法**——`capability` 的形状 `^[a-z]+(\.[a-z_]+)+$`（`provides` 与 `depends`
  两侧同规则）；一条**在册**——`depends.required`/`depends.optional` 的每一项必须命中
  `module.schema.toml` 的 `base_capabilities` 白名单。后者是"跨模块能力登记面"定型之前的过渡
  约束，它让依赖声明不至于写成空口白话。仲裁环节做到哪一步属实况，见 [`../status.md`](../status.md)。
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
细节表留给 `reference/rrd-registry.md`，本档只留判据）：

1. **task 号只增不回收**：`0` 是广播、`1` 是 rrd（`RR3_BROADCAST_DST_TASK` / `RR3_TASK_RRD`），
   模块从 `RR3_TASK_FIRST_MODULE`（值 2）起单调分配；条目 failed 或被移除后旧号作废、绝不复用
   ——否则一条迟到的旧心跳就能顶替新连接。
2. **到期判定用显式单调毫秒**，且用 `saturating_sub`：时钟倒退宁可判"未到期"，也不回绕成
   巨大差值把健康模块集体判死。
3. **在册模块上限** `RR3_REG_MAX_MODULES`（值 16）：目标 5+1 模块留余量，超额注册直接拒；
   表满但同名条目是 `Failed` 属合法的替换式重注册，不被容量门拒。

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
`InvalidParam(String)`）。占位实现 `Rr3UnavailableSource` 让五个 trait 的
每个方法都回 `Rr3HalError::Unsupported`——它承载的是设计裁定"**能力缺失必须显式回错，不许静默退化**"，
不是一时代替后端的临时件；object-safety 由 `lib.rs` 的 `dyn` 装箱用例在 `cargo test` 时机器校验
（注册表要持有异构设备集合，trait 对象装不进盒子就是接口设计错了）。

分层与落点（**只写位置，不写进度**）：

| 层 | 位置 | 承载 |
|---|---|---|
| traits + 数据类型 | `rr-hal/src/lib.rs` | 上面五个 trait 与配套类型，模块只见 trait、不见 Linux 细节 |
| 后端标识常量 | `rr-hal/src/backend.rs` | `RR3_BACKEND_{LINUX,SIM,PLACEHOLDER}`，写入日志与诊断主题 `rr3-runtime/module/state` |
| linux 后端（ALSA / gpiod / NetworkManager-D-Bus） | `rr-hal` 内 `linux` 后端 | 落点定在 `rr-hal`，承载面见 `backend.rs` 的表 |
| sim 后端（主机联调） | `rr-hal` 内 `sim` 后端 + `tools/sim` 的虚拟设备 | 让 CI 与无硬件开发共用同一套 trait |
| `shm` 加速路径 | `rr-ipc` 的传输层（不在 HAL） | 实时控制面的零拷贝通道，见 §7.4 与 ADR-0007 |

两条不可让的裁定：

1. **反射环不进 HAL**：1k–10kHz 的电机 FOC/PID 闭环留在 `rr3-act` 进程内，永不进 trait、
   更不进总线（ADR-0007 §裁定 1）。HAL 只承载慢速事件级接口。
2. **播放面在本节 trait 清单之外**：上表只列上行拾音 `Rr3AudioSource`；
   `rr3-interact` 消费侧的输出接口与后端在同一次接口定型里给形状，形状未定前本节不预先造名字。

新增 trait/后端时本节与 `rr-hal` 源码同批更新。谁在本节写下"某后端已可用"，就是把实况塞进了
设计文档——那类句子一律落 [`../status.md`](../status.md)。

---

## 6. IPC 与事件总线（Airy IPC，ADR-0006）

### 6.1 协议：采用 Airy IPC 线格式

- **128B 定长消息头**（Layout C，L1 永久稳定）+ **5 种 payload 类型**：REQUEST/RESPONSE（RPC）、EVENT（事件总线）、STREAM（音频/运动数据流，有序投递）、NOTIFICATION（心跳/系统通知）。枚举是 `Rr3PayloadType`（`Request` = 0x0001 起连续取值），字节布局的唯一事实源是 `contracts/envelope.schema.json`
- 事务面 payload 默认 JSON-RPC 2.0；rock3 的 topic 字符串与 `phase` 置于 payload 内，不占用消息头 reserved 区。`phase` 的字面量就是 §4.2 那张词表（`registered`/`running`/`degraded`/`failed`），由 `Rr3Phase::as_str()` 给出；模块主动退场那条广播额外用 `unregistered`
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
- **transport/mod.rs**：只有分层注记与两个路径常量，**没有 `trait Transport`**。这是裁定而不是缺项：
  零拷贝的 buffer 所有权移交语义与 ADR-0007 §裁定 3 的取舍必须一起定型，先落一个 trait 反而会
  造出一个要被推翻的接口。分层设计仍是 uds（事件/事务面）/ shm（实时控制面）/ tcp（多机，预留），
  反射环永远不进任何一层（ADR-0007 §裁定 1）；各层的实现进度见 [`../status.md`](../status.md)。

套接字路径的三级决定：`--socket` > 环境变量 `RR3_BUS_PATH`（常量键 `RR3_BUS_PATH_ENV`）>
`RR3_UDS_PATH_DEFAULT`（`/var/run/rock3-ipc.sock`，自有路径，避免与真实 Airymax 运行时同机冲突）。
横幅回显的路径在打印**之前**就已绝对化，与 `UnixListener::bind` 取同一个值——两处各说一套比直接
报错更难查。参数与默认值的字面量归 `reference/rrd-cli.md`（本档不复述）。

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
   判定是关联函数、不碰在册表，所以 **`sender_name` 必须由调用方从连接持有的 task 号反查**
   （`rr3_reg_name_of(src_task)`），**绝不得取自帧体或注册报文里自称的 name 字段**——这条前提
   本函数替不了调用方保证。越权只计数不牵连：`rr3_reg_note_denied` 饱和加，既不改发布者生命周期
   状态也不影响其它连接的转发。返回值与错误映射的细节表留给 `reference/rrd-rpc.md`。

入口 ③ 给的是**判定逻辑**；把它接到线上流量（UDS 受理路径上的发布分支）是 `conn.rs` 的职责，
"接到了哪一步"属实况，见 [`../status.md`](../status.md)。上面的代码块是**设计期草图**，
`contracts/topics.toml` 才是事实源，两者不一致时以注册表为准；注册表按"只增不改"演进，
各模块其余主题随里程碑逐条 append，不预先占位。登记之前，任何 manifest 的
`events.publishes`/`rpc.exposes` 引用未注册主题都会被入口 ② 拒绝。

> **`rr3-act` 的对账**：注册表里 `rr3-act` 的**已登记面是舵机与电机**
> （`rr3-act/servo/command` request、`rr3-act/servo/state` stream、`rr3-act/motor/reflex`
> notification、`rr3-act/led/effect` request，owner 角色写作"执行：舵机/电机、反射环宿主进程"）。
> ADR-0002 §1 把 rr3-act 的职责写作"继电器、未来运动控制（gpiod、PWM）"——继电器那一半不在
> 注册表里：`rr3-act/relay/*`（本节草图与 §7.2 数据流图里的 `relay/set`、`relay/state`）
> 只是**示意串**，入口 ② 与入口 ③ 都会拒。要落地必须先按"契约只增不改"把它们 **APPEND** 进
> `contracts/topics.toml`（同批更新 `rr-registry` 的形状 pin），再在 manifest 里引用；
> ADR-0002 的决策正文不改（继电器本身没有被本批裁定否定），在册与否只查注册表。

---

## 7. 关键数据流

### 7.1 启动链

设计上的完整链条（目标形态，四环由 rrd 的装载/仲裁/spawn/监督环节承担）：

```
kernel → init → rrd → 加载 manifests → 依赖仲裁 →
按 start_priority 拉起模块 → 心跳监督 → 事件总线就绪
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
   Unix     → `rr3_daemon_run`：清残留套接字 → `UnixListener::bind` → accept 循环，
              每连接一条线程（该环节永不返回，除非 bind 失败）
   非 Unix  → 往 stderr 打 `RRD_UNSUPPORTED` 并以非 0 退出
```

两条是设计约束、不是实现细节：

- **④ 在两个平台都做**。契约坏了要当场看见，而不是"等不到 UDS"——所以非 Unix 平台同样先装载、
  先打印横幅，之后才声明不支持。横幅因此是"装载成功"的凭证，不是"总线可用"的凭证。
- **②③ 的绝对化失败即致命**。按 CLI 原词回显会让运维按自己 shell 的 CWD 去连，而 daemon 绑的是
  rrd 的 CWD 相对路径——两处各说一套比直接报错更难查。

①–⑤ 与 ⑥ 的 Unix 分支属于 rrd 的**装配层**职责；目标链条里"加载 manifests → 依赖仲裁 →
按 `start_priority` 拉起 → 心跳监督"这四环落在 spawn/仲裁/监视环节上，本档不写它们到哪一步
（见 [`../status.md`](../status.md)）。`--manifest` 传入的路径在拉起时按 `start_priority` 重排；
注册环节本身**只拒语法**——`entry` 的绝对性在握手处查（相对路径即 `Rr3StatusCode::Invalid`，
映射 `AIRY_EINVAL`，并点名字段），路径的存在性与可执行位留给 spawn 那一刻，在册表不替文件系统猜。

### 7.2 对话主闭环（legacy main.c 对话策略的分布式版）

> **草图（示意）**：下图只表达数据流向。其中的主题串**以 `contracts/topics.toml` 为准**——
> `rr3-act/relay/state`、`rr3-brain/dialog/response`、`rr3-connect/wifi/state`、
> `rr3-power/battery/low` 等**均未登记**（对账见 §6.3 的 `rr3-act` 注记），使用前须先 append 契约。

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
| R1 | Buildroot + RK3566 DTS/音频补丁工作量超预期 | M1 | 先 Radxa ZERO 3 现成板验证，自绘板后置；必要时 Armbian 临时垫层 |
| R2 | ALSA 全双工延迟不达标 | M3 | legacy 闭环 2~5s 为基线，Linux 侧硬件换血预期更好；先实测再优化 |
| R3 | Python brain GIL/冷启动 | M4 | 策略预载；必要时 brain 拆子进程 |
| R4 | Rust↔Python 契约漂移 | M2+ | `contracts/` 目录 [SC] 唯一事实源；漂移由**三个入口**分层拦：`rr3-mkmod` 生成自检、`rr-registry` 契约测试、rrd 在册表的 `rr3_reg_publish_check`（三者职责与顺序见 §6.3）。前两个在提交前跑，第三个在线上判定；它接到线上流量的哪一段属实况，见 [`../status.md`](../status.md) |
| R5 | 0.5L 体积散热（RK3566 被动散热） | 硬件 | 选型时评估无风扇功耗档，后期实机温测 |
| R6 | A-IPC 自实现运行时与标准偏差 | M2 | IPC-T-001~003 必测；半年复查时对照标准版本变更 |
| R7 | 待机 <1W 需 suspend/电源状态机与 OS 深度协同 | M5 | rr3-power 与 rrd 电源状态机设计前置，模块声明 wake 权限 |
