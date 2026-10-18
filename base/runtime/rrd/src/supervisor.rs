//! spawn 与重启的唯一执行者（spec §5.3/§5.4）。一条线程、串行收尸与起进程。
//!
//! 为什么要有"所有权"这件事（`jobs` / `rr3_reg_own`）：没有 `SO_PEERCRED`（要 libc，破 K5），
//! rrd 认不出哪条连接是自己起的进程，只能反向用"我自己起过谁"当依据 ——
//! 所以本文件是"能起谁"的全集，而不是"该起谁"的裁判。**外来模块的计划一律跳过**，
//! 理由与代价见 E-1 / E-2。
//!
//! 本文件的四条**已知边界**（任何"崩溃自愈已完成"的措辞都不许盖过它们）：
//! ① **非本守护起的模块永不重启**（E-1/E-2 的代价）：它们照样被记账、照样出计划，
//!    但执行时只打一行 `跳过重启 {name}：不是本守护起的进程`。M3 若要"rrd 接管别人起的进程"，
//!    得回来补 identity 载体 + `SO_PEERCRED`（libc → 要先重裁 K5 依赖预算）。
//! ② **`rr3_reg_own` 与注册之间只靠 name 相关**：boot 一个已被外部占用的名字 = 配置错误，
//!    本轮按"子进程退出"处理（走 `rr3_reg_child_exit` 的 EarlyExit，吃预算、有上限），
//!    不为其发明第二种语义。
//! ③ **不持有 `SIGCHLD` 处理、不靠 `waitpid` 阻塞**：收尸靠 `try_wait` + 扫尸节拍（E-5 复用
//!    `--watchdog-tick-ms`），故"进程已死"到"rrd 知道"之间有**至多一个 sweep 的延迟**（生产 200ms）。
//! ④ **depends / degraded 不在本文件裁定**（原"两条不做"随 F2/ADR-0009 解除，但裁定权不在这）：
//!    boot 启动序由 `rr3_registry_plan_boot` 在 `daemon.rs` 仲裁后投递（环 / 不可满足都在
//!    那里拒）；降级传播只在本文件收尸那一路"记账之后"被动触发——置位/解除的账本在
//!    `registry.rs`（`rr3_reg_degrade_after` / `rr3_reg_undegrade_after`），本文件不判
//!    "谁该降级"，只把收尸事实交出去。降级不出重启计划、不吃预算（裁定 4）。
//!
//! 锁纪律（**违反即同线程重入自死锁**，而 `#[cfg(unix)]` 让 Windows L1 全绿也看不见）：
//! `rr3_lock(&shared).rr3_reg_*()` 一律先 `let` 绑结果再 `match`，**绝不**写进 `match` 的
//! 被匹配表达式或守卫 —— Rust 会把守卫临时量 `MutexGuard` 延寿到整个 `match` 语句结束，
//! 臂内/后续臂再取同一把锁就死（11a 在 `rr3_conn_finish` 上踩过一次，由 L2 的 e2e 超时抓出）。
//! brief 的骨架里那句 `None if rr3_lock(&shared).rr3_reg_hit_cap(&name) =>` 就是这个形状，
//! 本文件按同一条规矩把它改成 `let capped = …` 后再 `if`（见 `rr3_absorb_exit`）。
//!
//! 线程形状（E-5）：全 daemon 只有这一条执行线程加一条 watchdog。`recv_timeout(sweep)`
//! 既当收件也当节拍，每轮顺序固定为 收件 → `rr3_reap` → `rr3_fire_due`。
//! `sweep` 复用 `--watchdog-tick-ms`，但**表里有更早到期的计划时本拍只等到那一刻**（见
//! `rr3_spawn_supervisor` 里那段"取小者"）——否则 1 小时 tick 的既有测试里重启永不执行。
//! 为什么不再加一个线程：收尸与重启必须串行（并行的话"A 的收尸产出计划、B 的执行又起一个
//! 同名进程"没人排序），而这两件事都不阻塞别人（`try_wait` 不阻塞，`Command::spawn` 是微秒级）。
//!
//! 退避不是睡眠（E-4）：计划进 `due: Vec<(Instant, Rr3RestartPlan)>`，到期才起。
//! **绝不允许**在本文件里 `sleep(delay)`：那会让一个模块的 60 秒退避把整台机器的收尸停摆 60 秒
//! （其余模块的退出码、僵尸回收、重启全堵在后面）。
//!
//! 环境注入三件（E-6）：`RR3_BUS_PATH` / `RR3_MANIFEST` / `RR3_SIM_PEER_ROLE`。子进程
//! `stdout(Stdio::null())`、`stderr(Stdio::inherit())` —— **绝不能把子进程 stdout 接进 rrd 的
//! stdout 管道**：`RrdTest::rr3_wait_log` / `rr3_assert_no_log` 读的就是那条管道，混进子进程
//! 输出就把"rrd 说过什么"变成"这堆进程里谁说过什么"（`sim_peer` 打的是中文，一条 `已主动退场`
//! 就足以扰动 needle）。
//!
//! 日志用词纪律（与测试的否定 needle 同源，改字面量前要一起看）：
//! `spawn ` 只用于"成功起了一个进程"那一行；起不来一律写 `起进程失败`。
//! A-5③ 第 3 条要求新文件里的 `rr3_assert_no_log` 只能用 `spawn rr3-<名>` / `跳过重启` /
//! `停止重启` 这类**不与其它文案共用子串**的 needle。

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::daemon::{rr3_now_ms, rr3_state_frame};
use crate::manifest::{
    rr3_manifest_entry_is_absolute, rr3_manifest_facts_from_text, Rr3ManifestFacts,
};
use crate::registry::{rr3_phase_for_exit, Rr3ChildExit, Rr3Phase, Rr3RestartPlan, RrdRegistry};
use crate::{rr3_lock, rr3_log};

