//! 崩溃账：重启策略、退避计划与达上限查询。

use super::super::*;

impl RrdRegistry {
    /// 唯一的记账处（D-1）：`rr3_reg_death` / `rr3_reg_expire` / `rr3_reg_child_exit` 三路共用，
    /// 别处一律不许碰 `crash_streak`。返回重启计划或 None。
    /// 先判后加：策略说不重启、或已达上限 → 直接 None，**不增长** streak
    /// （与今天 `restarts` 的次序一致——达上限不再往里加，也不给计划）。
    pub(super) fn rr3_charge(
        &mut self,
        facts: &Rr3ManifestFacts,
        clean_exit: bool,
    ) -> Option<Rr3RestartPlan> {
        let restart = Self::rr3_restart_wanted(&facts.restart_policy, clean_exit);
        let streak = self.rr3_streak(facts.name.as_str());
        if !restart || streak >= RR3_RESTART_FAILURE_CAP {
            return None;
        }
        let attempt = streak + 1;
        self.crash_streak.insert(facts.name.clone(), attempt);
        Some(self.rr3_charge_plan(facts, attempt))
    }

    /// 崩溃账读取：同名连续失败次数（无账 = 0）。
    fn rr3_streak(&self, name: &str) -> u32 {
        self.crash_streak.get(name).copied().unwrap_or(0)
    }

    /// 退避表 → 重启计划：倍数 × base，封顶到 u64::MAX（永不 panic、不回绕）。
    fn rr3_charge_plan(&self, facts: &Rr3ManifestFacts, attempt: u32) -> Rr3RestartPlan {
        let idx = (attempt as usize - 1).min(RR3_RESTART_BACKOFF.len() - 1);
        Rr3RestartPlan {
            name: facts.name.clone(),
            attempt,
            // 退避倍数 × base：溢出既不 panic（debug 的 `*` 会 panic）也不回绕
            // （release 的 `*` 会 wrap），封顶到 u64::MAX 让"永远等下去"成为唯一保守解。
            // `restart_base_ms = 0` 的下界语义（→ 立即重启）已由 Task 7 的 CLI 门禁拦住
            // （`cli.rs::parse_restart_base`，提交 f69eda8：拒 0、拒超上界），本表不替它裁定语义。
            delay_ms: RR3_RESTART_BACKOFF[idx].saturating_mul(self.restart_base_ms),
            entry: facts.entry.clone(),
        }
    }

    /// 重启策略 → 是否重启（"never" 与非预期值一律否；校验已在 Task 2 挡住，这里只保证不 panic）。
    fn rr3_restart_wanted(policy: &str, clean_exit: bool) -> bool {
        match policy {
            "always" => true,
            "on-failure" => !clean_exit,
            _ => false,
        }
    }

    /// 达上限查询（D-5）：`crash_streak[name] >= RR3_RESTART_FAILURE_CAP`。
    /// 为什么单开一个查询而不是让 `rr3_charge` 返回三态：策略说"不重启"（never / on-failure
    /// 且干净）与预算说"到顶了"是两件不同的话，调用方要打的日志文案也不同，把判断塞进
    /// `Option<Rr3RestartPlan>` 会逼调用方自己再算一遍 streak（第二处真值）。
    /// **本轮"整体标 degraded"只有日志面**：`contracts/topics.toml` 没有承载运行时整体状态的
    /// topic（只有 `rr3-runtime/system/heartbeat` 与 `rr3-runtime/module/state`），新增 topic 属
    /// 改契约；11b 的日志文案里落地"运行时降级"。
    pub fn rr3_reg_hit_cap(&self, name: &str) -> bool {
        self.crash_streak.get(name).copied().unwrap_or(0) >= RR3_RESTART_FAILURE_CAP
    }

    /// 某 name 当前的崩溃连击数（表级、跨重注册成立）：观测面，不写字。
    pub fn rr3_reg_crash_streak(&self, name: &str) -> u32 {
        self.crash_streak.get(name).copied().unwrap_or(0)
    }
}
