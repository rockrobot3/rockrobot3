//! servo 原子件机制主体（五件套③，自原 src/servo.rs 逐字迁入，逻辑零改动）。
//!
//! servo/command 参数裁决（行程/时限）与 servo/state 限速门：状态变化必发布，
//! 但两次发布至少隔 min_interval——被限掉的这一次不缓存不补发，流是摘要不是
//! 逐拍镜像（命令面本就 <100Hz，限速只挡突发连发）。
//!
//! `Rr3RateGate` 真值在 rr-core `control`（批D₃，ADR-0018 裁定 5：机制核纯
//! 函数 + 注入时钟）——本文件 re-export 保既有路径，时钟由调用方给 ms。

use serde_json::{json, Value};

use super::Rr3ServoCfg;

pub use rr_core::control::Rr3RateGate;

/// servo/command 参数裁决：`{"angle_deg": int, "move_ms"?: int}`。
/// 业务性失败（缺参数/类型不符/越界）一律 `Err` 点名缘由，由调用方折成 result.error；
/// 缺 move_ms 取 config 上限（实机后端按它限时驱动，堵转由反射环兜）。
pub fn rr3_servo_parse_args(args: &Value, cfg: &Rr3ServoCfg) -> Result<(u16, u64), String> {
    let angle = args
        .get("angle_deg")
        .ok_or_else(|| "缺 angle_deg（int，行程内角度）".to_string())?;
    let angle = angle
        .as_u64()
        .ok_or_else(|| format!("angle_deg 必须是非负整数，收到 {angle}"))?;
    let angle = u16::try_from(angle).map_err(|_| format!("angle_deg 超出 u16 范围: {angle}"))?;
    if !(cfg.angle_min_deg..=cfg.angle_max_deg).contains(&angle) {
        return Err(format!(
            "angle_deg={angle} 超出行程 [{}, {}]",
            cfg.angle_min_deg, cfg.angle_max_deg
        ));
    }
    let move_ms = match args.get("move_ms") {
        None | Some(Value::Null) => cfg.move_timeout_ms,
        Some(v) => {
            let ms = v
                .as_u64()
                .ok_or_else(|| format!("move_ms 必须是非负整数，收到 {v}"))?;
            if ms == 0 {
                return Err("move_ms 必须大于 0".into());
            }
            if ms > cfg.move_timeout_ms {
                return Err(format!(
                    "move_ms={ms} 超出时限上限 {}（不给堵转留机会）",
                    cfg.move_timeout_ms
                ));
            }
            ms
        }
    };
    Ok((angle, move_ms))
}

/// servo/state 体：seq 逐条 +1（流序），angle_deg 是当前角
pub fn rr3_servo_state_body(seq: u64, angle_deg: u16) -> Value {
    json!({ "seq": seq, "angle_deg": angle_deg })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg() -> Rr3ServoCfg {
        Rr3ServoCfg {
            angle_min_deg: 0,
            angle_max_deg: 180,
            move_timeout_ms: 800,
            state_min_interval_ms: 50,
        }
    }

    #[test]
    fn parse_accepts_angle_with_optional_move_ms() {
        let (angle, ms) = rr3_servo_parse_args(&json!({"angle_deg": 90}), &cfg()).unwrap();
        assert_eq!((angle, ms), (90, 800), "缺 move_ms 取 config 上限");
        let (angle, ms) =
            rr3_servo_parse_args(&json!({"angle_deg": 0, "move_ms": 200}), &cfg()).unwrap();
        assert_eq!((angle, ms), (0, 200), "行程下界合法，时限显式生效");
        let (angle, _) =
            rr3_servo_parse_args(&json!({"angle_deg": 180, "move_ms": 800}), &cfg()).unwrap();
        assert_eq!(angle, 180, "行程上界合法");
    }

    #[test]
    fn parse_rejects_missing_non_integer_and_out_of_range() {
        let e = rr3_servo_parse_args(&json!({}), &cfg()).unwrap_err();
        assert!(e.contains("angle_deg"), "缺参数要点名: {e}");
        for bad in [
            json!({"angle_deg": -5}),
            json!({"angle_deg": "90"}),
            json!({"angle_deg": 90.5}),
            json!({"angle_deg": 70000}),
        ] {
            assert!(
                rr3_servo_parse_args(&bad, &cfg()).is_err(),
                "非合法非负整数必须拒: {bad}"
            );
        }
        for deg in [181u64, 70000] {
            let e = rr3_servo_parse_args(&json!({"angle_deg": deg}), &cfg()).unwrap_err();
            assert!(
                e.contains("行程") || e.contains("u16"),
                "越界要点名行程区间或数值上限: {e}"
            );
        }
    }

    #[test]
    fn parse_rejects_move_ms_beyond_timeout_and_zero() {
        let e =
            rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 801}), &cfg()).unwrap_err();
        assert!(e.contains("时限上限"), "超上限拒收: {e}");
        let e = rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 0}), &cfg()).unwrap_err();
        assert!(e.contains("大于 0"), "零时限拒收: {e}");
        let e =
            rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": "x"}), &cfg()).unwrap_err();
        assert!(e.contains("move_ms"), "类型不符点名: {e}");
    }

    /// 限速门判定真值随批D₃ 迁 rr-core `control`（逐拍/窗口用例随迁）——
    /// 本地只钉"re-export 可用"：同一构造 + 一次注入调用走通 rr-core 面。
    #[test]
    fn rate_gate_is_rr_core_reexport() {
        let mut gate = Rr3RateGate::rr3_gate_new(50);
        assert!(gate.rr3_gate_allow(1_000));
        assert!(!gate.rr3_gate_allow(1_010), "窗内连发被限（rr-core 判定）");
    }

    #[test]
    fn state_body_carries_seq_and_angle() {
        let v = rr3_servo_state_body(7, 135);
        assert_eq!(v["seq"], 7);
        assert_eq!(v["angle_deg"], 135);
    }
}
