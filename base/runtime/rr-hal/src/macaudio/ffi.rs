//! AudioQueue 手写外部声明与 ASBD/Buffer 结构。

use std::ptr;

use crate::{Rr3AudioCfg, Rr3ErrText, Rr3HalError};

/// `kAudioFormatLinearPCM` = 'lpcm'
const K_AUDIO_FORMAT_LINEAR_PCM: u32 = 0x6C70_636D;
/// `kLinearPCMFormatFlagIsSignedInteger`（= kAudioFormatFlagIsSignedInteger 别名）
const K_FLAG_SIGNED_INTEGER: u32 = 1 << 2;
/// `kLinearPCMFormatFlagIsPacked`
const K_FLAG_PACKED: u32 = 1 << 3;
/// s16le 交错 PCM 的格式旗：符号整数 + 紧凑打包（不设 NonInterleaved = 交错）
pub(super) const K_FLAGS_S16LE_INTERLEAVED: u32 = K_FLAG_SIGNED_INTEGER | K_FLAG_PACKED;

/// AudioStreamBasicDescription（AudioCoreTypes.h，repr(C) 与 C ABI 同形）
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct AudioStreamBasicDescription {
    pub(super) m_sample_rate: f64,
    pub(super) m_format_id: u32,
    pub(super) m_format_flags: u32,
    pub(super) m_bytes_per_packet: u32,
    pub(super) m_frames_per_packet: u32,
    pub(super) m_bytes_per_frame: u32,
    pub(super) m_channels_per_frame: u32,
    pub(super) m_bits_per_channel: u32,
}

/// AudioQueueBuffer（AudioQueue.h）：只声明本后端读写的字段，其余字段按 C 原形
/// 补齐保持 ABI 对齐（本后端自不分配该结构——句柄由 AllocateBuffer 返回）。
#[repr(C)]
pub(super) struct AudioQueueBuffer {
    pub(super) m_audio_data_bytes_capacity: u32,
    pub(super) m_audio_data: *mut u8,
    pub(super) m_audio_data_byte_size: u32,
    pub(super) m_user_data: *mut core::ffi::c_void,
    pub(super) m_packet_description_capacity: u32,
    pub(super) m_packet_descriptions: *mut core::ffi::c_void,
    pub(super) m_packet_description_count: u32,
}

pub(super) type AudioQueueRef = *mut core::ffi::c_void;
pub(super) type AudioQueueBufferRef = *mut AudioQueueBuffer;
/// OSStatus（i32；0 = noErr）
pub(super) type OsStatus = i32;

/// AudioQueueInputCallback（AudioQueue.h）：时间戳/包描述两参只透传不解析，用
/// `*const c_void` 承载（指针宽度恒同，ABI 安全且免整套 AudioTimeStamp 定义）。
pub(super) type InputCallback = unsafe extern "C" fn(
    user_data: *mut core::ffi::c_void,
    aq: AudioQueueRef,
    buffer: AudioQueueBufferRef,
    start_time: *const core::ffi::c_void,
    number_packet_descs: u32,
    packet_descs: *const core::ffi::c_void,
);
/// AudioQueueOutputCallback（AudioQueue.h）
pub(super) type OutputCallback = unsafe extern "C" fn(
    user_data: *mut core::ffi::c_void,
    aq: AudioQueueRef,
    buffer: AudioQueueBufferRef,
);

#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    pub(super) fn AudioQueueNewInput(
        in_format: *const AudioStreamBasicDescription,
        in_callback_proc: InputCallback,
        in_user_data: *mut core::ffi::c_void,
        in_callback_run_loop: *mut core::ffi::c_void,
        in_callback_run_loop_mode: *mut core::ffi::c_void,
        in_flags: u32,
        out_aq: *mut AudioQueueRef,
    ) -> OsStatus;
    pub(super) fn AudioQueueNewOutput(
        in_format: *const AudioStreamBasicDescription,
        in_callback_proc: OutputCallback,
        in_user_data: *mut core::ffi::c_void,
        in_callback_run_loop: *mut core::ffi::c_void,
        in_callback_run_loop_mode: *mut core::ffi::c_void,
        in_flags: u32,
        out_aq: *mut AudioQueueRef,
    ) -> OsStatus;
    pub(super) fn AudioQueueAllocateBuffer(
        aq: AudioQueueRef,
        in_buffer_byte_size: u32,
        out_buffer: *mut AudioQueueBufferRef,
    ) -> OsStatus;
    pub(super) fn AudioQueueEnqueueBuffer(
        aq: AudioQueueRef,
        in_buffer: AudioQueueBufferRef,
        in_num_packet_descs: u32,
        in_packet_descs: *const core::ffi::c_void,
    ) -> OsStatus;
    pub(super) fn AudioQueueStart(
        aq: AudioQueueRef,
        in_start_time: *const core::ffi::c_void,
    ) -> OsStatus;
    pub(super) fn AudioQueueStop(aq: AudioQueueRef, in_immediately: u8) -> OsStatus;
    pub(super) fn AudioQueueDispose(aq: AudioQueueRef, in_immediately: u8) -> OsStatus;
    pub(super) fn AudioQueueGetProperty(
        aq: AudioQueueRef,
        in_property_id: u32,
        out_data: *mut core::ffi::c_void,
        io_data_size: *mut u32,
    ) -> OsStatus;
}

/// kAudioQueueProperty_StreamFormat（'aqft'）：队列协商出的实际流格式——
/// 诊断面用（请求 16k/mono 与协商值不一致 = 转换/路由问题的第一证据）。
pub(super) const AQ_PROP_STREAM_FORMAT: u32 = 0x6171_6674;

/// 按采样规格组装 ASBD（s16le 交错线性 PCM；帧长 = 声道数 × 2 字节）。
pub(super) fn rr3_mac_asbd(cfg: Rr3AudioCfg) -> AudioStreamBasicDescription {
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
pub(super) fn rr3_mac_status_err(where_: &str, status: OsStatus) -> Rr3HalError {
    Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
        "mac {where_} 失败: OSStatus {status}"
    )))
}

/// 缓冲入队（失败回带 FFI 名字的错）。
pub(super) fn rr3_mac_enq(aq: AudioQueueRef, buf: AudioQueueBufferRef) -> Result<(), Rr3HalError> {
    // SAFETY：aq/buf 均为配好容量的合法句柄
    let status = unsafe { AudioQueueEnqueueBuffer(aq, buf, 0, ptr::null()) };
    if status != 0 {
        return Err(rr3_mac_status_err("AudioQueueEnqueueBuffer", status));
    }
    Ok(())
}
