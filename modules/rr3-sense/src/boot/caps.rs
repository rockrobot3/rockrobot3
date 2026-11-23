// SPDX-License-Identifier: Apache-2.0
//! 配置容量/域界判据（解析后统一校验，默认值同过一遍）。
//!
//! 全部 fail-closed：错误文案点名键、收到值与合法域；上界取"板上可用内存与
//! 定时器语义"的物理约束（8MiB 判向 ring、1h 策略毫秒量等），不是随手拍数。
//!
//! 拆分动因：`boot.rs` 508 行，容量判据与配置段解析分属两簇。

use super::*;

/// 采集面不变量上界（本轮审计）：采样率/声道/单拍时长属工程域，超界即配置错，
/// 不是扩展点——0 采样率/0 声道会让采集面永远静默却看起来活着，巨型
/// frame_ms/window_ms 会在 `frames_per_tick` 与判向 ring 分配处溢出或吃光内存。
const RR3_MAX_SAMPLE_RATE: u32 = 384_000;
const RR3_MAX_CHANNELS: u8 = 8;
const RR3_MAX_FRAME_MS: u64 = 1_000;
/// 判向窗 ring 的采样点总上界（8Mi 点 ≈ 16MiB）
const RR3_MAX_RING_SAMPLES: u128 = 8 * 1024 * 1024;
/// 反射环时间策略量上界（本轮静态审计）：门控尾窗/播放间隔/点头两段时序都是
/// 与 `Instant` 或 `Duration` 相加的毫秒量，畸形巨值在 debug 档触发加法溢出
/// panic、release 档回绕成"门立关/手势错时"——长于 10min 的门控与蓄势在真机上
/// 没有语义，超界即配置错，装载期点名拒（与 frame_ms/window_ms 同一 fail-closed 口径）。
const RR3_MAX_STRATEGY_MS: u64 = 600_000;

/// 采集/判据面不变量（解析后统一校验，默认值同过一遍）：全部 fail-closed，
/// 错误文案点名键、收到值与合法域。
pub(super) fn rr3_sense_cfg_check(cfg: &Rr3SenseConfig) -> Result<(), String> {
    rr3_media_cap_check(&cfg.audio, &cfg.vad)?;
    rr3_deg_cap_pair(cfg)?;
    rr3_ring_cap_check(cfg)?;
    rr3_span_cap_check(&cfg.reflex)
}

/// 采集面与 VAD 面域（采样/声道/单拍 + 阈值）。
fn rr3_media_cap_check(audio: &Rr3AudioConfig, vad: &Rr3VadConfig) -> Result<(), String> {
    rr3_audio_cap_check(audio)?;
    rr3_vad_cap_check(vad)
}

/// 两处角度键域（doa 档界 + reflex 死区）。
fn rr3_deg_cap_pair(cfg: &Rr3SenseConfig) -> Result<(), String> {
    rr3_deg_cap_check(
        "rr3-sense.doa",
        "bin_threshold_deg",
        cfg.doa.bin_threshold_deg,
    )?;
    rr3_deg_cap_check("rr3-sense.reflex", "deadband_deg", cfg.reflex.deadband_deg)
}

/// 采集面三键域（采样率/声道/单拍时长）。
fn rr3_audio_cap_check(audio: &Rr3AudioConfig) -> Result<(), String> {
    rr3_cap_rate(audio)?;
    rr3_cap_chan(audio)?;
    rr3_cap_frame(audio)
}

/// 单拍时长域（1..=上限）。
fn rr3_cap_frame(audio: &Rr3AudioConfig) -> Result<(), String> {
    if audio.frame_ms > RR3_MAX_FRAME_MS {
        return Err(format!(
            "[rr3-sense.audio] frame_ms 须在 1..={RR3_MAX_FRAME_MS}，收到 {}",
            audio.frame_ms
        ));
    }
    Ok(())
}