/// 一个"我能起谁"的条目（E-1：`jobs` 的全集只由 `--manifest` 填）。
#[derive(Debug, Clone)]
pub struct Rr3SpawnJob {
    /// 每次重启重读（E-2：不加 `Rr3ManifestFacts.text`，身份来源就是这个路径）
    pub manifest_path: PathBuf,
    /// 已按 manifest 所在目录解析成绝对路径，且 E-3 的存在性 + 可执行位已查过
    pub entry: PathBuf,
    pub facts: Rr3ManifestFacts,
    /// 物理绑定描述符文本（ADR-0018 裁定 4、批D④）：装配单 [hardware] 的序列化
    /// 文本，daemon 组装时从装配链透传（`--manifest` 直起路径为空串 = 无描述符）。
    /// spawn 时空值不注入——模块侧 rr-hal 维持"无描述符"fail-closed 状态。
    pub hal_bindings: String,
    /// 执行轴状态注入文本（批N3 件三，件五消歧扩形）：装配链轴档案
    /// （`hardware/<形态>/axis/*.toml`）中 instance 与轴读数（`RR3_HAL_PERIPH_AXIS`，
    /// 缺省契约常量）相等的恰一份合法装载的投影
    /// （`calibrated=<bool>,hard_stop=<词>,lo=<n>,hi=<n>,center=<n>`），daemon 同位
    /// 透传，spawn 时注入 `RR3_ACT_AXIS_STATE`；空串 = 不注入——模块侧折保守缺省
    /// （未标定不得自动，§6 硬约束），与 hal_bindings 同一条空值不注入纪律。
    pub axis_state: String,
}

/// supervisor 的两种请求。`Boot` 来自 daemon 的启动序（E-7），`Restart` 来自
/// conn 收尾与 watchdog 到期那两路交出的计划（`Rr3Death::Broadcast` / `Rr3Expiry.restart`）。
pub enum Rr3SuperMsg {
    Boot(Rr3SpawnJob),
    Restart(Rr3RestartPlan),
}

/// conn 线程与 watchdog 线程各持一份（`Sender` 可 clone）。发送失败只可能是
/// supervisor 已经死了 —— 那时打一行日志即可，两条调用方都不许 panic：
/// 守护进程不该因为起不了子进程就不服务已连上来的模块。
#[derive(Clone)]
pub struct Rr3Supervisor {
    tx: Sender<Rr3SuperMsg>,
}

impl Rr3Supervisor {
    pub fn rr3_new(tx: Sender<Rr3SuperMsg>) -> Self {
        Self { tx }
    }

