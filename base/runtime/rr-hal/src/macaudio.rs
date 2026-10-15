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
//!   [`Rr3AudioCfg`]），硬件到交付格式的重采样由 CoreAudio 内建 AudioConverter
//!   承担——后端不做手写重采样。
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

use std::collections::VecDeque;
use std::ptr;
use std::sync::{Condvar, Mutex};

use crate::{Rr3AudioCfg, Rr3AudioSink, Rr3AudioSource, Rr3ErrText, Rr3HalError};

// ---------------------------------------------------------------------------
// 手写 CoreAudio 绑定（只取本后端用到的面；字段类型按 AudioQueue.h/CoreAudioTypes.h）
// ---------------------------------------------------------------------------

/// `kAudioFormatLinearPCM` = 'lpcm'
const K_AUDIO_FORMAT_LINEAR_PCM: u32 = 0x6C70_636D;
/// `kLinearPCMFormatFlagIsSignedInteger`（= kAudioFormatFlagIsSignedInteger 别名）
const K_FLAG_SIGNED_INTEGER: u32 = 1 << 2;
/// `kLinearPCMFormatFlagIsPacked`
const K_FLAG_PACKED: u32 = 1 << 3;
/// s16le 交错 PCM 的格式旗：符号整数 + 紧凑打包（不设 NonInterleaved = 交错）
const K_FLAGS_S16LE_INTERLEAVED: u32 = K_FLAG_SIGNED_INTEGER | K_FLAG_PACKED;

/// AudioStreamBasicDescription（AudioCoreTypes.h，repr(C) 与 C ABI 同形）
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
struct AudioStreamBasicDescription {
    m_sample_rate: f64,
    m_format_id: u32,
    m_format_flags: u32,
    m_bytes_per_packet: u32,
    m_frames_per_packet: u32,
    m_bytes_per_frame: u32,
    m_channels_per_frame: u32,
    m_bits_per_channel: u32,
}

/// AudioQueueBuffer（AudioQueue.h）：只声明本后端读写的字段，其余字段按 C 原形
/// 补齐保持 ABI 对齐（本后端自不分配该结构——句柄由 AllocateBuffer 返回）。
#[repr(C)]
struct AudioQueueBuffer {
    m_audio_data_bytes_capacity: u32,
    m_audio_data: *mut u8,
    m_audio_data_byte_size: u32,
    m_user_data: *mut core::ffi::c_void,
    m_packet_description_capacity: u32,
    m_packet_descriptions: *mut core::ffi::c_void,
    m_packet_description_count: u32,
}

type AudioQueueRef = *mut core::ffi::c_void;
type AudioQueueBufferRef = *mut AudioQueueBuffer;
/// OSStatus（i32；0 = noErr）
type OsStatus = i32;

/// AudioQueueInputCallback（AudioQueue.h）：时间戳/包描述两参只透传不解析，用
/// `*const c_void` 承载（指针宽度恒同，ABI 安全且免整套 AudioTimeStamp 定义）。
type InputCallback = unsafe extern "C" fn(
    user_data: *mut core::ffi::c_void,
    aq: AudioQueueRef,
    buffer: AudioQueueBufferRef,
    start_time: *const core::ffi::c_void,
    number_packet_descs: u32,
    packet_descs: *const core::ffi::c_void,
);
/// AudioQueueOutputCallback（AudioQueue.h）
type OutputCallback = unsafe extern "C" fn(
    user_data: *mut core::ffi::c_void,
    aq: AudioQueueRef,
    buffer: AudioQueueBufferRef,
);

