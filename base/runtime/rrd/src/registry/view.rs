// registry 的只读视图与转发面：存活/名字/事实查询、广播扇出与留痕闸、
// 快照与健康行、点名路由的候选选择。写入路径在 lifecycle.rs。
use super::*;

impl RrdRegistry {
    pub fn rr3_reg_live(&self, name: &str) -> bool {
        self.modules
            .get(name)
            .map(|e| e.phase != Rr3Phase::Failed && e.tx.is_some())
            .unwrap_or(false)
    }

    /// 按 task 查名字（conn 层用它校验 `src_task` 是否属于该连接）
    pub fn rr3_reg_name_of(&self, src_task: u64) -> Option<String> {
        self.by_task.get(&src_task).cloned()
    }

    /// 取某模块的 facts（Task 11 的 spawn 序要用 start_priority 与 entry）
    pub fn rr3_reg_facts(&self, name: &str) -> Option<Rr3ManifestFacts> {
        self.modules.get(name).map(|e| e.facts.clone())
    }

    /// 投给除 `exclude_task` 外的所有**活**连接；返回成功入队数。接收者已 drop 不算错。
    /// **只做广播，不看 `frame.header.dst_task`**：该字段原样带上、本函数不读它，所以
    /// 它**不是**单播入口（`expired_module_is_excluded_from_fanout` 就把这条事实编码了：
    /// 帧的 dst 写的是已判死的 simb，实际收到的是 sima）。要点名发给某一个模块，请由
    /// conn 层直接用那条连接的 `Sender`，别指望本函数过滤。
    ///
    /// 有界队列的满侧策略（0.0.8b 审计 B-5 初版；本轮审计修正）：`try_send`
    /// 非阻塞——`Full` = 这条连接的 writer 暂时追不上广播（对端主循环在长动作里，
    /// 如 interact 播放整段 TTS），此时**只丢当前帧、保留连接**并计数：
    /// 事件总线是尽力投递，丢帧可由模块侧 seq 缺口对账看见，队列腾空后自动续投。
    /// 旧行为把 `tx=None` 当"断投至重连"，但模块没有重连信号——心跳是独立的
    /// RPC 通道，连接看起来一直活着，于是模块会**永久失聪**直到因别的原因重启；
    /// 一次播放期积压就足以触发，属可用性缺陷。`Disconnected` 才是 writer 真亡，
    /// 才置 `tx=None`。
    pub fn rr3_reg_fanout(&mut self, frame: &Rr3Frame, exclude_task: u64) -> usize {
        let mut n = 0;
        for (name, e) in &mut self.modules {
            if e.src_task == exclude_task {
                continue;
            }
            let Some(tx) = e.tx.as_ref() else {
                continue;
            };
            match tx.try_send(frame.clone()) {
                Ok(()) => n += 1,
                Err(TrySendError::Disconnected(_)) => e.tx = None,
                Err(TrySendError::Full(_)) => {
                    e.fan_dropped = e.fan_dropped.saturating_add(1);
                    // 每 64 帧点名一次：满队列期间不刷屏，又不把丢帧静默掉
                    if e.fan_dropped % 64 == 1 {
                        rr3_log(&format!(
                            "广播积压丢帧 {name}：收件队列满（{RR3_FAN_CAP} 帧上界），已丢 {} 帧——保留连接，主循环恢复读取后自动续投",
                            e.fan_dropped
                        ));
                    }
                }
            }
        }
        n
    }

    /// 广播面影子统计（广播面架构 ADR 前哨，0.0.8c）：按各在册模块 manifest
    /// `[events].subscribes` 统计本帧"本应被过滤"的目标连接数——**只记账与点名，
    /// 不改投递行为**（真实投递仍走 [`Self::rr3_reg_fanout`] 全量扇出）。
    /// 计数键：`fanout_shadow_frames`（含本应过滤目标的帧数）、`fanout_shadow_conns`
    /// （帧×连接总数）、`fanout_shadow/<topic>`（主题分账）。主题首次出现时点名
    /// 一条日志；后续由计数面读取，不逐帧刷屏。返回本帧本应过滤的连接数。
    pub fn rr3_reg_shadow(&mut self, topic: &str, exclude_task: u64) -> u64 {
        let mut n = 0u64;
        for e in self.modules.values() {
            if e.src_task == exclude_task || e.tx.is_none() {
                continue;
            }
            if !e.facts.subscribes.iter().any(|t| t == topic) {
                n += 1;
            }
        }
        if n == 0 {
            return 0;
        }
        self.rr3_reg_ctr_bump("fanout_shadow_frames", 1);
        self.rr3_reg_ctr_bump("fanout_shadow_conns", n);
        let key = format!("fanout_shadow/{topic}");
        let first = !self.counters.contains_key(&key);
        self.rr3_reg_ctr_bump(&key, n);
        if first {
            rr3_log(&format!(
                "影子统计（subscribes 过滤预演，不改投递）：{topic} 本应过滤 {n} 个连接；全量扇出照旧"
            ));
        }
        n
    }

