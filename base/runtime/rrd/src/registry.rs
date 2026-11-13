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
use std::sync::mpsc::{SyncSender, TrySendError};

use rr_ipc::Rr3Frame;
use rr_registry::Rr3TopicSet;

use crate::manifest::Rr3ManifestFacts;
use crate::rr3_log;

pub use model::*;

mod lifecycle;
mod model;
#[cfg(test)]
mod tests;
mod view;

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
    tx: Option<SyncSender<Rr3Frame>>,
    /// 广播队列满导致的丢帧计数（满侧不摘连接，只丢帧；见 `rr3_reg_fanout`）
    fan_dropped: u64,
}

#[derive(Debug)]
pub struct RrdRegistry {
    modules: BTreeMap<String, Entry>,
    by_task: BTreeMap<u64, String>,
    /// exposes 索引（批D2）：topic → 候选 task 升序集。`rr3.call` 路由按它取最小
    /// 存活被调方，不再全表扫 modules×rpc_exposes。只在注册/摘除时增删；判死或
    /// 摘发送端的条目**仍留在索引里**，由查表处的 live 判据过滤——索引是候选集，
    /// 不是活表，故失败历史不会让一次路由多出一次清扫。
    exposes: BTreeMap<String, BTreeSet<u64>>,
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
    /// 装配缺项预降级集（G4/ADR-0019 修订 1 ③）：boot 装配段校验缺项的模块**照常
    /// spawn**（件没病，重启治不了装配数据），但在册即 `Degraded`、且不被解除传播
    /// 捞回——装配声明是盘上静态数据，修数据 + 重启 rrd 是唯一解除路。
    /// daemon 在投 boot 队列前一次性写入（`rr3_reg_predegrade`）。
    pre_degraded: BTreeSet<String>,
    /// 能力 → 提供者名字集合（ADR-0009 裁定 1/3）。**boot 仲裁的产物**：daemon 在
    /// 投 boot 队列前一次性写入（含被仲裁拒绝的模块——它们的 provides 仍计入
    /// 可满足性宇宙），运行期**不**重排（不做清单 2：动态重仲裁不做）。
    /// 只含非 base 能力：base 内建能力不在表里即"永远在位"（`rr3_deps_satisfied`）。
    providers: BTreeMap<String, BTreeSet<String>>,
    /// 全局低电旗（0.0.2 方案 Q4 裁定的兜底动作面之一）。conn 转发
    /// `rr3-power/battery/low` 时置位（上升沿恰一次），supervisor 的唯一 spawn
    /// 漏斗（`rr3_boot_spawn`）读它拒一切新 spawn；conn 收到
    /// `rr3-power/battery/ok`（恢复沿，批N5 件二笔3，§17 运维留痕）时清位——
    /// 解除即恢复新 spawn。放本表的理由与 `denied_publish` 同款：
    /// conn 与 supervisor 两路共见的守护级状态，真值只许有一份。
    low_power: bool,
    /// 运维计数聚合表（0.0.4 批B，Q1/Q2 裁定）：键 = `模块名/键`（模块自报经心跳
    /// `counters` 字段归并）或 `boots`（rrd 自有，启动自增不走心跳）。write-through
    /// 持久化的内存真值，落盘点在 daemon 启动与本表心跳归并后的调用方（conn）。
    /// 与 `crash_streak`（崩溃重启账）分开：这里是业务运维账，"机器慢先看数字"。
    counters: BTreeMap<String, u64>,
    /// 键数上界越界只点名一次（避免每拍刷屏；[`RR3_CTR_KEY_CAP`]）。
    ctr_cap_noted: bool,
    /// 广播转发留痕闸（真板验收 2026-11-08）：每个 topic 上次的接收连接数。音频
    /// PCM 每帧一条"转发到 N 个连接"会冲垮日志环，只在连接数变化（含首次）时
    /// 留痕——拓扑变化才是运维要看的信号。
    fanout_last: BTreeMap<String, usize>,
}
