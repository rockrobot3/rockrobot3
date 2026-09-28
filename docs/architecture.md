# rock3 总体架构文档

- **状态**：v0.2（草案落定，随实现迭代）
- **日期**：2026-09-28
- **关联决策**：[ADR-0001](adr/ADR-0001-平台选型-linux66-buildroot-rk3566.md) · [ADR-0002](adr/ADR-0002-模块模型-5加1-进程隔离.md) · [ADR-0003](adr/ADR-0003-IPC选型-uds-jsonrpc-事件总线.md) · [ADR-0004](adr/ADR-0004-Airymax接入路径-运行时层对接.md) · [ADR-0005](adr/ADR-0005-命名规约-rr3前缀矩阵.md)
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

```
rock3/
├── contracts/                       ← [SC] 字节级共享契约（Rust/Python 双方唯一事实源）
│   ├── envelope.schema.json         ← 消息封套：定长头 + JSON-RPC 2.0 body
│   ├── topics.toml                  ← 全部主题注册表（rrd 校验前缀归属）
│   └── events/*.schema.json         ← 各模块事件负载 schema
│
├── base/
│   ├── os/                          ← Buildroot 层
│   │   ├── configs/rock3_zero3_defconfig
│   │   ├── board/radxa-zero3/       ← genimage.cfg（A/B 布局）、DTS overlay、post-build.sh
│   │   ├── patches/                 ← 内核/Buildroot 补丁（含 maskrom 自烧录）
│   │   └── package/                 ← rrd 与 rr3-* 模块打包
│   ├── runtime/                     ← Rust workspace
│   │   ├── rr-ipc/                  ← envelope 编解码、UDS 帧协议
│   │   ├── rr-bus/                  ← 主题匹配订阅、QoS、回压
│   │   ├── rr-registry/             ← manifest 解析、依赖仲裁、生命周期状态机
│   │   ├── rr-hal/                  ← traits + linux/{alsa,gpiod,nm} + sim/ 双实现
│   │   ├── rr-ota/                  ← slot 状态机、回滚判定、factory recovery
│   │   └── rrd/                     ← 主守护进程（装配层：supervisor/health/config）
│   └── ota/                         ← 分区布局文档 + rr3-ota-ctl CLI
│
├── modules/                         ← 模块统一内构：module.toml + config.toml + src/ + README.md (+ tests/)
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
│   └── sim/             bin: rr3-sim，mock HAL + 假模块，x86 全链路联调
│
├── docs/                ← 本文档 + naming.md + adr/
└── legacy/              ← 冻结：firmware-c/ + core/（只读参照，不再演进）
```

---

## 4. 模块模型

### 4.1 manifest（module.toml）

schema 定义在 `base/runtime/rr-registry/src/module.schema.toml`，mkmodule 与 rrd 共用：