#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioQueueNewInput(
        in_format: *const AudioStreamBasicDescription,
        in_callback_proc: InputCallback,
        in_user_data: *mut core::ffi::c_void,
        in_callback_run_loop: *mut core::ffi::c_void,
        in_callback_run_loop_mode: *mut core::ffi::c_void,
        in_flags: u32,
        out_aq: *mut AudioQueueRef,
    ) -> OsStatus;
    fn AudioQueueNewOutput(
        in_format: *const AudioStreamBasicDescription,
        in_callback_proc: OutputCallback,
        in_user_data: *mut core::ffi::c_void,
        in_callback_run_loop: *mut core::ffi::c_void,
        in_callback_run_loop_mode: *mut core::ffi::c_void,
        in_flags: u32,
        out_aq: *mut AudioQueueRef,
    ) -> OsStatus;
    fn AudioQueueAllocateBuffer(
        aq: AudioQueueRef,
        in_buffer_byte_size: u32,
        out_buffer: *mut AudioQueueBufferRef,
    ) -> OsStatus;
    fn AudioQueueEnqueueBuffer(
        aq: AudioQueueRef,
        in_buffer: AudioQueueBufferRef,
        in_num_packet_descs: u32,
        in_packet_descs: *const core::ffi::c_void,
    ) -> OsStatus;
    fn AudioQueueStart(aq: AudioQueueRef, in_start_time: *const core::ffi::c_void) -> OsStatus;
    fn AudioQueueStop(aq: AudioQueueRef, in_immediately: u8) -> OsStatus;
    fn AudioQueueDispose(aq: AudioQueueRef, in_immediately: u8) -> OsStatus;
    fn AudioQueueGetProperty(
        aq: AudioQueueRef,
        in_property_id: u32,
        out_data: *mut core::ffi::c_void,
        io_data_size: *mut u32,
    ) -> OsStatus;
}

// ---------------------------------------------------------------------------
// 策略量（后端缓冲形状；机制在下面的 trait 实现）
// ---------------------------------------------------------------------------

/// 拾音单 buffer 字节量（4096B = 2048 样点 = 128ms @16k mono s16；太小回调过密，
/// 太大拾音延迟变大）。
const MAC_IN_BUF_BYTES: u32 = 4096;
/// 拾音 buffer 个数（3 × 128ms ≈ 384ms 覆盖，回调抖动余量）。
const MAC_IN_BUF_COUNT: usize = 3;
/// 拾音 deque 上限（秒数 × 采样规格）——超限丢最旧（背压策略，对位板侧 audio_q）。
const MAC_INDEQUE_MAX_SECS: u64 = 2;
/// 回放单 buffer 字节量（3200B = 1600 样点 = 100ms @16k mono s16）。
const MAC_OUT_BUF_BYTES: u32 = 3200;
/// 回放 buffer 个数（4 × 100ms = 400ms 队列深度）。
const MAC_OUT_BUF_COUNT: usize = 4;
/// 回放 deque 高水位（秒数 × 采样规格）——`write` 到顶阻塞让出（对位 ALSA 排空）。
const MAC_OUT_WATERMARK_SECS: u64 = 1;
/// kAudioQueueProperty_StreamFormat（'aqft'）：队列协商出的实际流格式——
/// 诊断面用（请求 16k/mono 与协商值不一致 = 转换/路由问题的第一证据）。
const AQ_PROP_STREAM_FORMAT: u32 = 0x6171_6674;

// ---------------------------------------------------------------------------
// 纯逻辑（裁决/形状，可单测）
// ---------------------------------------------------------------------------

/// open 参数门禁：与 ALSA/sim 后端同一套要求（16bit 采样且 rate/channels 非零）。
fn rr3_mac_cfg_check(cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
    if cfg.sample_rate_hz == 0 || cfg.channels == 0 || cfg.bit_depth != 16 {
        return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
            &format!("mac 后端要求 16bit 采样且 rate/channels 非零，收到 {cfg:?}"),
        )));
    }
    Ok(())
}

