// SPDX-License-Identifier: Apache-2.0
//! 文本规则机制核（自 `rr3-brain` 抽出，W6-1）：
//! - 意图：user_text → 结构化动作清单；
//! - 唤醒：ASR 文本 → 唤醒判决（Ack / Command / FollowUp / Ignore）。
//!
//! Pi 与 MCU 共用同一份词表、否定护栏与唤醒归一/编辑距，板侧不再另写第二份规则。
//!
//! 策略层唯一归属仍在 brain（architecture §2 原则 3"brain 是唯一编排者"）：本
//! crate 只做**确定性词表匹配**，产出与契约主题一一对应的动作枚举；"动作怎么
//! 执行"归调用方（Pi 广播 intent/result，S3 直接推舵机/表情作业）。
//!
//! 词表 = relay 开/关两族 + 转向左/中/右三档（批F2 判据 8）。精确词表命中 +
//! 前两字符否定护栏，不做通用否定语义解析；只喂 user_text（ASR 结果），机器人
//! 自己的 reply_text 不参与匹配——防止机器人的话触发机器人的动作。

#![no_std]

extern crate alloc;

pub mod wake;

use alloc::vec::Vec;

/// 继电器写口（执行器认领面）
pub const RR3_TOPIC_RELAY_SET: &str = "rr3-act/relay/set";
/// 云台转向口
pub const RR3_TOPIC_SERVO_COMMAND: &str = "rr3-act/servo/command";

/// 吸合词表（legacy relay README："语音开灯/关灯"场景）
const RR3_ON_WORDS: [&str; 5] = ["开灯", "打开灯", "把灯打开", "开一下灯", "点亮"];
/// 断开词表
const RR3_OFF_WORDS: [&str; 5] = ["关灯", "把灯关掉", "关闭灯", "关掉灯", "熄灯"];

/// 转向档位角（批F2，判据 8 的"对应档位"）：中位 90° 对齐 sense 侧 DoA 的 center
/// 档，左右各让 20°——落 pan 轴行程 [70,110]（0.0.5 方案 C 裁定：云台机械行程
/// 远小于 SG90 全程，越程被 act 的行程裁决拒收点名，故三档必须落在行程内）。
pub const RR3_SERVO_ANGLE_LEFT: i64 = 70;
pub const RR3_SERVO_ANGLE_CENTER: i64 = 90;
pub const RR3_SERVO_ANGLE_RIGHT: i64 = 110;

/// 转向词表（与 sense 侧 DoA ±90° 三档同一语义坐标系：左/中/右）
const RR3_LEFT_WORDS: [&str; 4] = ["向左", "往左", "左转", "看左边"];
const RR3_RIGHT_WORDS: [&str; 4] = ["向右", "往右", "右转", "看右边"];
const RR3_CENTER_WORDS: [&str; 4] = ["回中", "看前面", "看中间", "回中间"];

/// 最小否定护栏：命中词前两个字符内出现这些字则弃——"别开灯"不吸合、"别向左"不转向。
/// 这是词表匹配的边界护栏，不是否定语义解析（负样本由本文件测试钉死）
const RR3_NEGATIVE_CHARS: [char; 6] = ['别', '不', '莫', '勿', '未', '没'];

/// 结构化动作（形状与 `brain-intent-result` 契约的 `{topic,args}` 一一对应）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3IntentAction {
    /// 继电器吸合/断开
    Relay { on: bool },
    /// 云台 pan 落角
    Servo { angle_deg: i64 },
}

impl Rr3IntentAction {
    /// 动作对应的契约主题（执行侧认领键）。
    pub fn rr3_topic(&self) -> &'static str {
        match self {
            Self::Relay { .. } => RR3_TOPIC_RELAY_SET,
            Self::Servo { .. } => RR3_TOPIC_SERVO_COMMAND,
        }
    }

    /// 继电器动作的 `on` 值（非 relay 动作回 `None`）。
    pub fn rr3_relay_on(&self) -> Option<bool> {
        match self {
            Self::Relay { on } => Some(*on),
            Self::Servo { .. } => None,
        }
    }

    /// 转向动作的目标角（非 servo 动作回 `None`）。
    pub fn rr3_angle_deg(&self) -> Option<i64> {
        match self {
            Self::Relay { .. } => None,
            Self::Servo { angle_deg } => Some(*angle_deg),
        }
    }
}

/// 词表命中 + 前邻否定护栏：任一词命中且其前两字符内无否定字才算真。
pub fn rr3_hit_word(text: &str, words: &[&str]) -> bool {
    for word in words {
        if rr3_word_hits(text, word) {
            return true;
        }
    }
    false
}

/// 单词命中且未被否定（命中位前两字符内无否定字）。
fn rr3_word_hits(text: &str, word: &str) -> bool {
    text.find(word).is_some_and(|hit| !rr3_negated(text, hit))
}

/// 命中前缀是否被否定词覆盖（前两字符窗，Python `text[max(0, hit-2):hit]`
/// 同形状——find 的字节位切出命中前缀，末两字符即字面「前两字符」）。
fn rr3_negated(text: &str, hit: usize) -> bool {
    text[..hit]
        .chars()
        .rev()
        .take(2)
        .any(|c| RR3_NEGATIVE_CHARS.contains(&c))
}

