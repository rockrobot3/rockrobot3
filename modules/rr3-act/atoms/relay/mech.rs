//! relay 原子件机制主体（五件套③，自原 src/relay.rs 逐字迁入，逻辑零改动）。
//!
//! 参数裁决（只认真布尔）+ 通断应用 + 摘要体 + 运维计数键。

use rr_hal::{rr3_block_on, Rr3GpioOut};
use serde_json::{json, Value};

/// relay/set 参数裁决：只认真布尔——`{"on": 1}`、`{"on": "true"}` 都拒，
/// 通断没有"近似真"。
pub fn rr3_relay_parse_args(args: &Value) -> Result<bool, String> {
    let Some(v) = args.get("on") else {
        return Err("缺 on（bool，true=吸合 false=断开）".into());
    };
    v.as_bool()
        .ok_or_else(|| format!("on 必须是布尔，收到 {v}"))
}

/// relay/set 应答体：回显生效后的通断
pub fn rr3_relay_result(on: bool) -> Value {
    json!({ "on": on })
}

/// 运维计数键（0.0.4 批B，方案 §4 归属表）：`rr3_relay_apply` 成功后按目标电平
/// 经 bus client 的 counters 通道自报，字面量只在这一个函数
pub fn rr3_relay_counter_key(on: bool) -> &'static str {
    if on {
        "relay_on"
    } else {
        "relay_off"
    }
}

/// relay/state 摘要体：变化即发，seq 是本状态流自己的计数
/// （contracts/events/act-relay_state.schema.json 的形状事实源）
pub fn rr3_relay_state_body(seq: u64, on: bool) -> Value {
    json!({ "seq": seq, "on": on })
}

/// 应用通断并落电平。HAL 写失败按业务性失败折 error（同 interact 的后端写失败口径）
pub fn rr3_relay_apply(gpio: &dyn Rr3GpioOut, on: bool) -> Result<bool, String> {
    rr3_block_on(gpio.set(on)).map_err(|e| format!("继电器写入失败: {e}"))?;
    Ok(on)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::sim::Rr3SimGpioOut;

    #[test]
    fn parse_accepts_only_real_booleans() {
        assert!(rr3_relay_parse_args(&json!({"on": true})).unwrap());
        assert!(!rr3_relay_parse_args(&json!({"on": false})).unwrap());
        for bad in [
            json!({}),
            json!({"on": 1}),
            json!({"on": "true"}),
            json!({"on": null}),
        ] {
            assert!(rr3_relay_parse_args(&bad).is_err(), "非真布尔必须拒: {bad}");
        }
        let e = rr3_relay_parse_args(&json!({"on": 1})).unwrap_err();
        assert!(e.contains("布尔"), "类型不符要点名: {e}");
    }

    /// apply 落电平且应答回显同一真值——通断语义没有第三态
    #[test]
    fn apply_sets_level_and_echoes_state() {
        let gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        assert!(rr3_relay_apply(&gpio, true).unwrap());
        assert!(gpio.rr3_sim_gpio_level(), "吸合落电平");
        assert_eq!(rr3_relay_result(true)["on"], true);
        assert!(!rr3_relay_apply(&gpio, false).unwrap());
        assert!(!gpio.rr3_sim_gpio_level(), "断开落电平");
    }

    /// 计数键两态钉死（0.0.4 批B）：键是模块内部运维词汇，字面量唯一落点
    #[test]
    fn counter_key_distinguishes_on_off() {
        assert_eq!(rr3_relay_counter_key(true), "relay_on");
        assert_eq!(rr3_relay_counter_key(false), "relay_off");
    }

    /// state 摘要体逐字对齐契约 schema（seq / on 两键，additionalProperties false）
    #[test]
    fn state_body_matches_contract_shape() {
        let v = rr3_relay_state_body(3, true);
        assert_eq!(v["seq"], 3);
        assert_eq!(v["on"], true);
        assert_eq!(
            v.as_object().map(|m| m.len()),
            Some(2),
            "schema 钉死两键，多键即漂移: {v}"
        );
    }
}