/// 按采样规格组装 ASBD（s16le 交错线性 PCM；帧长 = 声道数 × 2 字节）。
fn rr3_mac_asbd(cfg: Rr3AudioCfg) -> AudioStreamBasicDescription {
    let bytes_per_frame = cfg.channels as u32 * 2;
    AudioStreamBasicDescription {
        m_sample_rate: f64::from(cfg.sample_rate_hz),
        m_format_id: K_AUDIO_FORMAT_LINEAR_PCM,
        m_format_flags: K_FLAGS_S16LE_INTERLEAVED,
        m_bytes_per_packet: bytes_per_frame, // PCM 无压缩：包 = 帧
        m_frames_per_packet: 1,
        m_bytes_per_frame: bytes_per_frame,
        m_channels_per_frame: u32::from(cfg.channels),
        m_bits_per_channel: 16,
    }
}

/// OSStatus ≠ 0 折成 `InvalidParam`（文案含状态码；与 ALSA open 失败同映射——
/// open 面的错误归参数/设备态，不归 Busy）。
fn rr3_mac_status_err(where_: &str, status: OsStatus) -> Rr3HalError {
    Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
        "mac {where_} 失败: OSStatus {status}"
    )))
}

/// 锁的毒化恢复（linux.rs 同款：锁内无 panic 传播，毒锁照用）。
fn rr3_mac_lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------------------
// 音频源（拾音）
// ---------------------------------------------------------------------------

/// 拾音共享态：回调（AudioQueue 内部线程）与 `read`（模块线程）只经此交接。
/// `cap` 是 deque 样点上限（open 时按采样规格算好存住）——超限丢最旧的背压策略
/// 在回调侧执行，须随 Shared 携带。
struct SourceShared {
    state: Mutex<VecDeque<i16>>,
    cv: Condvar,
    cap: usize,
}

/// 拾音 deque 上限（样点数）：2 秒 × 采样规格。
fn rr3_mac_in_deque_cap(cfg: Rr3AudioCfg) -> usize {
    cfg.sample_rate_hz as usize * cfg.channels as usize * MAC_INDEQUE_MAX_SECS as usize
}

/// 拾音回调：整块拷入 deque 尾（超上限丢最旧），唤醒等待者，buffer 按容量重入队
/// 继续收。锁内无分配、无 panic 面（毒锁恢复 + 尺寸钳制）。
unsafe extern "C" fn rr3_mac_input_cb(
    user_data: *mut core::ffi::c_void,
    aq: AudioQueueRef,
    buffer: AudioQueueBufferRef,
    _start_time: *const core::ffi::c_void,
    _number_packet_descs: u32,
    _packet_descs: *const core::ffi::c_void,
) {
    let shared = &*(user_data as *const SourceShared);
    let filled_bytes = (*buffer).m_audio_data_byte_size;
    let cap_bytes = (*buffer).m_audio_data_bytes_capacity;
    // 钳制：字节数不超容量且为 s16 样点整数倍（CoreAudio 侧数据本应如此，防御性）
    let filled = (filled_bytes.min(cap_bytes) as usize) / 2 * 2;
    if filled > 0 {
        let src = (*buffer).m_audio_data as *const i16;
        let n = filled / 2;
        let mut state = rr3_mac_lock(&shared.state);
        if state.len() + n > shared.cap {
            let drop_n = (state.len() + n - shared.cap).min(state.len());
            state.drain(..drop_n);
        }
        state.extend(core::slice::from_raw_parts(src, n));
        shared.cv.notify_all();
    }
    // 重入队：请求整块容量继续收（m_audio_data_byte_size 置回容量）
    (*buffer).m_audio_data_byte_size = cap_bytes;
    let _ = AudioQueueEnqueueBuffer(aq, buffer, 0, ptr::null());
}

/// 一条已 open 的拾音流：AudioQueue 句柄 + 回调共享态（Box 持有，裸指针交 C）。
struct SourceStream {
    aq: AudioQueueRef,
    shared: *mut SourceShared,
}

// AudioQueue 句柄是线程安全的 C 对象；Shared 全程 Mutex 护栏——跨线程移交安全。
unsafe impl Send for SourceStream {}

