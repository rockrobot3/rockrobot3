//! `INFO` 观测面（自 commands.rs 抽出）：一拍取齐状态快照后拼行。
//!
//! 快照结构成组命名，替代原 18 元组；取锁只此一次，避免拼行中途状态跳变。
//! 应答形状是板侧对外契约，文案与字段序不得随拆分变化。

use std::sync::{Arc, Mutex};

use esp_idf_sys::{esp_get_free_heap_size, esp_get_minimum_free_heap_size};

use crate::power::RailTier;
use crate::servo::Axis;
use crate::state::{lock_bus, Bus};

/// 一次取锁读全 INFO 字段（取锁只在 [`info_snap`] 内发生一次）。
struct InfoSnap {
    pan: u32,
    tilt: u32,
    wander: bool,
    phase: u8,
    last_text: String,
    rail_mv: u16,
    rail_raw: u16,
    rail_valid: bool,
    rail_tier: RailTier,
    rail_low: u32,
    selfheal_skip: u32,
    audio_on: bool,
    audio_seq: u32,
    audio_sent: u64,
    audio_dropped: u64,
    audio_q: usize,
    job_drop: u32,
    reply_drop: u64,
}

fn info_snap(bus: &Arc<Mutex<Bus>>) -> InfoSnap {
    let b = lock_bus(bus);
    InfoSnap {
        pan: b.st.pan_deg,
        tilt: b.st.tilt_deg,
        wander: b.st.wander_on,
        phase: b.st.wander_phase,
        last_text: b.st.last_text.clone(),
        rail_mv: b.st.rail_mv,
        rail_raw: b.st.rail_raw,
        rail_valid: b.st.rail_valid,
        rail_tier: b.st.rail_tier,
        rail_low: b.st.rail_low_events,
        selfheal_skip: b.st.selfheal_skipped,
        audio_on: b.st.audio_on,
        audio_seq: b.st.audio_seq,
        audio_sent: b.st.audio_sent,
        audio_dropped: b.st.audio_dropped,
        audio_q: b.audio_q.len(),
        job_drop: b.st.job_dropped,
        reply_drop: b.st.reply_drop,
    }
}

/// INFO 的 5V 轨两栏：有效报毫伏与档名，无效点名 absent/unknown（原始码照回显）。
fn rail_fields(valid: bool, mv: u16, raw: u16, tier: RailTier) -> (String, &'static str) {
    if valid {
        (format!("{mv}mV raw={raw}"), tier.name())
    } else {
        (format!("absent raw={raw}"), "unknown")
    }
}

/// `INFO` 观测面：一拍取齐状态快照后拼行。
pub(super) fn cmd_info(bus: &Arc<Mutex<Bus>>) -> Result<String, String> {
    let InfoSnap {
        pan,
        tilt,
        wander,
        phase,
        last_text,
        rail_mv,
        rail_raw,
        rail_valid,
        rail_tier,
        rail_low,
        selfheal_skip,
        audio_on,
        audio_seq,
        audio_sent,
        audio_dropped,
        audio_q,
        job_drop,
        reply_drop,
    } = info_snap(bus);
    let (plo, phi) = Axis::Pan.range();
    let (tlo, thi) = Axis::Tilt.range();
    let (rail, rail_tier_name) = rail_fields(rail_valid, rail_mv, rail_raw, rail_tier);
    // SAFETY：两个堆查询都是无参只读的 IDF 系统调用，不触碰 Rust 内存状态。
    let (free, min_free) = unsafe { (esp_get_free_heap_size(), esp_get_minimum_free_heap_size()) };
    Ok(format!(
        "INFO pan={pan} tilt={tilt} lim=pan[{plo},{phi}]/tilt[{tlo},{thi}] wander={} phase={phase} rail={rail} rail_tier={rail_tier_name} low_events={rail_low} selfheal_skip={selfheal_skip} audio={} seq={audio_seq} sent={audio_sent} drop={audio_dropped} q={audio_q} job_drop={job_drop} reply_drop={reply_drop} oled_last=\"{last_text}\" free={free}B min_free={min_free}B",
        if wander { "on" } else { "off" },
        if audio_on { "on" } else { "off" },
    ))
}
