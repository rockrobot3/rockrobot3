// SPDX-License-Identifier: Apache-2.0
//! ESP32-UART 板面音频源后端：板面双麦流 → 宿主环形缓冲 →
//! [`Rr3AudioSource`](crate::Rr3AudioSource)（esp32 档）。
//!
//! 链路：板 `mic` 分片（16kHz 双声道 s16le，128ms/8192B 一片）→ UART2（设备与
//! 波特率走 env 覆盖，缺省与板固件共用 `RR3_BRIDGE_BAUD` 单点）→ 本件：开口 →
//! `audio_session::handshake`（与收流工具同一机制单点）→ 读线程大块裸读 →
//! [`FrameAssembler`](rr_ipc::FrameAssembler) 拆帧 → 四道门 → 环形缓冲（~1s，
//! 溢出丢最旧整帧并计数）→ `read()` 整声道出样。
//!
//! 纪律：
//! - **flock 单消费者**：宿主侧唯一持 conn 者就是本件；下行命令走本件旁路（有界
//!   队列 → 读线程单写者逐帧写出，帧构造与桥同源）。第二 opener 仍 fail-closed
//!   点名，不静默截流。回放恒为宿主扬声器，互斥面天然不成立。
//! - 旁路 fire-and-forget：板侧执行前即回成功（status 对表实录），回执由板帧分类
//!   面记账（不污音频四道门的 err_streak）；队列满丢最旧 + 计数（与板侧同口径）。
//! - 欠载：`read()` 非阻塞，无新数据回 0——消费面走 `pcm_empty_frames` 记账，
//!   饥饿可见但不阻塞节拍。
//! - 断流重连**永不放弃**：见过数据后连续零读（1.6s 静默）= 断流，就地重开口 +
//!   重握手，每轮重开 fd（宿主侧 USB RX 冻结唯一捅法），全败则外层续轮。与工具侧
//!   exit(4) 的差异是刻意的：源是常驻件，重连失败是持续饥饿而非一次性错误。
//! - 板-宿主时钟 ~2% 滑移（588s 实测"有损可用"）：溢丢是稳态损耗，VAD/ASR 可容忍；
//!   首溢点名一条日志，此后静默计数。
//! - 四道门（dst/payload 类型/topic/字段+b64）在本件实现，与 `rr3-audio-rec` 的
//!   解析同值锚定（rr-ipc 零第三方依赖，serde_json 不上提；双侧测试兜底防分叉）。
//!
//! cfg 门与 [`crate::esp32_uart`] 同口径 `any(linux, macos)`：本模块是 Pi 侧桥消费件，
//! espidf 档不载——其余平台编译即错，显式优于隐式。

mod cmd;
mod core;
mod evt;
mod reader;
mod ring;

#[cfg(test)]
mod tests;

/// 宿主 task 位形（帧头 dst 门）：真值单点在 rr-periph-proto（ADR-0024），本地
/// 别名保既有消费面与测试路径。
pub(crate) use rr_periph_proto::consts::RR3_TASK_HOST as RR3_HOST_TASK;
// 板外设语义命令 + 帧构造：真值单点在 rr-periph-proto（ADR-0024，宿主与固件
// 双向复用），本地 re-export 保既有 `rr_hal::esp32audio::Rr3PeriphCmd` 路径。
pub(crate) use rr_periph_proto::cmd::rr3_esp32_cmd_frame;
pub use rr_periph_proto::cmd::Rr3PeriphCmd;

pub use cmd::Rr3AudioCmdProducer;
pub use core::{Rr3Esp32UartAudioSource, RR3_ESP32_AUDIO_CHANNELS, RR3_ESP32_AUDIO_RATE};