impl Drop for SourceStream {
    fn drop(&mut self) {
        // 时序铁律：先 Stop(true)（同步等在途回调收尾）→ Dispose(true)（拆队列），
        // 之后 C 侧再无引用，才回收 Shared——回调-after-free 防线。
        unsafe {
            let _ = AudioQueueStop(self.aq, 1);
            let _ = AudioQueueDispose(self.aq, 1);
            drop(Box::from_raw(self.shared));
        }
    }
}

/// macOS 音频源（CoreAudio AudioQueue 输入，上行拾音）。
pub struct Rr3MacAudioSource {
    inner: Mutex<Option<SourceStream>>,
}

impl Rr3MacAudioSource {
    /// 构造（走系统缺省输入设备；open 时才真开）。
    pub fn rr3_mac_source_new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// 诊断面（真件探针用）：队列协商出的流格式。请求 16k/mono/s16 与协商值
    /// 不一致 = 转换/路由问题第一证据（acc② 排障加）。
    pub fn rr3_mac_source_stream_format_debug(&self) -> String {
        let guard = rr3_mac_lock(&self.inner);
        let Some(st) = guard.as_ref() else {
            return "未 open".into();
        };
        let mut fmt = AudioStreamBasicDescription {
            m_sample_rate: 0.0,
            m_format_id: 0,
            m_format_flags: 0,
            m_bytes_per_packet: 0,
            m_frames_per_packet: 0,
            m_bytes_per_frame: 0,
            m_channels_per_frame: 0,
            m_bits_per_channel: 0,
        };
        let mut size =
            u32::try_from(std::mem::size_of::<AudioStreamBasicDescription>()).unwrap_or(0);
        // SAFETY：aq 为 open 出参合法句柄，fmt/size 为本地出参，属性类型与尺寸匹配
        let status = unsafe {
            AudioQueueGetProperty(
                st.aq,
                AQ_PROP_STREAM_FORMAT,
                &mut fmt as *mut _ as *mut core::ffi::c_void,
                &mut size,
            )
        };
        if status != 0 {
            return format!("GetProperty 失败 status={status}");
        }
        format!(
            "rate={} ch={} bits={} bytes/frame={} flags=0x{:x} fmtid=0x{:x}",
            fmt.m_sample_rate,
            fmt.m_channels_per_frame,
            fmt.m_bits_per_channel,
            fmt.m_bytes_per_frame,
            fmt.m_format_flags,
            fmt.m_format_id
        )
    }
}

impl Rr3AudioSource for Rr3MacAudioSource {
    fn open(&mut self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        rr3_mac_cfg_check(cfg)?;
        let shared = Box::into_raw(Box::new(SourceShared {
            state: Mutex::new(VecDeque::with_capacity(rr3_mac_in_deque_cap(cfg))),
            cv: Condvar::new(),
            cap: rr3_mac_in_deque_cap(cfg),
        }));
        // 任何后续失败都要回收 Shared（回调可能已注册，故先 Dispose 再回收）
        let mut aq: AudioQueueRef = ptr::null_mut();
        let asbd = rr3_mac_asbd(cfg);
        // SAFETY：asbd/shared 为合法指针，回调是非捕获 fn，out_aq 为本地出参
        let status = unsafe {
            AudioQueueNewInput(
                &asbd,
                rr3_mac_input_cb,
                shared as *mut core::ffi::c_void,
                ptr::null_mut(), // NULL runloop = AudioQueue 内部线程回调
                ptr::null_mut(),
                0,
                &mut aq,
            )
        };
        if status != 0 {
            unsafe { drop(Box::from_raw(shared)) };
            return Err(rr3_mac_status_err("AudioQueueNewInput", status));
        }
        if let Err(e) = Self::prime_and_start(aq) {
            unsafe {
                let _ = AudioQueueStop(aq, 1);
                let _ = AudioQueueDispose(aq, 1);
                drop(Box::from_raw(shared));
            }
            return Err(e);
        }
        *rr3_mac_lock(&self.inner) = Some(SourceStream { aq, shared });
        Ok(())
    }

