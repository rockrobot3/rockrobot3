//! 意图→动作规则面（自 Python rr3_brain/intent.py 逐语义迁入，批N32 迁移件四；
//! 0.0.3 T3，方案 Q1 裁定）：user_text → 结构化 actions。
//!
//! 策略层唯一归属在 brain（architecture §2 原则 3"brain 是唯一编排者"）：brain 说
//! "意图"，执行器广播认领——actions 元素形状 `{topic, args}`，词汇即契约主题
//! （零新造）；执行侧另有归属门（topic 必须 ∈ 执行模块 exposes，rr3-act/src/intent.rs）。
//!
//! 词表来源 legacy/core/relay 语义（语音"开灯/关灯"，盘点见
//! docs/process/legacy-digest.md 的 relay 行）+ 批F2 转向三档（"向左/向右/回中"
//! → servo/command，判据 8）。规则面是**最小确定性注入点**：
//! 精确词表命中 + 前邻否定护栏，不做通用否定语义解析（"语义级理解随云 LLM
//! function-calling 演进"属 0.0.3 方案范围外）。只喂 user_text（ASR 结果），
//! 机器人自己的 reply_text 不参与匹配——防止机器人的话触发机器人的动作。

use serde_json::{json, Value};

/// 继电器写口（执行器认领面）
pub(crate) const RR3_TOPIC_RELAY_SET: &str = "rr3-act/relay/set";
/// 云台转向口
pub(crate) const RR3_TOPIC_SERVO_COMMAND: &str = "rr3-act/servo/command";

/// brain 侧自限词汇：rr3_extract_actions 只产这些 topic（执行侧 exposes 门是第二道）。
/// 声明面常量（Python ACTION_TOPICS 同形状）：词表即契约的自述，消费面在文档与
/// 执行侧归属门对账。
#[allow(dead_code)]
pub(crate) const RR3_ACTION_TOPICS: [&str; 2] = [RR3_TOPIC_RELAY_SET, RR3_TOPIC_SERVO_COMMAND];

/// 吸合词表（legacy relay README："语音开灯/关灯"场景）
const RR3_ON_WORDS: [&str; 5] = ["开灯", "打开灯", "把灯打开", "开一下灯", "点亮"];
/// 断开词表
const RR3_OFF_WORDS: [&str; 5] = ["关灯", "把灯关掉", "关闭灯", "关掉灯", "熄灯"];

/// 转向档位角（批F2，判据 8 的"对应档位"）：中位 90° 对齐 sense 侧 DoA 的 center
/// 档，左右各让 20°——落 pan 轴行程 [70,110]（0.0.5 方案 C 裁定 2026-10-12：
/// 云台机械行程远小于 SG90 全程，越程被 act 的行程裁决拒收点名，故三档必须
/// 落在行程内；不做满行程摆动）。
const RR3_SERVO_ANGLE_LEFT: i64 = 70;
const RR3_SERVO_ANGLE_CENTER: i64 = 90;
const RR3_SERVO_ANGLE_RIGHT: i64 = 110;

/// 转向词表（与 sense 侧 DoA ±90° 三档同一语义坐标系：左/中/右）
const RR3_LEFT_WORDS: [&str; 4] = ["向左", "往左", "左转", "看左边"];
const RR3_RIGHT_WORDS: [&str; 4] = ["向右", "往右", "右转", "看右边"];
const RR3_CENTER_WORDS: [&str; 4] = ["回中", "看前面", "看中间", "回中间"];

/// 最小否定护栏：命中词前两个字符内出现这些字则弃——"别开灯"不吸合、"别向左"不转向。
/// 这是词表匹配的边界护栏，不是否定语义解析（负样本由本文件测试钉死）
const RR3_NEGATIVE_CHARS: [char; 6] = ['别', '不', '莫', '勿', '未', '没'];

/// 词表命中 + 前邻否定护栏：任一词命中且其前两字符内无否定字才算真。
pub(crate) fn rr3_hit_word(text: &str, words: &[&str]) -> bool {
    for word in words {
        let Some(hit) = text.find(word) else {
            continue;
        };
        // 前两字符窗（Python text[max(0, hit-2):hit] 同形状——find 的字节位切出
        // 命中前缀，末两字符即字面「前两字符」，CJK 等宽与否则本表词边界内无害）
        let neg_hit = text[..hit]
            .chars()
            .rev()
            .take(2)
            .any(|c| RR3_NEGATIVE_CHARS.contains(&c));
        if neg_hit {
            continue;
        }
        return true;
    }
    false
}

