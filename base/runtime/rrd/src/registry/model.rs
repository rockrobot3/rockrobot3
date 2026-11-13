// registry 的类型模型：阶段/错误/回报枚举、到期与重启计划、上限常量与
// 纯策略函数（退避档、退出码→相位）。本文件不持有状态、不碰 I/O——
// 状态机本体在 lifecycle.rs，查询与扇出在 view.rs。
use super::RrdRegistry;
use rr_registry::Rr3TopicSet;
use std::fmt;

/// 在册模块上限（spec §10 K3：v0.0.1 目标 5+1 模块，留余量并拒超额）
pub const RR3_REG_MAX_MODULES: usize = 16;

/// 单连接广播收件队列上界（帧数）。有界化是 OOM 防线：不读 socket 却继续喂心跳的
/// 僵尸订阅者，无界队列会让 rrd 积压到进程被杀（0.0.8b 审计 B-5 实锤，Task 10
/// carry 本轮落地）。128 帧 × 音频片 ~11KB ≈ 1.4MB/连接最坏，×16 连接上限
/// ≈ 22MB——Pi/RK3566 量级可承受。队列满 = 该连接判慢，断投点名（见 fanout）。
pub const RR3_FAN_CAP: usize = 128;

/// 重启退避表（倍数 × restart_base_ms），末档封顶 60×base（spec §5.3）
pub const RR3_RESTART_BACKOFF: [u64; 7] = [1, 2, 4, 8, 16, 32, 60];

/// 连续失败上限：到顶即 failed 并永久停止重启（spec §5.3）
pub const RR3_RESTART_FAILURE_CAP: u32 = 5;

/// 到期原因。`Rr3Expiry::reason` 是 `&'static str`，让日志与集成测试可以逐字比对；
/// 注册完成前超时（`pre_register_timeout_ms`）的判定在 conn 层，届时新增取值即可。
pub const RR3_EXPIRE_HEARTBEAT_TIMEOUT: &str = "heartbeat_timeout";

/// 生命周期阶段。本文件**当前可发生**的转移：
/// `registered → running`（`rr3_reg_beat` 回报 `Rr3Beat::Promoted`）、
/// `registered | running → failed`（`rr3_reg_expire` 超时、`rr3_reg_death` 退出、
/// `rr3_reg_child_exit` 收尸）、`running → degraded`（`rr3_reg_degrade`：required
/// 能力的提供者全灭，ADR-0009 裁定 3）、`degraded → running`（`rr3_reg_undegrade`：
/// 任一提供者回 Running 且全部 required 能力重新就位）。
/// `Degraded` 的语义 = 既有兜底：心跳只续命不升级（`rr3_reg_beat` 归 `Kept`，一颗心跳
/// 不许把降级标注抹平）；降级**不**重启、不吃预算、不经 supervisor（裁定 4——下游进程
/// 没病，重启治不了依赖缺失）。
/// failed 之后同身份可重注册（换新 task 号），旧条目整体替换。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3Phase {
    /// 已注册、尚未收到任何心跳：此刻不判活（legacy 教训 76c0ad8）
    Registered,
    Running,
    /// required 能力的全部提供者离开 Running（ADR-0009 裁定 3；写入方
    /// `rr3_reg_degrade`，解除方 `rr3_reg_undegrade`）
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
    ReservedNamespace {
        topic: String,
    },
    /// `owner` 回显主题集里登记的 owner；**取不到就回显 `?`**——主题根本不在册、或在册但
    /// 登记文本没写 owner（解析成空串）都算取不到，两条分支同一写法（两处构造点共用
    /// `owner_for_echo`），不留"一会儿空串一会儿 `?`"的两种未知。
    /// 两条都是**终判拒绝**（这一条发布已经被拒了），不是"owner 未知、待定再查"——
    /// 日志读者别把 `owner=?` 读成待办事项。
    ForeignPrefix {
        topic: String,
        owner: String,
        sender: String,
    },
    /// "未注册"指的是**主题**不在主题集里（首段与发送方相符、名字却拼错或没登记），
    /// **不是**指发送方那个模块没在册：本函数是关联函数、不碰 `self`，看不见在册表。
    NotRegistered {
        topic: String,
    },
}

