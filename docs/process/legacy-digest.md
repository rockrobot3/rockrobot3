# legacy 消化盘点（三分类台账）——已归档

- **状态（2026-10-05 0.0.4 批F）**：**消化完成，台账归档**。待融合 3 项全部收敛
  （panel 批C/D、data 批B、btctl 批E，证据见下表），退役批已将 `legacy/` 整树删除
  （单提交，git 历史 + tag `v0-pre-legacy` 双锚点可回溯）。本文件保留为历史台账，
  不再演进；"某个 legacy 语义在新架构哪"仍可查下表与 architecture §8 映射。
- **原定位**：`legacy/`（ESP32 冻结区）逐组件消化盘点的**唯一落点**——conventions §5
  "只读消化 → 融合完成 → 退役"三阶段口径的排期台账。architecture §8 是"去向"的设计
  语义（哪类语义归哪个模块），本文件是"消化实况"（每个组件的语义走到哪一步、证据在哪）。
- **盘点日期**：2026-10-05（0.0.3 T2，锚点 `26025d1` 树）
- **三分类判据**：
  - **已融合**——该组件的核心语义在新架构已有实装落点，证据给到符号/文档路径；
    "板上件语义"（ALSA/NetworkManager/BlueZ 等真机后端）不影响分类，只作限定注记
  - **待融合**——有真实语义且新架构需要，但尚未实装；必须给出去向与排期批次
  - **退役**——无语义可迁（试验残留 / 平台不可移植 / 接口预留零实现）
- **只读约束（已随退役批解除）**：消化期本盘点只引用证据路径，不修改 `legacy/`
  内任何文件；退役批整树删除，约束随之完成使命。其 README 断链已知偏差已随
  退役批注销（status.md 同步）。

## 一、盘点表（core 九目录 + 三棵试验树）