    pub fn rr3_boot(&self, job: Rr3SpawnJob) {
        // 失败分支要点名是谁：`send` 吞掉的是整个消息，投递之后再读 `job` 已经晚了。
        // 一次 String clone 换"投递失败也报得出对象"，值得——这条路只在守护正在收尾时走。
        let name = job.facts.name.clone();
        if self.tx.send(Rr3SuperMsg::Boot(job)).is_err() {
            rr3_log(&format!("supervisor 已退出，boot 投递丢弃: {name}"));
        }
    }

    pub fn rr3_restart(&self, plan: Rr3RestartPlan) {
        let name = plan.name.clone();
        if self.tx.send(Rr3SuperMsg::Restart(plan)).is_err() {
            rr3_log(&format!("supervisor 已退出，重启投递丢弃: {name}"));
        }
    }
}

/// boot 期的配置错误不吃预算（E-3）：entry 解析后不存在 / 不是文件 / 没有可执行位 →
/// 打一行 `拒绝启动 {name}：entry {绝对路径} 不存在或不可执行`，**不 own、不 spawn、不记账**。
/// 理由：那是确定性配置错误，吃预算会在 1 秒内烧光 5 次上限并把一个可修的笔误升级成
/// "运行时降级"，而 spec §5.4 的原话是"否则注册阶段直接拒"——语义是**拒**，不是**重试到放弃**。
///
/// 为什么返回 `Option` 而不是 `Result<_, String>`：**两种失败点名的对象不同**——
/// manifest 读不动/解析失败时 rrd 连 `name` 都不知道，只能点名"是哪个文件"（E-7 末句）；
/// entry 不合格时点名的是"哪个模块的哪条 entry"。把文案塞进返回值再让 daemon 打，
/// 就等于逼调用方去猜这两种形状，日志出口反而分散。
pub fn rr3_boot_job_from_manifest(manifest_path: &Path) -> Option<Rr3SpawnJob> {
    let text = match std::fs::read_to_string(manifest_path) {
        Ok(t) => t,
        Err(e) => {
            rr3_log(&format!(
                "拒绝启动：manifest {} 读取失败: {e}",
                manifest_path.display()
            ));
            return None;
        }
    };
    let facts = match rr3_manifest_facts_from_text(&text) {
        Ok(f) => f,
        Err(errs) => {
            rr3_log(&format!(
                "拒绝启动：manifest {} 解析失败: {}",
                manifest_path.display(),
                errs.join("; ")
            ));
            return None;
        }
    };
    // E-7：绝对则原样，否则相对 **manifest 所在目录** join（不是 rrd 的 CWD——装机后模块的
    // 相对 entry 是相对它自己的 manifest 写的，跟着 CWD 走就成了一台机器一个目录树）。
    // 父目录取不到（`--manifest` 给的是裸文件名）时按空串 join，即由 rrd 的 CWD 展开——
    // 那是 CLI 原词本身有歧义，而回显出去的永远是最终解析后的那一条路径，不猜。
    let entry = if rr3_manifest_entry_is_absolute(&facts.entry) {
        PathBuf::from(&facts.entry)
    } else {
        manifest_path
            .parent()
            .unwrap_or(Path::new(""))
            .join(&facts.entry)
    };
    // E-3 的两项检查。`PermissionsExt::mode()` 在 `std::os::unix::fs` 里，仍是零外部依赖。
    match std::fs::metadata(&entry) {
        Ok(md) if md.is_file() && md.permissions().mode() & 0o111 != 0 => {}
        _ => {
            rr3_log(&format!(
                "拒绝启动 {}：entry {} 不存在或不可执行",
                facts.name,
                entry.display()
            ));
            return None;
        }
    }
    Some(Rr3SpawnJob {
        manifest_path: manifest_path.to_path_buf(),
        entry,
        facts,
        // `--manifest` 直起路径没有装配单，描述符与执行轴状态由 daemon 组装时统一透传
        hal_bindings: String::new(),
        axis_state: String::new(),
    })
}

/// 到期表（E-4）：`(起它的时刻, 计划)`。`Instant::now() + Duration::from_millis(delay_ms)` 在
/// `delay_ms == u64::MAX`（`rr3_charge` 的饱和值，语义"永不重启"）时会溢出 → 必须 `checked_add`，
/// 取不到就打 E-4 那句并丢弃——**不许**在这里改回 `sleep`，也不许 `unwrap`。
fn rr3_schedule_due(due: &mut Vec<(Instant, Rr3RestartPlan)>, plan: Rr3RestartPlan) {
    match Instant::now().checked_add(Duration::from_millis(plan.delay_ms)) {
        Some(at) => due.push((at, plan)),
        None => rr3_log(&format!("退避不可表示，视为永久停止 {}", plan.name)),
    }
}

