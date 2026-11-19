//! 自治运行核（supervisor-lite）：固定任务表、心跳、固定栅格看门狗与健康快照。
//!
//! 平台无关机制核（W3-1）：时间由调用方传入，零 IO、零线程、零分配；ESP-IDF
//! 固件与宿主测试共用。任务表固定六项（net / audio-in / dialog / audio-out /
//! motion / ui），与 ADR-0029 裁定 2 一致。
//!
//! 语义：
//! - 每个任务在 `timeouts_ms[i]` 内必须至少 beat 一次；超时即置 `Dead`，
//!   超时计数与重启请求各加一，由策略层决定如何重启。
//! - tick 按固定栅格推进；调用晚于栅格不补跳，超拍记 `overruns`。
//! - 显式降级（`Degraded`）不参与超时判死；显式致命（`rr3_sup_dead`）置
//!   `Dead` 并计一次重启请求。
//! - 所有状态迁移都是纯函数可测，不依赖真实时钟。

#![no_std]

/// 任务表容量（六项固定）。
pub const RR3_TASK_COUNT: usize = 6;

/// 自治任务（顺序即任务表下标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3Task {
    Net,
    AudioIn,
    Dialog,
    AudioOut,
    Motion,
    Ui,
}

impl Rr3Task {
    pub const ALL: [Self; RR3_TASK_COUNT] = [
        Self::Net,
        Self::AudioIn,
        Self::Dialog,
        Self::AudioOut,
        Self::Motion,
        Self::Ui,
    ];

    pub fn index(self) -> usize {
        match self {
            Self::Net => 0,
            Self::AudioIn => 1,
            Self::Dialog => 2,
            Self::AudioOut => 3,
            Self::Motion => 4,
            Self::Ui => 5,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Net => "net",
            Self::AudioIn => "audio-in",
            Self::Dialog => "dialog",
            Self::AudioOut => "audio-out",
            Self::Motion => "motion",
            Self::Ui => "ui",
        }
    }
}

/// 任务状态（四态；判别值供渲染面直接下标，禁止重排）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Rr3TaskState {
    /// 已登记、尚未首次心跳
    Starting = 0,
    /// 心跳在位
    Running = 1,
    /// 显式降级（不判死）
    Degraded = 2,
    /// 超时或显式死亡，等待重启
    Dead = 3,
}

/// 监督器配置（策略量由装配注入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3SupCfg {
    /// 固定栅格周期（ms）
    pub tick_ms: u64,
    /// 各任务心跳超时（ms），下标同 [`Rr3Task::index`]
    pub timeouts_ms: [u64; RR3_TASK_COUNT],
}

impl Default for Rr3SupCfg {
    fn default() -> Self {
        Self {
            tick_ms: 200,
            timeouts_ms: [5_000, 1_000, 10_000, 1_000, 1_000, 2_000],
        }
    }
}

/// 一次 tick 的变更摘要（位图，下标同任务表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rr3SupEvent {
    pub timed_out: u8,
    pub recovered: u8,
}

impl Rr3SupEvent {
    pub fn is_empty(self) -> bool {
        self.timed_out == 0 && self.recovered == 0
    }
}

/// 健康快照（供 `/health` 与日志渲染）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3SupHealth {
    pub state: [Rr3TaskState; RR3_TASK_COUNT],
    pub timeouts: [u32; RR3_TASK_COUNT],
    pub ticks: u64,
    pub overruns: u32,
    pub restarts: u32,
}

/// 监督器（每进程一份；单线程调用）。
pub struct Rr3Supervisor {
    cfg: Rr3SupCfg,
    next_tick_ms: u64,
    last_beat_ms: [u64; RR3_TASK_COUNT],
    state: [Rr3TaskState; RR3_TASK_COUNT],
    timeouts: [u32; RR3_TASK_COUNT],
    ticks: u64,
    overruns: u32,
    restarts: u32,
}

impl Rr3Supervisor {
    /// 构造：所有任务以 `now_ms` 为起点、状态 `Starting`。
    pub fn rr3_sup_new(cfg: Rr3SupCfg, now_ms: u64) -> Self {
        let mut cfg = cfg;
        cfg.tick_ms = cfg.tick_ms.max(1);
        for t in &mut cfg.timeouts_ms {
            *t = t_floor(*t);
        }
        Self {
            cfg,
            next_tick_ms: now_ms,
            last_beat_ms: [now_ms; RR3_TASK_COUNT],
            state: [Rr3TaskState::Starting; RR3_TASK_COUNT],
            timeouts: [0; RR3_TASK_COUNT],
            ticks: 0,
            overruns: 0,
            restarts: 0,
        }
    }

    /// 心跳：置 `Running`；若此前为 `Dead`，返回 true（恢复）。
    pub fn rr3_sup_beat(&mut self, task: Rr3Task, now_ms: u64) -> bool {
        let i = task.index();
        let recovered = self.state[i] == Rr3TaskState::Dead;
        self.state[i] = Rr3TaskState::Running;
        self.last_beat_ms[i] = now_ms;
        recovered
    }

