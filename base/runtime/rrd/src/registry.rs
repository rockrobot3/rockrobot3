//! 在册模块表与生命周期状态机（architecture.md §4.2 的状态集，不新增状态）。
//!
//! 平台无关：只碰 BTreeMap / mpsc / u64 单调毫秒。UDS 与进程派生都在 daemon 层
//! （`conn.rs`/`daemon.rs`，全部 `#[cfg(unix)]`）：UDS 监听与注册握手已随 Task 7 落地，
//! 进程派生与重启执行随 Task 11b（Task 11a 只把崩溃记账在本表收口成唯一一本账，
//! 不碰线程、不碰 spawn）。因此本文件的测试在
//! Windows 上照跑——M2 里"rrd 的行为"有相当一部分在本机就有证据（L1）。
//!
//! 三条不变量（改动前先读）：
//! 1. **task 号只增不回收**：0 是广播、1 是 rrd，模块从 `RR3_TASK_FIRST_MODULE` 起分配；
//!    模块 failed / 被移除后它的旧号作废，绝不复用（否则一条迟到的旧心跳能顶替新连接）。
//! 2. **到期判定用显式单调毫秒**，不存 `Instant`：`Instant` 不能回溯，测试就无法驱动
//!    "刚好在窗口内 / 刚好超窗"两种情形（见本任务"时钟约定"）。
//! 3. **越权只计数不牵连**：一次坏发布既不改发布者生命周期状态，也不影响其它连接的转发。

use std::collections::{BTreeMap, BTreeSet};
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

/// 生命周期阶段。本文件**当前可发生**的转移只有三条：
/// `registered → running`（`rr3_reg_beat` 回报 `Rr3Beat::Promoted`，`running` 目前只落到
/// 日志面、上线广播随 Task 11b）、`registered | running → failed`
/// （`rr3_reg_expire` 超时、`rr3_reg_death` 退出、`rr3_reg_child_exit` 收尸）。
/// `Degraded` 今天**没有任何置位路径**，`degraded → running` 那条边也就无从发生——
/// 它在此只是 spec §5.0 词表里的一个名字（`as_str()` 要回显它），不是已交付的状态。
/// **本轮终态里 degraded 是"有 enum 变体、无写入方"的死相位**：setter 与 base 能力门
/// （`depends.required` 的 `bus.publish`/`bus.subscribe`/`bus.rpc` 就位判定）都不在 11/11b，
/// 二者一个要新相位上线、一个要给 21 条生产主题建能力字典，都不阻塞 §9 验收。
/// `rr3_reg_beat` 现在遇 `Degraded` 归 `Kept`（只续命不升级），已把"一个心跳抹平降级"的欠账结清。
/// failed 之后同身份可重注册（换新 task 号），旧条目整体替换。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3Phase {
    /// 已注册、尚未收到任何心跳：此刻不判活（legacy 教训 76c0ad8）
    Registered,
    Running,
    /// 依赖的 base 能力未就位、或整体降级（setter 未落地：随 Task 9/11 与持久化一起）
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
    /// `owner` 回显主题集里登记的 owner；**取不到就回显 `?`**——主题根本不在册、或在册但
    /// 登记文本没写 owner（解析成空串）都算取不到，两条分支同一写法（两处构造点共用
    /// `owner_for_echo`），不留"一会儿空串一会儿 `?`"的两种未知。
    /// 两条都是**终判拒绝**（这一条发布已经被拒了），不是"owner 未知、待定再查"——
    /// 日志读者别把 `owner=?` 读成待办事项。
    ForeignPrefix { topic: String, owner: String, sender: String },
    /// "未注册"指的是**主题**不在主题集里（首段与发送方相符、名字却拼错或没登记），
    /// **不是**指发送方那个模块没在册：本函数是关联函数、不碰 `self`，看不见在册表。
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
    /// 本次入库（注册）的时刻：`rr3_reg_expire` 用它判"这一世有没有活满一整条自己的窗口"
    /// （D-2 恢复归零的基准，与判死用的 `last_beat_ms` 是两颗不同的基准，不许混用）。
    registered_at_ms: u64,
    tx: Option<Sender<Rr3Frame>>,
}

/// watchdog 到期回报。`elapsed_ms` / `watchdog_ms` 必须由**判定处**一并交出：
/// 线程侧要在日志里写清"距上次心跳 501ms ≥ 超时 500ms"，而表外的人拿不到 `last_beat_ms`
/// （`rr3_reg_facts` 只给 manifest 事实，不给心跳时刻）。少了这两个字段，日志就只能写
/// "判死了"，线上用例便无法把"判得对不对"钉成内容断言，只能靠掐时间——那是随机红的来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3Expiry {
    pub name: String,
    pub src_task: u64,
    pub reason: &'static str,
    pub elapsed_ms: u64,
    pub watchdog_ms: u64,
    /// 到期那一路的记账结果（D-8）：`always`/`on-failure` 都给计划，`never` 给 None。
    /// 11a 的 daemon 侧**只收下不使用**（`let _ = ex.restart;`）——真正接管它的是 11b 的
    /// supervisor；11a 若在这条路上打"重启计划"日志会当场打死 `e2e_watchdog` 的 W4。
    pub restart: Option<Rr3RestartPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3RestartPlan {
    pub name: String,
    pub attempt: u32,
    pub delay_ms: u64,
    pub entry: String,
}

/// 心跳的三态回报（D-6）：11b 要"广播一次 running"，而"一次"必须建立在
/// "这是注册后被接受的第一颗心跳"上，故 `rr3_reg_beat` 不能只回 bool。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3Beat {
    /// 不在册的 task，或已 failed：一条都不许给它升级
    Rejected,
    /// 已经是 running（或未来的 degraded）：只刷新到期基准
    Kept,
    /// registered → running：**只有这一条**让 conn 广播一次 state
    Promoted,
}

/// 死亡处理的终态回报（conn 收尾那一路）。三条公开路（conn EOF / watchdog 到期 / 子进程退出码）
/// 的记账都在 `rr3_charge` 汇成唯一一本账；本枚举只描述"调用方该不该广播、要不要等退出码"。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3Death {
    /// 本守护起的模块：conn 一句都不广播，退出码那一路负责整条终态
    DeferredToExitStatus,
    /// 已被 watchdog 判死：不重复广播、不重复记账（ledger R-A4 在这一条上结清）
    Silent,
    /// 刚由 live 转终态（或本就不在册 = unregister 后的收尾）：调用方须广播一条 state
    Broadcast { plan: Option<Rr3RestartPlan> },
}

impl Rr3Death {
    /// 取出重启计划：只有 `Broadcast` 携带计划，其余一律 None
    pub fn rr3_plan(self) -> Option<Rr3RestartPlan> {
        match self {
            Rr3Death::Broadcast { plan } => plan,
            _ => None,
        }
    }
}

/// 子进程退出码那一路的回报（11b 的 supervisor 收尸用；11a 只把表级记账钉死）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3ChildExit {
    /// 条目还活着：已置 failed 并记账，调用方按 `rr3_phase_for_exit(clean_exit)` 广播
    Crashed { src_task: u64, clean_exit: bool, plan: Option<Rr3RestartPlan> },
    /// 从未注册且已 stopped，或条目早已 failed：静默
    Silent,
    /// 子进程在注册成功之前就退出了：没有条目可广播，但已按 name 记过账
    EarlyExit { plan: Option<Rr3RestartPlan> },
}

