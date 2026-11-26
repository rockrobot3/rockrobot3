// SPDX-License-Identifier: Apache-2.0
//! 自治运行核（supervisor-lite）：固定六任务表（net / audio-in / dialog /
//! audio-out / motion / ui，ADR-0029 裁定 2）、心跳、固定栅格看门狗与健康快照。
//!
//! 平台无关机制核（W3-1）：时间由调用方传入，零 IO、零线程；任务表与健康 JSON
//! 渲染按 `alloc` 构串，ESP-IDF 固件与宿主测试共用。
//!
//! 不变量：每任务须在 `timeouts_ms[i]` 内 beat 一次，超时置 `Dead` 并各计一次
//! 超时与重启请求；tick 按固定栅格推进，晚到不补跳、超拍记 `overruns`；
//! `Degraded` 不参与判死，`rr3_sup_dead` 置死并计一次重启请求；状态迁移全是
//! 纯函数，不读真实时钟。

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;

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

/// 复位去抖闸门（策略层的"连续 N 拍仍 dead 才动作"机制核）：每拍输入
/// "是否存在持续 dead 的已接线任务"，连续 `hold` 拍为真才返回 **一次** true
/// （随后清零，避免同一停摆重复触发）；任一拍为假即清零。
pub struct Rr3RestartGate {
    hold: u32,
    streak: u32,
}

impl Rr3RestartGate {
    pub fn rr3_gate_new(hold: u32) -> Self {
        Self {
            hold: hold.max(1),
            streak: 0,
        }
    }

    /// 一拍输入；到阈值返回 true（一次性）。
    pub fn rr3_gate_step(&mut self, dead: bool) -> bool {
        self.streak = if dead {
            self.streak.saturating_add(1)
        } else {
            0
        };
        if self.streak < self.hold {
            return false;
        }
        self.streak = 0;
        true
    }
}

/// 健康快照 → `/api/board/health` JSON（机制件：形状单点，宿主测试可钉）。
///
/// `sntp` 词表由调用方给（"unknown"/"synced"/"failed"——时间源是策略面）；
/// 任务顺序恒为 [`Rr3Task::ALL`]，状态词为四态小写（判别值禁止重排）。
pub fn rr3_health_json(h: &Rr3SupHealth, sntp: &str) -> String {
    let mut tasks = String::new();
    for task in Rr3Task::ALL {
        let i = task.index();
        if i > 0 {
            tasks.push(',');
        }
        tasks.push_str(&format!(
            "{{\"name\":\"{}\",\"state\":\"{}\",\"timeouts\":{}}}",
            task.name(),
            rr3_state_word(h.state[i]),
            h.timeouts[i]
        ));
    }
    format!(
        "{{\"shell\":\"on\",\"sntp\":\"{sntp}\",\"ticks\":{},\"overruns\":{},\"restarts\":{},\"tasks\":[{tasks}]}}",
        h.ticks, h.overruns, h.restarts
    )
}

/// 任务状态词（四态，判别值即下标；禁止重排）。
pub fn rr3_state_word(v: Rr3TaskState) -> &'static str {
    const WORDS: [&str; 4] = ["starting", "running", "degraded", "dead"];
    WORDS[v as usize]
}

#[cfg(test)]
mod tests;
