//! 在册模块表与生命周期状态机（architecture.md §4.2 的状态集，不新增状态）。
//!
//! 平台无关：只碰 BTreeMap / mpsc / u64 单调毫秒。UDS 与进程派生都在 daemon 层
//! （`conn.rs`/`daemon.rs`，随 Task 8 落地且全部 `#[cfg(unix)]`）。因此本文件的测试在
//! Windows 上照跑——M2 里"rrd 的行为"有相当一部分在本机就有证据（L1）。
//!
//! 三条不变量（改动前先读）：
//! 1. **task 号只增不回收**：0 是广播、1 是 rrd，模块从 `RR3_TASK_FIRST_MODULE` 起分配；
//!    模块 failed / 被移除后它的旧号作废，绝不复用（否则一条迟到的旧心跳能顶替新连接）。
//! 2. **到期判定用显式单调毫秒**，不存 `Instant`：`Instant` 不能回溯，测试就无法驱动
//!    "刚好在窗口内 / 刚好超窗"两种情形（见本任务"时钟约定"）。
//! 3. **越权只计数不牵连**：一次坏发布既不改发布者生命周期状态，也不影响其它连接的转发。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::mpsc::Sender;

use rr_ipc::Rr3Frame;
use rr_registry::Rr3TopicSet;

use crate::manifest::Rr3ManifestFacts;

/// 在册模块上限（spec §10 K3：v0.0.1 目标 5+1 模块，留余量并拒超额）
pub const RR3_REG_MAX_MODULES: usize = 16;

/// 重启退避表（倍数 × restart_base_ms），末档封顶 60×base（spec §5.3）
pub const RR3_RESTART_BACKOFF: [u64; 7] = [1, 2, 4, 8, 16, 32, 60];

/// 连续失败上限：到顶即 failed 并永久停止重启（spec §5.3）
pub const RR3_RESTART_FAILURE_CAP: u32 = 5;

/// 到期原因。`Rr3Expiry::reason` 是 `&'static str`，让日志与集成测试可以逐字比对；
/// 注册完成前超时（`pre_register_timeout_ms`）的判定在 conn 层，届时新增取值即可。
pub const RR3_EXPIRE_HEARTBEAT_TIMEOUT: &str = "heartbeat_timeout";

/// 生命周期阶段：registered → running ⇄ degraded，任何超时/退出即 failed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3Phase {
    /// 已注册、尚未收到任何心跳：此刻不判活（legacy 教训 76c0ad8）
    Registered,
    Running,
    /// 依赖的 base 能力未就位、或整体降级（由 daemon 层在 spawn/依赖判定处置位，spec §5.0）
    Degraded,
    Failed,
}

impl Rr3Phase {
    /// 下行广播 `rr3-runtime/module/state` 里回显的阶段名（spec §5.0）
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Running => "running",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3RegisterError {
    Duplicate { name: String, src_task: u64 },
    ReservedName { name: String },
    Capacity { limit: usize },
}

impl fmt::Display for Rr3RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate { name, src_task } => {
                write!(f, "模块 {name} 已在册（task={src_task}），拒绝重复注册")
            }
            Self::ReservedName { name } => {
                write!(f, "模块名 {name} 属内建命名空间保留号，拒绝注册")
            }
            Self::Capacity { limit } => write!(f, "在册模块已达上限 {limit}，拒绝注册"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3PublishDeny {
    ReservedNamespace { topic: String },
    ForeignPrefix { topic: String, owner: String, sender: String },
    NotRegistered { topic: String },
}

impl fmt::Display for Rr3PublishDeny {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReservedNamespace { topic } => write!(
                f,
                "拒绝 {topic}：rr3-runtime/* 是守护进程的下行广播，模块不得冒充"
            ),
            Self::ForeignPrefix { topic, owner, sender } => {
                write!(f, "越权发布 {topic}（owner={owner}，发送方={sender}）")
            }
            Self::NotRegistered { topic } => write!(f, "未注册主题 {topic}，拒绝发布"),
        }
    }
}

