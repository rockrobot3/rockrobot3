//! 充电状态（`rr3-power/charger/state`）。
//! 原子件机制主体（五件套③，自原 src/charger.rs 逐字迁入，逻辑零改动）。
//!
//! 本流由最近一次 battery 读数的 `charging` 位**派生**（HAL 的 Rr3Power 只给
//! 一个三元组读数，不设独立 charger trait——充电器状态是电池读数的一部分）；
//! 拍点独立于 battery/state（config 的 state_period_ms），seq 也是本流自己的。

use serde_json::{json, Value};

/// charger/state 流体：charging 是最近一次 battery 读数的派生值
pub fn rr3_charger_state_body(seq: u64, charging: bool) -> Value {
    json!({ "seq": seq, "charging": charging })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charger_body_carries_own_seq_and_charging() {
        let v = rr3_charger_state_body(3, true);
        assert_eq!(v["seq"], 3);
        assert_eq!(v["charging"], true);
        assert_eq!(v.as_object().unwrap().len(), 2, "两字段，不加戏: {v}");
    }
}
