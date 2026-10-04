//! 继电器通断（`rr3-act/relay/set`，0.0.2 方案 Q1 裁定立格 `act.relay`）。
//!
//! 最小面：`{"on": bool}` 写一路 `Rr3GpioOut`。`relay/state` 摘要主题未登记
//! （守"不预先占位"），使用前须先按"契约只增不改" APPEND 再引用。

use rr_hal::{rr3_block_on, Rr3GpioOut};
use serde_json::{json, Value};

pub const RR3_RELAY_SET_TOPIC: &str = "rr3-act/relay/set";

/// 启动自述：本子模块的总线面一行说明
pub const SURFACE: &[&str] = &["relay: rpc rr3-act/relay/set (request)"];

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
}
