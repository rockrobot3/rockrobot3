//! 反射环 pan 目标换算（纯函数）。

use super::{RR3_REFLEX_PAN_CENTER_DEG, RR3_REFLEX_PAN_SWING_DEG};

/// 反射环换算（纯函数）：onset 判向角 → pan 目标角。全量判据过不了 = `None`
/// （不转头）：未启用 / 门开（播放回声）/ 死区内（正前方说话免抖动）。
/// 目标 = 中位 + clamp(deg, ±摆幅)——正负号直通（右声源右转头），板侧行程
/// fail-closed 之外这里是第二道软件限幅。
pub(super) fn rr3_reflex_pan(
    enable: bool,
    deadband_deg: i32,
    doa_deg: i32,
    gate_open: bool,
) -> Option<u16> {
    if !enable || gate_open || doa_deg.abs() < deadband_deg {
        return None;
    }
    Some(rr3_pan_swing(doa_deg))
}

/// 判向角 → pan 目标（中位 + ±摆幅限幅）。
fn rr3_pan_swing(doa_deg: i32) -> u16 {
    let swing = doa_deg.clamp(-RR3_REFLEX_PAN_SWING_DEG, RR3_REFLEX_PAN_SWING_DEG);
    (RR3_REFLEX_PAN_CENTER_DEG + swing) as u16
}
