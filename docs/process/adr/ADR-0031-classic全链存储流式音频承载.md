# ADR-0031：classic 全链的存储/流式音频承载

- **状态**：部分采纳（2026-11-22）：裁定 2（TTS 流式）生效；裁定 1/3（语音
  spool 与无 PSRAM VAD）在 classic 4MB 实测否决，机制保留给有 PSRAM/更大
  SRAM 的机型。详见“施工与验收 / W7-b4”。
- **日期**：2026-11-21
- **关联**：
  [ADR-0029](ADR-0029-单板自治与云直连-三独立形态全链收口.md)（单板全链口径）·
  [ADR-0030](ADR-0030-ESP全链档配置面可选与网络服务先行.md)（配置面可选）·
  [单板全链开发计划 §20.2](../dev/单板全链开发计划-2026-11-19.md)（W7 实测与静态预算）·
  [实测 0014](../../evidence/实测/0014.md)（classic 真板堆账）

## 背景

classic W7 实测已确认当前实现 **No-Go**：无 PSRAM，播放面后
`free=34,200B` / `int_free=61,024B`；ASR 8s 语音段 512KB、上行峰值约 768KB、
TTS 回包 512KB、播放队列最多约 1MB，均无法落内部 SRAM。`no-http` 档只消掉
配置面失败路径，稳态余量没有实质增加。

用户口径（2026-11-21）：任务是短消息、高频、快发；**RAM 只做工作台，语音段、
TTS 回包与日志落存储**，不把整段数据常驻 RAM。本 ADR 只裁定这一承载方式；
不承诺 classic 全链 Go，Go/No-Go 仍由 W7 实测决定。

## 决策

### 裁定 1：语音段走 flash spool，不常驻 RAM

- 为语音段建立**专用 flash spool 区**（建议 1MB，最终容量随分区表评审）。
- VAD 起音后按 20ms 块顺序写 spool；单段上限 8s（16k mono s16 = 256KB）。
- ASR multipart 按 Content-Length **从 spool 分块写** `esp_http_client_write`；
  RAM 只保留 20ms 块、4KB 搬运窗与 VAD/DoA 状态。
- ASR 结束即删除该段；同一时刻只允许一段语音，不并发。

### 裁定 2：TTS 回包边收边播，不聚合成完整 Vec

- `esp_http_client_read` 每 4KB 直接投 I2S 播放环（8–16KB），不构造完整
  TTS 响应 Vec。
- 播放环按块写 I2S；首块淡入、末块淡出；总字节数用于时长对账。
- 中途失败：停止播放、点名阶段与已收字节数；不静默当作完成。
- TTS 回包**不落 flash**，避免无谓写寿命消耗。

### 裁定 3：无 PSRAM 的 VAD 只保留小窗

- 20ms 块 + **200ms 预滚**（16k stereo s16 ≈ 12.8KB）放内部 SRAM。
- 语音段写 spool；spool 满/写失败：点名丢弃并计数，不阻塞音频线程，不伪造成功。
- 半双工门控沿用 `tts_gate`：播放期不写 spool。

### 裁定 4：配置面与网络服务边界沿用 ADR-0030

- 全链档不启 HTTP 配置面；net 服务独立且先起。
- 配网/诊断使用独立档或维护窗口。

### 裁定 5：TLS 与 Go/No-Go 仍按实测

- TLS 收发缓冲用 asymmetric/dynamic 配置，具体值以真板测量为准；不引入明文回退。
- Go 判据不变：30min `free ≥20% SRAM`、3 轮对话无 OOM、断网/存储失败显式点名。
- 本 ADR 不把静态预算当实测；未测前结论仍是“当前实现 No-Go / 候选未证”。

## 备选与否决

| 备选 | 否决理由 |
|---|---|
| 继续全量 RAM 缓冲 | W7 实测已 No-Go；不是调参问题 |
| 只支持短消息、不落存储 | 不能覆盖 8s 语音段与完整回复，不满足全链定义 |
| TTS 也落 flash 再播 | 增加写寿命与启动延迟；TTS 可边收边播，无需持久化 |
| 引入 MP3/AAC 解码 | 契约只收 PCM/WAV；解码器增 RAM/CPU/依赖，否决 |
| 直接换 WROVER/PSRAM | 属硬件路线，另案评估；不在本软件 ADR 内 |

## 后果

- 正面：ASR/TTS 峰值从数百 KB 降到小块 + spool，给 TLS/VAD 留出内部 SRAM；
  classic 全链第一次有可测量的 Go 候选路径。
- 负面：新增 flash 分区与 spool 机制；写寿命、擦除延迟、分区容量需要实测；
  ASR 多一次存储读写，时延增加。
- 风险：spool 写失败/满、TTS 中途断流、TLS 与存储同时占用导致新的低水位；
  三者都必须点名与计数，不得静默降级。

## 施工与验收

- **W7-b1**：分区表/flash spool 机制；host 可测的边界/失败路径。
- **W7-b1a（2026-11-21 落树）**：自定义分区表（classic 4MB：factory 0x2F0000 +
  spool 1MB，nvs/phy 偏移不变）与独立固件 spool 分区后端；记录机制在
  `rr-audio-mech::spool`——16B 头（magic/version/kind/seq/len/CRC32）、4KB 扇区
  惰性擦除、顺序写/分块读、`finish` 写回逐字节对账；断电只留无头载荷，`load`
  判无有效记录。host 单测覆盖越界、IO 失败、写回不一致、掉电与 CRC 损坏；CI
  双芯片构建并核验 `partition-table.bin` 含 spool 条目。**尚未接 VAD/dialog**，
  真板容量/擦除时延与写寿命在 W7-b4 记账。
