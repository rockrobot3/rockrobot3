//! 能力降级与解除传播（ADR-0009 裁定 3）。

use super::super::*;

impl RrdRegistry {
    /// 降级传播（ADR-0009 裁定 3）：`provider` 离开 Running（判死 / 退出 / 注销三路
    /// 都已先在本表记完账）之后调用。它提供的能力若再无任何 **Running** 的提供者，
    /// 依赖该能力的在册 `Running` 模块逐个置 `Degraded`；多提供者时任一存活即满足。
    /// 返回发生转移的 (模块名, task)，调用方照它广播 state——本方法只管表，不管线。
    /// **只置位，不记账不出计划**（裁定 4：降级不是崩溃，不吃预算不经 supervisor）。
    /// 前置：调用时 `provider` 的死亡已落账（三条死亡路各自先记账再传播），否则
    /// "它自己还 Running"会挡住判空。
    pub fn rr3_reg_degrade(&mut self, provider: &str) -> Vec<(String, u64)> {
        let caps: Vec<String> = self
            .providers
            .iter()
            .filter(|(_, names)| names.contains(provider))
            .map(|(c, _)| c.clone())
            .collect();
        let mut out = Vec::new();
        for cap in caps {
            out.extend(self.rr3_degrade_cap(&cap));
        }
        out
    }

    /// 单能力降级：该能力再无 Running 提供者时，依赖它的 Running 模块逐个置 Degraded。
    fn rr3_degrade_cap(&mut self, cap: &str) -> Vec<(String, u64)> {
        // 该能力还有在册且 Running 的提供者吗？刚死的那个已在表里置 Failed
        // （或已注销摘除），天然不在数内，无需按名排除。
        let names = self.providers.get(cap).cloned().unwrap_or_default();
        let any_alive = self
            .modules
            .values()
            .any(|e| e.phase == Rr3Phase::Running && names.contains(&e.name));
        if any_alive {
            return Vec::new();
        }
        let mut out = Vec::new();
        for e in self.modules.values_mut() {
            if e.phase == Rr3Phase::Running && e.facts.depends_required.iter().any(|d| d == cap) {
                e.phase = Rr3Phase::Degraded;
                out.push((e.name.clone(), e.src_task));
            }
        }
        out
    }

    /// 解除传播（ADR-0009 裁定 3 的恢复半边）：`provider` 回到 Running（`Promoted`
    /// 那一拍）之后调用。依赖它能力的在册 `Degraded` 模块逐个重算——**全部**
    /// required 能力重新就位才回 `Running`（只回来一个提供者不代表别的依赖也好了）。
    /// 返回与 `rr3_reg_degrade` 同形。
    pub fn rr3_reg_undegrade(&mut self, provider: &str) -> Vec<(String, u64)> {
        let caps: Vec<String> = self
            .providers
            .iter()
            .filter(|(_, names)| names.contains(provider))
            .map(|(c, _)| c.clone())
            .collect();
        let mut out = Vec::new();
        for cap in caps {
            // 先按不可变借用收齐本拍要重算的名字，再逐个改（两段式借用，与
            // `rr3_reg_expire` 同一条纪律）。刚解除的模块在下一轮 cap 里已是 Running，
            // 不会被重复处理。
            let candidates = self.rr3_cap_stuck(&cap);
            out.extend(self.rr3_undegrade_apply(candidates));
        }
        out
    }

    /// 本拍因 `cap` 回来而要重算的 Degraded 模块名（预降级不归能力传播管）。
    fn rr3_cap_stuck(&self, cap: &str) -> Vec<String> {
        self.modules
            .values()
            .filter(|e| {
                e.phase == Rr3Phase::Degraded
                    // G4：装配缺项的预降级不归能力传播管，解除路只有修数据 + 重启
                    && !self.pre_degraded.contains(e.name.as_str())
                    && e.facts.depends_required.iter().any(|d| d.as_str() == cap)
            })
            .map(|e| e.name.clone())
            .collect()
    }

    /// 重算候选：required 全部就位才回 Running。
    fn rr3_undegrade_apply(&mut self, candidates: Vec<String>) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        for name in candidates {
            let deps = self
                .modules
                .get(&name)
                .map(|e| e.facts.depends_required.clone())
                .unwrap_or_default();
            if self.rr3_deps_satisfied(&deps) {
                let e = self.modules.get_mut(&name).expect("刚读过、仍在册");
                e.phase = Rr3Phase::Running;
                out.push((name, e.src_task));
            }
        }
        out
    }

    /// 某模块的 required 能力是否全部就位。不在提供者表里的能力视为 base 内建
    /// （永远在位）。走线上注册的模块若依赖 boot 全集之外的能力，这里同样按"在位"
    /// 处理——那是"运行期动态重仲裁不做"（ADR-0009 不做清单 2）的同一条边界：
    /// 可满足性宇宙只在 boot 时定一次，线上补注册的模块不做二次裁定（实施报告记录）。
    fn rr3_deps_satisfied(&self, required: &[String]) -> bool {
        required.iter().all(|cap| match self.providers.get(cap) {
            None => true,
            Some(names) => names.iter().any(|n| self.rr3_mod_live(n)),
        })
    }

    /// 提供者是否在位：在册且相位 = Running。
    fn rr3_mod_live(&self, name: &str) -> bool {
        self.modules
            .get(name)
            .is_some_and(|e| e.phase == Rr3Phase::Running)
    }
}
