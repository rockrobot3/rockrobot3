# rock3 总体架构文档

- **状态**：v0.3（草案落定，随实现迭代）
- **日期**：2026-09-28
- **关联决策**：[ADR-0001](adr/ADR-0001-平台选型-linux66-buildroot-rk3566.md) · [ADR-0002](adr/ADR-0002-模块模型-5加1-进程隔离.md) · [ADR-0003](adr/ADR-0003-IPC选型-uds-jsonrpc-事件总线.md) · [ADR-0004](adr/ADR-0004-Airymax接入路径-运行时层对接.md) · [ADR-0005](adr/ADR-0005-命名规约-rr3前缀矩阵.md) · [ADR-0006](adr/ADR-0006-采用AiryIPC线格式与rr-ipc分层.md) · [ADR-0007](adr/ADR-0007-控制面分层与模块级升级.md)
- **命名速查**：[naming.md](naming.md)

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

> **状态（2026-09-29，v0.0.1）**：`contracts/`、`base/runtime/`（6 crate）、`tools/mkmodule`、
> `tools/ci/` 已建立并通过 `bash tools/ci/check.sh`；`modules/` 仅有规约 README（尚无模块，约定见
> [conventions.md](conventions.md)）；ESP32 在编固件已迁入 `legacy/`（tag `v0-pre-legacy`）。
> 下图/下表仍含 M1–M6 未落地项，标注"规划"者以里程碑表（§10）为准。

```
rock3/
├── contracts/                       ← [SC] 字节级共享契约（Rust/Python 双方唯一事实源）
│   ├── envelope.schema.json         ← A-IPC 128B 消息头 + 5 种 payload 类型契约（ADR-0006）
│   ├── topics.toml                  ← 全部主题注册表（校验入口见 §6.3）
│   ├── golden/event-header-128.hex  ← 一条真实头的 hex 向量（同机字节基准，见 contracts/README.md）
│   └── events/*.schema.json         ← 各模块事件负载 schema
│
├── base/
│   ├── os/                          ← Buildroot 层
│   │   ├── configs/rock3_zero3_defconfig
│   │   ├── board/radxa-zero3/       ← genimage.cfg（A/B 布局）、DTS overlay、post-build.sh
│   │   ├── patches/                 ← 内核/Buildroot 补丁（含 maskrom 自烧录）
│   │   └── package/                 ← rrd 与 rr3-* 模块打包
│   ├── runtime/                     ← Rust workspace
│   │   ├── rr-ipc/                  ← A-IPC 协议层（纯编解码）+ transport trait（uds/shm/tcp）
│   │   ├── rr-bus/                  ← 主题匹配订阅、QoS、回压
│   │   ├── rr-registry/             ← manifest 解析、依赖仲裁、生命周期状态机
│   │   ├── rr-hal/                  ← HAL trait 与数据类型（lib.rs）+ 后端名常量（backend.rs）；linux/sim 后端规划（M2+）
│   │   ├── rr-ota/                  ← slot 状态机、回滚判定、factory recovery、模块级更新（预留）
│   │   └── rrd/                     ← 主守护进程（装配层：supervisor/health/config）
│   └── ota/                         ← 分区布局文档 + rr3-ota-ctl CLI
│
├── modules/                         ← 模块统一内构（rr3-mkmod 落盘五件）：module.toml + config.toml + Cargo.toml + src/main.rs + README.md
│   ├── rr3-sense/       采集+VAD（src/audio/{capture,vad}.rs）
│   ├── rr3-act/         执行：relay.rs、motion/（预留）
│   ├── rr3-interact/    播放 player.rs + Web 面板 panel/{server.rs,www/}
│   ├── rr3-power/       battery.rs、charger.rs
│   ├── rr3-connect/     wifi.rs（NetworkManager/D-Bus）、bt.rs（BlueZ）
│   └── rr3-brain/       Python：dialog.py（对话策略）+ cloud/{asr,llm,tts}.py
│
├── tools/
│   ├── mkmodule/        bin: rr3-mkmod，生成合规模块骨架
│   ├── rrctl/           bin: rr3-ctl，订阅总线查状态/日志/触发 OTA
│   ├── sim/             bin: rr3-sim，mock HAL + 假模块，x86 全链路联调
│   └── ci/check.sh      ← 规约检查唯一入口（五步，见 conventions.md §2）
│
├── docs/                ← 本文档 + naming.md + conventions.md + adr/
└── legacy/              ← 冻结四棵 ESP32 树：firmware-c/ + core/ + firmware-rs/ + rust/
                          （只读参照，不再演进；组件计数见 legacy/README.md）
```

---

## 4. 模块模型

### 4.1 manifest（module.toml）

schema 定义在 `base/runtime/rr-registry/src/module.schema.toml`，mkmodule 与 rrd 共用：

