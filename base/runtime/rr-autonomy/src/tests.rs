// SPDX-License-Identifier: Apache-2.0
//! 自治运行核单测（自 `lib.rs` 同目录内联模块原样外置，判据不变）。

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
fn new_tasks_state() {
    let sup = Rr3Supervisor::rr3_sup_new(cfg(), 0);
    let h = sup.rr3_sup_health();
    assert_eq!(h.state, [Rr3TaskState::Starting; RR3_TASK_COUNT]);
    assert_eq!(h.ticks, 0);
    assert_eq!(h.restarts, 0);
}

#[test]
fn beat_task_alive() {
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
fn silence_dead_restart() {
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
fn beat_recovered() {
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
fn early_tick_skipped() {
    let mut sup = Rr3Supervisor::rr3_sup_new(cfg(), 0);
    assert!(sup.rr3_sup_tick(0).is_empty());
    assert_eq!(sup.rr3_sup_health().ticks, 1);
    assert!(sup.rr3_sup_tick(100).is_empty());
    assert_eq!(sup.rr3_sup_health().ticks, 1);
    assert!(sup.rr3_sup_tick(200).is_empty());
    assert_eq!(sup.rr3_sup_health().ticks, 2);
}

#[test]
fn late_tick_catch_up() {
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
fn degraded_not_reaped() {
    let mut sup = Rr3Supervisor::rr3_sup_new(cfg(), 0);
    sup.rr3_sup_beat(Rr3Task::Ui, 0);
    sup.rr3_sup_degrade(Rr3Task::Ui);
    sup.rr3_sup_tick(10_000);
    let h = sup.rr3_sup_health();
    assert_eq!(h.state[Rr3Task::Ui.index()], Rr3TaskState::Degraded);
    assert_eq!(h.timeouts[Rr3Task::Ui.index()], 0);
}

#[test]
fn arm_moves_running() {
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
fn arm_not_dead_task() {
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
fn restart_hold_ticks() {
    let mut g = Rr3RestartGate::rr3_gate_new(2);
    assert!(!g.rr3_gate_step(true));
    assert!(g.rr3_gate_step(true));
    assert!(!g.rr3_gate_step(true), "触发后清零，不重复放行");
    assert!(!g.rr3_gate_step(false));
    assert!(!g.rr3_gate_step(true));
    assert!(!g.rr3_gate_step(false));
    assert!(!g.rr3_gate_step(true), "中间一拍恢复即重新计数");
}

#[test]
fn health_json_stable() {
    let mut sup = Rr3Supervisor::rr3_sup_new(cfg(), 0);
    beat_all(&mut sup, 0);
    sup.rr3_sup_degrade(Rr3Task::Dialog);
    let j = rr3_health_json(&sup.rr3_sup_health(), "synced");
    assert!(
        j.starts_with("{\"shell\":\"on\",\"sntp\":\"synced\",\"ticks\":0,"),
        "{j}"
    );
    assert!(j.contains("\"name\":\"net\",\"state\":\"running\""), "{j}");
    assert!(
        j.contains("\"name\":\"dialog\",\"state\":\"degraded\""),
        "{j}"
    );
    assert!(j.ends_with("]}"), "{j}");
}

#[test]
fn state_cover_variants() {
    assert_eq!(rr3_state_word(Rr3TaskState::Starting), "starting");
    assert_eq!(rr3_state_word(Rr3TaskState::Running), "running");
    assert_eq!(rr3_state_word(Rr3TaskState::Degraded), "degraded");
    assert_eq!(rr3_state_word(Rr3TaskState::Dead), "dead");
}

#[test]
fn explicit_dead_once() {
    let mut sup = Rr3Supervisor::rr3_sup_new(cfg(), 0);
    sup.rr3_sup_dead(Rr3Task::Net);
    sup.rr3_sup_dead(Rr3Task::Net);
    assert_eq!(sup.rr3_sup_health().restarts, 1);
}
