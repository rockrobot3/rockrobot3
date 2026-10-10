//! servo 原子件机制主体（五件套③，自原 src/servo.rs 逐字迁入，逻辑零改动）。
//!
//! servo/command 的 Value 下钻（缺参/类型错/越 u16）已上移承载层（0.0.5 批I
//! A-8，ADR-0019 修订 4）：行程（[`rr3_servo_check_travel`]）与时限
//! （[`rr3_servo_check_timeout`]，缺 move_ms 回落 config 上限——实机后端按它
//! 限时驱动，堵转由反射环兜）的值域判定留件，文案是件面契约、被单测逐字
//! 断言。servo/state 限速门：状态变化必发布，但两次发布至少隔 min_interval
//! ——被限掉的这一次不缓存不补发，流是摘要不是逐拍镜像（命令面本就 <100Hz，
//! 限速只挡突发连发）。
//!
//! `Rr3RateGate` 真值在 rr-core `control`（批D₃，ADR-0018 裁定 5：机制核纯
//! 函数 + 注入时钟）——本文件 re-export 保既有路径，时钟由调用方给 ms。

use super::Rr3ServoCfg;

pub use rr_core::control::Rr3RateGate;

/// servo/command 行程判定（值域留件，批I A-8 拆层）：越行程点名区间。
/// 缺参/类型错/越 u16 的下钻文案在承载层（每句落在唯一一处）。
pub fn rr3_servo_check_travel(angle: u16, cfg: &Rr3ServoCfg) -> Result<(), String> {
    if !(cfg.angle_min_deg..=cfg.angle_max_deg).contains(&angle) {
        return Err(format!(
            "angle_deg={angle} 超出行程 [{}, {}]",
            cfg.angle_min_deg, cfg.angle_max_deg
        ));
    }
    Ok(())
}

/// servo/command 时限判定（值域留件，批I A-8 拆层）：`None` 回落 config 上限，
/// 零时限与超上限拒收——"超时即停，不给堵转留机会"。
pub fn rr3_servo_check_timeout(move_ms: Option<u64>, cfg: &Rr3ServoCfg) -> Result<u64, String> {
    match move_ms {
        None => Ok(cfg.move_timeout_ms),
        Some(0) => Err("move_ms 必须大于 0".into()),
        Some(ms) if ms > cfg.move_timeout_ms => Err(format!(
            "move_ms={ms} 超出时限上限 {}（不给堵转留机会）",
            cfg.move_timeout_ms
        )),
        Some(ms) => Ok(ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Rr3ServoCfg {
        Rr3ServoCfg {
            angle_min_deg: 0,
            angle_max_deg: 180,
            move_timeout_ms: 800,
            state_min_interval_ms: 50,
        }
    }

    #[test]
    fn check_travel_accepts_bounds_and_rejects_out_of_range() {
        assert!(rr3_servo_check_travel(0, &cfg()).is_ok(), "行程下界合法");
        assert!(rr3_servo_check_travel(180, &cfg()).is_ok(), "行程上界合法");
        let e = rr3_servo_check_travel(181, &cfg()).unwrap_err();
        assert!(e.contains("行程"), "越界要点名行程区间: {e}");
    }

    #[test]
    fn check_timeout_defaults_to_cfg_and_rejects_zero_beyond_limit() {
        assert_eq!(
            rr3_servo_check_timeout(None, &cfg()).unwrap(),
            800,
            "缺 move_ms 取 config 上限"
        );
        assert_eq!(rr3_servo_check_timeout(Some(200), &cfg()).unwrap(), 200);
        assert_eq!(rr3_servo_check_timeout(Some(800), &cfg()).unwrap(), 800);
        let e = rr3_servo_check_timeout(Some(801), &cfg()).unwrap_err();
        assert!(e.contains("时限上限"), "超上限拒收: {e}");
        let e = rr3_servo_check_timeout(Some(0), &cfg()).unwrap_err();
        assert!(e.contains("大于 0"), "零时限拒收: {e}");
    }

    /// 限速门判定真值随批D₃ 迁 rr-core `control`（逐拍/窗口用例随迁）——
    /// 本地只钉"re-export 可用"：同一构造 + 一次注入调用走通 rr-core 面。
    #[test]
    fn rate_gate_is_rr_core_reexport() {
        let mut gate = Rr3RateGate::rr3_gate_new(50);
        assert!(gate.rr3_gate_allow(1_000));
        assert!(!gate.rr3_gate_allow(1_010), "窗内连发被限（rr-core 判定）");
    }
}