```toml
[manifest]
schema_version = 1
name    = "rr3-sense"       # 必须与目录名/进程名/IPC 前缀一致（校验入口见下方要点与 §6.3）
version = "0.1.0"
type    = "module"          # module | brain
lang    = "rust"            # rust | python
entry   = "rr3-sense"

[provides]
capabilities = ["audio.capture", "audio.vad"]   # 能力声明，registry 依赖仲裁的唯一依据

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
- 事件/RPC 主题必须以 `manifest.name` 为前缀，越权发布一律拒绝；校验入口现为 `rr3-mkmod`
  生成自检 + `rr-registry` 契约测试，rrd 运行时注册校验属 M1/M2 待落地
- 模块间依赖走 capabilities，未来更换传感器模块不影响 brain

### 4.2 生命周期（rr-registry 状态机）

```
registered → resolved(依赖满足) → starting → running ⇄ degraded
                                    │                │
                                    ▼                ▼
                                  failed ◄──────── crash
                                    │
                                    ▼  按 restart_policy 重启 / 标记停用
```

- 心跳超时（watchdog_timeout_ms）→ 记录 → 按 restart_policy 处理
- required 能力缺失 → 模块停留 registered，标记整体 degraded

---

## 5. HAL 分层（rr-hal）

> 本节按 **当前 HEAD 实况**书写。v0.0.1 的 HAL 只有 trait 与数据类型，没有任何平台后端；
> 唯一实现处是 `base/runtime/rr-hal/src/lib.rs`，`base/runtime/rr-hal/src/backend.rs` 里只有
> 后端名常量（`RR3_BACKEND_{LINUX,SIM,PLACEHOLDER}`）与落地排期，没有实现文件。

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
`Rr3LinkState`，以及错误枚举 `Rr3HalError`。占位实现 `Rr3UnavailableSource` 让五个 trait 的
每个方法都返回 `Unsupported`——object-safety 由 `lib.rs` 的测试在 `cargo test` 时机器校验。

| 层 | 位置 | 状态 |
|---|---|---|
| traits + 数据类型 | `rr-hal/src/lib.rs` | 已在 HEAD |
| 占位后端 + 后端名常量 | `rr-hal/src/lib.rs` / `rr-hal/src/backend.rs` | 已在 HEAD |
| linux 后端（ALSA / gpiod / NetworkManager-D-Bus） | **规划（M2+），当前 HEAD 无此文件** | 未落地 |
| sim 后端（配合 `tools/sim`，主机联调） | **规划（M2+），当前 HEAD 无此文件** | 未落地 |
| 音频**输出** trait（播放，`rr3-interact` 的消费面） | **规划（M2+），当前 HEAD 无此 trait**（只有上行 `Rr3AudioSource`） | 未落地 |

> 本文档 v0.2 在此列过一套与 HEAD 不符的接口名与 `rr-hal/src/{traits,linux,sim}/` 目录布局
> （设计期草图当既成事实写），现已按上面的实况整段重写；后续新增 trait/后端时，本节须与
> `rr-hal` 源码同批更新。

---

## 6. IPC 与事件总线（Airy IPC，ADR-0006）

### 6.1 协议：采用 Airy IPC 线格式

- **128B 定长消息头**（Layout C，L1 永久稳定）+ **5 种 payload 类型**：REQUEST/RESPONSE（RPC）、EVENT（事件总线）、STREAM（音频/运动数据流，有序投递）、NOTIFICATION（心跳/系统通知）
- 事务面 payload 默认 JSON-RPC 2.0；rock3 的 topic 字符串与 `phase`（normal/degraded/recovery）置于 payload 内，不占用消息头 reserved 区
- RESPONSE.status 采用 AIRY_E* 子集；src_task 由 rrd（受信运行时）在模块注册时分配并校验
- capability 令牌、内核强制等 AirymaxRT 运行时机制不引入（单机可信环境），具身化时再评估（ADR-0007）

### 6.2 rr-ipc 分层

```
rr-ipc/
├── protocol/     ← 128B 头与 payload 纯编解码，与传输无关（Rust/Python 对齐同一 schema）
└── transport/    ← trait Transport：uds（M2 唯一实现）/ shm（控制面，M5）/ tcp（多机，预留）
```

UDS 路径 `/var/run/rock3-ipc.sock`（自有路径，避免与真实 Airymax 运行时同机冲突）。

### 6.3 主题命名

格式 `rr3-<模块>/<域>/<事件>`，全小写 snake_case。v0.0.1 规划的主题草图（**示意，非注册表**）：

```
rr3-sense/audio/vad_start      rr3-sense/audio/asr_partial   rr3-sense/audio/asr_final
rr3-brain/dialog/response      rr3-brain/dialog/error
rr3-act/relay/set              rr3-act/relay/state          ← 示意：relay/* 尚未登记
rr3-interact/audio/play_done
rr3-power/battery/state        rr3-power/battery/low         rr3-power/charger/event
rr3-connect/wifi/state         rr3-connect/bt/state
rr3-runtime/module/state       rr3-runtime/system/heartbeat
```

> **v0.0.1 裁定**：base 运行时主题命名空间改用 `rr3-runtime/`（如 `rr3-runtime/module/state`、
> `rr3-runtime/system/heartbeat`），不再使用 `base/` 前缀——契约要求每段主题首段等于某个
> `[manifest].name`，而模块名规则是 `^rr3-[a-z]+$`。全量已登记主题以
> [`contracts/topics.toml`](../contracts/topics.toml) 为准（7 owner / 21 topic）。

前缀归属的**校验入口现为** `rr3-mkmod` 生成自检 + `rr-registry` 契约测试（`cargo test -p
rr-registry`），**rrd 运行时注册校验属 M1/M2 待落地**。上面的代码块是**设计期草图**：
`contracts/topics.toml` 才是事实源（v0.0.1 收口时 7 owner / 21 topic），两者不一致时以注册表为准；
注册表按"只增不改"演进，各模块其余主题随里程碑（M2+）逐条 append，不预先占位。
登记之前，任何 manifest 的 `events.publishes`/`rpc.exposes` 引用未注册主题都会被契约测试拒绝。

> **`rr3-act` 的对账（Task 4 评审遗留项）**：注册表里 `rr3-act` 的**已登记面是舵机与电机**
> （`rr3-act/servo/command` request、`rr3-act/servo/state` stream、`rr3-act/motor/reflex`
> notification、`rr3-act/led/effect` request，owner 角色写作"执行：舵机/电机、反射环宿主进程"）。
> ADR-0002 §1 把 rr3-act 的职责写作"继电器、未来运动控制（gpiod、PWM）"——**继电器那一半
> 尚未进注册表**：`rr3-act/relay/*`（本节草图与 §7.2 数据流图里的 `relay/set`、`relay/state`）
> 都是**未登记主题，仅作示意**。要落地必须先按"契约只增不改"把它们 **APPEND** 进
> `contracts/topics.toml`（同批更新 `rr-registry` 的形状 pin 与本节的 7 owner / 21 topic 计数），
> 再在 manifest 里引用；ADR-0002 的决策正文不改（继电器本身没有被本批裁定否定），本节即其
> 状态注记位置。

---

## 7. 关键数据流

### 7.1 启动链

```
kernel → init → rrd → 加载 manifests → 依赖仲裁(resolved) →
按 start_priority 拉起模块 → 心跳监督 → 事件总线就绪
```

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
`rr3-power/battery/low`（**示意：该主题未登记，已登记的是 `rr3-power/battery/state`**）并直接执行保护动作。

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

| 里程碑 | 内容 | 验收标准 |
|---|---|---|
| M0 | 仓库骨架 + ADR-0001~0005 + legacy 冻结 | 目录树落地，rr3-mkmod 能生成合规模块 |
| M1 | base/os：Buildroot 在 Radxa ZERO 3 跑通 | 串口出 shell；maskrom 自烧录镜像成功 |
| M2 | rrd MVP：A-IPC 协议层 + uds transport + registry + sim 双假模块 | 注册/心跳/崩溃自动重启；IPC-T-001/002/003 通过 |
| M3 | rr3-sense + rr3-interact：ALSA 采集与播放 | 板上录音回放，VAD 事件上总线，音频走 STREAM |
| M4 | rr3-brain：移植 asr/llm/tts | 全语音对话闭环（legacy 功能等价） |
| M5 | rr3-act / rr3-connect / rr3-power | 语音控继电器 + WiFi 配网 + 电量上报；shm 控制面初版 |
| M6 | A/B OTA + 恢复出厂 | 断电升级不 brick，坏 slot 自动回滚；genimage 预留模块独立可写分区 |

---

## 11. 风险登记

| # | 风险 | 影响里程碑 | 缓解 |
|---|---|---|---|
| R1 | Buildroot + RK3566 DTS/音频补丁工作量超预期 | M1 | 先 Radxa ZERO 3 现成板验证，自绘板后置；必要时 Armbian 临时垫层 |
| R2 | ALSA 全双工延迟不达标 | M3 | legacy 闭环 2~5s 为基线，Linux 侧硬件换血预期更好；先实测再优化 |
| R3 | Python brain GIL/冷启动 | M4 | 策略预载；必要时 brain 拆子进程 |
| R4 | Rust↔Python 契约漂移 | M2+ | contracts/ 目录 [SC] 唯一事实源 + 契约测试校验（rrd 运行时注册校验属 M1/M2 待落地） |
| R5 | 0.5L 体积散热（RK3566 被动散热） | 硬件 | 选型时评估无风扇功耗档，后期实机温测 |
| R6 | A-IPC 自实现运行时与标准偏差 | M2 | IPC-T-001~003 必测；半年复查时对照标准版本变更 |
| R7 | 待机 <1W 需 suspend/电源状态机与 OS 深度协同 | M5 | rr3-power 与 rrd 电源状态机设计前置，模块声明 wake 权限 |