/// 采样率域（1..=上限）。
fn rr3_cap_rate(audio: &Rr3AudioConfig) -> Result<(), String> {
    if audio.sample_rate == 0 || audio.sample_rate > RR3_MAX_SAMPLE_RATE {
        return Err(format!(
            "[rr3-sense.audio] sample_rate 须在 1..={RR3_MAX_SAMPLE_RATE}，收到 {}",
            audio.sample_rate
        ));
    }
    Ok(())
}

/// 声道数域（1..=上限）。
fn rr3_cap_chan(audio: &Rr3AudioConfig) -> Result<(), String> {
    if audio.channels == 0 || audio.channels > RR3_MAX_CHANNELS {
        return Err(format!(
            "[rr3-sense.audio] channels 须在 1..={RR3_MAX_CHANNELS}，收到 {}",
            audio.channels
        ));
    }
    Ok(())
}

/// VAD 阈值域（i16 采样域；收尾阈值不得高于起音，反向滞回语义反转）。
fn rr3_vad_cap_check(vad: &Rr3VadConfig) -> Result<(), String> {
    if !(0..=32767).contains(&vad.rms_start) {
        return Err(format!(
            "[rr3-sense.vad] rms_start 须在 0..=32767（i16 采样域），收到 {}",
            vad.rms_start
        ));
    }
    if !(0..=32767).contains(&vad.rms_tail) {
        return Err(format!(
            "[rr3-sense.vad] rms_tail 须在 0..=32767（i16 采样域），收到 {}",
            vad.rms_tail
        ));
    }
    if vad.rms_tail > vad.rms_start {
        return Err(format!(
            "[rr3-sense.vad] rms_tail 不得高于 rms_start（收尾阈值须形成滞回），收到 tail={} start={}",
            vad.rms_tail, vad.rms_start
        ));
    }
    Ok(())
}

/// 角度键域（0..=180；段名/键名由调用方点名，便于定位配置错）。
fn rr3_deg_cap_check(tag: &str, key: &str, deg: i32) -> Result<(), String> {
    if !(0..=180).contains(&deg) {
        return Err(format!("[{tag}] {key} 须在 0..=180，收到 {deg}"));
    }
    Ok(())
}

/// 判向 ring 分配量 = rate × window_ms / 1000 × channels（采样点）：用 u128
/// 计算再比上界，避免 u64 乘法在畸形配置下溢出。
fn rr3_ring_cap_check(cfg: &Rr3SenseConfig) -> Result<(), String> {
    let ring_samples = cfg.audio.sample_rate as u128 * cfg.doa.window_ms as u128 / 1000
        * cfg.audio.channels as u128;
    if ring_samples > RR3_MAX_RING_SAMPLES {
        return Err(format!(
            "[rr3-sense.doa] window_ms={} 的判向 ring 需 {ring_samples} 采样点，超上界 {RR3_MAX_RING_SAMPLES}（约 16MiB）",
            cfg.doa.window_ms
        ));
    }
    Ok(())
}

/// 反射环四时间键的上界判定（判据单点；键名逐条点名便于定位配置错）。
fn rr3_span_cap_check(r: &Rr3ReflexConfig) -> Result<(), String> {
    for (key, v) in [
        ("gate_tail_ms", r.gate_tail_ms),
        ("gate_gap_ms", r.gate_gap_ms),
        ("nod_delay_ms", r.nod_delay_ms),
        ("nod_hold_ms", r.nod_hold_ms),
    ] {
        rr3_span_one(key, v)?;
    }
    Ok(())
}

/// 单键上界判定（键名逐条点名便于定位配置错）。
fn rr3_span_one(key: &str, v: u64) -> Result<(), String> {
    if v > RR3_MAX_STRATEGY_MS {
        return Err(format!(
            "[rr3-sense.reflex] {key} 须在 0..={RR3_MAX_STRATEGY_MS}，收到 {v}（门控/手势毫秒量与 Instant 相加，超界会溢出 panic 或回绕）"
        ));
    }
    Ok(())
}
