# 树莓派外设探针集（Pi 直连经典 ESP32 板）

板上线/现场批的**取证工具**：全部从 Pi 的 PL011（`/dev/ttyAMA0`，1.5M 档）直连板，
绕开宿主栈直接采线面与执行面证据。判据口径单点在 [`probe_common.py`](probe_common.py)
（波特率解析、开口、收流、128B 帧扫描与字节分类只此一处实现）。

## 0 通用前置

1. **停链**：探针独占 UART（sense 常驻持锁，`flock` 单消费者纪律）——
   `sudo systemctl stop rrd`，跑完 `sudo systemctl start rrd`。
2. **换口/换档**：`RR3_PROBE_TTY`（缺省 `/dev/ttyAMA0`）、`RR3_PROBE_BAUD`（缺省
   `B1500000`）。macOS 宿主上限 921600（UART 面平台限制，见 status.md U/V 轮）。
3. **退出码**：0 = 判据全过；非 0 = 有失败项（各脚本末行给 `[PASS]/[FAIL]` 与
   现场处置提示）。

## 1 用法表（按用途分组）

### A 线面与协议（板↔宿主通路健康度）

| 探针 | 用途 | 判据要点 |
|---|---|---|
| [`bw_probe.py`](bw_probe.py) | UART 带宽/误码连续性 | 撕裂 0 / seq 断档 0 / **CRC 不符 0** / 乱码 0B |
| [`bw_mixed.py`](bw_mixed.py) | 音频流＋命令面混跑压测 | 混跑期命令回包全 OK，帧面指标同 A |
| [`frame_probe.py`](frame_probe.py) | 帧面（128B 头＋CRC32）真机验证 | `rr3.call` 六例逐条应答 / `set_bindings` applied |
| [`p1_probe.py`](p1_probe.py) | P1 稳健性四探 | 残尾吞净 / 行面干净 / 观测字段齐 / 突发回包 |
| [`robust_probe.py`](robust_probe.py) | 行面 robustness 六探 | 超长行/半行超时/拼接/连发/写前记账 |
| [`ping_probe.py`](ping_probe.py) | **千次 ping 稳定性**（T1 条目5） | 1000/1000 应答、0 缺失、RTT 中位记档 |
| [`u3_capture.py`](u3_capture.py) | 命令面 canonical digest | 多次/跨档回包行集 sha256 自等 |

### B 执行面与观感

| 探针 | 用途 | 判据要点 |
|---|---|---|
| [`full_smoke.py`](full_smoke.py) | 全功能冒烟（11 条命令＋边界/交叉） | 逐条正则断言全过（含拒绝面两腿） |
| [`servo_motion.py`](servo_motion.py) | V4 舵机运动正确性 | 到位回读四相 / 连续动作不丢帧 / 双轴串行 |
| [`voice_respond.py`](voice_respond.py) | 声音事件→表情/转向闭环 | 每次喊话 2s 内 FACE+TILT+PAN 三串 OK |
| [`longrun.py`](longrun.py) | V5 长跑稳定性（并发） | ack 缺失 0 / drop 增量 0 / seq 断档 0 / min_free ≥100KB |
| [`doa_probe.py`](doa_probe.py) | DoA 判向判定实验 | 六轮带真值定向喊话，输出三特征证据（非验收断言） |

### C 音频与电源（声学面标定）

| 探针 | 用途 | 判据要点 |
|---|---|---|
| [`rms_probe.py`](rms_probe.py) | 流内 PCM RMS 实测 | 整体/每秒 RMS 输出，对表 VAD 阈（`rms_start`） |
| [`rail2.py`](rail2.py) | 轨压两点采样 | 静止＋WANDER 负载态各一读，对表万用表（≤2% 稳态） |

## 2 三支判据对表速查（现场最常问）

| 现象 | 先跑 | 看什么 |
|---|---|---|
| 板完全没回话 | A 组任一（如 `ping_probe`） | 全 MISS＋实收空字节 ⇒ 三线未接/接错位（U/V 轮教训：壳偏一针=DC 通数据不通） |
| 发送字节被原样收回 | 换波特率复测 | 换档仍复现 ⇒ **物理自环**（TX/RX 短接），重插排查 |
| 怎么喊都不回应 | `rms_probe` → 对比 `config-esp32.toml` 阈值 | 语音段 RMS 是否高于 `rms_start`；底噪是否高于阈值（结构失效） |
| 音量突然变小 | `amixer -c Device scontents` | 硬件 PCM 是否回落 38%（-73dB 面，敞口表第 24 行） |

## 3 新增探针的落点纪律

- 口径只在 `probe_common` 扩（帧扫描/字节分类/开口/收流），**不在各脚本复制第二份**；
- 退出码语义与上表一致；末行必须给 `[PASS]/[FAIL]` 与现场处置提示；
- 本表与 `ls *.py` 同批更新（新增一支即登记一行）。
