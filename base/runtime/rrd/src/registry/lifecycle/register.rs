//! 注册入表与 exposes 索引维护。

use super::super::*;

impl RrdRegistry {
    /// 注册：保留名 → 同名在册（failed 除外）→ 超额 → 分配 task
    ///
    /// `entry` 的绝对性门禁不在这里（计划"与 spec 的偏离 6"：注册只拒语法，存在性/可执行
    /// 位随 spawn 那一刻查），那是 conn/daemon 层的职责：路径绝对性已随 Task 7 落地
    /// （`conn.rs` 的握手拒相对 entry、回 AIRY_EINVAL 并点名字段）；存在性/可执行位已随
    /// Task 11b 落地，但**只查本守护要 spawn 的那批**——`supervisor.rs` 的
    /// `rr3_boot_job_parse` 在 boot 那一刻查（E-3，拒法是 `拒绝启动`）。走线上注册的
    /// 外来模块 rrd 永不 spawn 它（E-1），所以那条 entry 的存在性根本无人裁定，也不是本表的职责。
    pub fn rr3_reg_add(
        &mut self,
        facts: &Rr3ManifestFacts,
        tx: SyncSender<Rr3Frame>,
        now_ms: u64,
    ) -> Result<u64, Rr3RegisterError> {
        if facts.name == crate::topics_load::RR3_RESERVED_OWNER {
            return Err(Rr3RegisterError::ReservedName {
                name: facts.name.clone(),
            });
        }
        // 重启后同身份重注册：旧条目整体替换，旧 task 号作废但不回收。
        // `crash_streak` 是**表级**、按 name 计的账（D-1），不随旧条目一起作废——"连续 5 次"
        // 的预算正是靠这一条才跨得起重注册；旧代码把 `restarts` 写进 Entry 又被这次替换归零，
        // 是 5 次上限今天撞不到的根因（见 `crash_budget_survives_reregistration_and_caps_after_five_incarnations`）。
        // 这里只**记下**待替换条目的 task 号，真正的摘除延后到门+断言全过之后：
        // 被 catch_unwind 吞掉的 debug panic（坏 watchdog）不许留下已改一半的表
        // （`caught_panic_leaves_the_failed_entry_registered` 钉住）。
        let superseded = self.rr3_add_superseded(facts)?; // 容量门认 superseded：表满但同名条目是 Failed 时，旧条目即将腾出槽位，
                                                          // 这是合法的 Failed 重注册，不得被容量拒（与移位前的现行行为一致）。
        if superseded.is_none() && self.modules.len() >= RR3_REG_MAX_MODULES {
            return Err(Rr3RegisterError::Capacity {
                limit: RR3_REG_MAX_MODULES,
            });
        }
        // 入库前提：watchdog_ms 必须为正，否则 `now_ms - last_beat_ms >= watchdog_ms` 恒成立，
        // 首轮到期扫描就把所有模块秒判死。`rr3_manifest_parse` 只在**文本**读取期
        // 拒非正整数（`manifest.rs` 的 `req_int`），而 `Rr3ManifestFacts` 字段全 `pub`，
        // 直接构造的 facts 绕得过那道校验，故这里再钉一次。
        // 用 debug_assert 而不是返回 Err：绕过 facts 读取入口是调用方的编程错误，而
        // `Rr3RegisterError` 的三个变体是冻结接口，不能为它加第四个；release 侧的下界已有归属：
        // `watchdog_ms` 只能从 manifest 文本来，而 `manifest.rs` 的 `req_int` 在**读取期**（不分
        // debug/release）就拒非正整数，CLI 面也没有 watchdog 这个旋钮可绕——本表不替它裁定语义。
        // 位置在**三条拒绝门之后**，让本函数文档里那句检查序在 debug 构建里也成立：断言
        // 抢在前面的话，一个带坏 watchdog 的重复/保留名/超额注册会先 panic 而不是如实返回
        // Err，调用方拿不到拒绝理由（Task 6 的 `rr3_rpc_reg_status` 便无从映射——
        // 那个调用方已随 Task 7 落地，就是 `conn.rs` 注册被拒时走的 `rr3_refuse`）。
        debug_assert!(
            facts.watchdog_ms > 0,
            "watchdog_ms = 0 的模块 {} 一入库就会被到期扫描判死",
            facts.name
        );
        // 到这里门与断言全过，才动表：先摘旧 Failed 条目（task→name 映射与条目本体），
        // 再插入新条目。panic 路径在上面就返回了，走不到这一步。
        if let Some(t) = superseded {
            if let Some(old) = self.modules.remove(&facts.name) {
                self.rr3_drop_exposes(&old.facts, old.src_task);
            }
            self.by_task.remove(&t);
        }
        let task = self.next_task;
        self.next_task = self.next_task.saturating_add(1);
        self.by_task.insert(task, facts.name.clone());
        // D-4：新实例 = 又有人要它跑，撤下"已显式退场"标记，否则这个 name 后来退出时
        // 其子进程退出码会被当成"我让它停的"而永不记账。
        self.stopped.remove(&facts.name);
        // D-1：crash_streak 是表级、按 name 计的账，**不**随条目替换归零——重注册不许擦掉
        // "连续 5 次"的预算（这正是旧 Entry.restarts 做不到、由本任务收口的缺陷）。
        // 唯一清空路径是 `rr3_reg_expire` 的恢复归零段（活满一整条自己的窗口）。
        self.modules.insert(
            facts.name.clone(),
            Entry {
                name: facts.name.clone(),
                src_task: task,
                // G4：装配缺项的模块在册即 Degraded（rr3_reg_predegrade 的对面）
                phase: if self.pre_degraded.contains(facts.name.as_str()) {
                    Rr3Phase::Degraded
                } else {
                    Rr3Phase::Registered
                },
                facts: facts.clone(),
                last_beat_ms: now_ms,
                registered_at_ms: now_ms,
                tx: Some(tx),
                fan_dropped: 0,
            },
        );
        self.rr3_add_exposes(facts, task);
        Ok(task)
    }

    /// exposes 索引登记：逐主题挂 task（同一主题重复声明由集合去重）。
    fn rr3_add_exposes(&mut self, facts: &Rr3ManifestFacts, task: u64) {
        for topic in &facts.rpc_exposes {
            self.exposes.entry(topic.clone()).or_default().insert(task);
        }
    }

    /// exposes 索引摘除：条目整体离表（重注册替换 / unregister）时调用；摘空的主题
    /// 整键删掉——索引只跟"有过声明者"走，不随失败历史长存。
    pub(super) fn rr3_drop_exposes(&mut self, facts: &Rr3ManifestFacts, task: u64) {
        for topic in &facts.rpc_exposes {
            let empty = match self.exposes.get_mut(topic) {
                Some(tasks) => {
                    tasks.remove(&task);
                    tasks.is_empty()
                }
                None => false,
            };
            if empty {
                self.exposes.remove(topic);
            }
        }
    }

    /// 同名在册门：非 Failed 即拒，Failed 条目记下待替换 task。
    fn rr3_add_superseded(
        &self,
        facts: &Rr3ManifestFacts,
    ) -> Result<Option<u64>, Rr3RegisterError> {
        let Some(old) = self.modules.get(&facts.name) else {
            return Ok(None);
        };
        if old.phase != Rr3Phase::Failed {
            return Err(Rr3RegisterError::Duplicate {
                name: facts.name.clone(),
                src_task: old.src_task,
            });
        }
        Ok(Some(old.src_task))
    }
}
