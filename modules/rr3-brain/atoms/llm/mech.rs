//! llm 原子件机制主体（五件套③，批N32 件四 Rust 拆分定形）：对话历史窗口
//! **纯机制**——system 恒守首位 + 最近 [`RR3_LLM_HISTORY_ROUNDS`] 轮，裁剪只动
//! 最旧的成对话轮，失败轮回滚（无 IO 无环境读取无 JSON，⑧-b 依赖闭包纪律；
//! String/Vec 走 alloc 面）。历史策略移植 legacy/core/llm/src/llm.c；messages
//! 的 JSON 装配与 HTTP 传输在组合件级 [`crate::cloud`]（策略与 IO 不进机制核）。

/// 历史窗口轮数（一轮 = user + assistant 两条，裁剪只动最旧的成对话轮）
pub const RR3_LLM_HISTORY_ROUNDS: usize = 4;

/// 对话历史窗口（system + 最近 N 轮；单线程喂，非线程安全）。
///
/// 纪律：user 先入列、失败回滚（[`Self::rr3_rollback_last`]）——只有成功才追加
/// assistant（legacy 同形状）；system 恒守首位不被裁剪。
pub struct Rr3LlmHistory {
    history: Vec<(String, String)>,
}

impl Rr3LlmHistory {
    /// 装配构造：首条固定 system 人设（人设文案是策略值，由组合件注入）。
    pub fn rr3_new(system_prompt: &str) -> Self {
        Self {
            history: vec![("system".to_string(), system_prompt.to_string())],
        }
    }

    /// 一轮 user 入列并裁窗（先追加后裁剪——裁剪只动最旧的成对话轮，
    /// history[1..3]，system 恒守首位）。
    pub fn rr3_user_turn(&mut self, prompt: &str) {
        self.history.push(("user".to_string(), prompt.to_string()));
        while self.history.len() > 1 + RR3_LLM_HISTORY_ROUNDS * 2 {
            self.history.drain(1..3);
        }
    }

    /// 成功应答入列（调用方只在 HTTP 成功后调——失败轮走回滚）。
    pub fn rr3_assistant_turn(&mut self, reply: &str) {
        self.history
            .push(("assistant".to_string(), reply.to_string()));
    }

    /// 失败轮回滚：本轮 user 出列（对位 legacy：只有成功才追加 assistant）。
    pub fn rr3_rollback_last(&mut self) {
        self.history.pop();
    }

    /// 对话历史在册（组合件 messages 装配读口与测试面只读）。
    pub fn rr3_messages(&self) -> &[(String, String)] {
        &self.history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- 件级两腿对拍（⑧-a，§4.0.3 判据②） ----------

    /// ⑧-a 腿一：窗口裁剪确定性（system + 最近 4 轮，逐条留痕可复算——
    /// 问0/问1 出列、问2 起在册）
    #[test]
    fn parity_leg1_llm_history_window_trim_is_deterministic() {
        let mut h = Rr3LlmHistory::rr3_new("sys");
        for i in 0..RR3_LLM_HISTORY_ROUNDS + 2 {
            h.rr3_user_turn(&format!("问{i}"));
            h.rr3_assistant_turn(&format!("答{i}"));
        }
        let m = h.rr3_messages();
        assert_eq!(m.len(), 1 + RR3_LLM_HISTORY_ROUNDS * 2);
        assert_eq!(m[0].0, "system");
        assert_eq!(m[1].1, "问2", "最旧两条已裁");
        assert_eq!(m.last().unwrap().1, "答5");
    }

    /// ⑧-a 腿二：失败轮不入账（legacy llm.c——只有成功才追加 assistant，
    /// user 入列后回滚还原）
    #[test]
    fn parity_leg2_llm_failed_turn_rolls_back() {
        let mut h = Rr3LlmHistory::rr3_new("sys");
        h.rr3_user_turn("会失败");
        h.rr3_rollback_last();
        assert_eq!(h.rr3_messages().len(), 1, "回滚后只剩 system");
        assert_eq!(h.rr3_messages()[0].0, "system");
    }
}
