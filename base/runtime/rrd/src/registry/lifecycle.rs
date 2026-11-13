// registry 的生命周期面：注册/索引维护、心跳提升、到期判活、发布归属判定、
// 低电闸、死亡与重启记账、降级/恢复、子进程收尸。全部是 `RrdRegistry` 的
// 有状态方法——纯策略在 model.rs，只读视图在 view.rs。
use super::*;

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
    fn rr3_drop_exposes(&mut self, facts: &Rr3ManifestFacts, task: u64) {
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

    /// 主题首段（命名空间）：`a/b` → `a`，无分隔符即整串。
    fn rr3_pub_head(topic: &str) -> &str {
        topic.split('/').next().unwrap_or("")
    }

    /// 前三层安全判据的前两层：保留命名空间 → 前缀越权。
    fn rr3_pub_namespace(
        sender_name: &str,
        topic: &str,
        topics: &Rr3TopicSet,
    ) -> Result<(), Rr3PublishDeny> {
        if crate::topics_load::rr3_topics_reserved(topic) {
            return Err(Rr3PublishDeny::ReservedNamespace {
                topic: topic.to_string(),
            });
        }
        if Self::rr3_pub_head(topic) != sender_name {
            return Err(Rr3PublishDeny::ForeignPrefix {
                topic: topic.to_string(),
                owner: owner_for_echo(topics.rr3_topic_owner(topic)),
                sender: sender_name.to_string(),
            });
        }
        Ok(())
    }

    /// 发布归属判定。三层顺序即安全语义：下行冒充 → 前缀越权 → 未在册，`Ok` 之前再把
    /// 主题集里登记的 owner 与发送方比一次（见末段）。
    /// 关联函数（不碰 self），故生产路径与测试路径完全同码。
    ///
    /// 顺序不是洁癖：`rr3-runtime/*` 的冒充者在主题集里可以"合法在册"（owner 就写
    /// `rr3-runtime`），若先查前缀或先查在册，它会得到 ForeignPrefix/Ok 而不是"冒充"这条
    /// 最严重的定性。rrd 自己是该命名空间唯一合法发布者，走的是下行广播路径，不调本函数。
    ///
    /// **`sender_name` 的来源是调用方必须保证的前提，本函数无法替你保证**：签名收
    /// `&str`、不碰 `self`，所以它既不知道这个名字是否在册（从未注册、或已 `Failed`
    /// 的名字照样能拿到 `Ok`），也不知道它是不是报文里编出来的。唯一的防线建在调用侧：
    /// `sender_name` 一律由 `rr3_reg_name_of(src_task)` 从连接持有的 task 号**反查**得到，
    /// **绝不得取自帧体或注册报文里自称的 name 字段**。Task 7 已落地的上行处理（`conn.rs`）与
    /// 此后 Task 8 的发布分支都必须照此接线。
    ///
    /// 末段的 owner 比对不是重复劳动：首段 == `sender_name` 只是主题串的**语法**事实，
    /// `rr3_topic_owner(topic) == sender_name` 才是**在册表登记的事实**。二者今天恒等，
    /// 是因为 `rr3_topic_check` 强制"首段 == owner"，而那条前置只在 `rr3_topics_load`
    /// 的装载路径上被强制；`rr3_topic_parse` 本身是 public，绕开 check
    /// 造一张"首段是 rr3-sima、登记 owner 是 rr3-simb"的表并不费力。安全判定不该押在
    /// "上游一定查过"上，故这里就地把两条都查一遍（多一次 O(log n) 查表，无副作用）。
    pub fn rr3_reg_pub_check(
        sender_name: &str,
        topic: &str,
        topics: &Rr3TopicSet,
    ) -> Result<(), Rr3PublishDeny> {
        Self::rr3_pub_namespace(sender_name, topic, topics)?;
        rr3_pub_registered(topic, topics)?;
        // 首段相符只说明主题串"写着是这个命名空间"；在册 owner 是别人就是越权。
        // 取不到 owner（登记表里 owner 缺失 = 没写这个键，解析成空串）同样按越权拒，
        // 绝不因"查不到"而放行；回显与另一条 ForeignPrefix 分支一致地写 `?`（同走
        // `owner_for_echo`）。
        rr3_pub_owner(sender_name, topic, topics)
    }

    /// 在册 owner 与发送方是否不符。
    pub(super) fn rr3_owner_mismatch(owner: Option<&str>, sender: &str) -> bool {
        owner != Some(sender)
    }

    /// 记一次越权/被拒发布。只加计数：生命周期状态与别人的转发都不受影响。
    /// 饱和加：计数器撞顶（u32）后停在原值，绝不回绕成 0（回绕等于把审计线索抹平）。
    pub fn rr3_reg_note_denied(&mut self) {
        self.denied_publish = self.denied_publish.saturating_add(1);
    }

    pub fn rr3_reg_denied(&self) -> u32 {
        self.denied_publish
    }

    /// 置全局低电旗。返回是否为**首次**置位（上升沿）——conn 用它保证同一次低电段
    /// 只广播一次 `rr3-runtime/system/power`（重复 battery/low 不重复点名）。
    pub fn rr3_reg_lp_raise(&mut self) -> bool {
        let rising = !self.low_power;
        self.low_power = true;
        rising
    }

    /// 清全局低电旗（批N5 件二笔3，§17 运维留痕"low_power 旗补解除路径"）。
    /// 返回是否为真**下降沿**（此前确实在低电段）——conn 用它保证同一次低电段
    /// 只广播一次 `reason=battery-ok` 的解除点名（重复 battery/ok 不重复广播）。
    /// 与 raise 同一张表同一面：两个沿一个对，谁触发谁点名。
    pub fn rr3_reg_lp_lower(&mut self) -> bool {
        let falling = self.low_power;
        self.low_power = false;
        falling
    }

    /// supervisor 的 spawn 漏斗（`rr3_boot_spawn`）每 spawn 前读这一问。
    pub fn rr3_reg_lp_is_set(&self) -> bool {
        self.low_power
    }

    /// 唯一的记账处（D-1）：`rr3_reg_death` / `rr3_reg_expire` / `rr3_reg_child_exit` 三路共用，
    /// 别处一律不许碰 `crash_streak`。返回重启计划或 None。
    /// 先判后加：策略说不重启、或已达上限 → 直接 None，**不增长** streak
    /// （与今天 `restarts` 的次序一致——达上限不再往里加，也不给计划）。
    fn rr3_charge(&mut self, facts: &Rr3ManifestFacts, clean_exit: bool) -> Option<Rr3RestartPlan> {
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