/// 词表命中 → actions 清单（元素 {topic, args}）。
///
/// 吸合与断开各取第一个命中词表（同一句话先开后关则产两条动作，按序执行——
/// 继电器最终态由最后一条决定，这是词表匹配的既定语义，测试在册）；
/// 转向三档（左/中/右）各取一次命中，产 servo/command 动作（批F2 判据 8）。
pub(crate) fn rr3_extract_actions(user_text: &str) -> Vec<Value> {
    let text = user_text.trim();
    let mut actions = Vec::new();
    for (words, on) in [(&RR3_ON_WORDS[..], true), (&RR3_OFF_WORDS[..], false)] {
        if rr3_hit_word(text, words) {
            actions.push(json!({ "topic": RR3_TOPIC_RELAY_SET, "args": { "on": on } }));
        }
    }
    for (words, angle) in [
        (&RR3_LEFT_WORDS[..], RR3_SERVO_ANGLE_LEFT),
        (&RR3_RIGHT_WORDS[..], RR3_SERVO_ANGLE_RIGHT),
        (&RR3_CENTER_WORDS[..], RR3_SERVO_ANGLE_CENTER),
    ] {
        if rr3_hit_word(text, words) {
            actions.push(json!({
                "topic": RR3_TOPIC_SERVO_COMMAND,
                "args": { "angle_deg": angle },
            }));
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 吸合词表逐词命中 → relay on:true 恰一条
    #[test]
    fn on_word_yields_relay_true() {
        for text in ["开灯", "帮我把灯打开", "请开一下灯", "点亮"] {
            let actions = rr3_extract_actions(text);
            assert_eq!(actions.len(), 1, "{text:?} 应恰产一条动作: {actions:?}");
            assert_eq!(actions[0]["topic"], json!(RR3_TOPIC_RELAY_SET));
            assert_eq!(actions[0]["args"], json!({"on": true}));
        }
    }

    /// 断开词表逐词命中 → relay on:false 恰一条
    #[test]
    fn off_word_yields_relay_false() {
        for text in ["关灯", "把灯关掉", "关闭灯", "熄灯"] {
            let actions = rr3_extract_actions(text);
            assert_eq!(actions.len(), 1, "{text:?} 应恰产一条动作: {actions:?}");
            assert_eq!(actions[0]["topic"], json!(RR3_TOPIC_RELAY_SET));
            assert_eq!(actions[0]["args"], json!({"on": false}));
        }
    }

    /// 否定护栏（吸合族）：命中词前两字符内有否定字 → 弃
    #[test]
    fn negative_guard_rejects() {
        for text in ["别开灯", "不要关灯", "先别开灯，等等"] {
            assert!(rr3_extract_actions(text).is_empty(), "{text:?} 不得触发动作");
        }
    }

    /// 无关文本不产动作（空串同）
    #[test]
    fn unrelated_text_yields_nothing() {
        for text in ["今天天气怎么样", "你好", "讲个故事", ""] {
            assert!(rr3_extract_actions(text).is_empty(), "{text:?} 不得触发动作");
        }
    }

    /// 开与关同句产两条按序动作（relay 最终态由最后一条决定）
    #[test]
    fn on_and_off_in_one_sentence_yields_in_order() {
        let actions = rr3_extract_actions("开灯，等等再关灯");
        assert_eq!(actions.len(), 2, "开与关同句产两条按序动作: {actions:?}");
        assert_eq!(actions[0]["args"], json!({"on": true}));
        assert_eq!(actions[1]["args"], json!({"on": false}));
    }

    /// 载荷 schema（contracts/events/brain-intent-result.schema.json）钉的两键形状
    #[test]
    fn action_shape_matches_contract() {
        let all: Vec<Value> = rr3_extract_actions("开灯")
            .into_iter()
            .chain(rr3_extract_actions("向左"))
            .collect();
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

    /// 批F2 判据 8：向左/向右/回中 → servo/command 三档（70/110/90，落 pan 轴
    /// 行程 [70,110]——方案 C 分轴限幅后左右档必须收进行程内）
    #[test]
    fn turn_words_yield_servo_commands() {
        let cases: [(&[&str], i64); 3] = [
            (&["向左", "往左看", "左转一下", "看左边"], 70),
            (&["向右", "往右看", "右转一下", "看右边"], 110),
            (&["回中", "看前面", "看中间", "回中间"], 90),
        ];
        for (words, angle) in cases {
            for text in words {
                let actions = rr3_extract_actions(text);
                assert_eq!(actions.len(), 1, "{text:?} 应恰产一条动作: {actions:?}");
                assert_eq!(actions[0]["topic"], json!(RR3_TOPIC_SERVO_COMMAND));
                assert_eq!(actions[0]["args"], json!({"angle_deg": angle}));
            }
        }
    }

    /// 否定护栏同样管转向："别向左"不得转头
    #[test]
    fn turn_negative_guard_rejects() {
        for text in ["别向左", "不要向右", "先别回中"] {
            assert!(rr3_extract_actions(text).is_empty(), "{text:?} 不得触发转向");
        }
    }

    /// 两族动作同句共存：relay 在前、servo 在后，按序执行
    #[test]
    fn relay_and_turn_in_one_sentence_yield_in_order() {
        let actions = rr3_extract_actions("开灯，然后向左");
        assert_eq!(actions.len(), 2, "开灯+向左产两条按序动作: {actions:?}");
        assert_eq!(actions[0]["topic"], json!(RR3_TOPIC_RELAY_SET));
        assert_eq!(actions[0]["args"], json!({"on": true}));
        assert_eq!(actions[1]["topic"], json!(RR3_TOPIC_SERVO_COMMAND));
        assert_eq!(actions[1]["args"], json!({"angle_deg": 70}));
    }
}