    /// 显式降级：不参与超时判死。
    pub fn rr3_sup_degrade(&mut self, task: Rr3Task) {
        let i = task.index();
        if self.state[i] != Rr3TaskState::Dead {
            self.state[i] = Rr3TaskState::Degraded;
        }
    }

    /// 登记为已接线（`Starting`，以 `now_ms` 起算首个超时窗）：与
    /// [`Self::rr3_sup_degrade`] 互逆；**不置 `Running`、不伪造心跳**——首次
    /// 真实心跳到达前仍会被判死，`Dead` 也不因 arm 复活。
    pub fn rr3_sup_arm(&mut self, task: Rr3Task, now_ms: u64) {
        let i = task.index();
        if self.state[i] != Rr3TaskState::Dead {
            self.state[i] = Rr3TaskState::Starting;
            self.last_beat_ms[i] = now_ms;
        }
    }

    /// 显式致命：置 `Dead` 并计一次重启请求（不重复计数已死的任务）。
    pub fn rr3_sup_dead(&mut self, task: Rr3Task) {
        let i = task.index();
        if self.state[i] != Rr3TaskState::Dead {
            self.state[i] = Rr3TaskState::Dead;
            self.restarts = self.restarts.saturating_add(1);
        }
    }

    /// 固定栅格 tick：未到下一次栅格返回空事件；到点则扫描全部任务。
    pub fn rr3_sup_tick(&mut self, now_ms: u64) -> Rr3SupEvent {
        if now_ms < self.next_tick_ms {
            return Rr3SupEvent::default();
        }
        let late = now_ms - self.next_tick_ms;
        let steps = late / self.cfg.tick_ms + 1;
        if steps > 1 {
            let missed = (steps - 1).min(u32::MAX as u64) as u32;
            self.overruns = self.overruns.saturating_add(missed);
        }
        self.next_tick_ms = self
            .next_tick_ms
            .saturating_add(steps.saturating_mul(self.cfg.tick_ms));
        self.ticks = self.ticks.saturating_add(1);
        self.rr3_sup_scan(now_ms)
    }

    /// 扫描任务心跳；返回超时位图。
    fn rr3_sup_scan(&mut self, now_ms: u64) -> Rr3SupEvent {
        let mut ev = Rr3SupEvent::default();
        for task in Rr3Task::ALL {
            let i = task.index();
            if self.state[i] == Rr3TaskState::Dead || self.state[i] == Rr3TaskState::Degraded {
                continue;
            }
            let silent = now_ms.saturating_sub(self.last_beat_ms[i]);
            if silent >= self.cfg.timeouts_ms[i] {
                self.state[i] = Rr3TaskState::Dead;
                self.timeouts[i] = self.timeouts[i].saturating_add(1);
                self.restarts = self.restarts.saturating_add(1);
                ev.timed_out |= 1u8 << i;
            }
        }
        ev
    }

    /// 健康快照。
    pub fn rr3_sup_health(&self) -> Rr3SupHealth {
        Rr3SupHealth {
            state: self.state,
            timeouts: self.timeouts,
            ticks: self.ticks,
            overruns: self.overruns,
            restarts: self.restarts,
        }
    }
}