impl fmt::Display for Rr3PublishDeny {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReservedNamespace { topic } => write!(
                f,
                "拒绝 {topic}：rr3-runtime/* 是守护进程的下行广播，模块不得冒充"
            ),
            Self::ForeignPrefix {
                topic,
                owner,
                sender,
            } => {
                write!(f, "越权发布 {topic}（owner={owner}，发送方={sender}）")
            }
            Self::NotRegistered { topic } => write!(f, "未注册主题 {topic}，拒绝发布"),
        }
    }
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

/// `rr3.ctl.health` 的一行（ADR-0010 裁定 4）：快照行 + 心跳/看门狗配置 + 距上次心跳。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3HealthRow {
    pub name: String,
    pub src_task: u64,
    pub phase: Rr3Phase,
    pub heartbeat_ms: u64,
    pub watchdog_ms: u64,
    /// 判定处回显 `now - last_beat_ms`（与 `Rr3Expiry.elapsed_ms` 同一条理据：
    /// 表外的人拿不到 `last_beat_ms`，少回显就只能靠掐时间，那是随机红的来源）
    pub since_beat_ms: u64,
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
    Crashed {
        src_task: u64,
        clean_exit: bool,
        plan: Option<Rr3RestartPlan>,
    },
    /// 从未注册且已 stopped，或条目早已 failed：静默
    Silent,
    /// 子进程在注册成功之前就退出了：没有条目可广播，但已按 name 记过账
    EarlyExit { plan: Option<Rr3RestartPlan> },
}

/// 死因 → 下行 state 相位名的唯一映射（D-7）：conn 收尾与 11b 的 supervisor 两个广播点都必须
/// 经它。今天这条映射原来只写在 `conn.rs` 的 `if clean_exit` 里，11b 再加一个广播点就是两处真值。
pub fn rr3_phase_for_exit(clean_exit: bool) -> &'static str {
    if clean_exit {
        "unregistered"
    } else {
        "failed"
    }
}

/// `ForeignPrefix.owner` 的回显唯一写法：在册但登记文本没写 owner 键（解析成空串）与
/// 根本不在册（None）都渲染 `?`。`rr3_reg_pub_check` 的两处 `ForeignPrefix` 构造点
/// 都必须经它——任何一处绕过它就会把"同一种未知"写成两种回显（由
/// `foreign_prefix_owner_echo_is_question_mark_on_both_construction_sites` 钉住）。
pub(super) fn owner_for_echo(registered: Option<String>) -> String {
    registered
        .filter(|o| !o.is_empty())
        .unwrap_or_else(|| "?".to_string())
}

/// 主题在册判据（第三层归属门；未在册回 NotRegistered）。
pub(super) fn rr3_pub_registered(topic: &str, topics: &Rr3TopicSet) -> Result<(), Rr3PublishDeny> {
    if topics.rr3_topic_contains(topic) {
        return Ok(());
    }
    Err(Rr3PublishDeny::NotRegistered {
        topic: topic.to_string(),
    })
}

/// 在册 owner 与发送方比对（末段判据；取不到 owner 同样按越权拒）。
pub(super) fn rr3_pub_owner(
    sender: &str,
    topic: &str,
    topics: &Rr3TopicSet,
) -> Result<(), Rr3PublishDeny> {
    let owner = topics.rr3_topic_owner(topic);
    if RrdRegistry::rr3_owner_mismatch(owner.as_deref(), sender) {
        return Err(Rr3PublishDeny::ForeignPrefix {
            topic: topic.to_string(),
            owner: owner_for_echo(owner),
            sender: sender.to_string(),
        });
    }
    Ok(())
}

/// 运维计数聚合表的键数上界：模块自报键是**外部输入**（心跳 `counters`），
/// 键只增不减且随 write-through 落盘——故障模块每拍换键可把内存/计数器文件
/// 撑爆。六模块的静态键集远小于本值；越界即拒新键并一次性点名（既有键照常
/// 累加，不丢历史账）。
pub(crate) const RR3_CTR_KEY_CAP: usize = 4096;
