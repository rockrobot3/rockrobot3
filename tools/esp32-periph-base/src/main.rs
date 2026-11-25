// SPDX-License-Identifier: Apache-2.0
//! esp32-periph-base —— ESP32 classic / S3 板侧外设执行面固件（单进程多任务）。
//!
//! 两条面：控制面 [`crate::commands::cmd_loop`] 独占 UART2（逐字节收、双面分流、
//! 改状态、回包，不碰慢外设），运行面 [`crate::runtime::runtime_loop`] 独占 I2C0 +
//! LEDC（OLED 出字 / 舵机 / 五点扫描 + 自主节拍）。两面经 `Arc<Mutex<Bus>>` 通信，
//! 锁只护状态读写，绝不跨 I2C 持有。
//!
//! 引脚（全案 §5；与 0.0.7 实测接线一致）：UART2 = GPIO18 TX / GPIO19 RX（1.5M）；
//! I2C0 = GPIO21 SDA / GPIO22 SCL（100kHz，SSD1306 0x3C）；LEDC = GPIO32 pan /
//! GPIO33 tilt（避开 strapping 与 UART/I2C/I2S 面）；UART0 控制台经 `console-log.py`
//! 采集（raw 8N1 + 清 DTR/RTS）。
//!
//! 供电铁律：舵机由 UBEC 独立 5V 轨供电（堵转 ~700mA/只，禁接板载 5V），单点共地；
//! 无 UBEC 时复合动作按"pan 到位 → 整定 → tilt"串行降级，削峰值电流。
//!
//! 平台事实：xtensa LX6 无 64 位原子，状态面用 `Arc<Mutex<..>>`（角度 u32 直传）；
//! esp-idf-hal 0.44.1 的 I2c/Ledc/Uart 驱动均已 `Send`，wrapper 无需 unsafe 补。
//! 线程 16KB 是实测运行下限，勿调低。批次沿革见 docs/process/dev 与 status。

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
