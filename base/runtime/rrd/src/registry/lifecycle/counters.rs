//! 运维计数聚合、装配缺项预降级与能力提供者表。

use super::super::*;

impl RrdRegistry {
    pub fn new(restart_base_ms: u64) -> Self {
        Self {
            modules: BTreeMap::new(),
            by_task: BTreeMap::new(),
            exposes: BTreeMap::new(),
            next_task: rr_bus::RR3_TASK_FIRST_MODULE,
            restart_base_ms,
            denied_publish: 0,
            crash_streak: BTreeMap::new(),
            owned: BTreeSet::new(),
            stopped: BTreeSet::new(),
            pre_degraded: BTreeSet::new(),
            providers: BTreeMap::new(),
            low_power: false,
            counters: BTreeMap::new(),
            ctr_cap_noted: false,
            fanout_last: BTreeMap::new(),
        }
    }

    /// 运维计数：启动时注入持久化加载的全表（daemon 启动点），或整体替换。
    /// 运行期归并走 [`rr3_reg_ctr_merge`]，不走这里——这条只服务启动装载。
    pub fn rr3_reg_ctr_replace(&mut self, table: BTreeMap<String, u64>) {
        self.counters = table;
    }

    /// 装配缺项预降级标记（G4）：daemon 在投 boot 队列前写入的名字集。命中者
    /// 注册入表即 `Degraded`（见 `rr3_reg_register` 的插入相位），且不被
    /// [`rr3_reg_undegrade`] 捞回。
    pub fn rr3_reg_predegrade(&mut self, names: impl IntoIterator<Item = String>) {
        self.pre_degraded.extend(names);
    }

    /// 运维计数：单键累加（rrd 自有键的唯一写法，`boots` 自增即它）
    pub fn rr3_reg_ctr_bump(&mut self, key: &str, by: u64) {
        *self.counters.entry(key.to_string()).or_insert(0) += by;
    }

    /// 运维计数：把模块自报的一拍 counters 归并进聚合表——键加模块名前缀
    /// `module/key`（同名键不同模块互不覆盖，聚合表是全局命名空间），同键累加。
    /// 线上语义（b3c 2026-10-21 钉死）：心跳 `counters` 值 = 模块「自上一拍以来」
    /// 的**增量**（客户端记已发水位，rr-bus client.rs / brain bus.py 双端镜像），
    /// 归并累加后聚合表 = 生命周期累计——旧线上客户端曾每拍发全量快照被本方法
    /// 当增量再收一遍，pcm_frames 跑出 40 亿量级（快照 × 增量两语义同线各说各话
    /// 的双端分裂，2026-10-05 批B 落地即埋）。
    /// 返回是否产生了变化（调用方据此决定要不要 write-through 落盘，空拍不写盘）。
    pub fn rr3_reg_ctr_merge(&mut self, module: &str, counters: &BTreeMap<String, u64>) -> bool {
        if counters.is_empty() {
            return false;
        }
        self.rr3_ctr_fold(module, counters)
    }

    /// 逐键带模块名前缀累加；返回是否产生了变化。
    fn rr3_ctr_fold(&mut self, module: &str, counters: &BTreeMap<String, u64>) -> bool {
        let mut changed = false;
        for (k, v) in counters {
            changed |= self.rr3_ctr_one(format!("{module}/{k}"), *v);
        }
        changed
    }

    /// 单键累加（键数上界拒新键）：返回本键是否产生了变化。
    fn rr3_ctr_one(&mut self, key: String, v: u64) -> bool {
        if self.rr3_ctr_capped(&key) {
            return false;
        }
        let slot = self.counters.entry(key).or_insert(0);
        let before = *slot;
        *slot = before.saturating_add(v);
        *slot != before
    }

    /// 计数键数达上界时拒新键（既有键照常累加），并只点一次名。
    fn rr3_ctr_capped(&mut self, key: &str) -> bool {
        if self.counters.contains_key(key) || self.counters.len() < RR3_CTR_KEY_CAP {
            return false;
        }
        self.rr3_cap_note_once(key);
        true
    }

    /// 达上界只点一次名（后续拒新键不再刷屏）。
    fn rr3_cap_note_once(&mut self, key: &str) {
        if self.ctr_cap_noted {
            return;
        }
        self.ctr_cap_noted = true;
        rr3_log(&format!(
            "运维计数键数达上界 {RR3_CTR_KEY_CAP}，拒新键 {key}（既有键照常累加；查模块是否每拍换键）"
        ));
    }

    /// 运维计数：全表只读快照（`rr3.ctl.counters` 的读面与持久化写盘共用）
    pub fn rr3_reg_counters(&self) -> BTreeMap<String, u64> {
        self.counters.clone()
    }

    /// 写入能力 → 提供者表（boot 一次，见字段文档）。空表 = 无跨模块能力（全 base），
    /// 传播方法随之全部空转——与"没有模块声明 provides"的既有行为兼容。
    pub fn rr3_reg_providers(&mut self, providers: BTreeMap<String, BTreeSet<String>>) {
        self.providers = providers;
    }
}
