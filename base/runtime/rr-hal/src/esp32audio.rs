// SPDX-License-Identifier: Apache-2.0
//! ESP32-UART 板面音频源后端：板面双麦流 → 宿主环形缓冲 → [`Rr3AudioSource`](crate::Rr3AudioSource)。
//!
//! 链路：板 `mic` 分片（16kHz 双声道 s16le，128ms/8192B）→ UART2（缺省 `RR3_BRIDGE_BAUD`）
//! → `audio_session::handshake` → 读线程裸读 → [`FrameAssembler`](rr_ipc::FrameAssembler)
//! 拆帧 → 四道门 → 环形缓冲（~1s，溢丢最旧整帧并计数）→ `read()` 整声道出样。
//!
//! 纪律：flock 单消费者（第二 opener fail-closed）；下行命令走旁路有界队列、读线程单写者
//! 逐帧写出，fire-and-forget，队列满丢最旧并计数；`read()` 非阻塞，无数据回 0；断流重连
//! **永不放弃**（零读 1.6s 即重开口 + 重握手；源是常驻件）；板-宿主时钟 ~2% 滑移属稳态损耗。
//!
//! 四道门（dst / payload 类型 / topic / 字段+b64）与 `rr3-audio-rec` 同值锚定（双侧测试防分叉）。
//!
//! cfg 门 `any(linux, macos)`：Pi 侧桥消费件，espidf 档不载，其余平台编译即错。

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
