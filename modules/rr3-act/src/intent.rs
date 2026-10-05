//! intent/result 订阅执行面（0.0.3 T3，方案 Q1 裁定）：brain 产出结构化
//! `actions`（元素 `{topic, args}`，contracts/events/brain-intent-result.schema.json），
//! 本模块广播认领——**两道归属门**后才执行：
//!
//! ① 归属门：topic ∈ 本模块 exposes（不越权——发布侧 `rr3_reg_publish_check`
//!    四道门的精神在订阅执行面的对偶：别人的主题一个不碰）；
//! ② 开放门：topic ∈ 意图执行面已开放集（M5 最小面只认 `relay/set`；servo/led
//!    虽在 exposes 但不经意图驱动，扩面随真实需要 append，不预先占位）。
//!
//! 执行失败（参数裁决拒 / HAL 写失败）是**业务性失败**，逐条折日志继续收下一帧
//! （事件语义无应答回路）；传输性失败（发布 relay/state 写不出去）由调用方按
//! `exit(1)` 口径收口（main.rs 的发布 `?`）。

use serde_json::Value;

use crate::relay::{self, RR3_RELAY_SET_TOPIC};

/// 本模块 exposes 的全集（module.toml [rpc].exposes 同源——归属门的真值）
const EXPOSED: [&str; 3] = [
    "rr3-act/servo/command",
    "rr3-act/led/effect",
    RR3_RELAY_SET_TOPIC,
];

/// 处理 intent 载荷的 `actions` 字段：逐条两道门 + relay 执行。
/// 返回每条的处置结果（Ok = 生效后的通断；Err = 拒因），由调用方逐条落日志——
/// 纯函数不碰总线，sim GPIO 可直接断言落电平。
pub fn rr3_intent_actions_handle(actions: &Value, gpio: &dyn rr_hal::Rr3GpioOut) -> Vec<Result<bool, String>> {
    let Some(list) = actions.as_array() else {
        return vec![Err(format!(
            "actions 必须是数组，收到 {actions}"
        ))];
    };
    list.iter()
        .map(|item| rr3_intent_action_handle(item, gpio))
        .collect()
}

/// 单条动作：形状裁决 → 两道门 → relay 执行
fn rr3_intent_action_handle(item: &Value, gpio: &dyn rr_hal::Rr3GpioOut) -> Result<bool, String> {
    let topic = item
        .get("topic")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("动作缺 topic（字符串），收到 {item}"))?;
    let args = item
        .get("args")
        .ok_or_else(|| format!("动作缺 args（对象），收到 {item}"))?;
    if !EXPOSED.contains(&topic) {
        return Err(format!("越权拒执行 {topic}：不在本模块 exposes"));
    }
    if topic != RR3_RELAY_SET_TOPIC {
        return Err(format!(
            "已归属但意图执行面未开放 {topic}（当前只认 {RR3_RELAY_SET_TOPIC}）"
        ));
    }
    let on = relay::rr3_relay_parse_args(args)?;
    relay::rr3_relay_apply(gpio, on)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::sim::Rr3SimGpioOut;
    use serde_json::json;

    #[test]
    fn valid_action_drives_relay_gpio() {
        let gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/relay/set", "args": {"on": true}}]),
            &gpio,
        );
        assert_eq!(out, vec![Ok(true)], "合法动作应执行成功: {out:?}");
        assert!(gpio.rr3_sim_gpio_level(), "吸合必须落到 GPIO 电平");

        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/relay/set", "args": {"on": false}}]),
            &gpio,
        );
        assert_eq!(out, vec![Ok(false)]);
        assert!(!gpio.rr3_sim_gpio_level(), "断开必须落到 GPIO 电平");
    }

    #[test]
    fn foreign_topic_is_rejected_by_exposes_gate() {
        let gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-power/battery/state", "args": {}}]),
            &gpio,
        );
        let err = out[0].as_ref().unwrap_err();
        assert!(err.contains("越权"), "归属门拒因要点名越权: {err}");
        assert!(err.contains("exposes"), "拒因要点名归属依据: {err}");
        assert!(!gpio.rr3_sim_gpio_level(), "被拒动作不得碰 GPIO");
    }

    #[test]
    fn exposed_but_closed_topic_is_rejected_by_open_gate() {
        let gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        for topic in ["rr3-act/servo/command", "rr3-act/led/effect"] {
            let out = rr3_intent_actions_handle(&json!([{ "topic": topic, "args": {} }]), &gpio);
            let err = out[0].as_ref().unwrap_err();
            assert!(
                err.contains("未开放"),
                "归属但在册未开放的动作要由开放门拒: {err}"
            );
        }
    }

    #[test]
    fn malformed_actions_are_named_not_fatal() {
        let gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        // actions 不是数组
        let out = rr3_intent_actions_handle(&json!("开灯"), &gpio);
        assert!(out[0].as_ref().unwrap_err().contains("数组"));
        // 元素缺 topic / 缺 args
        let out = rr3_intent_actions_handle(&json!([{"args": {}}]), &gpio);
        assert!(out[0].as_ref().unwrap_err().contains("topic"));
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/relay/set"}]),
            &gpio,
        );
        assert!(out[0].as_ref().unwrap_err().contains("args"));
        // 参数裁决照常把门（非真布尔）
        let out = rr3_intent_actions_handle(
            &json!([{"topic": "rr3-act/relay/set", "args": {"on": 1}}]),
            &gpio,
        );
        assert!(out[0].as_ref().unwrap_err().contains("布尔"));
        assert!(!gpio.rr3_sim_gpio_level(), "全部被拒，GPIO 不得变化");
    }

    #[test]
    fn multiple_actions_execute_in_order() {
        let gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let out = rr3_intent_actions_handle(
            &json!([
                {"topic": "rr3-act/relay/set", "args": {"on": true}},
                {"topic": "rr3-act/relay/set", "args": {"on": false}},
            ]),
            &gpio,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], Ok(true));
        assert_eq!(out[1], Ok(false));
        assert!(!gpio.rr3_sim_gpio_level(), "按序执行后最终态=最后一条");
    }
}
