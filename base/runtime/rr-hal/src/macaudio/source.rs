//! 拾音面：Rr3MacAudioSource 与输入流。

use std::collections::VecDeque;
use std::ptr;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::{Rr3AudioCfg, Rr3AudioSource, Rr3ErrText, Rr3HalError};

use super::ffi::{
    rr3_mac_asbd, rr3_mac_enq, rr3_mac_status_err, AudioQueueAllocateBuffer, AudioQueueBufferRef,
    AudioQueueDispose, AudioQueueEnqueueBuffer, AudioQueueGetProperty, AudioQueueNewInput,
    AudioQueueRef, AudioQueueStart, AudioQueueStop, AudioStreamBasicDescription,
    AQ_PROP_STREAM_FORMAT,
};
use super::shared::{rr3_mac_cfg_check, rr3_mac_lock, rr3_mac_wait, SourceShared, RR3_MAC_WAIT_MS};

/// 拾音单 buffer 字节量（4096B = 2048 样点 = 128ms @16k mono s16；太小回调过密，
/// 太大拾音延迟变大）。
const MAC_IN_BUF_BYTES: u32 = 4096;
/// 拾音 buffer 个数（3 × 128ms ≈ 384ms 覆盖，回调抖动余量）。
const MAC_IN_BUF_COUNT: usize = 3;
/// 拾音 deque 上限（秒数 × 采样规格）——超限丢最旧（背压策略，对位板侧 audio_q）。
const MAC_INDEQUE_MAX_SECS: u64 = 2;

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
        rr3_mac_push_in(shared, (*buffer).m_audio_data as *const i16, filled / 2);
    }
    // 重入队：请求整块容量继续收（m_audio_data_byte_size 置回容量）
    (*buffer).m_audio_data_byte_size = cap_bytes;
    let _ = AudioQueueEnqueueBuffer(aq, buffer, 0, ptr::null());
}

/// 整块拷入 deque 尾（超上限丢最旧）并唤醒等待者。
fn rr3_mac_push_in(shared: &SourceShared, src: *const i16, n: usize) {
    let mut state = rr3_mac_lock(&shared.state);
    if state.len() + n > shared.cap {
        let drop_n = (state.len() + n - shared.cap).min(state.len());
        state.drain(..drop_n);
    }
    // SAFETY：src 指向回调给出的 buffer 数据区，n 为钳制后的样点数。
    state.extend(unsafe { core::slice::from_raw_parts(src, n) });
    shared.cv.notify_all();
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
    pub fn rr3_mac_stream_fmt(&self) -> String {
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

/// 建拾音队列：失败先回收共享态再折 InvalidParam（回调可能已注册，先 Dispose）。
fn rr3_mac_new_input(
    asbd: &AudioStreamBasicDescription,
    shared: *mut SourceShared,
) -> Result<AudioQueueRef, Rr3HalError> {
    let mut aq: AudioQueueRef = ptr::null_mut();
    // SAFETY：asbd/shared 为合法指针，回调是非捕获 fn，out_aq 为本地出参
    let status = unsafe {
        AudioQueueNewInput(
            asbd,
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
    Ok(aq)
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
        let asbd = rr3_mac_asbd(cfg);
        let aq = rr3_mac_new_input(&asbd, shared)?;
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
        let state = rr3_mac_lock(&shared.state);
        let (mut state, ready) = rr3_mac_wait(
            &shared.cv,
            state,
            |q| q.len() >= buf.len(),
            Duration::from_millis(RR3_MAC_WAIT_MS),
        );
        if !ready {
            let got = state.len();
            return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
                &format!(
                    "mac 拾音 {RR3_MAC_WAIT_MS}ms 未凑满 {}/{} 样点——输入设备掉线或回调停摆（查系统输入设备与麦克风授权）",
                    got,
                    buf.len()
                ),
            )));
        }
        rr3_mac_drain_into(&mut state, buf);
        Ok(buf.len())
    }
}

/// 从 deque 取 `buf.len()` 个样点填满调用方缓冲。
fn rr3_mac_drain_into(state: &mut VecDeque<i16>, buf: &mut [i16]) {
    for (d, b) in state.drain(..buf.len()).zip(buf.iter_mut()) {
        *b = d;
    }
}

impl Rr3MacAudioSource {
    /// 分配并预入队拾音 buffer（byte_size 置满容量 = 请求整块收满），Start 起流。
    fn prime_and_start(aq: AudioQueueRef) -> Result<(), Rr3HalError> {
        for _ in 0..MAC_IN_BUF_COUNT {
            rr3_mac_prime_in_buf(aq)?;
        }
        let status = unsafe { AudioQueueStart(aq, ptr::null()) };
        if status != 0 {
            return Err(rr3_mac_status_err("AudioQueueStart", status));
        }
        Ok(())
    }
}

/// 分配并预入队一块拾音 buffer（byte_size 置满容量 = 请求整块收满）。
fn rr3_mac_prime_in_buf(aq: AudioQueueRef) -> Result<(), Rr3HalError> {
    let mut buf: AudioQueueBufferRef = ptr::null_mut();
    // SAFETY：aq 为 NewInput 出参合法句柄，buf 为本地出参
    let status = unsafe { AudioQueueAllocateBuffer(aq, MAC_IN_BUF_BYTES, &mut buf) };
    if status != 0 {
        return Err(rr3_mac_status_err("AudioQueueAllocateBuffer", status));
    }
    unsafe {
        (*buf).m_audio_data_byte_size = MAC_IN_BUF_BYTES;
    }
    rr3_mac_enq(aq, buf)
}
