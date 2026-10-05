//! 舵机控制：接收 `rr3-act/servo/command`（角度/时限，<100Hz 事务面），
//! 摘要发布 `rr3-act/servo/state`（限速流）。驱动经 rr-hal trait（`Rr3PwmServo`），
//! 不直接碰 /sys；行程与时限裁决在这里（限值来自 config.toml），HAL 只执行。

use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub const RR3_SERVO_COMMAND_TOPIC: &str = "rr3-act/servo/command";
pub const RR3_SERVO_STATE_TOPIC: &str = "rr3-act/servo/state";

/// 启动自述：本子模块的总线面一行说明
pub const SURFACE: &[&str] =
    &["servo: rpc rr3-act/servo/command (request) | publish rr3-act/servo/state (stream，限速)"];

/// config `[rr3-act.servo]` 的形状（解析与裁决的唯一依据）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ServoCfg {
    pub angle_min_deg: u16,
    pub angle_max_deg: u16,
    /// 指令时限上限：超过它拒收——"超时即停，不给堵转留机会"
    pub move_timeout_ms: u64,
    /// servo/state 两次发布的最小间隔（限速流的"限速"）
    pub state_min_interval_ms: u64,
}

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

/// servo/state 限速门：状态变化必发布，但两次发布至少隔 min_interval。
/// 被限掉的这一次**不缓存不补发**——下一次发布自然携带最新角度：流是摘要，
/// 不是逐拍镜像（命令面本就 <100Hz，限速只挡突发连发）。
pub struct Rr3RateGate {
    min_interval: Duration,
    last: Option<Instant>,
}

impl Rr3RateGate {
    pub fn rr3_gate_new(min_interval_ms: u64) -> Self {
        Self {
            min_interval: Duration::from_millis(min_interval_ms),
            last: None,
        }
    }

    /// `now` 注入而非内部取时钟：测试用合成时刻，不打真实时钟。
    /// 放行即记账（返回 true 的同时刷新 last）。
    pub fn rr3_gate_allow(&mut self, now: Instant) -> bool {
        if let Some(t) = self.last {
            if now.duration_since(t) < self.min_interval {
                return false;
            }
        }
        self.last = Some(now);
        true
    }
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

    /// 限速门：首拍必放；窗内连发被限；窗外恢复放行。now 全部合成注入。
    #[test]
    fn rate_gate_admits_first_blocks_within_window_reopens_after() {
        let mut gate = Rr3RateGate::rr3_gate_new(50);
        let t0 = Instant::now();
        assert!(gate.rr3_gate_allow(t0), "首拍必放");
        assert!(!gate.rr3_gate_allow(t0 + Duration::from_millis(10)));
        assert!(!gate.rr3_gate_allow(t0 + Duration::from_millis(49)));
        assert!(
            gate.rr3_gate_allow(t0 + Duration::from_millis(50)),
            "恰好到窗放行"
        );
        assert!(!gate.rr3_gate_allow(t0 + Duration::from_millis(60)));
        assert!(
            gate.rr3_gate_allow(t0 + Duration::from_millis(110)),
            "窗从上次放行起算"
        );
    }

    #[test]
    fn state_body_carries_seq_and_angle() {
        let v = rr3_servo_state_body(7, 135);
        assert_eq!(v["seq"], 7);
        assert_eq!(v["angle_deg"], 135);
    }
}