/// 死因 → 下行 state 相位名的唯一映射（D-7）：conn 收尾与 11b 的 supervisor 两个广播点都必须
/// 经它。今天这条映射原来只写在 `conn.rs` 的 `if clean_exit` 里，11b 再加一个广播点就是两处真值。
pub fn rr3_phase_for_exit(clean_exit: bool) -> &'static str {
    if clean_exit { "unregistered" } else { "failed" }
}

/// `ForeignPrefix.owner` 的回显唯一写法：在册但登记文本没写 owner 键（解析成空串）与
/// 根本不在册（None）都渲染 `?`。`rr3_reg_publish_check` 的两处 `ForeignPrefix` 构造点
/// 都必须经它——任何一处绕过它就会把"同一种未知"写成两种回显（由
/// `foreign_prefix_owner_echo_is_question_mark_on_both_construction_sites` 钉住）。
fn owner_for_echo(registered: Option<String>) -> String {
    registered.filter(|o| !o.is_empty()).unwrap_or_else(|| "?".to_string())
}

#[derive(Debug)]
pub struct RrdRegistry {
    modules: BTreeMap<String, Entry>,
    by_task: BTreeMap<u64, String>,
    next_task: u64,
    restart_base_ms: u64,
    denied_publish: u32,
    /// 崩溃连击计数（D-1）：键是 **name**，所以重注册、换 task、条目被整体替换都不影响它。
    /// 唯一写入方是 `rr3_charge`；唯一清空方是 `rr3_reg_expire` 的恢复归零段（D-2）。
    /// 取代旧的 `Entry.restarts`（那条账随旧条目一起归零，导致 5 次上限永远撞不到）。
    crash_streak: BTreeMap<String, u32>,
    /// 所有权标记（D-3）：name ∈ owned ⟺ "这个进程是我起的，它的退出码才是死因的权威"。
    /// conn 见 EOF 时对 owned 的 name 不改相位、不记账、不广播，三件事一起让给 supervisor。
    owned: BTreeSet<String>,
    /// 显式退场标记（D-4）：`rr3_reg_remove`（unregister RPC）记进这里，`rr3_reg_add` 移出。
    /// 作用只有一个：已显式停掉的模块，其子进程后来退出时不得再被记账成一次崩溃。
    stopped: BTreeSet<String>,
}

impl RrdRegistry {
    pub fn new(restart_base_ms: u64) -> Self {
        Self {
            modules: BTreeMap::new(),
            by_task: BTreeMap::new(),
            next_task: rr_bus::RR3_TASK_FIRST_MODULE,
            restart_base_ms,
            denied_publish: 0,
            crash_streak: BTreeMap::new(),
            owned: BTreeSet::new(),
            stopped: BTreeSet::new(),
        }
    }

