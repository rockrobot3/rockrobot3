// SPDX-License-Identifier: Apache-2.0
//! 自治健康面集成腿（W3-3/W4-2）：按真实栅格跑监督核的状态迁移。
//!
//! 不依赖 ESP 工具链与真实时钟：240 拍 = 30 s 的喂拍/判死/恢复全在宿主跑完。
//! 判据与板侧 `/api/board/health` 同源（[`rr3_health_json`]），CI 腿
//! `tools/ci/platform/autonomy-face.sh` 直接执行本文件。

use rr_autonomy::{
    rr3_health_json, Rr3RestartGate, Rr3SupCfg, Rr3Supervisor, Rr3Task, Rr3TaskState,
    RR3_TASK_COUNT,
};

fn cfg() -> Rr3SupCfg {
    Rr3SupCfg {
        tick_ms: 200,
        timeouts_ms: [15_000, 1_500, 10_000, 1_000, 4_000, 4_000],
    }
}

fn armed() -> Rr3Supervisor {
    let mut sup = Rr3Supervisor::rr3_sup_new(cfg(), 0);
    for t in Rr3Task::ALL {
        sup.rr3_sup_degrade(t);
    }
    for t in [Rr3Task::Net, Rr3Task::AudioIn, Rr3Task::Motion, Rr3Task::Ui] {
        sup.rr3_sup_arm(t, 0);
    }
    sup
}

/// 30 s 稳态：四任务按各自节拍续命，零超时、零超拍、闸门不放行。
#[test]
fn steady_has_no_30s() {
    let mut sup = armed();
    // 构造即栅格点：先补起点拍，之后的 150 拍才是"准点"序列（否则首拍记一次超拍）。
    let _ = sup.rr3_sup_tick(0);
    let mut gate = Rr3RestartGate::rr3_gate_new(2);
    let mut now = 0u64;
    for _ in 0..150 {
        now += 200;
        if now % 1_000 == 0 {
            sup.rr3_sup_beat(Rr3Task::Net, now);
        }
        for t in [Rr3Task::AudioIn, Rr3Task::Motion, Rr3Task::Ui] {
            sup.rr3_sup_beat(t, now);
        }
        assert_eq!(sup.rr3_sup_tick(now).timed_out, 0);
        assert!(!gate.rr3_gate_step(false));
    }
    let h = sup.rr3_sup_health();
    assert_eq!(h.ticks, 151);
    assert_eq!(h.overruns, 0);
    assert_eq!(h.restarts, 0);
    assert_eq!(h.timeouts, [0; RR3_TASK_COUNT]);
    assert_eq!(h.state[Rr3Task::Net.index()], Rr3TaskState::Running);
    assert_eq!(h.state[Rr3Task::AudioIn.index()], Rr3TaskState::Running);
    assert_eq!(h.state[Rr3Task::Dialog.index()], Rr3TaskState::Degraded);
    assert!(rr3_health_json(&h, "unknown").contains("\"shell\":\"on\""));
}

/// 停摆 → 判死 → 抖闸 → 恢复：健康面按 1.5 s 判死，闸门连续 2 拍才放行。
#[test]
fn stall_restart() {
    let mut sup = armed();
    let mut gate = Rr3RestartGate::rr3_gate_new(2);
    let mut now = 0u64;
    let mut timed_out = 0u8;
    for _ in 0..10 {
        now += 200;
        for t in [Rr3Task::Net, Rr3Task::Motion, Rr3Task::Ui] {
            sup.rr3_sup_beat(t, now);
        }
        timed_out |= sup.rr3_sup_tick(now).timed_out;
    }
    assert_eq!(
        timed_out,
        1 << Rr3Task::AudioIn.index(),
        "只有 audio-in 停摆"
    );
    let h = sup.rr3_sup_health();
    assert_eq!(h.state[Rr3Task::AudioIn.index()], Rr3TaskState::Dead);
    assert_eq!(h.timeouts[Rr3Task::AudioIn.index()], 1);
    assert_eq!(h.restarts, 1);

    let dead = h.state[Rr3Task::AudioIn.index()] == Rr3TaskState::Dead;
    assert!(!gate.rr3_gate_step(dead), "首拍只计数");
    assert!(gate.rr3_gate_step(dead), "第二拍放行复位");
    assert!(sup.rr3_sup_beat(Rr3Task::AudioIn, now), "心跳恢复返回 true");
    assert_eq!(
        sup.rr3_sup_health().state[Rr3Task::AudioIn.index()],
        Rr3TaskState::Running
    );
}

/// 从未起过的已接线任务：判死并留痕；首次真实心跳到达才恢复（不伪造 Running）。
#[test]
fn never_goes_dead_beat() {
    let mut sup = armed();
    let mut now = 0u64;
    for _ in 0..10 {
        now += 200;
        for t in [Rr3Task::Net, Rr3Task::Motion, Rr3Task::Ui] {
            sup.rr3_sup_beat(t, now);
        }
        sup.rr3_sup_tick(now);
    }
    let h = sup.rr3_sup_health();
    assert_eq!(h.state[Rr3Task::AudioIn.index()], Rr3TaskState::Dead);
    assert_eq!(h.timeouts[Rr3Task::AudioIn.index()], 1);
    assert!(sup.rr3_sup_beat(Rr3Task::AudioIn, now));
    assert_eq!(
        sup.rr3_sup_health().state[Rr3Task::AudioIn.index()],
        Rr3TaskState::Running
    );
}