/// 词表命中 → 动作清单。
///
/// 吸合与断开各取第一个命中词表（同一句话先开后关则产两条动作，按序执行——
/// 继电器最终态由最后一条决定，这是词表匹配的既定语义，测试在册）；
/// 转向三档（左/中/右）各取一次命中，产 servo 动作（批F2 判据 8）。
pub fn rr3_extract_actions(user_text: &str) -> Vec<Rr3IntentAction> {
    let text = user_text.trim();
    let mut actions = rr3_relay_actions(text);
    actions.extend(rr3_turn_actions(text));
    actions
}

/// 吸合/断开词表 → relay 动作（各取一次命中，按序）。
fn rr3_relay_actions(text: &str) -> Vec<Rr3IntentAction> {
    let mut actions = Vec::new();
    for (words, on) in [(&RR3_ON_WORDS[..], true), (&RR3_OFF_WORDS[..], false)] {
        if rr3_hit_word(text, words) {
            actions.push(Rr3IntentAction::Relay { on });
        }
    }
    actions
}

/// 转向三档词表（左/中/右）→ servo 动作。
fn rr3_turn_actions(text: &str) -> Vec<Rr3IntentAction> {
    let mut actions = Vec::new();
    for (words, angle) in [
        (&RR3_LEFT_WORDS[..], RR3_SERVO_ANGLE_LEFT),
        (&RR3_RIGHT_WORDS[..], RR3_SERVO_ANGLE_RIGHT),
        (&RR3_CENTER_WORDS[..], RR3_SERVO_ANGLE_CENTER),
    ] {
        if rr3_hit_word(text, words) {
            actions.push(Rr3IntentAction::Servo { angle_deg: angle });
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 吸合词表逐词命中 → Relay{on:true} 恰一条
    #[test]
    fn word_yields_relay() {
        for text in ["开灯", "帮我把灯打开", "请开一下灯", "点亮"] {
            assert_eq!(
                rr3_extract_actions(text),
                [Rr3IntentAction::Relay { on: true }],
                "{text:?}"
            );
        }
    }

    /// 断开词表逐词命中 → Relay{on:false} 恰一条
    #[test]
    fn off_word_relay() {
        for text in ["关灯", "把灯关掉", "关闭灯", "熄灯"] {
            assert_eq!(
                rr3_extract_actions(text),
                [Rr3IntentAction::Relay { on: false }],
                "{text:?}"
            );
        }
    }

    /// 否定护栏（吸合族）：命中词前两字符内有否定字 → 弃
    #[test]
    fn negative_guard_rej() {
        for text in ["别开灯", "不要关灯", "先别开灯，等等"] {
            assert!(
                rr3_extract_actions(text).is_empty(),
                "{text:?} 不得触发动作"
            );
        }
    }

    /// 无关文本不产动作（空串同）
    #[test]
    fn unrelated_nothing() {
        for text in ["今天天气怎么样", "你好", "讲个故事", ""] {
            assert!(
                rr3_extract_actions(text).is_empty(),
                "{text:?} 不得触发动作"
            );
        }
    }

    /// 开与关同句产两条按序动作（relay 最终态由最后一条决定）
    #[test]
    fn off_yields_order() {
        assert_eq!(
            rr3_extract_actions("开灯，等等再关灯"),
            [
                Rr3IntentAction::Relay { on: true },
                Rr3IntentAction::Relay { on: false },
            ]
        );
    }

    /// 批F2 判据 8：向左/向右/回中 → 70/110/90 三档（落 pan 行程 [70,110]）
    #[test]
    fn turn_servo_commands() {
        let cases: [(&[&str], i64); 3] = [
            (&["向左", "往左看", "左转一下", "看左边"], 70),
            (&["向右", "往右看", "右转一下", "看右边"], 110),
            (&["回中", "看前面", "看中间", "回中间"], 90),
        ];
        for (words, angle) in cases {
            for text in words {
                assert_eq!(
                    rr3_extract_actions(text),
                    [Rr3IntentAction::Servo { angle_deg: angle }],
                    "{text:?}"
                );
            }
        }
    }

    /// 否定护栏同样管转向："别向左"不得转头
    #[test]
    fn turn_guard_rej() {
        for text in ["别向左", "不要向右", "先别回中"] {
            assert!(
                rr3_extract_actions(text).is_empty(),
                "{text:?} 不得触发转向"
            );
        }
    }

    /// 两族动作同句共存：relay 在前、servo 在后，按序执行
    #[test]
    fn relay_turn_order() {
        assert_eq!(
            rr3_extract_actions("开灯，然后向左"),
            [
                Rr3IntentAction::Relay { on: true },
                Rr3IntentAction::Servo { angle_deg: 70 },
            ]
        );
    }

    /// 动作自述面（主题/取值）与执行侧认领键同源
    #[test]
    fn action_topic_args() {
        let relay = Rr3IntentAction::Relay { on: false };
        assert_eq!(relay.rr3_topic(), RR3_TOPIC_RELAY_SET);
        assert_eq!(relay.rr3_relay_on(), Some(false));
        assert_eq!(relay.rr3_angle_deg(), None);
        let servo = Rr3IntentAction::Servo { angle_deg: 110 };
        assert_eq!(servo.rr3_topic(), RR3_TOPIC_SERVO_COMMAND);
        assert_eq!(servo.rr3_relay_on(), None);
        assert_eq!(servo.rr3_angle_deg(), Some(110));
    }
}
