//! 心跳相位提升与 watchdog 到期判活（含崩溃账的恢复归零）。

use super::super::*;

impl RrdRegistry {
    /// 心跳：`registered → running` 的唯一入口（回报 `Promoted`），并刷新到期基准。
    /// 未在册的 task、以及已 failed 的条目一律 `Rejected`（failed 的模块不得靠补心跳复活自己）。
    /// 已经是 `Running` / `Degraded` 的只 `Kept`（续命、不升级）——`Degraded` 归 Kept 这一支
    /// 把 registry.rs:254-260 那段"非 failed 无条件写 Running"的欠账当场结掉：degraded 落地时
    /// 一颗心跳不许把降级标注抹平。
    pub fn rr3_reg_beat(&mut self, src_task: u64, now_ms: u64) -> Rr3Beat {
        let Some(name) = self.by_task.get(&src_task).cloned() else {
            return Rr3Beat::Rejected;
        };
        let Some(e) = self.modules.get_mut(&name) else {
            return Rr3Beat::Rejected;
        };
        Self::rr3_beat_phase(e, now_ms)
    }

    /// 相位推进一拍：只有 `Registered → Running` 是升级（回报 `Promoted`）。
    fn rr3_beat_phase(e: &mut Entry, now_ms: u64) -> Rr3Beat {
        match e.phase {
            Rr3Phase::Failed => Rr3Beat::Rejected,
            // degraded 只续命不升级（本轮 degraded 尚无 setter，此支为 11b 之后兜底）
            Rr3Phase::Running | Rr3Phase::Degraded => {
                e.last_beat_ms = now_ms;
                Rr3Beat::Kept
            }
            Rr3Phase::Registered => {
                e.phase = Rr3Phase::Running;
                e.last_beat_ms = now_ms;
                Rr3Beat::Promoted
            }
        }
    }

    /// watchdog 扫描（唯一的监视线程调用）：**一趟扫描、两个生命周期决定**——
    /// 1. 判死：超过各自 watchdog_ms 即 failed 并回报（含到期那一路的记账 `restart`，D-8）；
    /// 2. 恢复归零（D-2）：某条目 `phase == Running` 且 `now_ms - registered_at_ms >= watchdog_ms`
    ///    （活过一整条自己的窗口 = 这一世真的起来了）即清掉它的 crash_streak。
    ///
    /// 为什么复用 expire 而不是新开 `rr3_reg_reconcile`：那需要每拍多锁一次全表，而两件事
    /// 共用同一个 `now_ms`、同一张表、同一个"只有持有全表时钟的线程才能裁定"的前提。
    /// 两颗基准不许混：归零看 **registered_at_ms**（这一世活了多久），判死看 **last_beat_ms**
    /// （多久没喂狗）。
    ///
    /// 段序（不是风格）：`phase == Failed { continue }` 在前（已判死的不重复回报、本拍也不归零），
    /// 恢复归零段随后，到期段最后。健康模块不受牵连，故每 tick 全表扫是安全的；连接句柄当场
    /// 丢弃，之后的 `rr3_reg_fanout` 不再往死连接塞。
    pub fn rr3_reg_expire(&mut self, now_ms: u64) -> Vec<Rr3Expiry> {
        // 先把判定所需的一切按不可变借用读出：`values_mut` 的可变借用会挡住循环体里对
        // `self.crash_streak` 与 `self.rr3_charge(&mut self)` 的借用（两段式借用）。
        let rows: Vec<(String, u64, u64, u64, Rr3Phase, u64)> = self
            .modules
            .values()
            .map(|e| {
                (
                    e.name.clone(),
                    e.src_task,
                    e.registered_at_ms,
                    e.last_beat_ms,
                    e.phase,
                    e.facts.watchdog_ms,
                )
            })
            .collect();
        let mut out = Vec::new();
        for (name, src_task, registered_at_ms, last_beat_ms, phase, watchdog_ms) in rows {
            // 段 1：已 failed 的直接跳过——既不重复回报，也不在本拍归零。
            if phase == Rr3Phase::Failed {
                continue;
            }
            // 段 2：恢复归零——活过一整条自己的窗口 = 上一世的崩溃账到此为止。
            self.rr3_reg_reset(&name, phase, registered_at_ms, now_ms, watchdog_ms);
            // 段 3：判死。saturating_sub：调用方的毫秒读数理论上单调，但真出现倒退时宁可判
            // "未到期"也不回绕成一个巨大差值把健康模块集体判死。差值提成绑定：判据与回报共用它。
            let elapsed_ms = now_ms.saturating_sub(last_beat_ms);
            self.rr3_reg_kill(name, src_task, elapsed_ms, watchdog_ms, &mut out);
        }
        out
    }

    /// 段 3 实体：到期即置 Failed、记账并回报（差值不到窗口则本拍无动作）。
    fn rr3_reg_kill(
        &mut self,
        name: String,
        src_task: u64,
        elapsed_ms: u64,
        watchdog_ms: u64,
        out: &mut Vec<Rr3Expiry>,
    ) {
        if elapsed_ms >= watchdog_ms {
            let facts = {
                let e = self.modules.get_mut(&name).expect("刚读出、仍在册");
                e.phase = Rr3Phase::Failed;
                e.tx = None;
                e.facts.clone()
            };
            // clean_exit = false：超时就是崩溃，与 EOF 是不是 FIN 无关。facts 必须先 clone，
            // 否则 `&mut self.modules` 的守卫会越过 `&mut self` 的 rr3_charge 活着。
            let restart = self.rr3_charge(&facts, false);
            out.push(Rr3Expiry {
                name,
                src_task,
                reason: RR3_EXPIRE_HEARTBEAT_TIMEOUT,
                elapsed_ms,
                watchdog_ms,
                restart,
            });
        }
    }

    /// 段 2 判据：活过一整条自己的窗口（`registered_at_ms` 基准，D-2）即清崩溃账。
    fn rr3_reg_reset(
        &mut self,
        name: &str,
        phase: Rr3Phase,
        registered_at_ms: u64,
        now_ms: u64,
        watchdog_ms: u64,
    ) {
        if phase == Rr3Phase::Running && now_ms.saturating_sub(registered_at_ms) >= watchdog_ms {
            self.crash_streak.remove(name);
        }
    }
}