    /// 广播转发留痕闸：返回 true = 该 topic 的接收连接数相对上次发生变化（含首次），
    /// 调用方据此才落日志。音频流每帧一条会把日志环/落盘冲垮，拓扑变化才值得记。
    pub fn rr3_reg_fan_changed(&mut self, topic: &str, n: usize) -> bool {
        match self.fanout_last.get(topic) {
            Some(&prev) if prev == n => false,
            _ => {
                self.fanout_last.insert(topic.to_string(), n);
                true
            }
        }
    }

    /// 快照按 **task 升序**（不是名字序）：状态广播与运维输出处要靠它对齐 task 号，
    /// 名字序纯属 `BTreeMap<String,_>` 的顺带结果，故必须显式排。
    pub fn rr3_reg_snapshot(&self) -> Vec<(String, u64, Rr3Phase)> {
        let mut v: Vec<(String, u64, Rr3Phase)> = self
            .modules
            .values()
            .map(|e| (e.name.clone(), e.src_task, e.phase))
            .collect();
        v.sort_by_key(|(_, t, _)| *t);
        v
    }

    /// 健康快照：与 `rr3_reg_snapshot` 同一排序（task 升序），每行多带心跳三数。
    /// 数据全部来自既有条目字段，不新增守护状态（ADR-0010 裁定 4）。
    pub fn rr3_reg_health(&self, now_ms: u64) -> Vec<Rr3HealthRow> {
        let mut v: Vec<Rr3HealthRow> = self
            .modules
            .values()
            .map(|e| Rr3HealthRow {
                name: e.name.clone(),
                src_task: e.src_task,
                phase: e.phase,
                heartbeat_ms: e.facts.heartbeat_ms,
                watchdog_ms: e.facts.watchdog_ms,
                since_beat_ms: now_ms.saturating_sub(e.last_beat_ms),
            })
            .collect();
        v.sort_by_key(|r| r.src_task);
        v
    }

    /// 查 `rr3.call` 的被调方（M3 模块间请求路由的路由表读取口）。
    /// 被调条件：**在册且活着**（phase ≠ `Failed` 且 `tx` 存在，与 `rr3_reg_live` 同一判据）
    /// 且 manifest 的 `[rpc].exposes` 里登记了这个 topic——call 的路由表来自被调方
    /// 自己的声明，不走 topics.toml 的发布归属（那是 publish 的检查，与 call 无关）。
    /// 多个模块 exposes 同一 topic 时取 **src_task 最小者**：候选集来自注册期维护的
    /// [`RrdRegistry::exposes`] 索引（升序），首个存活者即最小者，路由不随名字序漂。
    pub fn rr3_reg_callee_for(&self, topic: &str) -> Option<(String, u64)> {
        for task in self.exposes.get(topic)? {
            let Some(name) = self.by_task.get(task) else {
                continue;
            };
            let Some(e) = self.modules.get(name) else {
                continue;
            };
            if e.phase != Rr3Phase::Failed && e.tx.is_some() {
                return Some((e.name.clone(), e.src_task));
            }
        }
        None
    }

    /// 点名投递：按 task 找到那条连接的 `Sender` 投一帧。返回是否成功入队。
    /// 这是 `rr3_reg_fanout` 文档里"要点名请另用 Sender"的那个入口：fanout 的签名
    /// 冻结不动（它只广播），单播由本函数承担。找不到在册连接（已退场/已过期）或
    /// 对端收件人已 drop 都回 `false`，由调用方决定回什么错。
    pub fn rr3_reg_send_to(&self, task: u64, frame: &Rr3Frame) -> bool {
        let Some(name) = self.by_task.get(&task) else {
            return false;
        };
        let Some(entry) = self.modules.get(name) else {
            return false;
        };
        let Some(tx) = entry.tx.as_ref() else {
            return false;
        };
        tx.try_send(frame.clone()).is_ok()
    }
}
