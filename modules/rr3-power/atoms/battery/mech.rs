//! 电池采样与低电边沿（`rr3-power/battery/state` 周期流 + `rr3-power/battery/low`）。
//! 原子件机制主体（五件套③，自原 src/battery.rs 逐字迁入，逻辑零改动）。
//!
//! 关断决策在本地完成，不等总线、不等 brain（architecture §7.3 安全兜底）；
//! `battery/state` 只是摘要上报。低电事件的**边沿语义**（schema 的同一句话）：
//! 同一次低电段只发一次，恢复到阈值之上后再次越过才再发——`Rr3LowEdge` 是它的
//! 唯一真值（批D₃ 起真值在 rr-core `control`，ADR-0018 裁定 5：安全核纯函数，
//! 本文件 re-export 保既有路径）。cutoff 电压阈值本轮只解析不动作（0.0.2 方案
//! Q4：关机与充电策略归 rr3-power 演进与 M6），装配时回显进启动日志，不装"已实现"。

use rr_hal::Rr3BatteryState;
use serde_json::{json, Value};

pub use rr_core::control::Rr3LowEdge;

/// battery/state 流体：seq 是本流自己的计数（envelope 惯例，从 1 起）
pub fn rr3_battery_state_body(seq: u64, st: Rr3BatteryState) -> Value {
    json!({
        "seq": seq,
        "voltage_mv": st.voltage_mv,
        "percent": st.percent,
        "charging": st.charging,
    })
}

/// battery/low 事件体：形状钉在 contracts/events/power-battery-low.schema.json
/// （seq/percent/voltage_mv 三必填，additionalProperties=false——不加多余字段）。
pub fn rr3_battery_low_body(seq: u64, st: Rr3BatteryState) -> Value {
    json!({
        "seq": seq,
        "percent": st.percent,
        "voltage_mv": st.voltage_mv,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(mv: u16, percent: u8) -> Rr3BatteryState {
        Rr3BatteryState {
            voltage_mv: mv,
            percent,
            charging: false,
        }
    }

    /// 低电边沿真值批D₃ 迁 rr-core `control`（越阈/段内/再武装用例随迁）——
    /// 本地只钉"re-export 可用"：同一构造 + 一次注入调用走通 rr-core 面。
    #[test]
    fn low_edge_is_rr_core_reexport() {
        let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
        assert!(e.rr3_low_edge_cross(3290), "越阈触发（rr-core 判定）");
        assert!(!e.rr3_low_edge_cross(3295), "段内不重复报（rr-core 判定）");
    }

    #[test]
    fn state_body_carries_full_summary_with_own_seq() {
        let v = rr3_battery_state_body(7, st(3700, 50));
        assert_eq!(v["seq"], 7);
        assert_eq!(v["voltage_mv"], 3700);
        assert_eq!(v["percent"], 50);
        assert_eq!(v["charging"], false);
    }

    /// battery/low 体与 schema 逐字段对齐：三必填，且**没有**多余字段
    /// （additionalProperties=false，多带 charging 会直接违反契约）。
    #[test]
    fn low_body_matches_schema_exactly() {
        let v = rr3_battery_low_body(1, st(3250, 5));
        assert_eq!(v["seq"], 1);
        assert_eq!(v["percent"], 5);
        assert_eq!(v["voltage_mv"], 3250);
        assert_eq!(
            v.as_object().unwrap().len(),
            3,
            "schema additionalProperties=false，字段超集即违约: {v}"
        );
    }
}
