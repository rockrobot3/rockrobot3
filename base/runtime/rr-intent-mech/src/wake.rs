// SPDX-License-Identifier: Apache-2.0
//! 唤醒词门控机制核（自 `rr3-brain/src/dispatch/wake.rs` 抽出）：ASR 文本 →
//! 唤醒判决（Ack / Command / FollowUp / Ignore）。Pi 与 MCU 共用同一份归一、
//! 编辑距与短语匹配，板侧不再另写第二份规则。
//!
//! 语义（2026-10-31 用户裁定）：**ASR 文本前缀匹配，不是本地 KWS**；`enable=false`
//! 恒 `FollowUp`（全放行，既有行为逐点等价）。命中短语且无后续文本 → `Ack`；
//! 命中且带后续文本 → `Command`（后续文本进 LLM）；未命中但在跟进窗内 →
//! `FollowUp`；窗外未命中 → `Ignore`（静默，不 LLM、不 TTS）。
//!
//! 云端 ASR 对生僻名"洛克"不稳定（实机识成 骆天/洛天/要是/老师/洛哥/洛师），
//! 故短语表之后保留"你好"兜底：`你好` 后 ≤ 2 字视为名字或误识尾巴 → `Ack`；
//! 更长 → 去掉 `你好` 后进 LLM。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 唤醒词段的缺省应答（`ack` 缺键回落）：用户口径"我在呢 / 有什么可以帮您"
/// 的合并版，一句话即可（TTS 合成一次）。
pub const RR3_WAKE_ACK_DEFAULT: &str = "我在呢，有什么可以帮您？";
/// 唤醒后跟进窗缺省（`followup_ms` 缺键回落）：唤醒应答后 15s 内继续说话
/// 不必重复唤醒词；每接受一轮即续窗（对话式追问）。
pub const RR3_WAKE_FOLLOWUP_DEFAULT_MS: u64 = 15_000;

/// 唤醒词配置（`[rr3-brain.wake]` 同形；S3 单板按同值内建）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3WakeCfg {
    pub enable: bool,
    pub phrases: Vec<String>,
    pub ack: String,
    pub followup_ms: u64,
}

impl Default for Rr3WakeCfg {
    fn default() -> Self {
        Self {
            enable: false,
            phrases: alloc::vec!["你好洛克".to_string()],
            ack: RR3_WAKE_ACK_DEFAULT.to_string(),
            followup_ms: RR3_WAKE_FOLLOWUP_DEFAULT_MS,
        }
    }
}

/// 唤醒判决（纯函数，单测钉域）。`enable=false` 恒 `FollowUp`（全放行，既有
/// 行为逐点等价）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3WakeAction {
    /// 命中唤醒词且无后续文本 → 回固定应答（不进 LLM）
    Ack,
    /// 命中唤醒词且带后续文本 → 后续文本（归一后）进 LLM
    Command(String),
    /// 未命中但在唤醒后的跟进窗内 → 原文本进 LLM（续窗）
    FollowUp,
    /// 未命中且窗外 → 静默丢弃（不 LLM、不 TTS）
    Ignore,
}

pub fn rr3_wake_decide(
    text: &str,
    cfg: &Rr3WakeCfg,
    now_ms: u64,
    last_wake_ms: Option<u64>,
) -> Rr3WakeAction {
    if !cfg.enable {
        return Rr3WakeAction::FollowUp;
    }
    let norm = rr3_wake_norm(text);
    if let Some(action) = rr3_wake_hit(&norm, cfg) {
        return action;
    }
    let in_followup = last_wake_ms
        .is_some_and(|last| cfg.followup_ms > 0 && now_ms.saturating_sub(last) <= cfg.followup_ms);
    if in_followup {
        Rr3WakeAction::FollowUp
    } else {
        Rr3WakeAction::Ignore
    }
}

