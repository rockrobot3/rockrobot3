//! macaudio 的单测（只测纯逻辑；真设备证据走手跑实录，见 docs/status.md）。

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use super::ffi::*;
use super::shared::*;
use super::source::*;

use crate::{Rr3AudioCfg, Rr3AudioSource, Rr3HalError, RR3_AUDIO_CFG_16K_MONO16};

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

/// 有界等（B18 挂死面回归）：无唤醒源时到点返回 false，绝不永久挂起——
/// 设备掉线下 read/write 走这条分支带因上抛（此前是无超时 `cv.wait`）。
#[test]
fn wait_times_out_without_notify() {
    let m = Mutex::new(0usize);
    let cv = Condvar::new();
    let (guard, ready) = rr3_mac_wait(
        &cv,
        m.lock().unwrap(),
        |n| *n > 0,
        Duration::from_millis(30),
    );
    assert!(!ready, "无唤醒源必须到点返回，不得挂死");
    drop(guard);
}

/// 条件已成立时立即就绪，不吃满超时窗（短超时下也不假红）。
#[test]
fn wait_returns_ready_without_waiting() {
    let m = Mutex::new(3usize);
    let cv = Condvar::new();
    let (guard, ready) = rr3_mac_wait(
        &cv,
        m.lock().unwrap(),
        |n| *n >= 3,
        Duration::from_millis(500),
    );
    assert!(ready, "条件已成立应立即返回");
    drop(guard);
}

/// 真设备探针（缺省跳过，不进 CI——CI 真开麦是纪律红线）。手跑：
/// `cargo test -p rr-hal macaudio -- --ignored --nocapture`
/// （会开真麦 ~1s，宿主终端首次触发 TCC 麦克风授权弹窗）。
#[test]
#[ignore = "真设备冒烟：会开真麦，手跑见注"]
fn mac_source_real_device_reads_nonzero_audio() {
    let mut src = Rr3MacAudioSource::rr3_mac_source_new();
    src.open(RR3_AUDIO_CFG_16K_MONO16).expect("mac 源 open");
    println!("queue 协商流格式: {}", src.rr3_mac_stream_fmt());
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