```toml
[manifest]
schema_version = 1
name    = "rr3-sense"       # 必须与目录名/进程名/IPC 前缀一致，rrd 启动时校验
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
- 事件/RPC 主题必须以 `manifest.name` 为前缀，越权发布在注册时拒绝
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

```rust
// traits/ —— [IND] 独立层：模块只见 trait，不见 Linux 细节
#[async_trait]
pub trait Rr3AudioInput: Send + Sync {
    async fn start_capture(&self, cfg: AudioCfg) -> Result<PcmStream>;
    async fn set_mic_mute(&self, on: bool) -> Result<()>;
}
pub trait Rr3AudioOutput {
    async fn play(&self, pcm: PcmStream, gain: u8) -> Result<()>;
}
pub trait Rr3GpioOut  { async fn set_level(&self, level: bool) -> Result<()>; }
pub trait Rr3PowerMon { async fn battery(&self) -> Result<BatteryState>; }
pub trait Rr3NetLink  { async fn state(&self) -> Result<LinkState>; }
```

| 层 | 位置 | 说明 |
|---|---|---|
| traits | `rr-hal/src/traits/` | 平台无关接口 |
| linux 实现 | `rr-hal/src/linux/{alsa,gpiod,nm}.rs` | ALSA / gpiod / NetworkManager(D-Bus) |
| sim 实现 | `rr-hal/src/sim/` | 主机联调（配合 rr3-sim） |

---

## 6. IPC 与事件总线

### 6.1 消息封套（contracts/envelope.schema.json，[SC] 字节级契约）

定长 32B 消息头 + JSON-RPC 2.0 body（对齐 AirymaxRT gateway 规约）：

```json
{
  "trace_id": "b7f3…",
  "ts_ns": 1790000000000000000,
  "source": "rr3-sense",
  "phase": "normal",
  "topic": "rr3-sense/audio/vad_start",
  "params": { "rms": 4120 }
}
```

`phase ∈ {normal, degraded, recovery}` 贯穿全链路，供 brain 与 base 做降级决策。

### 6.2 主题命名

格式 `rr3-<模块>/<域>/<事件>`，全小写 snake_case。已规划主题：

```
rr3-sense/audio/vad_start      rr3-sense/audio/asr_partial   rr3-sense/audio/asr_final
rr3-brain/dialog/response      rr3-brain/dialog/error
rr3-act/relay/set              rr3-act/relay/state
rr3-interact/audio/play_done
rr3-power/battery/state        rr3-power/battery/low         rr3-power/charger/event
rr3-connect/wifi/state         rr3-connect/bt/state
base/module/lifecycle          base/module/crash             base/ota/progress
```

主题全量登记于 `contracts/topics.toml`，rrd 启动时按 manifest.name 校验前缀归属。

---

## 7. 关键数据流

### 7.1 启动链

```
kernel → init → rrd → 加载 manifests → 依赖仲裁(resolved) →
按 start_priority 拉起模块 → 心跳监督 → 事件总线就绪
```

### 7.2 对话主闭环（legacy main.c 对话策略的分布式版）

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

急停、限位、低电保护逻辑在 rrd/rr-hal：即使 rr3-brain 崩溃或未注册，rrd 仍监听 `rr3-power/battery/low` 并直接执行保护动作。

---

## 8. legacy 迁移映射

| legacy（已冻结） | 去向 | 方式 |
|---|---|---|
| core/audio | rr3-sense（采集+VAD）/ rr3-interact（播放） | HAL 换 ALSA，VAD 逻辑移植 |
| core/asr · llm · tts | rr3-brain/cloud/ | Python 直接移植 HTTP 逻辑（含 Content-Length 等踩坑） |
| core/relay | rr3-act | gpiod |
| core/link/wifi · btctl | rr3-connect | NetworkManager(D-Bus) / BlueZ |
| core/panel | rr3-interact/panel | HTML 资产直接搬入 |
| core/applog · data | base/runtime | 日志与计数是基座职责 |
| firmware-c/main/config.h | rr3-brain/config.toml | 对话策略参数平移 |
| firmware-c/main/main.c | rr3-brain/dialog.py | 编排逻辑移植 |

---

## 9. Airymax 接入路径（摘要，详见 ADR-0004）

- 接入点定在**运行时层**：rock3 经 JSON-RPC 对接 AirymaxRT gateway（agentrt 仓真实可用，35 万行 C）
- OS 层每半年复查 Airymax distro 仓的 aarch64/headless 产物，有则评估切换
- sdk-rust（Apache-2.0）可作 rr3-connect 试点依赖
- 许可证走 Apache-2.0 路径；memoryrovol（商业 EULA）不引入

---

## 10. 里程碑

| 里程碑 | 内容 | 验收标准 |
|---|---|---|
| M0 | 仓库骨架 + ADR-0001~0005 + legacy 冻结 | 目录树落地，rr3-mkmod 能生成合规模块 |
| M1 | base/os：Buildroot 在 Radxa ZERO 3 跑通 | 串口出 shell；maskrom 自烧录镜像成功 |
| M2 | rrd MVP：bus + registry + sim 双假模块 | x86 与板上均能注册/心跳/崩溃自动重启 |
| M3 | rr3-sense + rr3-interact：ALSA 采集与播放 | 板上录音回放，VAD 事件上总线 |
| M4 | rr3-brain：移植 asr/llm/tts | 全语音对话闭环（legacy 功能等价） |
| M5 | rr3-act / rr3-connect / rr3-power | 语音控继电器 + WiFi 配网 + 电量上报 |
| M6 | A/B OTA + 恢复出厂 | 断电升级不 brick，坏 slot 自动回滚 |

---

## 11. 风险登记

| # | 风险 | 影响里程碑 | 缓解 |
|---|---|---|---|
| R1 | Buildroot + RK3566 DTS/音频补丁工作量超预期 | M1 | 先 Radxa ZERO 3 现成板验证，自绘板后置；必要时 Armbian 临时垫层 |
| R2 | ALSA 全双工延迟不达标 | M3 | legacy 闭环 2~5s 为基线，Linux 侧硬件换血预期更好；先实测再优化 |
| R3 | Python brain GIL/冷启动 | M4 | 策略预载；必要时 brain 拆子进程 |
| R4 | Rust↔Python 契约漂移 | M2+ | contracts/ 目录 [SC] 唯一事实源 + rrd 启动校验 |
| R5 | 0.5L 体积散热（RK3566 被动散热） | 硬件 | 选型时评估无风扇功耗档，后期实机温测 |
