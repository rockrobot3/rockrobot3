// SPDX-License-Identifier: Apache-2.0
//! esp32-periph-base —— ESP32 classic / S3 双芯片板侧外设执行面固件。
//!
//! 职责（全案 §4 批B + 批C）：**SSD1306 I2C 驱动出字 + LEDC PWM 云台两轴逐度转向 +
//! UART2 双面指令接入**。批B 是独立固件档（行协议面）；批C 在本 crate 上加**帧
//! 执行面**（rr-ipc 能力帧，不进城——路线二裁定：板不注册总线身份，宿主 HAL 桥
//! 后端 rr-hal::esp32_uart 直连发 `rr3.call` 帧，本固件只是被点名的执行面）；
//! 批D 加 I2S——同一固件 crate 分批合入（全案 §5 板上固件形态）。
//!
//! 双面分流（批C）：指令循环逐字节收，首字节 = `0x31`（帧头 magic "1ERA" 的 LE
//! 首字节）走帧面（`rr3_framebuf_read` 整帧收 → rr3.call 分发 → Response 应答）；
//! 其余走批B 行面（调试与舵机五点 SCAN 判据沿用）。行面首字节集全为字母，
//! 分流无歧义。
//!
//! 引脚预算（全案 §5 约束面，与 0.0.7 实测接线零冲突）：
//! - UART2 = GPIO18→TX / GPIO19←RX（1.5M，CP2102 指令面；批B 行协议临时面，
//!   批C 换能力帧——接线不变；批D② 升 921600 音频带宽档，Q1=(a) 带宽账裁定；
//!   2026-10-30 链路换树莓派 PL011 三线直连后重评，压实定档 1.5M）——**订正**：原记 17→TX/16←RX
//!   已过时，GPIO16/17 一装 UART2 驱动即把 I2C0（21/22）打死，故弃用，真板钉 18/19。
//! - I2C0  = GPIO21 SDA / GPIO22 SCL（100kHz，SSD1306 0x3C；400k 在跳线面时序边际，
//!   真板实录页写 ESP_FAIL，降 100k 稳过——见 init 段注）
//! - LEDC  = GPIO32 水平轴（pan）/ GPIO33 俯仰轴（tilt）——避开 strapping 面
//!   （0/2/5/12/15）、UART2（16/17）、I2C（21/22）、I2S 候选（25/26/27，批D）、
//!   VSPI 缺省组（18/19/23，点阵预留不占用不使能）
//! - UART0 = 控制台（经板载 USB-UART 桥输出；classic 缺省 GPIO1/3、S3 GPIO43/44）。
//!   观测走 `tools/deploy/console-log.py`（raw 8N1 + 清 DTR/RTS）——设备节点随
//!   系统枚举变化，不在源码写死。S3 档显式关闭 USB-Serial/JTAG 副通道，日志只
//!   走本口（boot 横幅 `console=uart0@<baud>` 自证）。
//!
//! 供电铁律（全案 §5）：舵机由 UBEC 独立 5V 轨供电（堵转 ~700mA/只，禁接板载
//! 5V），UBEC/板/舵机单点共地；信号线 3.3V 逻辑直驱 SG90 级兼容。
//!
//! esp-idf-hal 0.44.1 API 对表实录（vendored 源码核对，批B）：
//! - `I2cDriver::new(i2c, sda, scl, &I2cConfig::new().baudrate(Hertz))`（i2c.rs
//!   L207；`I2cConfig = config::Config` alias L66）；事务面 `write(addr, bytes,
//!   timeout: TickType_t)` 每调一次 START-addr-bytes-STOP（SSD1306 每命令/每
//!   数据块一传输）；探测用单字节 0x00 写（控制字节后 STOP，芯片无害）。
//! - `LedcTimerDriver::new(timer, &TimerConfig::new().frequency(Hertz(50))
//!   .resolution(Resolution::Bits14))`（ledc.rs L108）；`LedcDriver::new(channel,
//!   &timer, pin)`（L201，B: Borrow<LedcTimerDriver>——同一 timer 借用两次驱动
//!   两通道，timer 未 move）；`set_duty(u32)`（L289）。50Hz×2^14=819200Hz，
//!   LEDC 源 80MHz 分频 97.66 → 实际 ≈50.0Hz（±0.3%），SG90 容差内。
//!   peripherals.ledc 的 timer0–3/channel0–7 为低速组（L625），Bits14 上限
//!   同样适用。
//! - I2cDriver/LedcDriver/UartDriver 上游 0.44.1 **均已标 `Send`**（vendored 实证：
//!   i2c.rs L393 / ledc.rs L334 / uart.rs L653），故两平面 wrapper
//!   （[`crate::periph::CtlUart`]/[`crate::periph::RtPeriph`]）自动 Send、**无需手补
//!   unsafe impl**（旧单 wrapper 的手补系误判，同批删除并订正）。
//! - UART2 整装 `UartDriver::new(uart, tx, rx, cts, rts, config)`（cts/rts 传
//!   None；同口二次 install 必败的教训，哨兵 API 对表实录在案，照抄不赘）。
//!   不 split：控制面单线程读写同驱动，owned move 即可。
//! - xtensa LX6 无 64 位原子；状态面（[`crate::state::PeriphState`]）以 `Arc<Mutex<..>>` 护，
//!   角度用 u32 直传，无需原子。
//!
//! 线程模型（0.0.5 现场处置续批 2026-10-12：**控制面/运行面分离**）：入口只做
//! IDF 补丁与转交 `boot.rs`（外设开箱/两平面起序/音频面/状态环全在 boot.rs），
//! 随后起**两个** 16KB 线程（8KB→16KB 运行下限钉死的教训在案，勿再调低）：
//! - **控制面** [`crate::commands::cmd_loop`]（独占 UART2）：逐字节收、双面分流、解析、
//!   校验、**改状态**、回包——**不碰 I2C/LEDC**，慢外设一律不阻塞命令吞吐（消"卡顿"）；
//! - **运行面** [`crate::runtime::runtime_loop`]（独占 I2C0 + LEDC）：取控制面推送的
//!   硬件作业 [`crate::state::Job`] 执行（OLED 出字 / 舵机移动 / 五点扫描），并跑自主节拍。
//!
//! 两面经 `Arc<Mutex<Bus>>` 通信（哨兵 Arc<Mutex> 纪律同款，xtensa 面已被其背书）：
//! `Bus { jobs, outbox, st }`——控制面推作业 + 改 `st`，运行面取作业执行 + 回推 outbox
//! 行。**锁只护状态读写，绝不跨 I2C 持有**（运行面"决策段持锁纯算，渲染段脱锁慢跑"）。
//!
//! 舵机**串行降级**（同批，用户明文裁定）：板载 5V 直供、无 UBEC 时两轴并行驱动的峰值
//! 电流叠加会拉塌轨压（真板 BOD / 舵机发烫实录），故复合动作一律**先 pan 到位 → 整定
//! [`crate::board::AXIS_SETTLE_MS`] → 再 tilt**（任一时刻只驱动一轴，削峰值）；单轴命令直动。
//! SCAN 同守此律（每点 pan/tilt 两段），WANDER 扫摆**同拍只动一轴**（pan/tilt 错拍）。