/// 唤醒词归一判据：空白与中英文标点一律剔除（ASR 输出常带 "你好，洛克。"
/// 一类标点，归一后前缀匹配才稳）；ASCII 标点走通用判，中文标点显式列举。
fn rr3_wake_is_punct(c: char) -> bool {
    c.is_whitespace()
        || c.is_ascii_punctuation()
        || matches!(
            c,
            '，' | '。'
                | '！'
                | '？'
                | '、'
                | '：'
                | '；'
                | '…'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '（'
                | '）'
                | '【'
                | '】'
                | '《'
                | '》'
                | '·'
                | '～'
                | '—'
        )
}

/// 归一文本（去标点/空白 + ASCII 小写），供唤醒短语前缀匹配。
fn rr3_wake_norm(s: &str) -> String {
    s.chars()
        .filter(|c| !rr3_wake_is_punct(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// 编辑距离（字符级 Levenshtein）：唤醒词容错判据用（2026-10-31 实机：
/// ASR 把 "你好洛克" 识别成 "你好洛师"，一字之差把精确前缀匹配挡在门外）。
fn rr3_wake_edit_dist(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = alloc::vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// 唤醒短语的前缀容错阈值（2026-10-31）：短语 ≤4 字容 1 字（ASR 同音/近音误识），
/// 更长短语容 2 字。过松会把 "你好老师"（距离 2）之类日常短语误唤醒，故不全局放宽。
fn rr3_wake_max_edits(phrase_chars: usize) -> usize {
    if phrase_chars >= 6 {
        2
    } else {
        1
    }
}

/// 短语表逐条匹配，未中再走"你好"兜底（2026-10-31 用户裁定：云端 ASR 对"洛克"
/// 不稳定，同一句每次识成骆天/洛天/要是/老师/洛哥/洛师）。兜底放在短语表之后：
/// 已知短语能精确剥掉前缀（"你好洛师，今天天气"→命令只留"今天天气"），兜底只
/// 处理短语表没覆盖的变体——"你好"后 ≤2 字视为名字或误识尾巴 → Ack；更长 →
/// 当命令（去前缀）进 LLM。
fn rr3_wake_hit(norm: &str, cfg: &Rr3WakeCfg) -> Option<Rr3WakeAction> {
    let norm_chars: Vec<char> = norm.chars().collect();
    if let Some(action) = rr3_phrase_scan(&norm_chars, &cfg.phrases) {
        return Some(action);
    }
    rr3_wake_fallback(norm)
}

/// 短语表未中时的"你好"兜底（≤2 字 = Ack，更长 = 命令）。
fn rr3_wake_fallback(norm: &str) -> Option<Rr3WakeAction> {
    let rest = norm.strip_prefix("你好")?.trim();
    Some(if rest.chars().count() <= 2 {
        Rr3WakeAction::Ack
    } else {
        Rr3WakeAction::Command(rest.to_string())
    })
}

/// 短语表逐条匹配（首中即返）。
fn rr3_phrase_scan(norm_chars: &[char], phrases: &[String]) -> Option<Rr3WakeAction> {
    for phrase in phrases {
        if let Some(action) = rr3_wake_phrase(norm_chars, phrase) {
            return Some(action);
        }
    }
    None
}

/// 单条短语：归一后空串跳过；ASR 吞词尾（实机"你好洛克"→"你好，洛。"）时文本
/// 比短语短 1 字仍按前缀容错认（短 2 字以上不认——"你好"这类日常问候不误唤醒）。
fn rr3_wake_phrase(norm_chars: &[char], phrase: &str) -> Option<Rr3WakeAction> {
    let pn = rr3_wake_norm(phrase);
    let pn_chars: Vec<char> = pn.chars().collect();
    if pn_chars.is_empty() {
        return None;
    }
    if norm_chars.len() < pn_chars.len() {
        return rr3_wake_short(norm_chars, &pn_chars);
    }
    let prefix: String = norm_chars[..pn_chars.len()].iter().collect();
    if rr3_wake_edit_dist(&prefix, &pn) > rr3_wake_max_edits(pn_chars.len()) {
        return None;
    }
    let rest: String = norm_chars[pn_chars.len()..].iter().collect();
    let rest = rest.trim();
    Some(rr3_wake_tail(rest))
}

/// 短语命中后：无后续文本 = 固定应答，否则命令进 LLM。
fn rr3_wake_tail(rest: &str) -> Rr3WakeAction {
    if rest.is_empty() {
        Rr3WakeAction::Ack
    } else {
        Rr3WakeAction::Command(rest.to_string())
    }
}

/// 短一字的容错前缀（ASR 吞词尾）：短 2 字以上不认，编辑距内回 Ack。
fn rr3_wake_short(norm_chars: &[char], pn_chars: &[char]) -> Option<Rr3WakeAction> {
    if norm_chars.len() + 1 < pn_chars.len() {
        return None;
    }
    let prefix: String = norm_chars.iter().collect();
    let phrase_prefix: String = pn_chars[..norm_chars.len()].iter().collect();
    (rr3_wake_edit_dist(&prefix, &phrase_prefix) <= rr3_wake_max_edits(pn_chars.len()))
        .then_some(Rr3WakeAction::Ack)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Rr3WakeCfg {
        Rr3WakeCfg {
            enable: true,
            ..Default::default()
        }
    }

    #[test]
    fn wake_states_window() {
        let c = cfg();
        assert_eq!(rr3_wake_decide("你好洛克", &c, 0, None), Rr3WakeAction::Ack);
        assert_eq!(
            rr3_wake_decide("你好，洛克！", &c, 0, None),
            Rr3WakeAction::Ack,
            "ASR 标点归一后仍命中"
        );
        assert_eq!(
            rr3_wake_decide("你好洛师。", &c, 0, None),
            Rr3WakeAction::Ack,
            "一字误识容错命中"
        );
        assert_eq!(
            rr3_wake_decide("你好，洛。", &c, 0, None),
            Rr3WakeAction::Ack,
            "短一字（吞词尾）仍命中"
        );
        for t in [
            "你好骆天。",
            "你好洛天。",
            "你好要是。",
            "你好老师。",
            "你好洛哥。",
            "你好",
        ] {
            assert_eq!(
                rr3_wake_decide(t, &c, 0, None),
                Rr3WakeAction::Ack,
                "你好开头应唤醒: {t}"
            );
        }
        assert_eq!(
            rr3_wake_decide("你好，今天天气怎么样？", &c, 0, None),
            Rr3WakeAction::Command("今天天气怎么样".to_string()),
            "你好 + 长文本 → 命令"
        );
        assert_eq!(
            rr3_wake_decide("你好洛师，今天天气怎么样？", &c, 0, None),
            Rr3WakeAction::Command("今天天气怎么样".to_string()),
            "误识 + 命令仍能区分出命令"
        );
        assert_eq!(
            rr3_wake_decide("你好洛克，今天天气怎么样？", &c, 0, None),
            Rr3WakeAction::Command("今天天气怎么样".to_string())
        );
        assert_eq!(
            rr3_wake_decide("今天天气怎么样", &c, 0, None),
            Rr3WakeAction::Ignore,
            "从未唤醒的非唤醒文本静默"
        );
        assert_eq!(
            rr3_wake_decide("今天天气怎么样", &c, 10_000, Some(0)),
            Rr3WakeAction::FollowUp,
            "跟进窗内追问放行"
        );
        assert_eq!(
            rr3_wake_decide("今天天气怎么样", &c, c.followup_ms, Some(0)),
            Rr3WakeAction::FollowUp,
            "窗边界（<=）收"
        );
        assert_eq!(
            rr3_wake_decide("今天天气怎么样", &c, c.followup_ms + 1, Some(0)),
            Rr3WakeAction::Ignore,
            "窗外不收"
        );
        let off = Rr3WakeCfg {
            enable: false,
            ..Default::default()
        };
        assert_eq!(
            rr3_wake_decide("今天天气怎么样", &off, 0, None),
            Rr3WakeAction::FollowUp,
            "关闭门控 = 全放行（旧版行为）"
        );
    }
}
