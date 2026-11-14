//! 播放面：Rr3MacAudioSink 与输出流。

use std::collections::VecDeque;
use std::ptr;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::{Rr3AudioCfg, Rr3AudioSink, Rr3ErrText, Rr3HalError};

use super::ffi::{
    rr3_mac_asbd, rr3_mac_enq, rr3_mac_status_err, AudioQueueAllocateBuffer, AudioQueueBufferRef,
    AudioQueueDispose, AudioQueueEnqueueBuffer, AudioQueueNewOutput, AudioQueueRef,
    AudioQueueStart, AudioQueueStop, AudioStreamBasicDescription,
};
use super::shared::{
    rr3_mac_cfg_check, rr3_mac_lock, rr3_mac_wait, SinkShared, SinkState, RR3_MAC_WAIT_MS,
};

/// 回放单 buffer 字节量（3200B = 1600 样点 = 100ms @16k mono s16）。
const MAC_OUT_BUF_BYTES: u32 = 3200;
/// 回放 buffer 个数（4 × 100ms = 400ms 队列深度）。
const MAC_OUT_BUF_COUNT: usize = 4;
/// 回放 deque 高水位（秒数 × 采样规格）——`write` 到顶阻塞让出（对位 ALSA 排空）。
const MAC_OUT_WATERMARK_SECS: u64 = 1;

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
    rr3_mac_fill_out(shared, dst, cap_samples);
    (*buffer).m_audio_data_byte_size = cap_bytes;
    let _ = AudioQueueEnqueueBuffer(aq, buffer, 0, ptr::null());
}

/// 从 deque 取满一块（不足补零=静音拍），唤醒 write。
fn rr3_mac_fill_out(shared: &SinkShared, dst: *mut i16, cap_samples: usize) {
    let mut state = rr3_mac_lock(&shared.state);
    let n = state.deque.len().min(cap_samples);
    for (i, s) in state.deque.drain(..n).enumerate() {
        unsafe { *dst.add(i) = s };
    }
    if n < cap_samples {
        unsafe { ptr::write_bytes(dst.add(n), 0, cap_samples - n) };
    }
    shared.cv.notify_all();
}

/// 一条已 open 的回放流：同 [`SourceStream`](super::source::SourceStream) 的生命周期纪律。
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

/// 建回放队列：失败先回收共享态再折 InvalidParam（与拾音 open 同款）。
fn rr3_mac_new_output(
    asbd: &AudioStreamBasicDescription,
    shared: *mut SinkShared,
) -> Result<AudioQueueRef, Rr3HalError> {
    let mut aq: AudioQueueRef = ptr::null_mut();
    // SAFETY：同拾音 open
    let status = unsafe {
        AudioQueueNewOutput(
            asbd,
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
    Ok(aq)
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
        let asbd = rr3_mac_asbd(cfg);
        let aq = rr3_mac_new_output(&asbd, shared)?;
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
            let (guard, take) = rr3_mac_room(shared, state, i, samples.len())?;
            state = guard;
            state.deque.extend(&samples[i..i + take]);
            i += take;
        }
        Ok(())
    }
}

/// 等可写空位：队列低于高水位即回（守卫, 本次可写样点数）；超时即点名回错。
fn rr3_mac_room<'a>(
    shared: &SinkShared,
    state: std::sync::MutexGuard<'a, SinkState>,
    written: usize,
    total: usize,
) -> Result<(std::sync::MutexGuard<'a, SinkState>, usize), Rr3HalError> {
    let (state, roomy) = rr3_mac_wait(
        &shared.cv,
        state,
        |s| s.deque.len() < s.watermark,
        Duration::from_millis(RR3_MAC_WAIT_MS),
    );
    if !roomy {
        return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
            &format!(
                "mac 回放 {RR3_MAC_WAIT_MS}ms 队列未排水（已写 {written}/{total} 样点）——输出设备掉线或回调停摆"
            ),
        )));
    }
    let room = state.watermark - state.deque.len();
    let take = room.min(total - written);
    Ok((state, take))
}

impl Rr3MacAudioSink {
    /// 分配回放 buffer（先清零，首拍静音而非垃圾）、全部入队，Start 起流。
    fn prime_and_start(aq: AudioQueueRef) -> Result<(), Rr3HalError> {
        for _ in 0..MAC_OUT_BUF_COUNT {
            rr3_mac_out_buf(aq)?;
        }
        let status = unsafe { AudioQueueStart(aq, ptr::null()) };
        if status != 0 {
            return Err(rr3_mac_status_err("AudioQueueStart", status));
        }
        Ok(())
    }
}

/// 分配并预入队一块回放 buffer（先清零，首拍静音而非垃圾）。
fn rr3_mac_out_buf(aq: AudioQueueRef) -> Result<(), Rr3HalError> {
    let mut buf: AudioQueueBufferRef = ptr::null_mut();
    // SAFETY：aq 为 NewOutput 出参合法句柄
    let status = unsafe { AudioQueueAllocateBuffer(aq, MAC_OUT_BUF_BYTES, &mut buf) };
    if status != 0 {
        return Err(rr3_mac_status_err("AudioQueueAllocateBuffer", status));
    }
    unsafe {
        ptr::write_bytes((*buf).m_audio_data, 0, MAC_OUT_BUF_BYTES as usize);
        (*buf).m_audio_data_byte_size = MAC_OUT_BUF_BYTES;
    }
    rr3_mac_enq(aq, buf)
}