#[derive(Debug)]
struct Entry {
    name: String,
    src_task: u64,
    phase: Rr3Phase,
    facts: Rr3ManifestFacts,
    /// 相对进程起点的单调毫秒读数（见本任务"时钟约定"）
    last_beat_ms: u64,
    restarts: u32,
    tx: Option<Sender<Rr3Frame>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3Expiry {
    pub name: String,
    pub src_task: u64,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3RestartPlan {
    pub name: String,
    pub attempt: u32,
    pub delay_ms: u64,
    pub entry: String,
}

#[derive(Debug)]
pub struct RrdRegistry {
    modules: BTreeMap<String, Entry>,
    by_task: BTreeMap<u64, String>,
    next_task: u64,
    restart_base_ms: u64,
    denied_publish: u32,
}

impl RrdRegistry {
    pub fn new(restart_base_ms: u64) -> Self {
        Self {
            modules: BTreeMap::new(),
            by_task: BTreeMap::new(),
            next_task: rr_bus::RR3_TASK_FIRST_MODULE,
            restart_base_ms,
            denied_publish: 0,
        }
    }

    /// 注册：保留名 → 同名在册（failed 除外）→ 超额 → 分配 task
    ///
    /// `entry` 的绝对性门禁不在这里（计划"与 spec 的偏离 6"：注册只拒语法，存在性/可执行
    /// 位随 spawn 那一刻查），那是 Task 6/8 的 conn/daemon 层职责。
    pub fn rr3_reg_add(
        &mut self,
        facts: &Rr3ManifestFacts,
        tx: Sender<Rr3Frame>,
        now_ms: u64,
    ) -> Result<u64, Rr3RegisterError> {
        if facts.name == crate::topics_load::RR3_RESERVED_OWNER {
            return Err(Rr3RegisterError::ReservedName { name: facts.name.clone() });
        }
        if let Some(old) = self.modules.get(&facts.name) {
            if old.phase != Rr3Phase::Failed {
                return Err(Rr3RegisterError::Duplicate {
                    name: facts.name.clone(),
                    src_task: old.src_task,
                });
            }
            // 重启后同身份重注册：旧条目整体替换，旧 task 号作废但不回收。
            // 注意 restarts 随旧条目一起作废——"连续 5 次"的计数要跨重注册成立，
            // 就得由 daemon 层（Task 8）在派生新进程时把预算带进来，本表不猜。
            self.by_task.remove(&old.src_task);
            self.modules.remove(&facts.name);
        }
        if self.modules.len() >= RR3_REG_MAX_MODULES {
            return Err(Rr3RegisterError::Capacity { limit: RR3_REG_MAX_MODULES });
        }
        let task = self.next_task;
        self.next_task = self.next_task.saturating_add(1);
        self.by_task.insert(task, facts.name.clone());
        self.modules.insert(
            facts.name.clone(),
            Entry {
                name: facts.name.clone(),
                src_task: task,
                phase: Rr3Phase::Registered,
                facts: facts.clone(),
                last_beat_ms: now_ms,
                restarts: 0,
                tx: Some(tx),
            },
        );
        Ok(task)
    }

    /// 心跳：registered/degraded → running 并刷新到期基准。
    /// 未在册的 task、以及已 failed 的条目一律拒（failed 的模块不得靠补心跳复活自己）。
    pub fn rr3_reg_beat(&mut self, src_task: u64, now_ms: u64) -> bool {
        let Some(name) = self.by_task.get(&src_task).cloned() else { return false };
        let Some(e) = self.modules.get_mut(&name) else { return false };
        if e.phase == Rr3Phase::Failed {
            return false;
        }
        e.phase = Rr3Phase::Running;
        e.last_beat_ms = now_ms;
        true
    }

    /// watchdog 扫描（唯一的监视线程调用）：超过各自 watchdog_ms 即 failed 并回报。
    /// 健康模块不受牵连，故每 tick 全表扫是安全的；连接句柄当场丢弃，之后的
    /// `rr3_reg_fanout` 不再往死连接塞。
    pub fn rr3_reg_expire(&mut self, now_ms: u64) -> Vec<Rr3Expiry> {
        let mut out = Vec::new();
        for e in self.modules.values_mut() {
            if e.phase == Rr3Phase::Failed {
                continue;
            }
            // saturating_sub：调用方的毫秒读数理论上单调，但真出现倒退时宁可判"未到期"
            // 也不回绕成一个巨大差值把健康模块集体判死。
            if now_ms.saturating_sub(e.last_beat_ms) >= e.facts.watchdog_ms {
                e.phase = Rr3Phase::Failed;
                e.tx = None;
                out.push(Rr3Expiry {
                    name: e.name.clone(),
                    src_task: e.src_task,
                    reason: RR3_EXPIRE_HEARTBEAT_TIMEOUT,
                });
            }
        }
        out
    }

    /// 发布归属判定。三层顺序即安全语义：下行冒充 → 前缀越权 → 未在册。
    /// 关联函数（不碰 self），故生产路径与测试路径完全同码。
    ///
    /// 顺序不是洁癖：`rr3-runtime/*` 的冒充者在主题集里可以"合法在册"（owner 就写
    /// `rr3-runtime`），若先查前缀或先查在册，它会得到 ForeignPrefix/Ok 而不是"冒充"这条
    /// 最严重的定性。rrd 自己是该命名空间唯一合法发布者，走的是下行广播路径，不调本函数。
    pub fn rr3_reg_publish_check(
        sender_name: &str,
        topic: &str,
        topics: &Rr3TopicSet,
    ) -> Result<(), Rr3PublishDeny> {
        if crate::topics_load::rr3_topics_is_reserved_topic(topic) {
            return Err(Rr3PublishDeny::ReservedNamespace { topic: topic.to_string() });
        }
        let head = topic.split('/').next().unwrap_or("");
        if head != sender_name {
            return Err(Rr3PublishDeny::ForeignPrefix {
                topic: topic.to_string(),
                owner: topics.rr3_topic_set_owner(topic).unwrap_or_else(|| "?".to_string()),
                sender: sender_name.to_string(),
            });
        }
        if !topics.rr3_topic_set_contains(topic) {
            return Err(Rr3PublishDeny::NotRegistered { topic: topic.to_string() });
        }
        Ok(())
    }

    /// 记一次越权/被拒发布。只加计数：生命周期状态与别人的转发都不受影响。
    /// 饱和加：计数器撞顶（u32）后停在原值，绝不回绕成 0（回绕等于把审计线索抹平）。
    pub fn rr3_reg_note_denied(&mut self) {
        self.denied_publish = self.denied_publish.saturating_add(1);
    }

    pub fn rr3_reg_denied(&self) -> u32 {
        self.denied_publish
    }

    /// 死亡处理：策略决定是否重启；连续失败到上限即永久 failed
    pub fn rr3_reg_death(
        &mut self,
        name: &str,
        clean_exit: bool,
        now_ms: u64,
    ) -> Option<Rr3RestartPlan> {
        let e = self.modules.get_mut(name)?;
        e.tx = None;
        e.last_beat_ms = now_ms;
        e.phase = Rr3Phase::Failed;
        let restart = match e.facts.restart_policy.as_str() {
            "always" => true,
            "on-failure" => !clean_exit,
            _ => false, // "never" 与非预期值（校验已在 Task 2 挡住，这里只保证不 panic）
        };
        if !restart || e.restarts >= RR3_RESTART_FAILURE_CAP {
            return None;
        }
        e.restarts += 1;
        let idx = (e.restarts as usize - 1).min(RR3_RESTART_BACKOFF.len() - 1);
        Some(Rr3RestartPlan {
            name: name.to_string(),
            attempt: e.restarts,
            // 退避倍数 × base：溢出既不 panic（debug 的 `*` 会 panic）也不回绕
            // （release 的 `*` 会 wrap），封顶到 u64::MAX 让"永远等下去"成为唯一保守解。
            // `restart_base_ms = 0` 的下界语义（→ 立即重启）属 Task 7/8 的 CLI 门禁，
            // 这里不替它裁定（计划把该项显式划给了 Task 7/8）。
            delay_ms: RR3_RESTART_BACKOFF[idx].checked_mul(self.restart_base_ms).unwrap_or(u64::MAX),
            entry: e.facts.entry.clone(),
        })
    }

    pub fn rr3_reg_remove(&mut self, name: &str) -> bool {
        match self.modules.remove(name) {
            Some(e) => {
                self.by_task.remove(&e.src_task);
                true
            }
            None => false,
        }
    }

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

    /// 取某模块的 facts（Task 7 的 spawn 序要用 start_priority 与 entry）
    pub fn rr3_reg_facts(&self, name: &str) -> Option<Rr3ManifestFacts> {
        self.modules.get(name).map(|e| e.facts.clone())
    }

    /// 投给除 `exclude_task` 外的所有在册连接；返回成功入队数。接收者已 drop 不算错。
    pub fn rr3_reg_fanout(&self, frame: &Rr3Frame, exclude_task: u64) -> usize {
        self.modules
            .values()
            .filter(|e| e.src_task != exclude_task)
            .filter_map(|e| e.tx.as_ref())
            .filter(|tx| tx.send(frame.clone()).is_ok())
            .count()
    }

    /// 快照按 **task 升序**（不是名字序）：状态广播与运维输出处要靠它对齐 task 号，
    /// 名字序纯属 `BTreeMap<String,_>` 的顺带结果，故必须显式排。
    pub fn rr3_reg_snapshot(&self) -> Vec<(String, u64, Rr3Phase)> {
        let mut v: Vec<(String, u64, Rr3Phase)> =
            self.modules.values().map(|e| (e.name.clone(), e.src_task, e.phase)).collect();
        v.sort_by_key(|(_, t, _)| *t);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::rr3_manifest_facts_from_text;
    use std::sync::mpsc;

    fn facts(name: &str, hb: u64, wd: u64, policy: &str) -> Rr3ManifestFacts {
        let text = format!(
            r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/{name}"

[events]
publishes = []
subscribes = []

[resources]
restart_policy = "{policy}"

[health]
heartbeat_ms = {hb}
watchdog_timeout_ms = {wd}
"#
        );
        rr3_manifest_facts_from_text(&text).expect("测试 manifest 必须可读")
    }

    fn chan() -> (Sender<Rr3Frame>, std::sync::mpsc::Receiver<Rr3Frame>) {
        mpsc::channel()
    }

    /// 分配从 2 起单调递增，0/1 永不出现；快照按 task 升序
    #[test]
    fn task_ids_start_at_two_and_increase() {
        let mut r = RrdRegistry::new(1000);
        let a = r.rr3_reg_add(&facts("rr3-sima", 200, 1000, "always"), chan().0, 0).unwrap();
        let b = r.rr3_reg_add(&facts("rr3-simb", 200, 1000, "always"), chan().0, 0).unwrap();
        assert_eq!(a, rr_bus::RR3_TASK_FIRST_MODULE);
        assert_eq!(b, a + 1);
        assert_eq!(r.rr3_reg_snapshot(), vec![("rr3-sima".into(), 2, Rr3Phase::Registered), ("rr3-simb".into(), 3, Rr3Phase::Registered)]);
    }

    /// 快照的序是 task 升序而非名字升序：先注册 z 再注册 a，顺序必须跟着 task 走
    /// （少了 sort_by_key 这一行，`BTreeMap<String,_>` 的迭代序会把名字序冒充成 task 序）
    #[test]
    fn snapshot_orders_by_task_not_by_name() {
        let mut r = RrdRegistry::new(1000);
        let z = r.rr3_reg_add(&facts("rr3-zeta", 200, 1000, "always"), chan().0, 0).unwrap();
        let a = r.rr3_reg_add(&facts("rr3-alpha", 200, 1000, "always"), chan().0, 0).unwrap();
        assert!(a > z, "后注册的号必须更大");
        assert_eq!(
            r.rr3_reg_snapshot(),
            vec![("rr3-zeta".into(), z, Rr3Phase::Registered), ("rr3-alpha".into(), a, Rr3Phase::Registered)],
            "名字序（alpha, zeta）与 task 序（zeta, alpha）相反，才能抓出没排序的实现"
        );
    }

    /// 四个阶段名与 `as_str()` 一对一：下行状态广播与集成测试按这些字面量比对
    #[test]
    fn phase_names_are_the_wire_vocabulary() {
        assert_eq!(Rr3Phase::Registered.as_str(), "registered");
        assert_eq!(Rr3Phase::Running.as_str(), "running");
        assert_eq!(Rr3Phase::Degraded.as_str(), "degraded");
        assert_eq!(Rr3Phase::Failed.as_str(), "failed");
        // 四条必须是互不相同的字符串，否则状态广播会把两种状态发成同一个词
        let names = [
            Rr3Phase::Registered.as_str(),
            Rr3Phase::Running.as_str(),
            Rr3Phase::Degraded.as_str(),
            Rr3Phase::Failed.as_str(),
        ];
        let mut sorted: Vec<&str> = names.into_iter().collect();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "阶段名互不相同: {names:?}");
    }

    /// 注册拒绝的三条文案逐字钉住（运维日志与 Task 10 的 wire 用例按字面比对）
    #[test]
    fn register_error_display_is_verbatim() {
        assert_eq!(
            Rr3RegisterError::Duplicate { name: "rr3-sima".into(), src_task: 7 }.to_string(),
            "模块 rr3-sima 已在册（task=7），拒绝重复注册"
        );
        assert_eq!(
            Rr3RegisterError::ReservedName { name: "rr3-runtime".into() }.to_string(),
            "模块名 rr3-runtime 属内建命名空间保留号，拒绝注册"
        );
        assert_eq!(
            Rr3RegisterError::Capacity { limit: RR3_REG_MAX_MODULES }.to_string(),
            "在册模块已达上限 16，拒绝注册"
        );
    }

    /// 三条拒绝文案各自点名自己的关键信息：冒充点名性质、越权点名 owner+sender、
    /// 未点名主题点名主题（Task 10 靠这些子串区分被哪一层拒的）
    #[test]
    fn publish_deny_display_distinguishes_the_three_layers() {
        let d = Rr3PublishDeny::ReservedNamespace { topic: "rr3-runtime/module/state".into() };
        assert_eq!(
            d.to_string(),
            "拒绝 rr3-runtime/module/state：rr3-runtime/* 是守护进程的下行广播，模块不得冒充"
        );
        let f = Rr3PublishDeny::ForeignPrefix {
            topic: "rr3-simb/pong/state".into(),
            owner: "rr3-simb".into(),
            sender: "rr3-sima".into(),
        };
        assert_eq!(f.to_string(), "越权发布 rr3-simb/pong/state（owner=rr3-simb，发送方=rr3-sima）");
        assert_eq!(
            Rr3PublishDeny::NotRegistered { topic: "rr3-sima/x/state".into() }.to_string(),
            "未注册主题 rr3-sima/x/state，拒绝发布"
        );
    }

    /// 同名在册必须拒，且不留影子条目
    #[test]
    fn duplicate_name_is_rejected() {
        let mut r = RrdRegistry::new(1000);
        let t = r.rr3_reg_add(&facts("rr3-sima", 200, 1000, "always"), chan().0, 0).unwrap();
        let e = r
            .rr3_reg_add(&facts("rr3-sima", 200, 1000, "always"), chan().0, 0)
            .expect_err("同名在册必须拒");
        assert_eq!(e, Rr3RegisterError::Duplicate { name: "rr3-sima".into(), src_task: t });
        assert_eq!(r.rr3_reg_snapshot().len(), 1);
        assert_eq!(e.to_string(), "模块 rr3-sima 已在册（task=2），拒绝重复注册");
        // 拒重复不许改已在册条目的状态，也不许换掉它的 task 号
        assert_eq!(r.rr3_reg_snapshot()[0], ("rr3-sima".to_string(), t, Rr3Phase::Registered));
        assert_eq!(r.rr3_reg_name_of(t).as_deref(), Some("rr3-sima"));
    }

    /// rr3-runtime 是 rrd 自己的号段，模块冒充即最危险的伪造面
    #[test]
    fn reserved_module_name_is_rejected() {
        let mut r = RrdRegistry::new(1000);
        assert_eq!(
            r.rr3_reg_add(&facts("rr3-runtime", 200, 1000, "always"), chan().0, 0),
            Err(Rr3RegisterError::ReservedName { name: "rr3-runtime".into() })
        );
        assert!(r.rr3_reg_snapshot().is_empty());
    }

    #[test]
    fn capacity_limit_is_enforced() {
        let mut r = RrdRegistry::new(1000);
        for i in 0..RR3_REG_MAX_MODULES {
            r.rr3_reg_add(&facts(&format!("rr3-m{i}"), 200, 1000, "never"), chan().0, 0).unwrap();
        }
        assert_eq!(
            r.rr3_reg_add(&facts("rr3-over", 200, 1000, "never"), chan().0, 0),
            Err(Rr3RegisterError::Capacity { limit: RR3_REG_MAX_MODULES })
        );
        // 超额拒绝不许留下半个条目，也不许消耗一个 task 号
        assert_eq!(r.rr3_reg_snapshot().len(), RR3_REG_MAX_MODULES);
        assert_eq!(r.rr3_reg_name_of(rr_bus::RR3_TASK_FIRST_MODULE + RR3_REG_MAX_MODULES as u64), None);
    }

    /// 注册即 registered；收到心跳才 running；未在册 task 的心跳被拒
    #[test]
    fn heartbeat_promotes_registered_to_running() {
        let mut r = RrdRegistry::new(1000);
        let t = r.rr3_reg_add(&facts("rr3-sima", 200, 1000, "always"), chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Registered);
        assert!(r.rr3_reg_beat(t, 100));
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Running);
        assert!(!r.rr3_reg_beat(99, 200), "未在册 task 的心跳必须拒");
    }

    /// 到期完全由显式毫秒驱动：窗口内不判死，超窗口判死转 failed，此后喂狗无效
    #[test]
    fn expire_uses_explicit_monotonic_millis() {
        let mut r = RrdRegistry::new(1000);
        let t = r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        assert!(r.rr3_reg_beat(t, 400));
        assert!(r.rr3_reg_expire(499).is_empty(), "距上次心跳 499-400=99ms < 500ms 不得判死");
        assert_eq!(
            r.rr3_reg_expire(901),
            vec![Rr3Expiry { name: "rr3-sima".into(), src_task: t, reason: "heartbeat_timeout" }]
        );
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
        assert!(!r.rr3_reg_beat(t, 950), "已 failed 的模块不得靠补心跳复活");
    }

    /// registered（一颗心跳都没拿到）同样受 watchdog 约束
    #[test]
    fn registered_without_any_beat_still_expires() {
        let mut r = RrdRegistry::new(1000);
        let t = r.rr3_reg_add(&facts("rr3-sima", 100, 300, "always"), chan().0, 0).unwrap();
        assert!(r.rr3_reg_expire(299).is_empty());
        assert_eq!(r.rr3_reg_expire(300)[0].src_task, t, "从注册时刻起算");
    }

    /// 到期扫描不得牵连健康模块（唯一监视线程可安全每 tick 全表扫）
    #[test]
    fn expire_only_reports_stale_modules() {
        let mut r = RrdRegistry::new(1000);
        let a = r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        r.rr3_reg_add(&facts("rr3-simb", 100, 5000, "always"), chan().0, 0).unwrap();
        r.rr3_reg_beat(a, 1000);
        let e = r.rr3_reg_expire(1400);
        assert_eq!(e.len(), 0, "simb 窗口 5000ms 未到、sima 刚喂过");
        // sima 继续喂狗（它自己的窗口是 500ms，不喂它就该和 simb 一起被报）
        r.rr3_reg_beat(a, 5900);
        let e2 = r.rr3_reg_expire(6000);
        assert_eq!(e2.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), vec!["rr3-simb"]);
        // 只有 simb 被判死：sima 仍活着且句柄未被牵连
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Running, "健康模块不得被邻居超时改掉状态");
        assert_eq!(r.rr3_reg_expire(6200).len(), 0, "已 failed 的条目不得重复回报");
    }

    /// 越权发布：前缀不等于注册名即拒，文案带 owner/sender
    #[test]
    fn publish_rejects_foreign_prefix() {
        let set = topic_set(&[("rr3-sima/ping/state", "rr3-sima"), ("rr3-simb/pong/state", "rr3-simb")]);
        assert!(RrdRegistry::rr3_reg_publish_check("rr3-sima", "rr3-sima/ping/state", &set).is_ok());
        assert_eq!(
            RrdRegistry::rr3_reg_publish_check("rr3-sima", "rr3-simb/pong/state", &set)
                .expect_err("越权必须拒"),
            Rr3PublishDeny::ForeignPrefix {
                topic: "rr3-simb/pong/state".into(),
                owner: "rr3-simb".into(),
                sender: "rr3-sima".into(),
            }
        );
    }

    /// 主题名少于两段（`rr3-sima` 独占一段）时前缀仍须等于注册名，越权面不因段数缩小
    #[test]
    fn publish_rejects_foreign_prefix_with_single_segment_topic() {
        let set = topic_set(&[("rr3-sima/ping/state", "rr3-sima"), ("rr3-simb/pong/state", "rr3-simb")]);
        assert_eq!(
            RrdRegistry::rr3_reg_publish_check("rr3-sima", "rr3-simb", &set),
            Err(Rr3PublishDeny::ForeignPrefix {
                topic: "rr3-simb".into(),
                owner: "?".into(),
                sender: "rr3-sima".into(),
            })
        );
    }

    /// 冒充内建命名空间优先于一切判定（哪怕该主题真在主题集里）
    #[test]
    fn publish_rejects_runtime_impersonation_first() {
        let set = topic_set(&[("rr3-runtime/module/state", "rr3-runtime")]);
        assert_eq!(
            RrdRegistry::rr3_reg_publish_check("rr3-sima", "rr3-runtime/module/state", &set)
                .expect_err("下行广播冒充必须拒"),
            Rr3PublishDeny::ReservedNamespace { topic: "rr3-runtime/module/state".into() }
        );
        // 该主题既在册、owner 又是 rr3-runtime：若顺序颠倒会先撞上"在册"或"前缀"两层，
        // 得到 Ok / ForeignPrefix 而不是"冒充"这条定性
        assert!(set.rr3_topic_set_contains("rr3-runtime/module/state"));
    }

    /// `rr3-runtime/*` 对任何模块都是禁区，与它在主题集里有没有条目无关
    #[test]
    fn publish_rejects_reserved_namespace_whatever_the_sender_claims() {
        let set = topic_set(&[("rr3-sima/ping/state", "rr3-sima")]);
        assert_eq!(
            RrdRegistry::rr3_reg_publish_check("rr3-sima", "rr3-runtime/system/heartbeat", &set),
            Err(Rr3PublishDeny::ReservedNamespace { topic: "rr3-runtime/system/heartbeat".into() })
        );
        assert_eq!(
            RrdRegistry::rr3_reg_publish_check("rr3-sima", "rr3-runtime", &set),
            Err(Rr3PublishDeny::ReservedNamespace { topic: "rr3-runtime".into() })
        );
    }

    /// 前缀对但没在册（拼错域名）也必须拒
    #[test]
    fn publish_rejects_unregistered_topic() {
        let set = topic_set(&[("rr3-sima/ping/state", "rr3-sima")]);
        assert_eq!(
            RrdRegistry::rr3_reg_publish_check("rr3-sima", "rr3-sima/ping/others", &set),
            Err(Rr3PublishDeny::NotRegistered { topic: "rr3-sima/ping/others".into() })
        );
    }

    /// 越权只计数，不影响在册状态与后续合法发布
    #[test]
    fn denied_publish_is_counted_not_fatal() {
        let mut r = RrdRegistry::new(1000);
        r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        r.rr3_reg_note_denied();
        r.rr3_reg_note_denied();
        assert_eq!(r.rr3_reg_denied(), 2);
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Registered, "越权不改生命周期状态");
        assert!(r.rr3_reg_live("rr3-sima"), "越权不得掐掉自己的连接");
        assert_eq!(r.rr3_reg_expire(100).len(), 0, "计数不改动到期基准");
    }

    /// never：死亡即 failed，不给重启计划
    #[test]
    fn never_policy_never_restarts() {
        let mut r = RrdRegistry::new(1000);
        r.rr3_reg_add(&facts("rr3-simb", 100, 500, "never"), chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_death("rr3-simb", false, 600), None);
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
    }

    /// always：按退避表给延迟；连续到上限即停止
    #[test]
    fn always_policy_backs_off_and_caps() {
        let mut r = RrdRegistry::new(1000);
        r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        let mut delays = Vec::new();
        for i in 0..RR3_RESTART_FAILURE_CAP {
            let p = r.rr3_reg_death("rr3-sima", false, 600).expect("未达上限必须给计划");
            assert_eq!(p.attempt, i + 1);
            assert_eq!(p.entry, "/usr/bin/rr3-sima");
            delays.push(p.delay_ms);
        }
        assert_eq!(delays, vec![1000, 2000, 4000, 8000, 16000], "退避必须按表 × base");
        assert_eq!(r.rr3_reg_death("rr3-sima", false, 600), None, "达上限必须停止重启");
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
    }

    /// 退避乘法是纯算术：base 大到会溢出时封顶，绝不 panic（debug 的 `*` 会 panic）
    /// 也绝不回绕（release 的 `*` 会 wrap 成一个小延迟＝"疯狂重启"，正是 spec §5.3 要避免的）
    #[test]
    fn backoff_multiply_saturates_instead_of_wrapping() {
        let base = u64::MAX / 2;
        let mut r = RrdRegistry::new(base);
        r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_death("rr3-sima", false, 600).expect("第 1 次有计划").delay_ms, base);
        // 2×base 仍可表示，必须给出精确值（封顶实现若写成"一律 u64::MAX"在这里就红）
        assert_eq!(
            r.rr3_reg_death("rr3-sima", false, 600).expect("第 2 次有计划").delay_ms,
            base.checked_mul(2).expect("前提：2×base 不溢出")
        );
        // 第 3 次是 4×base，溢出 → 封顶（回绕的话会拿到一个很小的数）
        assert_eq!(r.rr3_reg_death("rr3-sima", false, 600).expect("第 3 次有计划").delay_ms, u64::MAX);
    }

    /// on-failure：干净退出不重启，非零/crash 才重启
    #[test]
    fn on_failure_only_restarts_unclean_exit() {
        let mut r = RrdRegistry::new(1000);
        r.rr3_reg_add(&facts("rr3-sense", 100, 500, "on-failure"), chan().0, 0).unwrap();
        assert!(r.rr3_reg_death("rr3-sense", true, 600).is_none(), "干净退出不得重启");
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
        assert!(r.rr3_reg_remove("rr3-sense"));
        r.rr3_reg_add(&facts("rr3-sense", 100, 500, "on-failure"), chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_death("rr3-sense", false, 600).expect("非零退出必须给计划").delay_ms, 1000);
    }

    /// 未知策略值不得 panic、也不得擅自重启（校验在 Task 2 已挡，运行期只保证不猜）
    #[test]
    fn unexpected_policy_string_is_treated_as_never() {
        let mut r = RrdRegistry::new(1000);
        let mut f = facts("rr3-sima", 100, 500, "always");
        f.restart_policy = "ALWAYS".into(); // 大小写错 = 非预期值
        r.rr3_reg_add(&f, chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_death("rr3-sima", false, 600), None, "非预期值必须按 never 处理");
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
    }

    /// 不在册的名字问死亡：None 且不得凭空造条目
    #[test]
    fn death_of_unknown_name_is_none() {
        let mut r = RrdRegistry::new(1000);
        r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_death("rr3-nobody", false, 600), None);
        assert_eq!(r.rr3_reg_snapshot().len(), 1);
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Registered, "问错名字不得改掉别人的状态");
    }

    /// 退避表自证：7 档、末档 60、单调不减、无 0
    #[test]
    fn backoff_table_is_monotonic_with_cap() {
        assert_eq!(RR3_RESTART_BACKOFF.len(), 7);
        assert_eq!(*RR3_RESTART_BACKOFF.last().unwrap(), 60);
        assert!(RR3_RESTART_BACKOFF.windows(2).all(|w| w[0] <= w[1]));
        assert!(RR3_RESTART_BACKOFF.iter().all(|v| *v > 0));
    }

    /// failed 后同身份可重注册，但必须换新 task（旧号不复用）
    #[test]
    fn failed_entry_can_be_re_registered_with_new_task() {
        let mut r = RrdRegistry::new(1000);
        let t = r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        r.rr3_reg_death("rr3-sima", false, 600);
        assert!(!r.rr3_reg_live("rr3-sima"), "failed 即无活连接");
        let t2 = r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 700).expect("failed 后必须可重注册");
        assert_eq!(t2, t + 1, "重注册必须换新 task 号");
        assert_eq!(r.rr3_reg_snapshot().len(), 1);
        // 旧号作废：迟到的旧心跳既不能喂到新条目身上，也不能改名存实亡的旧条目
        assert_eq!(r.rr3_reg_name_of(t), None, "旧 task 号必须从映射里摘掉");
        assert!(!r.rr3_reg_beat(t, 800), "旧 task 号不得再被认成在册");
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Registered);
        assert!(r.rr3_reg_live("rr3-sima"));
    }

    /// fanout：跳过发起者；对端已 drop 只是少计，不 panic
    #[test]
    fn fanout_skips_sender_and_survives_dropped_peer() {
        let mut r = RrdRegistry::new(1000);
        let (txa, rxa) = chan();
        let (txb, rxb) = chan();
        let ta = r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), txa, 0).unwrap();
        r.rr3_reg_add(&facts("rr3-simb", 100, 500, "always"), txb, 0).unwrap();
        let frame = Rr3Frame {
            header: rr_ipc::Rr3Header::rr3_hdr_broadcast(rr_ipc::opcode::SEND, 0),
            payload: vec![],
        };
        assert_eq!(r.rr3_reg_fanout(&frame, ta), 1, "只发给非发起者");
        assert!(rxa.try_recv().is_err());
        assert!(rxb.try_recv().is_ok());
        assert_eq!(
            r.rr3_reg_fanout(&frame, rr_bus::RR3_BROADCAST_DST_TASK),
            2,
            "0 不是任何模块的 task，两人都该收到"
        );
        assert!(rxa.try_recv().is_ok(), "上一行里发起者也不能被排除");
        drop(rxb);
        assert_eq!(r.rr3_reg_fanout(&frame, ta), 0);
    }

    /// 到期后连接句柄被丢掉：之后 fanout 不再往死连接塞
    #[test]
    fn expired_module_is_excluded_from_fanout() {
        let mut r = RrdRegistry::new(1000);
        let (txa, rxa) = chan();
        let (txb, _rxb) = chan();
        r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), txa, 0).unwrap();
        let b = r.rr3_reg_add(&facts("rr3-simb", 100, 300, "always"), txb, 0).unwrap();
        r.rr3_reg_expire(400);
        assert_eq!(r.rr3_reg_fanout(&frame_to(b), 0), 1, "只剩 sima 活着");
        assert!(rxa.try_recv().is_ok());
    }

    /// 移除后既不在快照里也不再是 fanout 目标，task 号也不回收
    #[test]
    fn removed_module_leaves_the_table_entirely() {
        let mut r = RrdRegistry::new(1000);
        let (txa, rxa) = chan();
        let a = r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), txa, 0).unwrap();
        assert!(r.rr3_reg_remove("rr3-sima"));
        assert!(!r.rr3_reg_remove("rr3-sima"), "重复移除必须返回 false");
        assert!(!r.rr3_reg_live("rr3-sima"));
        assert_eq!(r.rr3_reg_name_of(a), None);
        assert!(r.rr3_reg_snapshot().is_empty());
        let frame = frame_to(rr_bus::RR3_BROADCAST_DST_TASK);
        assert_eq!(r.rr3_reg_fanout(&frame, 0), 0, "已移除的连接不得再被投递");
        assert!(rxa.try_recv().is_err(), "死条目还留着句柄就会被塞消息");
    }

    /// 小测试辅助：主题集
    fn topic_set(entries: &[(&str, &str)]) -> Rr3TopicSet {
        use std::collections::BTreeSet;
        let mut owners: BTreeSet<&str> = BTreeSet::new();
        for (_, o) in entries {
            owners.insert(o);
        }
        let mut text = String::from("schema_version = 1\n");
        for o in &owners {
            text.push_str(&format!("\n[[owner]]\nname = \"{o}\"\n"));
        }
        for (t, o) in entries {
            text.push_str(&format!("\n[[topic]]\nname = \"{t}\"\nkind = \"event\"\nowner = \"{o}\"\n"));
        }
        let set = rr_registry::rr3_registry_topic_set_from_text(&text);
        assert!(set.rr3_topic_set_check().is_empty(), "测试主题集自身必须合法: {:?}", set.rr3_topic_set_check());
        set
    }

    fn frame_to(dst: u64) -> Rr3Frame {
        let mut h = rr_ipc::Rr3Header::rr3_hdr_broadcast(rr_ipc::opcode::SEND, 0);
        h.dst_task = dst;
        Rr3Frame { header: h, payload: vec![] }
    }
}