/// 一个终态的**唯一**落点：广播（若该广播）、点名理由的日志、计划进到期表。
/// 收尸那一路与"起进程失败"那一路共用它（两条路只是 `status` 的来源不同）。
///
/// 锁纪律就在下面第一行：`verdict` 由调用方 `let` 绑好后传进来，本函数体内每次
/// `rr3_lock` 都单独成句，臂内不再取第二把锁。
fn rr3_absorb_exit(
    shared: &Arc<Mutex<RrdRegistry>>,
    name: &str,
    status: &str,
    verdict: Rr3ChildExit,
    due: &mut Vec<(Instant, Rr3RestartPlan)>,
) {
    let plan = match verdict {
        Rr3ChildExit::Crashed {
            src_task,
            clean_exit,
            plan,
        } => {
            let phase = rr3_phase_for_exit(clean_exit);
            // E-9：这里的排除项是 `crashed.src_task`（它的 tx 已是 None，排除属形式，
            // 与 expire 那一路同形）；conn 收尾那一路排的是 `RR3_TASK_RRD`（等于不排除）。
            // 两种口径是**已知**的第三/第四种口径，归 Task 13 的 A-3 文档化，此处不"统一"——
            // 统一成哪一种都没有判据支持。
            let n =
                rr3_lock(shared).rr3_reg_fanout(&rr3_state_frame(name, phase, src_task), src_task);
            rr3_log(&format!(
                "{name} 子进程退出 status={status} → 广播 {phase} 到 {n} 个连接"
            ));
            // ADR-0009 裁定 3：本路刚记完收尸账（`rr3_reg_child_exit` 已置 Failed），
            // 死者若是某能力的提供者且再无人在位 → 下游逐个置 Degraded 并广播。
            // EarlyExit / Silent 两路不传播：前者从未注册过（没有"离开 Running"的相位
            // 事件），后者的死亡已由别人记过、传播也归那一拍。
            let degraded = rr3_lock(shared).rr3_reg_degrade_after(name);
            for (dep, task) in degraded {
                // 排除项 `RR3_TASK_RRD`（不排除，E-9 同一理据）：降级要送到依赖者自己耳朵里。
                let n = rr3_lock(shared).rr3_reg_fanout(
                    &rr3_state_frame(&dep, Rr3Phase::Degraded.as_str(), task),
                    rr_bus::RR3_TASK_RRD,
                );
                rr3_log(&format!(
                    "{dep} 依赖断供 → degraded（提供者 {name} 离开 Running）广播到 {n} 个连接"
                ));
            }
            plan
        }
        Rr3ChildExit::EarlyExit { plan } => {
            rr3_log(&format!(
                "{name} 未注册即退出 status={status}（无 state 可广播）"
            ));
            plan
        }
        Rr3ChildExit::Silent => {
            rr3_log(&format!(
                "{name} 子进程退出 status={status}，终态已有人记过"
            ));
            None
        }
    };
    match plan {
        Some(p) => rr3_schedule_due(due, p),
        // 硬规矩：`rr3_reg_hit_cap` 先 `let` 绑，**绝不写进 match 守卫**（守卫里的 `MutexGuard`
        // 会延寿到整个 match 语句结束，而调用方随后还要再取同一把锁）。
        None => {
            let capped = rr3_lock(shared).rr3_reg_hit_cap(name);
            if capped {
                rr3_log(&format!(
                    "停止重启 {name}：连续崩溃达上限，运行时降级（spec §5.3）"
                ));
            }
        }
    }
}

