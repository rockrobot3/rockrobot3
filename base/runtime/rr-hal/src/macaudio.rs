// SPDX-License-Identifier: Apache-2.0
//! macOS（CoreAudio/AudioQueue）音频后端：宿主真麦拾音 / 扬声器回放。
//!
//! 编译门单重 `target_os = "macos"`：AudioQueue 走手写 `extern "C"` + framework 链接，
//! 不引 libc crate。缺省仍 sim（CI 不真开麦），真件须显式 `RR3_HAL_BACKEND=mac`；
//! 启用后失败显式回错，不静默回落。
//!
//! 实现要点：inFormat 直接请求 s16le 交错 PCM，重采样交 CoreAudio；拾音回调整块 memcpy 入
//! 共享 deque（超限丢最旧），`read` 在锁内 Condvar 等满；回放 `write` 入 deque（超 1s 高
//! 水位阻塞），输出回调取满一块、不足补零。回调与调用方只经 `Shared`（Mutex+Condvar）交接，
//! 回调体内无分配、无锁外 Rust 调用。`Drop` 先 `AudioQueueStop(true)` 等在途回调收尾，再
//! `AudioQueueDispose(true)`。
//!
//! 设备：不传 device 走系统缺省；麦克风 TCC 首次弹窗，拒权表现为 `AudioQueueStart` 失败或拾音静默。

mod ffi;
mod shared;
mod sink;
mod source;

#[cfg(test)]
mod tests;

pub use sink::Rr3MacAudioSink;
pub use source::Rr3MacAudioSource;