| legacy 组件 | 语义摘要 | 分类 | 去向 / 证据 |
|---|---|---|---|
| `core/audio` | I2S 全双工采集+播放；单采样 IIR 去直流+增益、峰值输出（VAD 用）；16kHz/256 帧契约；提示音 | **已融合** | 采集+VAD → `modules/rr3-sense`（vad_start 事件与 PCM 流，事件 schema [`contracts/events/sense-audio-vad_start.schema.json`](../../contracts/events/sense-audio-vad_start.schema.json)）；播放 → `modules/rr3-interact`（`rr3-interact/tts/text` 事务面）；IIR/增益/峰值语义随 sense 实装；HAL 换 ALSA（板上件，cfg 门，status.md M3 行）。M3 收口 |
| `core/asr` · `core/llm` · `core/tts` | 三家云 HTTP 客户端（含 Content-Length 等踩坑） | **已融合** | `modules/rr3-brain/rr3_brain/cloud/`（urllib real + `RR3_BRAIN_CLOUD=mock` 注入点，ADR-0014）；M4 收口 |
| `core/relay` | 继电器通断（`relay_set(bool)`，默认断开，GPIO21） | **已融合** | `modules/rr3-act/src/relay.rs` 的 `rr3_relay_parse_args` / `rr3_relay_apply`（只认真布尔）；格位 `act.relay`（0.0.2 Q1 裁定 append，[`contracts/slots.toml`](../../contracts/slots.toml)）+ 主题 `rr3-act/relay/set`（[`contracts/topics.toml`](../../contracts/topics.toml)）。其"语音开灯/关灯"触发语义 → 0.0.3 T3 brain 意图规则面（本版本收口，见 [0.0.3 方案](dev/0.0.3版本洛克三号收口方案.md) D3） |
| `core/applog` | RAM 环形日志（200 条）+ JSON 导出（新的在前）+ 逐条转发回调 | **已融合** | rrd 固定容量日志环 512 行（`base/runtime/rrd/src/lib.rs` 的 `RR3_LOG_RING_CAP`，ADR-0010 裁定 5：stdout 行为一字不动、追加进环）+ `rr3.ctl.logs` 读面（`conn.rs`）；容量 200→512 是有意的增强，非漂移 |
| `core/data` | NVS 持久化运行计数（boots / chat_rounds / asr_ok·asr_fail / llm_fail / tts_fail / relay_on·relay_off / errors），"机器慢先看数字"的运维语义 | **已融合**（0.0.4 批B） | 心跳可选 `counters` 映射双端镜像（Rust [`rr-bus-api client.rs`](../../base/runtime/rr-bus-api/src/client.rs)（批N9 件一起承载面自 rr-bus 迁此） + Python bus.py，ADR-0014 裁定 1）+ brain/act 计数自报；rrd 聚合进在册表（`registry.rs`）+ 持久化原子写（[`rrd counters.rs`](../../base/runtime/rrd/src/counters.rs)，`RR3_DATA_DIR`/cfg(linux) 缺省 `/data/rr3/counters.json`，rr_data 分区挂载点，断电不丢）+ `rr3.ctl.counters` 读面（`rpc.rs`）。NVS 细节不迁，只留"断电不丢"语义 |
| `core/panel` | Web 操作面板（内置 HTTP + 单文件内嵌页）：状态页 / WiFi 扫描连接页 / 硬件控制页（继电器、测试音、自检）/ 系统页（重启、恢复出厂 WiFi）；HTTP API 同功能 | **已融合**（0.0.4 批C+批D） | 面板宿主 → [`rr3-interact/panel`](../../modules/rr3-interact/src/panel/server.rs)（std 手写 HTTP，十二端点：批C 九端点 + 批D 配网三端点 BusCall 透传）；单文件静态页 `include_str!` 内嵌四页签（状态/硬件/系统/WiFi，批C + 批D WiFi 页实装）；继电器/测试音/自检/重启（`rr3.ctl.reboot`）批C 落位；WiFi 扫描连接与 AP 配网语义与 `rr3-connect` 配网面合流（批D：`Rr3Provision` + mode 随流发布）；**"恢复出厂 WiFi"（只清凭据）≠ M6 整机恢复出厂**两语义不合并已在面板系统页文案点名（批C/D） |
| `core/link/wifi` | WiFi STA + AP 热点（esp_wifi / nvs_flash） | **已融合**（sim 侧） | `modules/rr3-connect`：`net/link` 状态机 + `cloud/session`（0.0.2 D7）；HAL `Rr3SimLink`（sim 后端）；NetworkManager 属板上件（cfg 门骨架）。AP 配网模式语义随 panel 配网页排期（0.0.4）。板上证据未 claim（硬件未采购） |
| `core/link/btctl` | 经典蓝牙 SPP 串口控制台（日志回放 / 远程命令） | **已收敛**（0.0.4 批E） | 日志回放 → rrd 日志环 + `rr3.ctl.logs`（实装在案）；远程命令 → `rr3-ctl` 总线运维面（实装在案）；"蓝牙作为一条链路" → `net.bt` 格位（在册）+ [`rr3-connect` 状态机骨架](../../modules/rr3-connect/atoms/bt/mech.rs)（批C④ 重铺前为 src/bt.rs）（0.0.4 批E，`Rr3BtState` 三态纯函数转移单测钉死）。**退役声明（Q5 裁定）**：btctl 组件语义至此全部承接/立骨架完毕——BlueZ 真射频链路属板上件（cfg 门骨架不 claim），实装随硬件采购后置板上 BlueZ 批次，`legacy/core/link/btctl` 随退役批整树移除（本目录只有 README+CMakeLists，无 .c 源文件，无代码可迁） |
| `core/link/eth` | 以太网（接口预留，未接 PHY） | **退役** | 零实现语义；按"不预先占位"纪律，新架构在真有 PHY 的板上批次再议，不迁代码 |
| `core/link/nearlink` | 星闪 NearLink（接口预留，本硬件无射频） | **退役** | 零实现语义；同上 |
| `firmware-c/main` | main.c 对话编排（薄壳哲学）+ config.h 对话策略参数 | **已融合** | 编排 → `modules/rr3-brain/rr3_brain/dialog.py` 状态机 + `__main__.py` 主循环（M4，ADR-0014"brain 是唯一编排者"，architecture §2 原则 3）；参数 → `modules/rr3-brain/config.toml`（`[rr3-brain.dialog]`） |
| `firmware-c/`（整树宿主） | ESP-IDF v6.1 主固件工程（CMake 挂载 core 12 组件） | **退役** | 宿主本体随 ESP32 平台退役（ADR-0015 转 RK3566）；其全部组成语义已由 core/ 各行覆盖（见上），无独立语义残留。CMake 组件挂载模式不迁（新架构的装配语义由 ADR-0012 装配单承接） |
| `firmware-rs/` | Rust 语音固件实验（esp-idf-hal） | **退役** | 试验残留；且 `.cargo/config.toml` 与 `sdkconfig.defaults` 钉死他机绝对路径，本就不可构建（legacy/README"已知不可构建项"）；Rust 主线已由 `base/runtime/` workspace 承接 |
| `rust/` | esp-hal 外设试验（blink/OLED/舵机/DHT11） | **退役** | blink 级试验代码，无语义可迁；OLED/舵机/DHT11 的 ESP32 驱动对 RK3566 平台不可移植（Linux 侧走内核驱动 / gpiod / ALSA 另起）。注意：舵机语义已在 `rr3-act` 独立融合（HAL trait + sim 后端），不以本树为源 |
| `core/README.md` | 统一组件模板 + 密钥注入纪律（`SILICONFLOW_API_KEY` 环境变量注入、缺失 FATAL_ERROR） | **已融合** | "密钥不进仓库、构建期 env 注入、缺失即 fail-fast"已落 `rr3-brain`（缺 key 报错点名不静默降级，ADR-0014 裁定 2）与 conventions（[conventions.md](conventions.md) 密钥条目引用此处）；组件目录模板的"四类结构角色"由 [naming.md](naming.md) §4 与 `rr3-mkmod` 生成器承接 |
| `legacy/README.md` | 冻结区说明、迁移锚点 tag `v0-pre-legacy`、回滚指引（git revert） | **已退役**（0.0.4 批F） | 回滚指引随整树删除自然失效（git 历史本身即回滚依据）；`docs/architecture.md` 断链已知偏差已随退役批注销 |