/// 超时下界（0 视为 1ms，避免配置 0 导致首拍即判死）。
fn t_floor(ms: u64) -> u64 {
    ms.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Rr3SupCfg {
        Rr3SupCfg {
            tick_ms: 200,
            timeouts_ms: [5_000, 1_000, 10_000, 1_000, 1_000, 2_000],
        }
    }

    /// 只让 `task` 参与判死，其余任务给足超时——单测隔离用。
    fn cfg_only(task: Rr3Task, timeout_ms: u64) -> Rr3SupCfg {
        let mut c = Rr3SupCfg {
            tick_ms: 200,
            timeouts_ms: [100_000; RR3_TASK_COUNT],
        };
        c.timeouts_ms[task.index()] = timeout_ms;
        c
    }

    /// 全员先 beat 一次，避免未 beat 的 Starting 任务干扰断言。
    fn beat_all(sup: &mut Rr3Supervisor, now_ms: u64) {
        for t in Rr3Task::ALL {
            sup.rr3_sup_beat(t, now_ms);
        }
    }

    #[test]
    fn new_starts_all_tasks_in_starting_state() {
        let sup = Rr3Supervisor::rr3_sup_new(cfg(), 0);
        let h = sup.rr3_sup_health();
        assert_eq!(h.state, [Rr3TaskState::Starting; RR3_TASK_COUNT]);
        assert_eq!(h.ticks, 0);
        assert_eq!(h.restarts, 0);
    }

    #[test]
    fn beat_within_window_keeps_task_alive() {
        let mut sup = Rr3Supervisor::rr3_sup_new(cfg_only(Rr3Task::AudioIn, 1_000), 0);
        beat_all(&mut sup, 0);
        assert!(!sup.rr3_sup_beat(Rr3Task::AudioIn, 100));
        assert!(sup.rr3_sup_tick(200).is_empty());
        assert!(!sup.rr3_sup_beat(Rr3Task::AudioIn, 300));
        assert!(sup.rr3_sup_tick(800).is_empty());
        let h = sup.rr3_sup_health();
        assert_eq!(h.state[Rr3Task::AudioIn.index()], Rr3TaskState::Running);
    }

    #[test]
    fn silence_past_timeout_marks_dead_and_counts_restart() {
        let mut sup = Rr3Supervisor::rr3_sup_new(cfg_only(Rr3Task::AudioIn, 1_000), 0);
        beat_all(&mut sup, 0);
        sup.rr3_sup_beat(Rr3Task::AudioIn, 100);
        let ev = sup.rr3_sup_tick(1_100);
        assert_eq!(ev.timed_out, 1 << Rr3Task::AudioIn.index());
        let h = sup.rr3_sup_health();
        assert_eq!(h.state[Rr3Task::AudioIn.index()], Rr3TaskState::Dead);
        assert_eq!(h.timeouts[Rr3Task::AudioIn.index()], 1);
        assert_eq!(h.restarts, 1);
    }

    #[test]
    fn beat_after_timeout_reports_recovered() {
        let mut sup = Rr3Supervisor::rr3_sup_new(cfg_only(Rr3Task::Motion, 1_000), 0);
        beat_all(&mut sup, 0);
        sup.rr3_sup_tick(1_200);
        assert!(sup.rr3_sup_beat(Rr3Task::Motion, 1_300));
        assert_eq!(
            sup.rr3_sup_health().state[Rr3Task::Motion.index()],
            Rr3TaskState::Running
        );
    }

    #[test]
    fn early_tick_is_skipped() {
        let mut sup = Rr3Supervisor::rr3_sup_new(cfg(), 0);
        assert!(sup.rr3_sup_tick(0).is_empty());
        assert_eq!(sup.rr3_sup_health().ticks, 1);
        assert!(sup.rr3_sup_tick(100).is_empty());
        assert_eq!(sup.rr3_sup_health().ticks, 1);
        assert!(sup.rr3_sup_tick(200).is_empty());
        assert_eq!(sup.rr3_sup_health().ticks, 2);
    }

    #[test]
    fn late_tick_counts_overruns_without_catch_up() {
        let mut sup = Rr3Supervisor::rr3_sup_new(cfg(), 0);
        sup.rr3_sup_tick(1_000); // 晚 5 拍：missed = 5
        let h = sup.rr3_sup_health();
        assert_eq!(h.overruns, 5);
        assert_eq!(h.ticks, 1);
        // 下一拍按 1200 起算，不补跳
        assert!(sup.rr3_sup_tick(1_100).is_empty());
        assert!(sup.rr3_sup_tick(1_200).is_empty());
        assert_eq!(sup.rr3_sup_health().ticks, 2);
    }

    #[test]
    fn degraded_task_is_not_reaped() {
        let mut sup = Rr3Supervisor::rr3_sup_new(cfg(), 0);
        sup.rr3_sup_beat(Rr3Task::Ui, 0);
        sup.rr3_sup_degrade(Rr3Task::Ui);
        sup.rr3_sup_tick(10_000);
        let h = sup.rr3_sup_health();
        assert_eq!(h.state[Rr3Task::Ui.index()], Rr3TaskState::Degraded);
        assert_eq!(h.timeouts[Rr3Task::Ui.index()], 0);
    }

    #[test]
    fn arm_moves_degraded_to_starting_without_faking_running() {
        let mut sup = Rr3Supervisor::rr3_sup_new(cfg_only(Rr3Task::AudioIn, 1_000), 0);
        sup.rr3_sup_degrade(Rr3Task::AudioIn);
        sup.rr3_sup_arm(Rr3Task::AudioIn, 500);
        assert_eq!(
            sup.rr3_sup_health().state[Rr3Task::AudioIn.index()],
            Rr3TaskState::Starting
        );
        let ev = sup.rr3_sup_tick(1_600);
        assert_eq!(ev.timed_out, 1 << Rr3Task::AudioIn.index());
        assert_eq!(
            sup.rr3_sup_health().state[Rr3Task::AudioIn.index()],
            Rr3TaskState::Dead
        );
    }

    #[test]
    fn arm_does_not_revive_dead_task() {
        let mut sup = Rr3Supervisor::rr3_sup_new(cfg_only(Rr3Task::Net, 1_000), 0);
        beat_all(&mut sup, 0);
        sup.rr3_sup_tick(1_200);
        sup.rr3_sup_arm(Rr3Task::Net, 1_300);
        assert_eq!(
            sup.rr3_sup_health().state[Rr3Task::Net.index()],
            Rr3TaskState::Dead
        );
    }

    #[test]
    fn explicit_dead_counts_restart_once() {
        let mut sup = Rr3Supervisor::rr3_sup_new(cfg(), 0);
        sup.rr3_sup_dead(Rr3Task::Net);
        sup.rr3_sup_dead(Rr3Task::Net);
        assert_eq!(sup.rr3_sup_health().restarts, 1);
    }
}
