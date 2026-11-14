//! ESP32-UART 板面音频源后端（A 批 c3）：板面 INMP441 双麦流 → 宿主环形缓冲 →
//! [`Rr3AudioSource`](crate::Rr3AudioSource)（`rr3_audio_source()` esp32 档）。
//!
//! 链路：板面 mic.rs 分片组帧（16kHz 双声道 s16le，128ms/8192B 一片，A-IPC
//! Event 帧，topic `rr3-sense/audio/pcm`）→ UART2（[`crate::esp32_uart::
//! RR3_HAL_PERIPH_UART_ENV`] 指定设备，[`crate::esp32_uart::
//! RR3_HAL_PERIPH_BAUD_ENV`] 波特率——与板固件 `UART2_BAUD_HZ` 共用
//! `rr_periph_proto::consts::RR3_BRIDGE_BAUD`（1.5M）单点真值，env 仅覆盖）→ 本件
//! 收流：开口 → `rr_ipc::audio_session::handshake`（settle/防御 OFF/ON 应答
//! 扫描，与 rr3-audio-rec 同一机制单点）→ 读线程大块裸读（判据② 节奏）→
//! [`FrameAssembler`](rr_ipc::FrameAssembler) 拆帧 → 四道门解析 → 环形缓冲
//! （~1s，溢出丢最旧整帧 + 计数）→ `read()` 整声道帧出样。
//!
//! 纪律：
//! - flock 单消费者（`Rr3UartConn` 开口即跨进程排他）：**宿主侧唯一持 conn 者
//!   = 本件**，板外设命令下行唯一通路 = 本件命令旁路（B 批 b1：语义命令入
//!   有界队列 → 读线程单写者逐帧写出，帧构造走
//!   [`crate::esp32_uart::rr3_bridge_req_frame`] 桥同形单点）；rr3-audio-rec
//!   等第二 opener 仍 fail-closed 点名，不是静默截流；A 链回放恒宿主扬声器
//!   （backend.rs sink 侧 esp32 档显式落占位），互斥面天然不成立。
//! - 旁路 fire-and-forget：板侧执行**前**回成功（status.md 对表实录），回执由
//!   板帧分类面放行记账（不污音频四道门的 err_streak）；队列满丢最旧 + 计数
//!   （板侧 audio_q 同款背压口径）。
//! - 欠载语义：`read()` 非阻塞，无新数据回 0——消费面（rr3-sense）走既有
//!   `pcm_empty_frames` 记账（饥饿可见，不阻塞节拍）。
//! - 断流重连**永不放弃**：见过数据后连续 [`RESYNC_ZERO_BAIL`](rr_ipc::audio_session::RESYNC_ZERO_BAIL) 次零读（1.6s
//!   静默）= 线上断流，就地重开口重握手（[`HANDSHAKE_TRIES`](rr_ipc::audio_session::HANDSHAKE_TRIES) 轮 + 歇口，
//!   每轮重开 fd——宿主侧 USB RX 冻结唯一捅法）；全败后外层循环续轮。与
//!   工具侧 fail-honest exit(4) 的差异是刻意的：源是常驻件，不新增
//!   `Rr3HalError` 变体（重连失败不是一次性调用的错误，是持续饥饿）。
//! - 板-宿主时钟 ~2% 滑移（run4 588s 实测"有损可用"）：环形缓冲溢丢是稳态
//!   损耗，VAD/ASR 可容忍；首溢点名一条日志，此后静默计数。
//! - 四道门解析（dst/payload 类型/topic/字段+b64）在本件内实现，与
//!   tools/rr3-audio-rec 的 `parse_audio_event` **同值锚定**（rr-ipc 零第三方
//!   依赖约束，serde_json 面不上提；门序与判据逐字同源，双侧测试兜底防分叉）。
//!
//! cfg 宿主档门（批N8 件三，与 [`crate::esp32_uart`] 同口径 any(linux, macos)）：
//! 本模块是 Pi 侧桥消费件（rr-ipc 的 UART/会话机制面同在宿主档门内），espidf
//! 档不载——其余平台编译即错，显式优于隐式。

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