    fn read(&mut self, buf: &mut [i16]) -> Result<usize, Rr3HalError> {
        let guard = rr3_mac_lock(&self.inner);
        let Some(st) = guard.as_ref() else {
            return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
                "mac 源 read 前必须 open",
            )));
        };
        let shared = unsafe { &*st.shared };
        let mut state = rr3_mac_lock(&shared.state);
        while state.len() < buf.len() {
            state = shared.cv.wait(state).unwrap_or_else(|p| p.into_inner());
        }
        for (d, b) in state.drain(..buf.len()).zip(buf.iter_mut()) {
            *b = d;
        }
        Ok(buf.len())
    }
}

impl Rr3MacAudioSource {
    /// 分配并预入队拾音 buffer（byte_size 置满容量 = 请求整块收满），Start 起流。
    fn prime_and_start(aq: AudioQueueRef) -> Result<(), Rr3HalError> {
        for _ in 0..MAC_IN_BUF_COUNT {
            let mut buf: AudioQueueBufferRef = ptr::null_mut();
            // SAFETY：aq 为 NewInput 出参合法句柄，buf 为本地出参
            let status = unsafe { AudioQueueAllocateBuffer(aq, MAC_IN_BUF_BYTES, &mut buf) };
            if status != 0 {
                return Err(rr3_mac_status_err("AudioQueueAllocateBuffer", status));
            }
            unsafe {
                (*buf).m_audio_data_byte_size = MAC_IN_BUF_BYTES;
                let status = AudioQueueEnqueueBuffer(aq, buf, 0, ptr::null());
                if status != 0 {
                    return Err(rr3_mac_status_err("AudioQueueEnqueueBuffer", status));
                }
            }
        }
        let status = unsafe { AudioQueueStart(aq, ptr::null()) };
        if status != 0 {
            return Err(rr3_mac_status_err("AudioQueueStart", status));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 播放面（回放）
// ---------------------------------------------------------------------------

/// 回放共享态：`write`（模块线程）入队，输出回调（AudioQueue 内部线程）排水。
struct SinkShared {
    state: Mutex<SinkState>,
    cv: Condvar,
}

struct SinkState {
    deque: VecDeque<i16>,
    /// 高水位（样点数），open 时按采样规格算好存住
    watermark: usize,
}

/// 回放回调：从 deque 取满一块（不足补零=静音拍），Enqueue 续播，唤醒 write。
unsafe extern "C" fn rr3_mac_output_cb(
    user_data: *mut core::ffi::c_void,
    aq: AudioQueueRef,
    buffer: AudioQueueBufferRef,
) {
    let shared = &*(user_data as *const SinkShared);
    let cap_bytes = (*buffer).m_audio_data_bytes_capacity;
    let cap_samples = cap_bytes as usize / 2;
    let dst = (*buffer).m_audio_data as *mut i16;
    let mut state = rr3_mac_lock(&shared.state);
    let n = state.deque.len().min(cap_samples);
    for (i, s) in state.deque.drain(..n).enumerate() {
        *dst.add(i) = s;
    }
    if n < cap_samples {
        ptr::write_bytes(dst.add(n), 0, cap_samples - n);
    }
    shared.cv.notify_all();
    drop(state);
    (*buffer).m_audio_data_byte_size = cap_bytes;
    let _ = AudioQueueEnqueueBuffer(aq, buffer, 0, ptr::null());
}

/// 一条已 open 的回放流：同 [`SourceStream`] 的生命周期纪律。
struct SinkStream {
    aq: AudioQueueRef,
    shared: *mut SinkShared,
}

unsafe impl Send for SinkStream {}

impl Drop for SinkStream {
    fn drop(&mut self) {
        // 同 SourceStream：先同步停，再拆，最后回收
        unsafe {
            let _ = AudioQueueStop(self.aq, 1);
            let _ = AudioQueueDispose(self.aq, 1);
            drop(Box::from_raw(self.shared));
        }
    }
}

/// macOS 播放面（CoreAudio AudioQueue 输出，下行出话）。
pub struct Rr3MacAudioSink {
    inner: Mutex<Option<SinkStream>>,
}

impl Rr3MacAudioSink {
    /// 构造（走系统缺省输出设备；open 时才真开）。
    pub fn rr3_mac_sink_new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

impl Rr3AudioSink for Rr3MacAudioSink {
    fn open(&mut self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        rr3_mac_cfg_check(cfg)?;
        let watermark =
            cfg.sample_rate_hz as usize * cfg.channels as usize * MAC_OUT_WATERMARK_SECS as usize;
        let shared = Box::into_raw(Box::new(SinkShared {
            state: Mutex::new(SinkState {
                deque: VecDeque::with_capacity(watermark),
                watermark,
            }),
            cv: Condvar::new(),
        }));
        let mut aq: AudioQueueRef = ptr::null_mut();
        let asbd = rr3_mac_asbd(cfg);
        // SAFETY：同拾音 open
        let status = unsafe {
            AudioQueueNewOutput(
                &asbd,
                rr3_mac_output_cb,
                shared as *mut core::ffi::c_void,
                ptr::null_mut(),
                ptr::null_mut(),
                0,
                &mut aq,
            )
        };
        if status != 0 {
            unsafe { drop(Box::from_raw(shared)) };
            return Err(rr3_mac_status_err("AudioQueueNewOutput", status));
        }
        if let Err(e) = Self::prime_and_start(aq) {
            unsafe {
                let _ = AudioQueueStop(aq, 1);
                let _ = AudioQueueDispose(aq, 1);
                drop(Box::from_raw(shared));
            }
            return Err(e);
        }
        *rr3_mac_lock(&self.inner) = Some(SinkStream { aq, shared });
        Ok(())
    }

    fn write(&mut self, samples: &[i16]) -> Result<(), Rr3HalError> {
        let guard = rr3_mac_lock(&self.inner);
        let Some(st) = guard.as_ref() else {
            return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
                "mac 扬声器 write 前必须 open",
            )));
        };
        let shared = unsafe { &*st.shared };
        let mut state = rr3_mac_lock(&shared.state);
        let mut i = 0usize;
        while i < samples.len() {
            while state.deque.len() >= state.watermark {
                state = shared.cv.wait(state).unwrap_or_else(|p| p.into_inner());
            }
            let room = state.watermark - state.deque.len();
            let take = room.min(samples.len() - i);
            state.deque.extend(&samples[i..i + take]);
            i += take;
        }
        Ok(())
    }
}

impl Rr3MacAudioSink {
    /// 分配回放 buffer（先清零，首拍静音而非垃圾）、全部入队，Start 起流。
    fn prime_and_start(aq: AudioQueueRef) -> Result<(), Rr3HalError> {
        for _ in 0..MAC_OUT_BUF_COUNT {
            let mut buf: AudioQueueBufferRef = ptr::null_mut();
            // SAFETY：aq 为 NewOutput 出参合法句柄
            let status = unsafe { AudioQueueAllocateBuffer(aq, MAC_OUT_BUF_BYTES, &mut buf) };
            if status != 0 {
                return Err(rr3_mac_status_err("AudioQueueAllocateBuffer", status));
            }
            unsafe {
                ptr::write_bytes((*buf).m_audio_data, 0, MAC_OUT_BUF_BYTES as usize);
                (*buf).m_audio_data_byte_size = MAC_OUT_BUF_BYTES;
                let status = AudioQueueEnqueueBuffer(aq, buf, 0, ptr::null());
                if status != 0 {
                    return Err(rr3_mac_status_err("AudioQueueEnqueueBuffer", status));
                }
            }
        }
        let status = unsafe { AudioQueueStart(aq, ptr::null()) };
        if status != 0 {
            return Err(rr3_mac_status_err("AudioQueueStart", status));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 单测（只测纯逻辑；真设备证据走手跑实录，见 docs/status.md）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RR3_AUDIO_CFG_16K_MONO16;

    #[test]
    fn cfg_check_rejects_non_16bit_and_zero() {
        assert!(rr3_mac_cfg_check(RR3_AUDIO_CFG_16K_MONO16).is_ok());
        for bad in [
            Rr3AudioCfg {
                sample_rate_hz: 0,
                channels: 1,
                bit_depth: 16,
            },
            Rr3AudioCfg {
                sample_rate_hz: 16000,
                channels: 0,
                bit_depth: 16,
            },
            Rr3AudioCfg {
                sample_rate_hz: 16000,
                channels: 1,
                bit_depth: 8,
            },
        ] {
            let err = rr3_mac_cfg_check(bad).unwrap_err();
            assert!(
                matches!(err, Rr3HalError::InvalidParam(_)),
                "非法 cfg 应回 InvalidParam: {bad:?}"
            );
        }
    }

    #[test]
    fn asbd_shape_is_s16le_interleaved() {
        let a = rr3_mac_asbd(RR3_AUDIO_CFG_16K_MONO16);
        assert_eq!(a.m_sample_rate, 16000.0);
        assert_eq!(a.m_format_id, 0x6C70_636D, "m_format_id 应为 'lpcm'");
        assert_eq!(
            a.m_format_flags, K_FLAGS_S16LE_INTERLEAVED,
            "s16le 交错 = SignedInteger|Packed"
        );
        assert_eq!(a.m_bytes_per_frame, 2);
        assert_eq!(a.m_bytes_per_packet, 2);
        assert_eq!(a.m_frames_per_packet, 1);
        assert_eq!(a.m_channels_per_frame, 1);
        assert_eq!(a.m_bits_per_channel, 16);
        // 双声道 44.1k：帧长 = 2×2 字节
        let b = rr3_mac_asbd(Rr3AudioCfg {
            sample_rate_hz: 44100,
            channels: 2,
            bit_depth: 16,
        });
        assert_eq!(b.m_bytes_per_frame, 4);
        assert_eq!(b.m_channels_per_frame, 2);
        assert_eq!(b.m_sample_rate, 44100.0);
    }

    #[test]
    fn status_err_carries_code() {
        let e = rr3_mac_status_err("AudioQueueStart", -50);
        let Rr3HalError::InvalidParam(t) = &e else {
            panic!("open 面错误应折 InvalidParam");
        };
        assert!(t.rr3_err_text_as_str().contains("OSStatus -50"));
    }

    /// 真设备探针（缺省跳过，不进 CI——CI 真开麦是纪律红线）。手跑：
    /// `cargo test -p rr-hal macaudio -- --ignored --nocapture`
    /// （会开真麦 ~1s，宿主终端首次触发 TCC 麦克风授权弹窗）。
    #[test]
    #[ignore = "真设备冒烟：会开真麦，手跑见注"]
    fn mac_source_real_device_reads_nonzero_audio() {
        let mut src = Rr3MacAudioSource::rr3_mac_source_new();
        src.open(RR3_AUDIO_CFG_16K_MONO16).expect("mac 源 open");
        println!(
            "queue 协商流格式: {}",
            src.rr3_mac_source_stream_format_debug()
        );
        println!("--- 10s 拾音包络（请对麦克风说话，块峰 >3000 即语音级）---");
        let mut buf = [0i16; 8000]; // 0.5s @16k
        for b in 0..20 {
            let n = src.read(&mut buf).expect("mac 源 read");
            assert_eq!(n, buf.len());
            let peak = buf.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
            let energy: f64 = buf.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
            let rms = (energy / n as f64).sqrt() as u32;
            let bar = "█".repeat((peak / 500).min(40) as usize);
            println!("块{b:02}  peak={peak:5}  rms={rms:5}  {bar}");
        }
    }
}
