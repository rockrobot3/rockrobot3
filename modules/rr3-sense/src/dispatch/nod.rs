//! 点头手势两段计划与到点推进。

use std::time::{Duration, Instant};

use rr3mod::BusHandle;
use rr_hal::backend::Rr3AudioCmdProducer;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use rr_hal::backend::Rr3PeriphCmd;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::RR3_REFLEX_TILT_CMDS_KEY;

/// 点头手势两段计划（纯数据；时序在 [`rr3_reflex_nod_plan`] 一次算定）：
/// 蓄势段 = 回 tilt 中位，上仰段 = 中位 + nod_deg。两段各自到点下发。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Rr3NodState {
    pub(super) dip_deg: u16,
    pub(super) lift_deg: u16,
    pub(super) dip_at: Instant,
    pub(super) lift_at: Instant,
    pub(super) dip_sent: bool,
}

/// 点头推进：蓄势段/上仰段到点即下发并推进，`None` = 手势结束。
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn rr3_nod_advance(
    tx: &Rr3AudioCmdProducer,
    bus: &BusHandle,
    st: Rr3NodState,
    now: Instant,
) -> Option<Rr3NodState> {
    if let Some(next) = rr3_nod_dip_step(tx, bus, st, now) {
        return Some(next);
    }
    if rr3_nod_lift_due(&st, now) {
        rr3_nod_tilt(tx, bus, st.lift_deg);
        eprintln!("反射环: 点头示意 tilt={}°", st.lift_deg);
        return None;
    }
    Some(st)
}

/// 蓄势段：到点即下发并标记已蓄势（`None` = 未到点或已蓄势）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rr3_nod_dip_step(
    tx: &Rr3AudioCmdProducer,
    bus: &BusHandle,
    st: Rr3NodState,
    now: Instant,
) -> Option<Rr3NodState> {
    if !rr3_nod_dip_due(&st, now) {
        return None;
    }
    rr3_nod_tilt(tx, bus, st.dip_deg);
    eprintln!("反射环: 点头蓄势 tilt={}°", st.dip_deg);
    Some(Rr3NodState {
        dip_sent: true,
        ..st
    })
}

/// 蓄势段到点判据（未蓄势且时刻已到）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rr3_nod_dip_due(st: &Rr3NodState, now: Instant) -> bool {
    !st.dip_sent && now >= st.dip_at
}

/// 上仰段到点判据（已蓄势且时刻已到）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rr3_nod_lift_due(st: &Rr3NodState, now: Instant) -> bool {
    st.dip_sent && now >= st.lift_at
}

/// 单脚下发：命令旁路 + 计数（蓄势/上仰两段共用）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rr3_nod_tilt(tx: &Rr3AudioCmdProducer, bus: &BusHandle, deg: u16) {
    tx.rr3_cmd_push(Rr3PeriphCmd::ServoTilt(deg));
    bus.counter_bump(RR3_REFLEX_TILT_CMDS_KEY, 1);
}

/// 点头手势计划（纯函数）：pan 下发时刻 `now` → 两段 tilt 目标与到点时刻。
/// `nod_deg = 0` = 不做手势（旧版逐点等价）。目标 = tilt 中位（蓄势）→ 中位 +
/// `nod_deg`（上仰）；绝对角越出舵机全域 0..=180 不生成（板侧行程 fail-closed
/// 是最终裁决，这里只防无意义负角）。
/// 仅宿主桥档消费（espidf 档无命令旁路）；保留定义供测试与机制面同源。
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
pub(super) fn rr3_reflex_nod_plan(
    center_deg: i32,
    nod_deg: i32,
    delay_ms: u64,
    hold_ms: u64,
    now: Instant,
) -> Option<Rr3NodState> {
    if nod_deg == 0 {
        return None;
    }
    let lift_deg = center_deg + nod_deg;
    if !(0..=180).contains(&lift_deg) {
        return None;
    }
    Some(Rr3NodState {
        dip_deg: center_deg as u16,
        lift_deg: lift_deg as u16,
        dip_at: now + Duration::from_millis(delay_ms),
        lift_at: now + Duration::from_millis(delay_ms + hold_ms),
        dip_sent: false,
    })
}