/// 起进程的唯一漏斗：boot 与到期重启都过这里（Task 10 的 D-7 同一条教训——
/// 护栏长在唯一的入口上，比在每个调用点各写一遍强）。
///
/// 次序**固定**（spec §5.4）：`rr3_reg_own(name)`（在锁里）→ `Command::spawn` →
/// 成功后 `children.insert` + `spawn …` 日志；起进程失败 → `rr3_reg_unown` +
/// 立刻 `rr3_reg_child_exit(&facts, false, now)` 走 EarlyExit 记账（E-3 末段：吃预算、有上限）。
/// **为什么 own 在前**：子进程可能在同一毫秒内注册完并死掉，晚一步 own 会让 conn 那一路
/// 先看到 EOF 并按"非 owned"处理 → 广播 `unregistered`，把退出码能给出的 `failed` 抢掉。
/// 这是本设计里唯一一处"次序决定语义"的地方，故测试里有对应判据（S1 的相位断言）。
// 参数是"启动一次所需的全部现场"（registry/子进程表/到期堆/任务单各司其职），
// 收拢成结构体只为让 clippy 闭嘴反而增加一层无谓的打包拆包——显式豁免。
#[allow(clippy::too_many_arguments)]
fn rr3_boot_spawn(
    shared: &Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    children: &mut BTreeMap<String, Child>,
    due: &mut Vec<(Instant, Rr3RestartPlan)>,
    job: &Rr3SpawnJob,
    attempt: u32,
    bus_path: &Path,
    boot_role: &str,
) {
    let name = job.facts.name.clone();
    // 低电兜底（0.0.2 方案 Q4 裁定）：全局低电旗生效后**停止一切新 spawn**——boot 与
    // 重启共用本漏斗（D-7：护栏长在唯一入口上），一问全挡。挂死的旧进程不在此杀：
    // "杀而不重启"属关机策略，归 rr3-power 演进与 M6；本面只保证不再起新的。
    if rr3_lock(shared).rr3_reg_low_power_is_set() {
        rr3_log(&format!(
            "低电兜底：拒绝 spawn {name}（全局低电旗已置位，停止新 spawn）"
        ));
        return;
    }
    if let Some(mut old) = children.remove(&name) {
        // 走到这里只有一种形状：到期判死挂住的子进程（conn/收尸那两路都会先把 handle 交出来）。
        // 不杀就是"两份同名进程并存 + 旧 handle 被 insert 覆盖成永久孤儿"——
        // 覆盖 `Child` 不等于杀进程，std 的 drop 既不 kill 也不 wait。
        let _ = old.kill();
        let _ = old.wait(); // 当场收掉，绝不留僵尸；这里阻塞是故意的（就等这一个）
        rr3_log(&format!("杀掉挂死的 {name} pid={}", old.id()));
    }
    rr3_lock(shared).rr3_reg_own(&name);
    // E-6 的三件环境：连哪儿（rrd 自己的 socket 绝对路径）、是谁（该 job 的 manifest
    // 绝对路径，每次重启重读）、做什么（`--boot-role` 的值，原样透传、rrd 不解释）。
    // 物理绑定描述符（批D④）只在非空时追加注入：空 = 装配单未给 [hardware]（或
    // --manifest 直起），模块侧 rr-hal 维持"无描述符"fail-closed，应急 env 可救。
    let mut cmd = Command::new(&job.entry);
    cmd.env("RR3_BUS_PATH", bus_path)
        .env("RR3_MANIFEST", &job.manifest_path)
        .env("RR3_SIM_PEER_ROLE", boot_role);
    if !job.hal_bindings.is_empty() {
        cmd.env("RR3_HAL_BINDINGS", &job.hal_bindings);
    }
    // 执行轴状态（批N3 件三）：空值不注入——模块侧折保守缺省（未标定不得自动），
    // 与 hal_bindings 同一条"空 = 无真值，fail-closed"纪律
    if !job.axis_state.is_empty() {
        cmd.env("RR3_ACT_AXIS_STATE", &job.axis_state);
    }
    // 见文件头的 stdout 纪律：子进程输出绝不进 rrd 自己那条管道。
    let spawned = cmd.stdout(Stdio::null()).stderr(Stdio::inherit()).spawn();
    match spawned {
        Ok(child) => {
            let pid = child.id();
            children.insert(name.clone(), child);
            // `attempt>0` 才附尾注：boot 那一次没有"第几次重启"可说
            //（S1 正是靠这个后缀把第一世与第二世分开）。
            let tail = if attempt > 0 {
                format!(" attempt={attempt}")
            } else {
                String::new()
            };
            rr3_log(&format!(
                "spawn {name} pid={pid} entry={}{}",
                job.entry.display(),
                tail
            ));
        }
        Err(e) => {
            // 撤下所有权标记：这一次根本没有进程，conn 那一路无从等退出码；标记留着
            // 就会把同名新实例的 EOF 永远钉在 `DeferredToExitStatus` 上。
            rr3_lock(shared).rr3_reg_unown(&name);
            // 已知边界②：`Command::spawn` 返回 Err（ENOMEM / ETXTBSY 这类瞬态，或名字被外部
            // 占用后的意外形状）走 EarlyExit 那一路，吃预算、有上限（E-3 末段）。
            let reason = format!("起进程失败: {e}");
            let verdict = rr3_lock(shared).rr3_reg_child_exit(&job.facts, false, rr3_now_ms(t0));
            rr3_absorb_exit(shared, &name, &reason, verdict, due);
        }
    }
}

