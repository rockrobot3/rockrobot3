// SPDX-License-Identifier: Apache-2.0
//! 意图→动作规则面（自 Python rr3_brain/intent.py 逐语义迁入，批N32 迁移件四；
//! 0.0.3 T3，方案 Q1 裁定）：user_text → 结构化 actions。
//!
//! **W6-1 起规则本体已抽到 [`rr_intent_mech`]**（Pi 与 S3 单板全链共用同一份
//! 词表与否定护栏，板侧不再写第二份）。本文件只保留 brain 面适配：把机制核的
//! 动作枚举落成 `brain-intent-result` 契约的 `{topic, args}` JSON 形状。
//!
//! 策略层唯一归属在 brain（architecture §2 原则 3"brain 是唯一编排者"）：brain 说
//! "意图"，执行器广播认领——actions 元素形状 `{topic, args}`，词汇即契约主题
//! （零新造）；执行侧另有归属门（topic 必须 ∈ 执行模块 exposes，rr3-act/src/intent.rs）。
//!
//! 只喂 user_text（ASR 结果），机器人自己的 reply_text 不参与匹配——防止机器人的
//! 话触发机器人的动作。

use rr_intent_mech::Rr3IntentAction;
use serde_json::{json, Value};

/// 继电器写口（执行器认领面）
pub(crate) const RR3_TOPIC_RELAY_SET: &str = rr_intent_mech::RR3_TOPIC_RELAY_SET;
/// 云台转向口
pub(crate) const RR3_TOPIC_SERVO_COMMAND: &str = rr_intent_mech::RR3_TOPIC_SERVO_COMMAND;

/// brain 侧自限词汇：`rr3_extract_actions` 只产这些 topic（执行侧 exposes 门是第二道）。
/// 声明面常量（Python ACTION_TOPICS 同形状）：词表即契约的自述，消费面在文档与
/// 执行侧归属门对账。
#[allow(dead_code)]
pub(crate) const RR3_ACTION_TOPICS: [&str; 2] = [RR3_TOPIC_RELAY_SET, RR3_TOPIC_SERVO_COMMAND];

/// 词表命中 → actions 清单（元素 `{topic, args}`；顺序与机制核一致）。
pub(crate) fn rr3_extract_actions(user_text: &str) -> Vec<Value> {
    rr_intent_mech::rr3_extract_actions(user_text)
        .into_iter()
        .map(rr3_action_json)
        .collect()
}

/// 动作枚举 → 契约 JSON 形状（只此一处转换，键名与 schema 钉死）。
fn rr3_action_json(action: Rr3IntentAction) -> Value {
    match action {
        Rr3IntentAction::Relay { on } => {
            json!({ "topic": RR3_TOPIC_RELAY_SET, "args": { "on": on } })
        }
        Rr3IntentAction::Servo { angle_deg } => {
            json!({ "topic": RR3_TOPIC_SERVO_COMMAND, "args": { "angle_deg": angle_deg } })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 载荷 schema（contracts/events/brain-intent-result.schema.json）钉的两键形状
    #[test]
    fn action_shape_matches_contract() {
        let all: Vec<Value> = rr3_extract_actions("开灯，然后向左");
        assert_eq!(all.len(), 2, "开灯+向左产两条按序动作: {all:?}");
        for action in all {
            let mut keys: Vec<&str> = action
                .as_object()
                .expect("动作须对象")
                .keys()
                .map(|s| s.as_str())
                .collect();
            keys.sort();
            assert_eq!(keys, vec!["args", "topic"], "恰 topic/args 两键");
            assert!(action["args"].is_object(), "args 须对象");
        }
    }

    /// 适配面顺序：relay 在前、servo 在后，args 值逐字落契约
    #[test]
    fn mixed_sentence_keeps_order_and_args() {
        let actions = rr3_extract_actions("开灯，等等再关灯，然后向右");
        assert_eq!(actions.len(), 3, "{actions:?}");
        assert_eq!(actions[0]["args"], json!({"on": true}));
        assert_eq!(actions[1]["args"], json!({"on": false}));
        assert_eq!(actions[2]["topic"], json!(RR3_TOPIC_SERVO_COMMAND));
        assert_eq!(actions[2]["args"], json!({"angle_deg": 110}));
    }

    /// 否定护栏经适配面仍生效（"别向左"不得产动作）
    #[test]
    fn negative_guard_survives_adapter() {
        assert!(rr3_extract_actions("别向左").is_empty());
        assert!(rr3_extract_actions("今天天气怎么样").is_empty());
    }
}
