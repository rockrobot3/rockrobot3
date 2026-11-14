//! macOS（CoreAudio/AudioQueue）音频后端：宿主真麦拾音 / 扬声器回放。
//!
//! 编译门是单重平台门 `target_os = "macos"`（同 linux_gpio/linux_power 先例，不挂
//! cargo feature）：AudioQueue 是系统框架 C API，绑定走**手写 extern "C" +
//! `#[link(kind = "framework")]`**（依赖纪律先例 conventions.md"POSIX 附着为手写
//! extern 面，不引 libc crate"）——crates.io 零增量，Cargo.toml 零改动。
//!
//! 选择面（backend.rs）纪律：macOS 平台**缺省仍 sim**（CI/e2e 不真开麦的防线），
//! 真件走显式 `RR3_HAL_BACKEND=mac`（与非 linux 显式 linux 落占位同一套"显式启用
//! 真件"裁定）；使用时失败显式回错（fail-closed），不静默换 sim。
//!
//! 实现要点：
//! - AudioQueue 的 inFormat 即交付格式：直接请求 s16le 交错 PCM（采样率/声道随
//!   [`Rr3AudioCfg`](crate::Rr3AudioCfg)），硬件到交付格式的重采样由 CoreAudio
//!   内建 AudioConverter 承担——后端不做手写重采样。
//! - 拾音：回调把整块 buffer memcpy 入共享 deque（超上限丢最旧，对位板侧 audio_q
//!   背压策略）；`read` 锁内 Condvar 等满调用方缓冲再拷走——ALSA `readi` 满帧阻塞
//!   同语义。
//! - 回放：`write` 入共享 deque（超 1s 高水位阻塞让出，对位 ALSA `writei` 排空
//!   阻塞）；输出回调从 deque 取满一块、不足补零（静音拍，对位 ALSA underrun 后
//!   静音）再 Enqueue 续播——流不停摆，played 语义只随 write 的样本推进。
//! - 线程安全：回调与调用方只经 `Shared`（Mutex + Condvar）交接；回调体内无分配、
//!   无锁外调用 Rust API；`Drop` 先 `AudioQueueStop(true)` 同步等在途回调收尾、
//!   再 `AudioQueueDispose(true)` 拆队列，**之后**才回收 Shared——时序依据是两个
//!   API 的同步语义（文档承诺立即模式下阻塞至回调完成），C 侧此后无引用。
//! - 设备语义：不传 device 走系统缺省输入/输出设备（与 ALSA `default` 同语义）；
//!   麦克风 TCC 授权：宿主终端首次使用弹授权窗，拒权表现为 `AudioQueueStart`
//!   失败（此处显式回错）或拾音静默（排障走系统设置→隐私→麦克风）。

mod ffi;
mod shared;
mod sink;
mod source;

#[cfg(test)]
mod tests;

pub use sink::Rr3MacAudioSink;
pub use source::Rr3MacAudioSource;