/// 扫尸（E-5 每轮的第二段）。**先收集 `(name, status)` 再处理**：`iter_mut` 里不许 `remove`。
///
/// 退出码是 owned 模块死因的**唯一权威**：`abort` 走 signal(6)、`kill -9` 走 signal(9)，
/// 两者都不可能被误读成"体面退场"——这正是 conn 那一路看不见的（见 A-6）。
fn rr3_reap(
    shared: &Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    jobs: &BTreeMap<String, Rr3SpawnJob>,
    children: &mut BTreeMap<String, Child>,
    due: &mut Vec<(Instant, Rr3RestartPlan)>,
) {
    let mut exited: Vec<(String, ExitStatus)> = Vec::new();
    for (name, child) in children.iter_mut() {
        // `try_wait` 不阻塞；`Ok(None)` = 还活着；`Err` 只可能是 ECHILD 这类"没得收"，跳过它。
        if let Ok(Some(status)) = child.try_wait() {
            exited.push((name.clone(), status));
        }
    }
    for (name, status) in exited {
        // 先把 handle 交出来：之后无论走哪条分支，这个 pid 都不再是"我的活"（绝不留僵尸，
        // 也绝不让下一轮把它当还活着的进程重复收一次）。
        let _ = children.remove(&name);
        let clean = status.success();
        let job = jobs.get(&name).expect("children 里的 name 必有 job");
        // 硬规矩：先 `let` 绑结果。记账 + 相位 + 广播**只在这一次**发生——owned 的模块，
        // conn 那一路已把相位/记账/广播三件事全让了出来（`Rr3Death::DeferredToExitStatus`）。
        let verdict = rr3_lock(shared).rr3_reg_child_exit(&job.facts, clean, rr3_now_ms(t0));
        rr3_absorb_exit(shared, &name, &status.to_string(), verdict, due);
    }
}

/// 到期执行。**退避不是睡眠**：这里只问"到点了没有"，从不 sleep（E-4）。
fn rr3_fire_due(
    shared: &Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    jobs: &BTreeMap<String, Rr3SpawnJob>,
    children: &mut BTreeMap<String, Child>,
    due: &mut Vec<(Instant, Rr3RestartPlan)>,
    bus_path: &Path,
    boot_role: &str,
) {
    if due.is_empty() {
        return;
    }
    let now = Instant::now();
    let mut ready: Vec<Rr3RestartPlan> = Vec::new();
    due.retain(|(at, p)| {
        if *at <= now {
            ready.push(p.clone());
            false
        } else {
            true
        }
    });
    for plan in ready {
        // 硬规矩：`rr3_reg_live` 先 `let` 绑，绝不写进被匹配表达式/守卫（文件头那段）。
        // 这一问挡住的是"外来模块自己回来了"那一格（A-5② 的同名窗口）：判死到到期之间它若
        // 重新注册并喂了狗，再起一份就是拿同名注册去撞 EBUSY。
        let live = rr3_lock(shared).rr3_reg_live(&plan.name);
        if live {
            rr3_log(&format!("取消重启 {}：已有在册实例", plan.name));
            continue;
        }
        // E-1：**只有本守护起的进程才 spawn 回来**。`jobs` 是"我能起谁"的全集，
        // 判据就是这一条查询（外来模块的计划在这里被跳过，连它的 entry 都不碰）。
        // 这里不"统一"成"照 plan.entry 硬起"：那要求 rrd 知道该给外来进程注入什么身份（E-2）。
        let job = jobs.get(&plan.name).cloned();
        match job {
            Some(job) => rr3_boot_spawn(
                shared,
                t0,
                children,
                due,
                &job,
                plan.attempt,
                bus_path,
                boot_role,
            ),
            None => rr3_log(&format!("跳过重启 {}：不是本守护起的进程", plan.name)),
        }
    }
}

