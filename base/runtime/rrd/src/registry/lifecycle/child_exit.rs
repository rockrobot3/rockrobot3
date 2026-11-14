//! 子进程退出码那一路：在册崩溃/不在册早退的记账与判定。

use super::super::*;

impl RrdRegistry {
    /// 子进程退出码那一路（11b 的 supervisor 收尸用；11a 只把表级记账钉死）。
    /// 与 `rr3_reg_death` 的分工：owned 的模块，其 EOF 走 DeferredToExitStatus（相位/账/广播
    /// 三件事全留给本函数），所以本函数是那条让出来的终态的唯一落点。
    pub fn rr3_reg_child_exit(
        &mut self,
        facts: &Rr3ManifestFacts,
        clean_exit: bool,
        now_ms: u64,
    ) -> Rr3ChildExit {
        let _ = now_ms;
        // 收过尸就撤所有权标记：无论走哪条分支，这次退出都已被 supervisor 处理完，
        // 否则同名新实例的 EOF 会永远等一个不再来的退出码（`owned_conn_death...` 钉这一条）。
        self.owned.remove(facts.name.as_str());
        let present = self
            .modules
            .get(facts.name.as_str())
            .map(|e| (e.phase, e.src_task));
        let Some((phase, src_task)) = present else {
            return self.rr3_child_absent(facts, clean_exit);
        };
        // 已被 watchdog 判死：收尸不得双计（R-A4 的封印）。
        if phase == Rr3Phase::Failed {
            return Rr3ChildExit::Silent;
        }
        // 在册且活着：相位、句柄、账都在这一次到位。
        self.rr3_child_crashed(facts, clean_exit, src_task)
    }

    /// 在册崩溃那一路：置 Failed、摘句柄、记账。
    fn rr3_child_crashed(
        &mut self,
        facts: &Rr3ManifestFacts,
        clean_exit: bool,
        src_task: u64,
    ) -> Rr3ChildExit {
        let e = self
            .modules
            .get_mut(facts.name.as_str())
            .expect("刚确认在册");
        e.tx = None;
        e.phase = Rr3Phase::Failed;
        Rr3ChildExit::Crashed {
            src_task,
            clean_exit,
            plan: self.rr3_charge(facts, clean_exit),
        }
    }

    /// 不在册那一路：显式 stop 过的退出不记账，其余按 EarlyExit 吃预算。
    fn rr3_child_absent(&mut self, facts: &Rr3ManifestFacts, clean_exit: bool) -> Rr3ChildExit {
        if self.stopped.contains(facts.name.as_str()) {
            Rr3ChildExit::Silent
        } else {
            // 从没注册过 → 没有条目可广播，但账必须按 name 记（坏 entry 不能空转重起）。
            Rr3ChildExit::EarlyExit {
                plan: self.rr3_charge(facts, clean_exit),
            }
        }
    }
}