// 自治壳（W3-2b）：健康快照与监督核接线；/api/board/health 常驻。
mod autonomy;
mod board;
// 启动装配序（外设开箱/两平面/音频面/状态环）：硬件时序契约集中一处。
mod boot;
// capture 原子件机制核（批N23 件三收编：atoms/capture/mech.rs——audio.capture
// 板侧执行面的采集变换纯算术：帧解析/增益钳位/DC 抑制分片变换/背压入队；接线
// 照 modules 先例 #[path = "../atoms/<名>/<文件>.rs"]）。
#[path = "../atoms/capture/mech.rs"]
mod capture;
mod commands;
mod cred;
// face 原子件机制核（批N23 件二收编：atoms/face/mech.rs——ui.face 板侧执行面
// 的胶囊眼位图几何与表情基态预设，文件级机制/策略分离；接线照 modules 先例
// #[path = "../atoms/<名>/<文件>.rs"]）。
#[path = "../atoms/face/mech.rs"]
mod face;
mod frame;
mod httpd;
mod link;
// 堆账阶段采样（W7 内存门观测面）：只读 IDF 堆账，不参与分配。
mod mem;
mod mic;
mod oled;
mod periph;
// 云 TTS 客户端（W4-2b）：仅 tts 档编译；与播放面共用 tts gate。
#[cfg(feature = "tts")]
mod cloud;
// 对话整轮执行（W6-1/W6-2）：语音段 → ASR → 意图 → LLM → TTS → 本板播放。
#[cfg(feature = "tts")]
mod dialog;
// PSRAM 定容缓冲（W6-3 内存门）：大块音频缓冲放外部 PSRAM。
#[cfg(feature = "tts")]
mod psram;
// I2S1 TX 真实 PCM 播放面（W4-2）：仅 --features tts 编译；与 spk-tone 互斥。
#[cfg(feature = "tts")]
mod playout;
// 语音段 flash spool 分区后端（ADR-0031 W7-b1a）：仅 tts 全链档编译；
// 分区缺位/几何不符在 boot 点名，不在音频热路径写擦。
mod ports_impl;
mod power;
mod runtime;
#[cfg(feature = "tts")]
mod spool;
// I2S TX 功放/扬声器上板验收（W4-1）：仅 --features spk-tone 编译，默认固件零行为变化。
#[cfg(feature = "spk-tone")]
mod speaker;
// 自治形态网络时间（W3-2a）：仅 --features autonomy 编译。
#[cfg(feature = "autonomy")]
mod sntp;
// servo 原子件机制核（批N23 件一收编：atoms/servo/mech.rs，文件级机制/策略
// 分离——机制纯函数与行程生成物在件内，策略量留 board.rs；接线照 modules
// 先例 #[path = "../atoms/<名>/<文件>.rs"]）。
#[path = "../atoms/servo/mech.rs"]
mod servo;
mod state;

use esp_idf_sys::link_patches;

fn main() {
    // IDF 绑定面补丁（esp-idf-hal 工程惯例起手，哨兵同款）
    link_patches();
    boot::rr3_boot();
}