/// 起唯一那条 supervisor 线程。`sweep` 复用 `--watchdog-tick-ms`（E-5：生产 200ms / 测试 20ms），
/// 不新增 CLI 旋钮。
pub fn rr3_spawn_supervisor(
    rx: Receiver<Rr3SuperMsg>,
    shared: Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    sweep: Duration,
    bus_path: PathBuf,
    boot_role: String,
) -> JoinHandle<()> {
    thread::spawn(move || {
        // 与 watchdog 同一口径：日志打在**线程内**，它证明的是"线程真的跑起来过"，
        // 而不是"走到了 spawn 那一行"。
        rr3_log(&format!("supervisor 启动 sweep={}ms", sweep.as_millis()));
        let mut jobs: BTreeMap<String, Rr3SpawnJob> = BTreeMap::new();
        let mut children: BTreeMap<String, Child> = BTreeMap::new();
        let mut due: Vec<(Instant, Rr3RestartPlan)> = Vec::new();
        loop {
            // 一拍等多久：`sweep` 与"最近一个到期点"取小者。
            // 为什么不能只用 `sweep`：`recv_timeout` 一收到消息就立刻醒，醒完这一轮又 park 回
            // `sweep` —— 而既有 e2e 线级用例的 tick 是 1 小时（`RR3_QUIET_WATCHDOG_TICK_MS`），
            // 那样"20ms 后到期的重启"永远等不到 `rr3_fire_due` 被叫到，E-1 那句 `跳过重启`
            // 也就永远打不出来（`e2e_sim_peer` 那条对照用例正面等的就是它）。
            // 取小者同时把 E-4 补全：退避表是**到期时刻**队列，到点了就得有人看表；
            // "没东西到期时这一拍仍等于 `--watchdog-tick-ms`"这句话没被改动（E-5 的接线照旧）。
            let wait = match due.iter().map(|(at, _)| *at).min() {
                Some(at) => sweep.min(at.saturating_duration_since(Instant::now())),
                None => sweep,
            };
            // 一次收件即一拍：`Timeout` 就是"这一拍没信"（节拍本身）。
            let mut keep_running = true;
            match rx.recv_timeout(wait) {
                Ok(Rr3SuperMsg::Boot(job)) => {
                    // 先登记"我能起谁"再起第一世：`rr3_reap` 里那句 `expect("children 里的 name
                    // 必有 job")` 就建立在这个顺序上（`children` 的键只可能来自这里）。
                    // 这里不打日志：可见证据是 `rr3_boot_spawn` 的 `spawn …` 那一行，
                    // 多打一条"登记了"只是把同一件事说两遍（日志面每多一行就要多审一次 needle）。
                    let for_spawn = job.clone();
                    jobs.insert(job.facts.name.clone(), job);
                    rr3_boot_spawn(
                        &shared,
                        t0,
                        &mut children,
                        &mut due,
                        &for_spawn,
                        0,
                        &bus_path,
                        &boot_role,
                    );
                }
                Ok(Rr3SuperMsg::Restart(plan)) => rr3_schedule_due(&mut due, plan),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    // brief 把 Disconnected 也当"这一拍没信"，但它是**永久**状态：
                    // `recv_timeout` 在已断开的通道上立即返回 Err，于是"每拍等 sweep"退化成
                    // 100% 忙等（正是本仓拒 `--watchdog-tick-ms 0` 的那条理由）。
                    // 现值：daemon 永不 shutdown（`incoming()` 不返回），而 watchdog 与每条 conn
                    // 各持一份 Sender，所以这一支在真路径上不可达；真到了那一刻说明进程正在收尾，
                    // 剩下的子进程由 rrd 退出与 `e2e_spawn_restart` 的 S5（孤儿卫生）负责。
                    // 冲突已写进实施报告：这一支按"退出而不忙等"实现，语义与 brief 相同
                    //（都不再处理新请求），只是不烧核。
                    rr3_log("supervisor 退出：已无投递方（不再忙等）");
                    keep_running = false;
                }
            }
            rr3_reap(&shared, t0, &jobs, &mut children, &mut due);
            rr3_fire_due(
                &shared,
                t0,
                &jobs,
                &mut children,
                &mut due,
                &bus_path,
                &boot_role,
            );
            if !keep_running {
                break;
            }
        }
    })
}
