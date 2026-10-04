//! 电池采样与低电边沿（`rr3-power/battery/state` 周期流 + `rr3-power/battery/low`）。
//!
//! 关断决策在本地完成，不等总线、不等 brain（architecture §7.3 安全兜底）；
//! `battery/state` 只是摘要上报。低电事件的**边沿语义**（schema 的同一句话）：
//! 同一次低电段只发一次，恢复到阈值之上后再次越过才再发——`Rr3LowEdge` 是它的
//! 唯一真值。cutoff 电压阈值本轮只解析不动作（0.0.2 方案 Q4：关机与充电策略
//! 归 rr3-power 演进与 M6），装配时回显进启动日志，不装"已实现"。

use rr_hal::Rr3BatteryState;
use serde_json::{json, Value};

pub const SURFACE: &[&str] = &[
    "battery: 电压/电量/温度摘要 -> rr3-power/battery/state (event)，关断为本地决策",
    "battery: 低电越阈 -> rr3-power/battery/low (event)，同一次低电段恰发一次",
];

/// config `[rr3-power.battery]` 的形状（采样周期 + 两级阈值）
#[derive(Debug, Clone, Copy)]
pub struct Rr3BatteryCfg {
    pub sample_period_ms: u64,
    pub low_voltage_mv: u16,
    pub cutoff_voltage_mv: u16,
}

/// 低电边沿机：armed 时电压跌破阈值 → 报一次并解除武装；电压回到阈值**之上**
/// （严格大于，恰好等于阈值仍算低电段内）→ 重新武装。状态只有这一位。
pub struct Rr3LowEdge {
    low_voltage_mv: u16,
    armed: bool,
}

impl Rr3LowEdge {
    pub fn rr3_low_edge_new(low_voltage_mv: u16) -> Self {
        Self {
            low_voltage_mv,
            armed: true,
        }
    }

    /// 每拍采样喂一次电压。返回 true = 此刻应发布 battery/low（本次低电段的首拍）。
    pub fn rr3_low_edge_cross(&mut self, voltage_mv: u16) -> bool {
        if self.armed && voltage_mv <= self.low_voltage_mv {
            self.armed = false;
            return true;
        }
        if voltage_mv > self.low_voltage_mv {
            self.armed = true;
        }
        false
    }
}

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

    /// 边沿机主语义：越阈第一拍恰报一次，段内后续拍不再报——schema 的
    /// "同一次低电段不重复发布"钉在这里。
    #[test]
    fn low_edge_fires_once_per_episode() {
        let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
        assert!(!e.rr3_low_edge_cross(3900), "高电平不触发");
        assert!(e.rr3_low_edge_cross(3300), "恰好等于阈值算越阈（≤）");
        assert!(!e.rr3_low_edge_cross(3250), "段内第二拍不再报");
        assert!(!e.rr3_low_edge_cross(3200), "段内继续跌也不再报");
    }

    /// 恢复再武装：电压回到阈值之上才重新武装；恢复后再次越阈再报一次。
    /// "回到 3300 整"仍是低电段内（严格大于才武装），抖动在阈值上不抖报。
    /// 注意武装本身**不是**事件：cross 返回 true 的唯一含义是"发布 battery/low"，
    /// 若在 3301 这种高电压拍返回 true，就会发出一段电压高于阈值的"低电"事件——
    /// 所以重新武装是静默的状态转换，下一次越阈（3200）才再次报。
    #[test]
    fn low_edge_rearms_only_above_threshold() {
        let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
        assert!(e.rr3_low_edge_cross(3290));
        assert!(!e.rr3_low_edge_cross(3300), "等于阈值不恢复");
        assert!(
            !e.rr3_low_edge_cross(3301),
            "严格大于才重新武装——武装本身不触发，否则发出高电压的'低电'事件"
        );
        assert!(!e.rr3_low_edge_cross(3301), "武装后高电平拍照旧不触发");
        assert!(e.rr3_low_edge_cross(3200), "重新武装已生效：再次越阈再报");
        assert!(!e.rr3_low_edge_cross(3200));
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