- **W7-b1a CI 回执（2026-11-21，`17ac68f1`）**：gate（含 host 三平台单测）与
  platforms 的 classic/S3 两条 esp32-gates 全绿；两芯片构建产物均核出 spool
  分区，Actions 固件制品（solo / no-http）在册。真板 `[spool]` boot 行随下一次
  烧录会话取证（写入路径尚未接线，本身不产生磨损）。
- **W7-b2（2026-11-21 落树，`12676402`）**：TTS 回包 4KB 边收边播——头窗识别
  WAV/裸 PCM → `Rr3PcmStream` 全局淡入淡出 → 16KB 播放环；首块前按契约重试、
  开播后不重放；Content-Length 与 WAV 声明长度双向对账，不完整回包显式中止。
  播放队列由“2 条全量 PCM”降为 16KB 环，512KB 回包 Vec 不再存在。CI 双芯片绿；
  真板听感、时长误差与 30min 内存列在 W7-b4 取。
- **W7-b3**：无 PSRAM VAD 小窗 + spool 写入；半双工与丢段计数。
- **W7-b3a（2026-11-21 落树，`657f1af8`）**：ASR multipart 改 4KB 分块上行——
  `rr-cloud-proto` 出 `Rr3AsrPlan`（head/wav/tail/content_len，与整段组包逐字节
  对拍），`cloud/http` 按头/WAV/样本/尾顺序写 `esp_http_client_write`；删除
  256KB s16le 与 256KB 整段 multipart 两份临时体。重试仍 2 次/500ms。语音段
  缓冲（VAD 段）尚未落 spool，无 PSRAM 写入路径仍归 W7-b3b。
- **W7-b3b（2026-11-21 落树，`67472d7b`）**：无 PSRAM 的 spool 写入路径全链接通。
  - **写线程**：`spool` 分区后端独占一条 `spool-w` 线程，擦/写/读全在其内串行；
    音频线程只做非阻塞投递（`SyncSender` 容量 16 块 = 320ms）。块内联
    `[i16; 320]`（20ms mono），零堆分配；队列满即弃段并计数，绝不阻塞 I2S。
  - **状态即背压**：`Idle → Rec → Done → Idle`；`Done` 未释放前开新段失败并点名，
    不覆盖、不排队。写线程空闲 2s 仍处 `Rec` 时自行弃段（语音侧异常兜底）。
  - **上传校验**：`dialog` 先向写线程发 `Verify` 做整段 CRC 对账，一致才上传；
    不符即清理并弃轮。ASR 源抽为 `Rr3PcmSrc`（内存切片 / spool 分块两实现），
    与既有 multipart、2 次/500ms 重试共用同一份传输代码；PCM 字节数由
    `content_len` 反推，不设易漂移的独立参数。
  - **语音策略**：无 PSRAM 时 200ms 预滚（10 块，起音判定窗本身），起音窗内定
    声道（能量高者）后段内不跳变，逐块落盘；单段 8s 上限；`oled_hold` 只判向
    不落段。S3（有 PSRAM）保持内存路径，不起 spool 写线程、不占 6KB 栈。
  - **启动清扫**：上电只读记录头，残留段点名后清理（语音跨重启已过期，不补传）。
  - **CI 回执（2026-11-21，`67472d7b`）**：gate `44901448372`、integration
    `44901448361`、platforms `44901448369` 全绿；classic 与 S3 两条 esp32-gates
    均过（clippy `-D warnings` / 五档构建 / 测试码编译 / 分区核验），
    `esp32-classic-solo-firmware` 等固件制品在册。中途红因已按根因修复：
    `doa.rs` 触 500 行门 → 下混拆 `chan.rs`（`144bdd1f`）；预滚窗借用冲突
    E0502 → `mem::take` 迭代（`6615961d`）；ASR 参数数与 spool 枚举体积 →
    计划反推字节数 + 内联块显式预算（`67472d7b`）。
  - **未过 Go 门**：真板 spool 擦写时延、I2S DMA 是否被 flash 擦除的 cache 停顿
    拖出欠载、30min 内存曲线均未测，归 W7-b4；classic 全链仍 No-Go。
- **W7-b4（2026-11-22，classic 4MB 无 PSRAM，CI 制品 `d5963ce3`）**：30 min soak
  完成——1 次上电复位、0 Guru/panic/心跳超时/OOM；`status free` 61 样本
  36,288–36,448 B（7.0% SRAM），`min_free=1,884 B`；MIC 采样率 15,947–16,130 Hz；
  播放面/WiFi/net 在线，HTTP 配置面按最大连续堆降级关闭，本地 VAD/spool 按
  堆预留门停用。真板逐层红因与修复：6 KB 写线程栈不足（→12 KB）、语音 48 KB
  预算超过启动余量（→spool 门）、httpd 16 KB 任务碎片失败挤掉播放栈（→HTTP
  门）、播放 16 KB 栈在 15.3 KB 最大连续块处 OOM（→起栈前等连续块）、
  net-watch 16 KB 栈在 8 KB 块处 OOM（→6 KB）、监督壳对“未启动”误报即将复位
  （→按 ever 拆分点名）。**裁定：classic 全链/板上 VAD No-Go**——TLS 无余量、
  HTTP 与语音入口均不可用；TTS/ASR 流式继续有效，spool 机制保留给 PSRAM 或
  更大 SRAM 机型。Go 判据（30min free ≥20% SRAM）未达，不再以 classic 4MB
  作为全链候选。

每件单独提交、单独 CI 回执；W7-b4 已完成并按实测裁定 classic 4MB No-Go
（见上），不得再以静态预算或单一时点 `free` 反推 Go。
