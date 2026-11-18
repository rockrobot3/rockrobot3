//! 连接 EOF 的死亡处理、条目摘除与所有权标记。

use super::super::*;

impl RrdRegistry {
    /// 死亡处理（conn EOF 那一路）：判定序见下，每一步都可被一条表级用例指认。
    ///
    /// `now_ms` 在本函数里**没有读者**，签名却是冻结的（Task 7 的 conn 层照样传
    /// `rr3_ctx_now_ms()`）：把它写进
    /// `last_beat_ms` 是一处死写——`rr3_reg_expire` 跳过 failed、`rr3_reg_beat` 拒 failed、
    /// 同身份重注册时整条 `Entry` 被替换，没有任何路径会再读那颗基准，故收下但不写。
    /// 死亡时刻若将来要进日志，那是新增字段的事，不是复用到期基准。
    pub fn rr3_reg_death(&mut self, name: &str, clean_exit: bool, now_ms: u64) -> Rr3Death {
        let _ = now_ms; // 现值：死亡时刻不进基准（原注释保留）
                        // 序不是风格问题，三行各有归属：
                        //   1) 已 failed → 谁都不许再广播、再记账（R-A4 在这一条上结清）
                        //   2) **不在册必须先于 owned 判定**：unregister 已把条目摘掉，此时"等退出码"会让
                        //      `unregistered` 永远没人广播（收尸那一路对不在册的条目走 stopped/Silent，也不会补）
                        //   3) 在册且 owned → 让位给退出码那一路（conn 不改相位、不记账、不广播）
        let Some(entry) = self.modules.get(name) else {
            return Rr3Death::Broadcast { plan: None }; // unregister 之后的收尾：照今天广播 unregistered
        };
        if entry.phase == Rr3Phase::Failed {
            return Rr3Death::Silent;
        }
        if self.owned.contains(name) {
            return Rr3Death::DeferredToExitStatus;
        }
        self.rr3_death_fail(name, clean_exit)
    }

    /// 判死后落账：摘发送端 → 标 failed → 按策略记重启账。
    fn rr3_death_fail(&mut self, name: &str, clean_exit: bool) -> Rr3Death {
        {
            let e = self.modules.get_mut(name).expect("上面确认过条目在");
            e.tx = None;
            e.phase = Rr3Phase::Failed;
        }
        // 两段式借用：`&mut self.modules` 的守卫不能越过 `&mut self` 的方法调用活着
        let facts = self.modules.get(name).expect("刚改过相位").facts.clone();
        Rr3Death::Broadcast {
            plan: self.rr3_charge(&facts, clean_exit),
        }
    }

    pub fn rr3_reg_remove(&mut self, name: &str) -> bool {
        match self.modules.remove(name) {
            Some(e) => {
                self.by_task.remove(&e.src_task);
                self.rr3_drop_exposes(&e.facts, e.src_task);
                // D-4：显式退场（unregister RPC）记进 stopped——之后这条 name 的子进程退出码
                // 不得被收尸路当成一次崩溃记账。rr3_reg_add 会把它移回。
                self.stopped.insert(name.to_string());
                true
            }
            None => false,
        }
    }

    /// 所有权标记（D-3）：name ∈ owned ⟺ "这个进程是我起的，它的退出码才是死因的权威"。
    /// 写入方是 Task 11b 的 supervisor（`rr3_boot_spawn` 在 `Command::spawn` **之前** own，
    /// 次序理由见该函数文档），清除方有两处：起进程失败时当场 `rr3_reg_unown`，以及
    /// `rr3_reg_child_exit` 收完这一次尸时撤下（同名新实例的 EOF 不该等一个不再来的退出码）。
    /// 11a 之前生产路径无人调用它，conn 见 EOF 因此永不走 `DeferredToExitStatus`。
    pub fn rr3_reg_own(&mut self, name: &str) {
        self.owned.insert(name.to_string());
    }

    pub fn rr3_reg_unown(&mut self, name: &str) {
        self.owned.remove(name);
    }

    pub fn rr3_reg_is_owned(&self, name: &str) -> bool {
        self.owned.contains(name)
    }
}