    /// 注册：保留名 → 同名在册（failed 除外）→ 超额 → 分配 task
    ///
    /// `entry` 的绝对性门禁不在这里（计划"与 spec 的偏离 6"：注册只拒语法，存在性/可执行
    /// 位随 spawn 那一刻查），那是 conn/daemon 层的职责：路径绝对性已随 Task 7 落地
    /// （`conn.rs` 的握手拒相对 entry、回 AIRY_EINVAL 并点名字段）；存在性/可执行位已随
    /// Task 11b 落地，但**只查本守护要 spawn 的那批**——`supervisor.rs` 的
    /// `rr3_boot_job_from_manifest` 在 boot 那一刻查（E-3，拒法是 `拒绝启动`）。走线上注册的
    /// 外来模块 rrd 永不 spawn 它（E-1），所以那条 entry 的存在性根本无人裁定，也不是本表的职责。
    pub fn rr3_reg_add(
        &mut self,
        facts: &Rr3ManifestFacts,
        tx: Sender<Rr3Frame>,
        now_ms: u64,
    ) -> Result<u64, Rr3RegisterError> {
        if facts.name == crate::topics_load::RR3_RESERVED_OWNER {
            return Err(Rr3RegisterError::ReservedName { name: facts.name.clone() });
        }
        // 重启后同身份重注册：旧条目整体替换，旧 task 号作废但不回收。
        // `crash_streak` 是**表级**、按 name 计的账（D-1），不随旧条目一起作废——"连续 5 次"
        // 的预算正是靠这一条才跨得起重注册；旧代码把 `restarts` 写进 Entry 又被这次替换归零，
        // 是 5 次上限今天撞不到的根因（见 `crash_budget_survives_reregistration_and_caps_after_five_incarnations`）。
        // 这里只**记下**待替换条目的 task 号，真正的摘除延后到门+断言全过之后：
        // 被 catch_unwind 吞掉的 debug panic（坏 watchdog）不许留下已改一半的表
        // （`caught_panic_leaves_the_failed_entry_registered` 钉住）。
        let mut superseded: Option<u64> = None;
        if let Some(old) = self.modules.get(&facts.name) {
            if old.phase != Rr3Phase::Failed {
                return Err(Rr3RegisterError::Duplicate {
                    name: facts.name.clone(),
                    src_task: old.src_task,
                });
            }
            superseded = Some(old.src_task);
        }
        // 容量门认 superseded：表满但同名条目是 Failed 时，旧条目即将腾出槽位，
        // 这是合法的 Failed 重注册，不得被容量拒（与移位前的现行行为一致）。
        if superseded.is_none() && self.modules.len() >= RR3_REG_MAX_MODULES {
            return Err(Rr3RegisterError::Capacity { limit: RR3_REG_MAX_MODULES });
        }
        // 入库前提：watchdog_ms 必须为正，否则 `now_ms - last_beat_ms >= watchdog_ms` 恒成立，
        // 首轮到期扫描就把所有模块秒判死。`rr3_manifest_facts_from_text` 只在**文本**读取期
        // 拒非正整数（`manifest.rs` 的 `req_int`），而 `Rr3ManifestFacts` 字段全 `pub`，
        // 直接构造的 facts 绕得过那道校验，故这里再钉一次。
        // 用 debug_assert 而不是返回 Err：绕过 facts 读取入口是调用方的编程错误，而
        // `Rr3RegisterError` 的三个变体是冻结接口，不能为它加第四个；release 侧的下界已有归属：
        // `watchdog_ms` 只能从 manifest 文本来，而 `manifest.rs` 的 `req_int` 在**读取期**（不分
        // debug/release）就拒非正整数，CLI 面也没有 watchdog 这个旋钮可绕——本表不替它裁定语义。
        // 位置在**三条拒绝门之后**，让本函数文档里那句检查序在 debug 构建里也成立：断言
        // 抢在前面的话，一个带坏 watchdog 的重复/保留名/超额注册会先 panic 而不是如实返回
        // Err，调用方拿不到拒绝理由（Task 6 的 `rr3_rpc_status_for_register` 便无从映射——
        // 那个调用方已随 Task 7 落地，就是 `conn.rs` 注册被拒时走的 `rr3_refuse`）。
        debug_assert!(
            facts.watchdog_ms > 0,
            "watchdog_ms = 0 的模块 {} 一入库就会被到期扫描判死",
            facts.name
        );
        // 到这里门与断言全过，才动表：先摘旧 Failed 条目（task→name 映射与条目本体），
        // 再插入新条目。panic 路径在上面就返回了，走不到这一步。
        if let Some(t) = superseded {
            self.by_task.remove(&t);
            self.modules.remove(&facts.name);
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
                phase: Rr3Phase::Registered,
                facts: facts.clone(),
                last_beat_ms: now_ms,
                registered_at_ms: now_ms,
                tx: Some(tx),
            },
        );
        Ok(task)
    }

    /// 心跳：`registered → running` 的唯一入口（回报 `Promoted`），并刷新到期基准。
    /// 未在册的 task、以及已 failed 的条目一律 `Rejected`（failed 的模块不得靠补心跳复活自己）。
    /// 已经是 `Running` / `Degraded` 的只 `Kept`（续命、不升级）——`Degraded` 归 Kept 这一支
    /// 把 registry.rs:254-260 那段"非 failed 无条件写 Running"的欠账当场结掉：degraded 落地时
    /// 一颗心跳不许把降级标注抹平。
    pub fn rr3_reg_beat(&mut self, src_task: u64, now_ms: u64) -> Rr3Beat {
        let Some(name) = self.by_task.get(&src_task).cloned() else { return Rr3Beat::Rejected };
        let Some(e) = self.modules.get_mut(&name) else { return Rr3Beat::Rejected };
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
            if phase == Rr3Phase::Running && now_ms.saturating_sub(registered_at_ms) >= watchdog_ms {
                self.crash_streak.remove(&name);
            }
            // 段 3：判死。saturating_sub：调用方的毫秒读数理论上单调，但真出现倒退时宁可判
            // "未到期"也不回绕成一个巨大差值把健康模块集体判死。差值提成绑定：判据与回报共用它。
            let elapsed_ms = now_ms.saturating_sub(last_beat_ms);
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
        out
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
    /// `rr3_topic_set_owner(topic) == sender_name` 才是**在册表登记的事实**。二者今天恒等，
    /// 是因为 `rr3_topic_set_check` 强制"首段 == owner"，而那条前置只在 `rr3_topics_load`
    /// 的装载路径上被强制；`rr3_registry_topic_set_from_text` 本身是 public，绕开 check
    /// 造一张"首段是 rr3-sima、登记 owner 是 rr3-simb"的表并不费力。安全判定不该押在
    /// "上游一定查过"上，故这里就地把两条都查一遍（多一次 O(log n) 查表，无副作用）。
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
                owner: owner_for_echo(topics.rr3_topic_set_owner(topic)),
                sender: sender_name.to_string(),
            });
        }
        if !topics.rr3_topic_set_contains(topic) {
            return Err(Rr3PublishDeny::NotRegistered { topic: topic.to_string() });
        }
        // 首段相符只说明主题串"写着是这个命名空间"；在册 owner 是别人就是越权。
        // 取不到 owner（登记表里 owner 缺失 = 没写这个键，解析成空串）同样按越权拒，
        // 绝不因"查不到"而放行；回显与另一条 ForeignPrefix 分支一致地写 `?`（同走
        // `owner_for_echo`）。
        let owner = topics.rr3_topic_set_owner(topic);
        if owner.as_deref() != Some(sender_name) {
            return Err(Rr3PublishDeny::ForeignPrefix {
                topic: topic.to_string(),
                owner: owner_for_echo(owner),
                sender: sender_name.to_string(),
            });
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

    /// 唯一的记账处（D-1）：`rr3_reg_death` / `rr3_reg_expire` / `rr3_reg_child_exit` 三路共用，
    /// 别处一律不许碰 `crash_streak`。返回重启计划或 None。
    /// 先判后加：策略说不重启、或已达上限 → 直接 None，**不增长** streak
    /// （与今天 `restarts` 的次序一致——达上限不再往里加，也不给计划）。
    fn rr3_charge(&mut self, facts: &Rr3ManifestFacts, clean_exit: bool) -> Option<Rr3RestartPlan> {
        let restart = match facts.restart_policy.as_str() {
            "always" => true,
            "on-failure" => !clean_exit,
            _ => false, // "never" 与非预期值（校验已在 Task 2 挡住，这里只保证不 panic）
        };
        let streak = self.crash_streak.get(facts.name.as_str()).copied().unwrap_or(0);
        if !restart || streak >= RR3_RESTART_FAILURE_CAP {
            return None;
        }
        let attempt = streak + 1;
        self.crash_streak.insert(facts.name.clone(), attempt);
        let idx = (attempt as usize - 1).min(RR3_RESTART_BACKOFF.len() - 1);
        Some(Rr3RestartPlan {
            name: facts.name.clone(),
            attempt,
            // 退避倍数 × base：溢出既不 panic（debug 的 `*` 会 panic）也不回绕
            // （release 的 `*` 会 wrap），封顶到 u64::MAX 让"永远等下去"成为唯一保守解。
            // `restart_base_ms = 0` 的下界语义（→ 立即重启）已由 Task 7 的 CLI 门禁拦住
            // （`cli.rs::parse_restart_base`，提交 f69eda8：拒 0、拒超上界），本表不替它裁定语义。
            delay_ms: RR3_RESTART_BACKOFF[idx].checked_mul(self.restart_base_ms).unwrap_or(u64::MAX),
            entry: facts.entry.clone(),
        })
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
        match self.modules.get(name).map(|e| e.phase) {
            Some(Rr3Phase::Failed) => return Rr3Death::Silent,
            None => return Rr3Death::Broadcast { plan: None }, // unregister 之后的收尾：照今天广播 unregistered
            Some(_) if self.owned.contains(name) => return Rr3Death::DeferredToExitStatus,
            Some(_) => {}
        }
        {
            let e = self.modules.get_mut(name).expect("上面确认过条目在");
            e.tx = None;
            e.phase = Rr3Phase::Failed;
        }
        // 两段式借用：`&mut self.modules` 的守卫不能越过 `&mut self` 的方法调用活着
        let facts = self.modules.get(name).expect("刚改过相位").facts.clone();
        Rr3Death::Broadcast { plan: self.rr3_charge(&facts, clean_exit) }
    }

    pub fn rr3_reg_remove(&mut self, name: &str) -> bool {
        match self.modules.remove(name) {
            Some(e) => {
                self.by_task.remove(&e.src_task);
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
        let present = self.modules.get(facts.name.as_str()).map(|e| (e.phase, e.src_task));
        match present {
            None => {
                // 不在册：若被显式 stop 过（D-4），它的退出不是崩溃，不得记账。
                if self.stopped.contains(facts.name.as_str()) {
                    Rr3ChildExit::Silent
                } else {
                    // 从没注册过 → 没有条目可广播，但账必须按 name 记（坏 entry 不能空转重起）。
                    Rr3ChildExit::EarlyExit { plan: self.rr3_charge(facts, clean_exit) }
                }
            }
            // 已被 watchdog 判死：收尸不得双计（R-A4 的封印）。
            Some((Rr3Phase::Failed, _)) => Rr3ChildExit::Silent,
            // 在册且活着：相位、句柄、账都在这一次到位。
            Some((_, src_task)) => {
                let e = self.modules.get_mut(facts.name.as_str()).expect("刚确认在册");
                e.tx = None;
                e.phase = Rr3Phase::Failed;
                Rr3ChildExit::Crashed {
                    src_task,
                    clean_exit,
                    plan: self.rr3_charge(facts, clean_exit),
                }
            }
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

    /// 取某模块的 facts（Task 11 的 spawn 序要用 start_priority 与 entry）
    pub fn rr3_reg_facts(&self, name: &str) -> Option<Rr3ManifestFacts> {
        self.modules.get(name).map(|e| e.facts.clone())
    }

    /// 投给除 `exclude_task` 外的所有**活**连接；返回成功入队数。接收者已 drop 不算错。
    /// **只做广播，不看 `frame.header.dst_task`**：该字段原样带上、本函数不读它，所以
    /// 它**不是**单播入口（签名冻结，不改）——`expired_module_is_excluded_from_fanout`
    /// 就把这条事实编码了：帧的 dst 写的是已判死的 simb，实际收到的是 sima。
    /// 要点名发给某一个模块，请由 conn 层直接用那条连接的 `Sender`，别指望本函数过滤。
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

    /// 容量门的那半句 `superseded.is_none() &&` **承重**（Task 7 评审遗留 ②）：表满但其中一个
    /// 同名条目已是 Failed 时，它即将被整体替换、不占新槽位，这是合法的 Failed 重注册，
    /// 绝不会被容量拒。Task 5 的 17 组变异实验里，删掉这半句后"表满仍可重注册 Failed"
    /// 立刻变成 `Capacity` 拒而当时全绿 —— 因为没有任何用例把两件事同时凑齐（表满 + 同名 Failed）。
    /// 这里就把那个组合钉住，并带上两个对照：新名字在满表上仍被拒（门没被整条删掉），
    /// 未 Failed 的同名仍是 `Duplicate` 而不是容量错（门的序也没换）。
    #[test]
    fn full_table_still_allows_re_registering_a_failed_module() {
        let mut r = RrdRegistry::new(1000);
        for i in 0..RR3_REG_MAX_MODULES {
            r.rr3_reg_add(&facts(&format!("rr3-m{i}"), 200, 1000, "never"), chan().0, 0).unwrap();
        }
        // 对照 2：满表上的**新**名字必须仍被容量拒
        assert_eq!(
            r.rr3_reg_add(&facts("rr3-over", 200, 1000, "never"), chan().0, 0),
            Err(Rr3RegisterError::Capacity { limit: RR3_REG_MAX_MODULES }),
            "对照：容量门本身不许被整条删掉"
        );
        // 对照 3：满表上的同名**非 Failed** 条目仍按 Duplicate 拒（三条门的序：在册先于容量）
        assert_eq!(
            r.rr3_reg_add(&facts("rr3-m0", 200, 1000, "never"), chan().0, 0),
            Err(Rr3RegisterError::Duplicate { name: "rr3-m0".into(), src_task: rr_bus::RR3_TASK_FIRST_MODULE }),
            "对照：同名在册该回 Duplicate，不是 Capacity"
        );

        let old = rr_bus::RR3_TASK_FIRST_MODULE;
        assert!(r.rr3_reg_death("rr3-m0", true, 10).rr3_plan().is_none(), "never 不给重启计划");
        assert_eq!(r.rr3_reg_snapshot().len(), RR3_REG_MAX_MODULES, "failed 不出表，槽位数不变");
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed, "用例前提：待替换条目确为 Failed");

        // 本用例的主角：表满 + 同名 Failed → 必须放行换新 task，而不是回 Capacity
        let again = r
            .rr3_reg_add(&facts("rr3-m0", 200, 1000, "never"), chan().0, 20)
            .expect("表满时重注册 Failed 的同名条目必须放行（容量门的 superseded 半句）");
        assert_eq!(again, old + RR3_REG_MAX_MODULES as u64, "重注册拿下一个新号，旧号作废不回收");
        assert_eq!(r.rr3_reg_snapshot().len(), RR3_REG_MAX_MODULES, "替换而非追加：槽位数不得增长");
        assert_eq!(r.rr3_reg_name_of(old), None, "旧 task 号必须从映射里摘掉");
        assert_eq!(r.rr3_reg_name_of(again).as_deref(), Some("rr3-m0"));
        assert_eq!(
            r.rr3_reg_snapshot().iter().filter(|(n, _, _)| n == "rr3-m0").count(),
            1,
            "同名条目不得同时存在两条"
        );
        assert!(r.rr3_reg_live("rr3-m0"), "重注册之后它又是一条活连接");
    }

    /// 注册期不许把 `watchdog_ms = 0` 收进表里：那样 `now - last_beat >= 0` 恒成立，
    /// 首轮到期扫描就把全表秒判死。`rr3_manifest_facts_from_text` 已经把非正整数拒在
    /// 读取期，但 `Rr3ManifestFacts` 字段全 `pub`，调用方可以直接构造绕过它——所以入库
    /// 那一刻还得钉一次。`cargo test` 跑的是 debug 构建（`debug_assertions` 开），
    /// 故这里用 catch_unwind 断言它当场被拦；release 侧的下界不在本表也不在 CLI（没有 watchdog
    /// 这个旋钮），由 `manifest.rs` 的 `req_int` 在文本读取期拦住（Task 4 已落，debug/release 都拦）。
    #[cfg(debug_assertions)]
    #[test]
    fn zero_watchdog_is_caught_at_register_time() {
        let mut r = RrdRegistry::new(1000);
        let mut f = facts("rr3-sima", 100, 500, "always");
        f.watchdog_ms = 0; // 绕过 manifest.rs 校验的唯一途径：pub 字段直接改
        let caught =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.rr3_reg_add(&f, chan().0, 0)));
        assert!(caught.is_err(), "watchdog_ms = 0 必须在入库那一刻被拦下，不能留给到期扫描去判死");
    }

    /// 三条拒绝门必须先于 watchdog 断言：`rr3_reg_add` 的文档序是"保留名 → 同名在册 →
    /// 超额 → 分配 task"，而断言原先坐在函数第一行，debug 构建里一个带着坏 watchdog 的
    /// 重复注册会 panic 而不是如实返回 Duplicate——调用方拿不到拒绝理由，也就回不出
    /// 正确错误码（Task 6 的 `rr3_rpc_status_for_register` 正是拿这个 Err 去映射，它的调用方
    /// `conn.rs` 已随 Task 7 落地）。
    /// 三条门各给一个"坏 watchdog + 该拒"的输入，缺一条就抓不到。
    #[cfg(debug_assertions)]
    #[test]
    fn register_gates_run_before_the_watchdog_assertion() {
        let mut r = RrdRegistry::new(1000);
        let t = r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();

        let mut reserved = facts("rr3-runtime", 100, 500, "always");
        reserved.watchdog_ms = 0;
        assert_eq!(
            r.rr3_reg_add(&reserved, chan().0, 0),
            Err(Rr3RegisterError::ReservedName { name: "rr3-runtime".into() }),
            "保留名必须先拒，不能让 watchdog 断言把拒绝变成 panic"
        );
        let mut dup = facts("rr3-sima", 100, 500, "always");
        dup.watchdog_ms = 0;
        assert_eq!(
            r.rr3_reg_add(&dup, chan().0, 0),
            Err(Rr3RegisterError::Duplicate { name: "rr3-sima".into(), src_task: t }),
            "同名在册必须先拒"
        );
        // 循环补 15 条，加上已有的 rr3-sima 正好占满 16 槽；下面的 over 是第 17 次尝试，必须被容量门拒
        for i in 0..RR3_REG_MAX_MODULES - 1 {
            r.rr3_reg_add(&facts(&format!("rr3-m{i}"), 100, 500, "never"), chan().0, 0).unwrap();
        }
        let mut over = facts("rr3-over", 100, 500, "never");
        over.watchdog_ms = 0;
        assert_eq!(
            r.rr3_reg_add(&over, chan().0, 0),
            Err(Rr3RegisterError::Capacity { limit: RR3_REG_MAX_MODULES }),
            "超额必须先拒"
        );
        assert_eq!(
            r.rr3_reg_snapshot().len(),
            RR3_REG_MAX_MODULES,
            "三次拒绝都不许留下影子条目"
        );
    }

    /// watchdog 断言移位后，被 catch_unwind 捕获的 debug panic 不许留下"已改过的表"：
    /// Failed 同身份重注册路径上，旧条目的摘除原先发生在断言**之前**，panic 一旦被外层
    /// 吞掉，Failed 条目与它的 task→name 映射就双双蒸发（发布归属、fanout、
    /// `rr3_reg_name_of` 全部受影响）。现在摘除延后到门+断言全过之后，panic 时表必须原样。
    /// 仅 debug 构建（`debug_assert` 在 release 被编译掉，这条窗口在 release 不存在）。
    #[cfg(debug_assertions)]
    #[test]
    fn caught_panic_leaves_the_failed_entry_registered() {
        let mut r = RrdRegistry::new(1000);
        let t = r.rr3_reg_add(&facts("rr3-sima", 100, 500, "never"), chan().0, 0).unwrap();
        assert!(r.rr3_reg_death("rr3-sima", true, 600).rr3_plan().is_none(), "never 不给重启计划");
        assert_eq!(r.rr3_reg_snapshot(), vec![("rr3-sima".into(), t, Rr3Phase::Failed)]);
        let mut again = facts("rr3-sima", 100, 500, "never");
        again.watchdog_ms = 0; // 绕过 facts 读取入口，触发入库断言
        let caught =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.rr3_reg_add(&again, chan().0, 700)));
        assert!(caught.is_err(), "watchdog_ms = 0 必须仍被断言拦下");
        assert_eq!(
            r.rr3_reg_snapshot(),
            vec![("rr3-sima".into(), t, Rr3Phase::Failed)],
            "被捕获的 panic 不得改动在册表：Failed 条目与 task 号必须原样留在表里"
        );
        assert_eq!(r.rr3_reg_name_of(t).as_deref(), Some("rr3-sima"), "task→name 映射同样不许被摘掉");
    }

    /// 注册即 registered；收到心跳才 running；未在册 task 的心跳被拒
    #[test]
    fn heartbeat_promotes_registered_to_running() {
        let mut r = RrdRegistry::new(1000);
        let t = r.rr3_reg_add(&facts("rr3-sima", 200, 1000, "always"), chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Registered);
        assert_eq!(r.rr3_reg_beat(t, 100), Rr3Beat::Promoted);
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Running);
        assert_eq!(r.rr3_reg_beat(99, 200), Rr3Beat::Rejected, "未在册 task 的心跳必须拒");
    }

    /// 到期完全由显式毫秒驱动：窗口内不判死，超窗口判死转 failed，此后喂狗无效
    #[test]
    fn expire_uses_explicit_monotonic_millis() {
        let mut r = RrdRegistry::new(1000);
        let t = r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_beat(t, 400), Rr3Beat::Promoted);
        assert!(r.rr3_reg_expire(499).is_empty(), "距上次心跳 499-400=99ms < 500ms 不得判死");
        assert_eq!(
            r.rr3_reg_expire(901),
            vec![Rr3Expiry {
                name: "rr3-sima".into(),
                src_task: t,
                reason: "heartbeat_timeout",
                elapsed_ms: 501,
                watchdog_ms: 500,
                restart: Some(Rr3RestartPlan {
                    name: "rr3-sima".into(), attempt: 1, delay_ms: 1000, entry: "/usr/bin/rr3-sima".into(),
                }),
            }]
        );
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
        assert_eq!(r.rr3_reg_beat(t, 950), Rr3Beat::Rejected, "已 failed 的模块不得靠补心跳复活");
    }

    /// 回报里的两个数必须就是判定本身：`elapsed_ms == now - last_beat`、
    /// `watchdog_ms == 该模块自己的超时值`。线程侧的日志逐字引用它们，
    /// 日志说"501 ≥ 500"而实际按别的数判死，就是线上唯一可读的判死证据在撒谎。
    #[test]
    fn expiry_report_carries_the_numbers_that_decided_it() {
        let mut r = RrdRegistry::new(1000);
        let t = r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_beat(t, 400), Rr3Beat::Promoted, "夹具心跳必须先被接受，否则 elapsed 无从谈起");
        let e = &r.rr3_reg_expire(1_001)[0];
        assert_eq!(e.elapsed_ms, 601, "elapsed 必须是 now-last_beat，不是 now-注册时刻");
        assert_eq!(e.watchdog_ms, 500, "必须回显该模块自己的 watchdog_ms，不是全局常量");
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
        let _ = r.rr3_reg_beat(a, 1000);
        let e = r.rr3_reg_expire(1400);
        assert_eq!(e.len(), 0, "simb 窗口 5000ms 未到、sima 刚喂过");
        // sima 继续喂狗（它自己的窗口是 500ms，不喂它就该和 simb 一起被报）
        let _ = r.rr3_reg_beat(a, 5900);
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

    /// 归属判定的**最后一道**看的是主题集里登记的 owner，而不是主题串的首段：
    /// 首段相等只可能来自"表没被 `rr3_topic_set_check` 复验过"（check 强制首段 == owner，
    /// `rr-registry/src/topics.rs` 的 `rr3_topic_set_check`），而那条前置只在
    /// `rr3_topics_load` 的装载路径上成立。`rr3_registry_topic_set_from_text` 是 public，
    /// 任何绕开 check 造出来的表都可能带着"首段是 rr3-sima、owner 登记的是 rr3-simb"的条目。
    /// 故本用例**刻意不用** `topic_set()`（它 assert 过 check，结构上造不出这张表）。
    #[test]
    fn publish_check_uses_the_registered_owner_not_just_the_first_segment() {
        let set = rr_registry::rr3_registry_topic_set_from_text(
            r#"schema_version = 1
[[owner]]
name = "rr3-simb"
[[topic]]
name = "rr3-sima/ping/state"
kind = "event"
owner = "rr3-simb"
"#,
        );
        // 夹具前提三条：首段就是发送方自己、主题确实在册、owner 登记的却是别人。
        assert_eq!(set.rr3_topic_set_owner("rr3-sima/ping/state").as_deref(), Some("rr3-simb"));
        assert!(set.rr3_topic_set_contains("rr3-sima/ping/state"));
        assert!(
            !set.rr3_topic_set_check().is_empty(),
            "前提：这张表本来就形状不合法（首段 ≠ owner），装载路径永不产出它，只用于验第三道防线"
        );
        assert_eq!(
            RrdRegistry::rr3_reg_publish_check("rr3-sima", "rr3-sima/ping/state", &set),
            Err(Rr3PublishDeny::ForeignPrefix {
                topic: "rr3-sima/ping/state".into(),
                owner: "rr3-simb".into(),
                sender: "rr3-sima".into(),
            }),
            "前缀对得上但在册 owner 是别人：必须按越权拒，而不是因为首段相符就放行"
        );
    }

    /// 在册表里 owner 缺失（`[[topic]]` 没写 owner 键，解析成空串）时，越权文案的 owner
    /// 必须回显 `?`：与 `Rr3PublishDeny::ForeignPrefix` 的文档、也与"主题根本不在册"那条
    /// 分支（`owner: ...unwrap_or_else(|| "?")`）一致——同一种"不知道 owner 是谁"不许一会儿
    /// 空串一会儿 `?`，否则日志里两种写法要人猜是不是两回事。
    /// 夹具要点：主题**在册**且首段与发送方相符，所以前两道分支都不会命中，只有第三道
    /// （登记 owner 与发送方比对）能拒——本用例钉的就是它的回显值。
    #[test]
    fn publish_foreign_prefix_echoes_question_mark_when_owner_is_missing() {
        let set = rr_registry::rr3_registry_topic_set_from_text(
            "\n[[owner]]\nname = \"rr3-sima\"\n[[topic]]\nname = \"rr3-sima/ping/state\"\nkind = \"event\"\n",
        );
        assert!(set.rr3_topic_set_contains("rr3-sima/ping/state"), "用例前提：主题在册");
        assert_eq!(set.rr3_topic_set_owner("rr3-sima/ping/state").as_deref(), Some(""));
        assert_eq!(
            RrdRegistry::rr3_reg_publish_check("rr3-sima", "rr3-sima/ping/state", &set),
            Err(Rr3PublishDeny::ForeignPrefix {
                topic: "rr3-sima/ping/state".into(),
                owner: "?".into(),
                sender: "rr3-sima".into(),
            }),
            "登记 owner 缺失必须回显 ?，不得回显空串"
        );
    }

    /// `owner=?` 回显的一致性：`ForeignPrefix` 有**两处**构造点（前缀不符 / owner 比对不符），
    /// 两处遇到"在册但登记文本没写 owner 键（解析成空串）"都必须回显 `?`——同一种"不知道
    /// owner 是谁"不许一处空串一处 `?`。本用例把 `Rr3PublishDeny::ForeignPrefix` 文档里那句
    /// "两条分支同一写法"逐分支钉住：前一段打第一处构造点（head ≠ sender），
    /// 后一段打第三道分支（head == sender、在册、owner 却取不到）。
    #[test]
    fn foreign_prefix_owner_echo_is_question_mark_on_both_construction_sites() {
        // 第一处构造点：主题 rr3-sima/x/y 在册且 owner 键缺失，发送方 rr3-simb → 前缀不符分支
        let set = rr_registry::rr3_registry_topic_set_from_text(
            "\n[[owner]]\nname = \"rr3-sima\"\n[[topic]]\nname = \"rr3-sima/x/y\"\nkind = \"event\"\n",
        );
        assert_eq!(set.rr3_topic_set_owner("rr3-sima/x/y").as_deref(), Some(""));
        assert_eq!(
            RrdRegistry::rr3_reg_publish_check("rr3-simb", "rr3-sima/x/y", &set),
            Err(Rr3PublishDeny::ForeignPrefix {
                topic: "rr3-sima/x/y".into(),
                owner: "?".into(),
                sender: "rr3-simb".into(),
            }),
            "前缀不符分支在 owner 键缺失时必须回显 ?，不得回显登记的空串"
        );
        // 第三道分支：首段与发送方相符、主题在册、owner 键缺失 → 只有 owner 比对能拒
        let set2 = rr_registry::rr3_registry_topic_set_from_text(
            "\n[[owner]]\nname = \"rr3-sima\"\n[[topic]]\nname = \"rr3-sima/ping/state\"\nkind = \"event\"\n",
        );
        assert_eq!(
            RrdRegistry::rr3_reg_publish_check("rr3-sima", "rr3-sima/ping/state", &set2),
            Err(Rr3PublishDeny::ForeignPrefix {
                topic: "rr3-sima/ping/state".into(),
                owner: "?".into(),
                sender: "rr3-sima".into(),
            }),
            "owner 比对分支在 owner 键缺失时必须回显 ?（与 publish_foreign_prefix_echoes_question_mark_when_owner_is_missing 同路径，这里双分支同测）"
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
        assert_eq!(r.rr3_reg_death("rr3-simb", false, 600).rr3_plan(), None);
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
    }

    /// `--restart-base-ms 0` 的**现值**是 `delay_ms = 0`（第 1 档 1×0、第 2 档 2×0 都是 0，
    /// 即"立即重启"）。本条只把**本表算术**的现值钉死，不裁下限：下界已由 Task 7 的 CLI 门禁
    /// 拦住（`cli.rs::parse_restart_base` 拒 0，提交 f69eda8），所以配置面根本进不来 0。
    /// 它挡的是有人"顺手"在延迟算术里给 base 补一个 `.max(1)` 之类的下界——
    /// 那会让这里从 0 变成 1，而配置面看起来什么都没改。
    #[test]
    fn zero_restart_base_ms_currently_yields_zero_delay() {
        let mut r = RrdRegistry::new(0);
        r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        let p = r.rr3_reg_death("rr3-sima", false, 600).rr3_plan().expect("base=0 不改变是否给计划");
        assert_eq!((p.attempt, p.delay_ms), (1, 0), "现值：第 1 档 1×0 = 0");
        // 真实崩溃循环里两次死亡之间必隔一次重注册（换 task），这里照此补：
        // 新语义下对同一条 failed 条目连调 death 会被判 Silent，取不到第 2 档。
        r.rr3_reg_remove("rr3-sima");
        r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 650).unwrap();
        assert_eq!(
            r.rr3_reg_death("rr3-sima", false, 700).rr3_plan().expect("第 2 次仍有计划").delay_ms,
            0,
            "现值：第 2 档 2×0 仍是 0（不是被补成 1ms）"
        );
    }

    /// 退避乘法是纯算术：base 大到会溢出时封顶，绝不 panic（debug 的 `*` 会 panic）
    /// 也绝不回绕（release 的 `*` 会 wrap 成一个小延迟＝"疯狂重启"，正是 spec §5.3 要避免的）
    #[test]
    fn backoff_multiply_saturates_instead_of_wrapping() {
        let base = u64::MAX / 2;
        let mut r = RrdRegistry::new(base);
        r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_death("rr3-sima", false, 600).rr3_plan().expect("第 1 次有计划").delay_ms, base);
        // 三次取档之间各补一次 remove + add（真实循环里每次死亡隔着一次重注册）：
        // 新语义对同一条 failed 条目连调 death 会判 Silent，跨重注册才是本表算术的真实形状。
        r.rr3_reg_remove("rr3-sima");
        r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 610).unwrap();
        // 2×base 仍可表示，必须给出精确值（封顶实现若写成"一律 u64::MAX"在这里就红）
        assert_eq!(
            r.rr3_reg_death("rr3-sima", false, 620).rr3_plan().expect("第 2 次有计划").delay_ms,
            base.checked_mul(2).expect("前提：2×base 不溢出")
        );
        r.rr3_reg_remove("rr3-sima");
        r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 630).unwrap();
        // 第 3 次是 4×base，溢出 → 封顶（回绕的话会拿到一个很小的数）
        assert_eq!(r.rr3_reg_death("rr3-sima", false, 640).rr3_plan().expect("第 3 次有计划").delay_ms, u64::MAX);
    }

    /// on-failure：干净退出不重启，非零/crash 才重启
    #[test]
    fn on_failure_only_restarts_unclean_exit() {
        let mut r = RrdRegistry::new(1000);
        r.rr3_reg_add(&facts("rr3-sense", 100, 500, "on-failure"), chan().0, 0).unwrap();
        assert!(r.rr3_reg_death("rr3-sense", true, 600).rr3_plan().is_none(), "干净退出不得重启");
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
        assert!(r.rr3_reg_remove("rr3-sense"));
        r.rr3_reg_add(&facts("rr3-sense", 100, 500, "on-failure"), chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_death("rr3-sense", false, 600).rr3_plan().expect("非零退出必须给计划").delay_ms, 1000);
    }

    /// 未知策略值不得 panic、也不得擅自重启（校验在 Task 2 已挡，运行期只保证不猜）
    #[test]
    fn unexpected_policy_string_is_treated_as_never() {
        let mut r = RrdRegistry::new(1000);
        let mut f = facts("rr3-sima", 100, 500, "always");
        f.restart_policy = "ALWAYS".into(); // 大小写错 = 非预期值
        r.rr3_reg_add(&f, chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_death("rr3-sima", false, 600).rr3_plan(), None, "非预期值必须按 never 处理");
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
    }

    /// 不在册的名字问死亡：None 且不得凭空造条目
    #[test]
    fn death_of_unknown_name_is_none() {
        let mut r = RrdRegistry::new(1000);
        r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_death("rr3-nobody", false, 600).rr3_plan(), None);
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
        let _ = r.rr3_reg_death("rr3-sima", false, 600);
        assert!(!r.rr3_reg_live("rr3-sima"), "failed 即无活连接");
        let t2 = r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 700).expect("failed 后必须可重注册");
        assert_eq!(t2, t + 1, "重注册必须换新 task 号");
        assert_eq!(r.rr3_reg_snapshot().len(), 1);
        // 旧号作废：迟到的旧心跳既不能喂到新条目身上，也不能改名存实亡的旧条目
        assert_eq!(r.rr3_reg_name_of(t), None, "旧 task 号必须从映射里摘掉");
        assert_eq!(r.rr3_reg_beat(t, 800), Rr3Beat::Rejected, "旧 task 号不得再被认成在册");
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Registered);
        assert!(r.rr3_reg_live("rr3-sima"));
    }

    /// `rr3_reg_facts` 的现值钉住（Task 11 的 spawn 序靠它取 start_priority / entry）：
    /// 在册名字必须原样回**入库时那份 facts**（逐字段相等，不是只回个名字），不在册的名字
    /// 回 None——既不能 panic，也不能回一条半数据（空的 start_priority 会让 spawn 序排错位）。
    #[test]
    fn facts_roundtrip_for_known_and_unknown_names() {
        let mut r = RrdRegistry::new(1000);
        let f = facts("rr3-sima", 100, 500, "on-failure");
        r.rr3_reg_add(&f, chan().0, 0).unwrap();
        assert_eq!(
            r.rr3_reg_facts("rr3-sima").as_ref(),
            Some(&f),
            "取回的 facts 必须与入库的那份逐字段相同"
        );
        assert_eq!(r.rr3_reg_facts("rr3-nobody"), None, "不在册的名字必须 None");
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

    /// 真实崩溃循环：每次死亡之间隔着一次重注册（新 task）。今天必然红——
    /// `rr3_reg_add` 把 `restarts` 归零，第 6 次仍给得出计划，5 次上限永远撞不到。
    #[test]
    fn crash_budget_survives_reregistration_and_caps_after_five_incarnations() {
        let mut r = RrdRegistry::new(1000);
        let f = || facts("rr3-sima", 100, 500, "always");
        let mut delays = Vec::new();
        for i in 0..RR3_RESTART_FAILURE_CAP {
            let t = r.rr3_reg_add(&f(), chan().0, i as u64 * 10).unwrap();
            let p = r.rr3_reg_death("rr3-sima", false, i as u64 * 10 + 5).rr3_plan()
                .unwrap_or_else(|| panic!("第 {} 次崩溃应有计划", i + 1));
            assert_eq!(p.attempt, i + 1, "attempt 必须跨重注册连续增长");
            assert_eq!(p.name, "rr3-sima");
            assert_eq!(p.entry, "/usr/bin/rr3-sima");
            delays.push(p.delay_ms);
            assert!(r.rr3_reg_live("rr3-sima") == false, "判死后不得算活");
            let _ = t;
        }
        assert_eq!(delays, vec![1000, 2000, 4000, 8000, 16000], "退避必须按表 × base，且序不因重注册被打乱");
        let t = r.rr3_reg_add(&f(), chan().0, 99);
        assert_eq!(r.rr3_reg_death("rr3-sima", false, 100).rr3_plan(), None, "达上限必须停止重启");
        assert!(r.rr3_reg_hit_cap("rr3-sima"));
        assert!(!r.rr3_reg_hit_cap("rr3-nobody"), "没配过的名字不得误报达上限");
        let _ = t;
    }

    /// 到期那一路也记账，且**只记一次**：判死后 conn 再收尾不得双计（ledger R-A4 的收口）
    #[test]
    fn expiry_charges_once_and_the_later_conn_death_is_silent() {
        let mut r = RrdRegistry::new(1000);
        let t = r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0).unwrap();
        let _ = r.rr3_reg_beat(t, 100);
        let e = r.rr3_reg_expire(700).remove(0);
        assert_eq!((e.elapsed_ms, e.watchdog_ms), (600, 500));
        assert_eq!(e.restart.clone().map(|p| p.attempt), Some(1), "到期必须给出第 1 次重启计划");
        assert_eq!(r.rr3_reg_death("rr3-sima", true, 800), Rr3Death::Silent, "已判死的条目不得再广播一次终态");
        assert_eq!(e.restart.map(|p| p.delay_ms), Some(1000));
        assert_eq!(r.rr3_reg_crash_streak("rr3-sima"), 1, "双路只记一次账");
    }

    /// `never` 策略下到期不给计划——`Rr3Expiry.restart` 不是装饰
    #[test]
    fn expiry_of_a_never_module_reports_no_restart() {
        let mut r = RrdRegistry::new(1000);
        let t = r.rr3_reg_add(&facts("rr3-simb", 100, 500, "never"), chan().0, 0).unwrap();
        let _ = r.rr3_reg_beat(t, 100);
        assert_eq!(r.rr3_reg_expire(700).remove(0).restart, None);
    }

    /// 活过一整条窗口 = 上一世的账到此为止（spec"仍在 watchdog 超时内死亡"的正向写法）。
    /// 两颗基准不许混：归零看 **registered_at_ms**（这一世活了多久），判死看 **last_beat_ms**
    /// （多久没喂狗）。所以本条一直喂狗（永不判死），只在"注册后第 521ms"那一刻等归零。
    #[test]
    fn surviving_a_watchdog_window_clears_the_crash_budget() {
        let mut r = RrdRegistry::new(1000);
        let f = || facts("rr3-sima", 100, 500, "always");
        let t = r.rr3_reg_add(&f(), chan().0, 0).unwrap();
        assert_eq!(r.rr3_reg_death("rr3-sima", false, 10).rr3_plan().map(|p| p.attempt), Some(1));
        assert_eq!(r.rr3_reg_crash_streak("rr3-sima"), 1, "第 1 世崩过，账上记着 1");

        let t2 = r.rr3_reg_add(&f(), chan().0, 100).unwrap();
        assert_ne!(t2, t, "重注册必须换新 task，否则本条测的是同一世");
        for at in [120u64, 220, 320, 420, 520] {
            // 只有第一颗是升级；后面四颗必须是"只续命"（D-6 的另一半判据）
            let want = if at == 120 { Rr3Beat::Promoted } else { Rr3Beat::Kept };
            assert_eq!(r.rr3_reg_beat(t2, at), want, "第 {at}ms 的心跳相位回报不对");
            assert!(r.rr3_reg_expire(at + 1).is_empty(), "喂狗后 1ms 绝不许判死");
            if at < 520 {
                assert_eq!(r.rr3_reg_crash_streak("rr3-sima"), 1, "没活满窗口之前不许归零");
            }
        }
        // 注册于 100 + watchdog 500 → 归零时刻是 now_ms >= 600；620 那颗心跳之后第一次扫描到点
        assert!(r.rr3_reg_expire(621).is_empty(), "健康模块不该被扫死");
        assert_eq!(r.rr3_reg_crash_streak("rr3-sima"), 0, "第 2 世已活满 521ms ≥ 500ms 窗口");
        // 简报冲突（见实施报告）：第 2 世全程喂狗、健康存活，`rr3_reg_add` 对**在册非 Failed**
        // 的同名条目按 Duplicate 拒（这是既有不变量，`duplicate_name_is_rejected` 钉着，不许为
        // 本条让步）。故 t3 那句"第 3 世"必须先让第 2 世退场——最小忠实实现：insert 一行 remove
        // 把条目摘掉，再重注册。判据的语义（归零后第 3 世从 0 重计）不变。
        let _ = r.rr3_reg_remove("rr3-sima");
        let t3 = r.rr3_reg_add(&f(), chan().0, 700).unwrap();
        assert_eq!(r.rr3_reg_death("rr3-sima", false, 701).rr3_plan().map(|p| p.attempt), Some(1),
                   "归零之后第 3 世从零重计（不是从 2 起步 = 4 秒退避）");
        let _ = (t, t2, t3);
    }

    /// 所有权：owned 的连接断线时 conn 一路什么都不做（等退出码）
    #[test]
    fn owned_conn_death_defers_terminal_state_to_the_exit_status() {
        let mut r = RrdRegistry::new(1000);
        let f = facts("rr3-sima", 100, 500, "always");
        let t = r.rr3_reg_add(&f, chan().0, 0).unwrap();
        let _ = r.rr3_reg_beat(t, 10);
        r.rr3_reg_own("rr3-sima");
        assert_eq!(r.rr3_reg_death("rr3-sima", true, 20), Rr3Death::DeferredToExitStatus);
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Running, "deferred 不得改相位");
        assert_eq!(r.rr3_reg_crash_streak("rr3-sima"), 0, "deferred 不得记账");
        // 收尸路接手：相位、句柄、账都在这一次到位
        match r.rr3_reg_child_exit(&f, false, 25) {
            Rr3ChildExit::Crashed { src_task, clean_exit, plan } => {
                assert_eq!(src_task, t);
                assert!(!clean_exit);
                assert_eq!(plan.map(|p| p.attempt), Some(1));
            }
            other => panic!("退出码那一路必须给出 Crashed，实际 {other:?}"),
        }
        assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
        assert!(!r.rr3_reg_is_owned("rr3-sima"), "收过尸必须撤下所有权标记");
        // 撤标之后，同名新实例的 EOF 回到 conn 那一路（否则第二次崩溃会永远等一个不再来的退出码）
        let _t2 = r.rr3_reg_add(&f, chan().0, 30).unwrap();
        assert!(matches!(r.rr3_reg_death("rr3-sima", false, 40), Rr3Death::Broadcast { .. }));
    }

    /// 两种到达次序都不许双计：到期先判死 vs conn 先 deferred
    #[test]
    fn death_and_expiry_in_either_order_charge_exactly_once() {
        let f = || facts("rr3-sima", 100, 500, "always");
        // (a) watchdog 先判死并记账，收尸后到 → 收尸必须是 Silent（R-A4 的封印就在这半条）
        let mut a = RrdRegistry::new(1000);
        let ta = a.rr3_reg_add(&f(), chan().0, 0).unwrap();
        let _ = a.rr3_reg_beat(ta, 10);
        a.rr3_reg_own("rr3-sima");
        assert_eq!(a.rr3_reg_expire(600).remove(0).restart.map(|p| p.attempt), Some(1));
        assert!(matches!(a.rr3_reg_child_exit(&f(), false, 610), Rr3ChildExit::Silent),
                "已判死的条目被收尸不得再记一次、也不得再广播一次");
        assert_eq!(a.rr3_reg_crash_streak("rr3-sima"), 1);
        // (b) conn 先 deferred（什么都不做），收尸后到 → 只有收尸记一次
        let mut b = RrdRegistry::new(1000);
        let tb = b.rr3_reg_add(&f(), chan().0, 0).unwrap();
        let _ = b.rr3_reg_beat(tb, 10);
        b.rr3_reg_own("rr3-sima");
        assert_eq!(b.rr3_reg_death("rr3-sima", true, 20), Rr3Death::DeferredToExitStatus);
        assert_eq!(b.rr3_reg_crash_streak("rr3-sima"), 0, "deferred 一路不许先记账");
        assert!(matches!(b.rr3_reg_child_exit(&f(), false, 25), Rr3ChildExit::Crashed { .. }));
        assert_eq!(b.rr3_reg_crash_streak("rr3-sima"), 1);
    }

    /// 显式 unregister 之后，子进程退出码不得变成一次崩溃
    #[test]
    fn unregister_then_child_exit_does_not_charge() {
        let mut r = RrdRegistry::new(1000);
        let f = facts("rr3-sima", 100, 500, "always");
        let t = r.rr3_reg_add(&f, chan().0, 0).unwrap();
        let _ = r.rr3_reg_beat(t, 10);
        r.rr3_reg_own("rr3-sima");
        assert!(r.rr3_reg_remove("rr3-sima"), "unregister 必须真的摘掉条目");
        assert!(matches!(r.rr3_reg_child_exit(&f, true, 20), Rr3ChildExit::Silent));
        assert_eq!(r.rr3_reg_crash_streak("rr3-sima"), 0, "我让它停的，不是它崩的");
        // 新实例重新登记 → stopped 让位（否则一个名字被 stop 过一次就永远起不来了）
        let t2 = r.rr3_reg_add(&f, chan().0, 30).unwrap();
        let _ = r.rr3_reg_beat(t2, 40);
        assert!(matches!(r.rr3_reg_death("rr3-sima", false, 50), Rr3Death::Broadcast { plan: Some(_) }));
    }

    /// 从未注册的子进程：没有条目可广播，但账必须记（否则坏 entry 能把机器转着圈地重起）
    #[test]
    fn child_that_never_registered_is_charged_by_name_until_the_cap() {
        let mut r = RrdRegistry::new(1000);
        let f = facts("rr3-sima", 100, 500, "always");
        for i in 0..RR3_RESTART_FAILURE_CAP {
            match r.rr3_reg_child_exit(&f, false, i as u64 * 10) {
                Rr3ChildExit::EarlyExit { plan } => assert_eq!(plan.map(|p| p.attempt), Some(i + 1)),
                other => panic!("第 {} 次应为 EarlyExit，实际 {other:?}", i + 1),
            }
            assert_eq!(r.rr3_reg_snapshot().len(), 0, "没注册过就不许凭空造条目");
        }
        assert!(matches!(r.rr3_reg_child_exit(&f, false, 999), Rr3ChildExit::EarlyExit { plan: None }));
        assert!(r.rr3_reg_hit_cap("rr3-sima"));
    }

    /// 死因→phase 的映射只有这一处（conn 与 supervisor 共用）
    #[test]
    fn phase_for_exit_is_the_single_source_of_the_two_words() {
        assert_eq!(rr3_phase_for_exit(true), "unregistered");
        assert_eq!(rr3_phase_for_exit(false), "failed");
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