## 二、分类汇总（归档时点：0.0.4 批F，待融合清零）

| 分类 | 项 | 终态 |
|---|---|---|
| 已融合 | audio、asr/llm/tts、relay、applog、link/wifi、firmware-c main+config 语义、密钥注入纪律、data（0.0.4 批B）、panel（0.0.4 批C+批D）、btctl（0.0.4 批E，骨架+退役声明）（10 项） | 全部收敛，证据见盘点表 |
| 待融合 | ——（0 项；data/panel/btctl 三项已分别随 0.0.4 批B/批C+D/批E 收敛） | 清零确认 |
| 退役 | firmware-c 宿主、firmware-rs、rust、link/eth、link/nearlink（5 项） | 已随 0.0.4 批F 整树移除 |

## 三、退役触发条件（三阶段 ② → ③）——已满足，退役已执行

1. ~~本表"待融合"3 项全部收敛~~ **已满足**（panel 批C+D 实装、data 批B 实装、
   btctl 批E 骨架+Q5 退役声明，证据同上表）；
2. 退役批单独一提交执行整树删除，同批完成四件收尾——**已执行（0.0.4 批F）**：
   conventions §5 ⑤ 门 legacy 排除条款撤除（[`../../tools/ci/check.sh`](../../tools/ci/check.sh)
   的 `grep -v '^legacy/'` 与注释段）、architecture §8 标注消化完成、
   status.md"已知偏差"注销 legacy/README 断链条目、本文件三分类表归零归档；
3. ~~删除前双远端推送并确认 CI 指针一致~~ **已满足**（批E 提交 `586641d`
   双远端 `ls-remote` 核对一致在案；本批删除提交随批推送再核对）。

## 四、消费指引（归档口径）

- 想知道"某个 legacy 语义在新架构哪"：查本表或 architecture §8 映射表，按证据列
  路径读代码；`legacy/` 目录本体已随退役批删除，源码只存在于 git 历史
  （tag `v0-pre-legacy` 锚点 + 冻结区提交记录）
- 发现某语义有 bug 或缺失：落到新架构对应模块修（`legacy/` 已不存在，
  不再有"冻结区不动"的约束）
- 本文件为历史台账，不再演进；新语义的去向登记归 architecture 与各模块 README
