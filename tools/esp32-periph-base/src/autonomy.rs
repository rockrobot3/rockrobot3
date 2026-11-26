// SPDX-License-Identifier: Apache-2.0
//! 自治壳：把 `rr-autonomy` 监督核接到板侧，提供 `/api/board/health` 真状态与
//! "接线任务心跳 → 判死 → 整机复位"闭环。
//!
//! 接线范围：net / audio-in / motion / ui 为真心跳；`tts` 档另接 `audio-out`；
//! `dialog` 尚未落地，显式 `rr3_sup_degrade` 不参与判死——不伪造心跳，也不把
//! "未实现"报成故障。
//!
//! 心跳通道：按任务位图（[`BEATS`]）合并——生产者只做一次原子 OR，不排队、不丢拍、
//! 不取锁；监督线程每 200 ms 栅格 `swap` 清零后逐位续命。音频与运行线程的心跳成本
//! 与耗时无关，不会把音频路径拖进锁竞争。
//! 重启策略（判死归 `rr-autonomy`，动作归本壳）：有过心跳的已接线任务持续 dead 才
//! 复位（`ever` 闸门）——从未起过的任务如实留在 dead，不制造重启环；连续
//! [`DEAD_TICKS`] 拍（≈400 ms）仍 dead 才动作。Rust 无法安全终止卡在 IDF 阻塞调用里
//! 的线程，故重启粒度是整机 `esp_restart`；`/api/board/health` 只读本模块静态快照。

use std::sync::atomic::{AtomicBool, AtomicI8, AtomicU8, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use rr_autonomy::{
    Rr3RestartGate, Rr3SupCfg, Rr3SupHealth, Rr3Supervisor, Rr3Task, Rr3TaskState, RR3_TASK_COUNT,
};

/// SNTP 状态（未知/已同步/失败）。
pub(crate) const SNTP_UNKNOWN: i8 = 0;
pub(crate) const SNTP_SYNCED: i8 = 1;
pub(crate) const SNTP_FAILED: i8 = -1;

/// audio-out 看门狗窗：播放线程空转心跳周期 [`crate::board::PLAY_IDLE_MS`] 的
/// 3 倍余量. 旧值 1000 ms 与空转周期相等，调度抖动即被误判 dead（W4-2 真板
/// 2026-11-20 实录：boot 后连续两次 `心跳超时 task=audio-out` 又立即恢复）。
#[cfg(feature = "tts")]
const AUDIO_OUT_TIMEOUT_MS: u64 = 3_000;
#[cfg(not(feature = "tts"))]
const AUDIO_OUT_TIMEOUT_MS: u64 = 1_000;

/// 判据在编译期成立：看门狗窗必须 ≥ 空转心跳的两倍，否则空闲即误判。
#[cfg(feature = "tts")]
const _: () = assert!(
    AUDIO_OUT_TIMEOUT_MS >= crate::board::PLAY_IDLE_MS * 2,
    "audio-out 看门狗窗必须 ≥ 空转心跳周期的 2 倍"
);

/// 监督策略量（板侧）：栅格 200 ms；超时按各任务真实节拍留裕量。
/// net 15 s（含 boot 窗内 DHCP 等连）；audio-in 1.5 s（read 批 ≈32 ms）；
/// motion/ui 4 s（SCAN 单点 2.4 s，进度点心跳见 runtime）；audio-out 见
/// [`AUDIO_OUT_TIMEOUT_MS`]；dialog 10 s 但 degraded 下不参与判死。
const SUP_CFG: Rr3SupCfg = Rr3SupCfg {
    tick_ms: 200,
    timeouts_ms: [15_000, 1_500, 10_000, AUDIO_OUT_TIMEOUT_MS, 4_000, 4_000],
};

/// 板侧已接线任务（心跳真值来源）。`audio-out`/`dialog` 只在 tts 档存在
/// （W4-2 播放面 + W6 对话 worker）；未编入时保持 degraded，不把"没有这条腿"
/// 报成故障。
#[cfg(feature = "tts")]
const WIRED: [Rr3Task; 6] = [
    Rr3Task::Net,
    Rr3Task::AudioIn,
    Rr3Task::Dialog,
    Rr3Task::AudioOut,
    Rr3Task::Motion,
    Rr3Task::Ui,
];
#[cfg(not(feature = "tts"))]
const WIRED: [Rr3Task; 4] = [Rr3Task::Net, Rr3Task::AudioIn, Rr3Task::Motion, Rr3Task::Ui];

/// 连续判死拍数（200 ms × 2 ≈ 400 ms 去抖，避免瞬态误复位）。
const DEAD_TICKS: u32 = 2;

static SNTP_STATE: AtomicI8 = AtomicI8::new(SNTP_UNKNOWN);
static SNAP: Mutex<Option<Rr3SupHealth>> = Mutex::new(None);
/// 心跳位图：bit = 任务下标。生产者 `fetch_or`，监督线程 `swap(0)` 取走。
static BEATS: AtomicU8 = AtomicU8::new(0);
/// 监督壳是否在跑——默认固件不跑，`rr3_beat` 直接短路（零原子写）。
static SUP_ON: AtomicBool = AtomicBool::new(false);

/// 起自治监督壳：六任务先标 degraded，再接 `WIRED`（`Starting`，首窗起算）；
/// 按 [`SUP_CFG`] 栅格 tick，健康快照供 `/api/board/health`。
pub(crate) fn rr3_start() {
    let now = crate::state::now_ms();
    let mut sup = Rr3Supervisor::rr3_sup_new(SUP_CFG, now);
    for task in Rr3Task::ALL {
        sup.rr3_sup_degrade(task);
    }
    for task in WIRED {
        sup.rr3_sup_arm(task, now);
    }
    SUP_ON.store(true, Ordering::Release);
    let wired = rr3_names(WIRED.iter().fold(0u8, |m, t| m | 1 << t.index()));
    #[cfg(feature = "tts")]
    let tail = "dialog 接线（W6）";
    #[cfg(not(feature = "tts"))]
    let tail = "dialog degraded（无 tts 档）";
    println!(
        "[autonomy] 监督壳起：栅格 {}ms；已接线 {wired}；{tail}",
        SUP_CFG.tick_ms
    );
    let spawned = thread::Builder::new()
        .name("autonomy".into())
        .stack_size(crate::board::THREAD_STACK)
        .spawn(move || rr3_tick_loop(sup));
    if let Err(e) = spawned {
        println!("[autonomy] 起监督壳线程失败: {e:?}");
    }
}

/// 单任务心跳（任意线程可调）：置位图，永不阻塞；监督壳未启动时为空操作。
pub(crate) fn rr3_beat(task: Rr3Task) {
    if !SUP_ON.load(Ordering::Acquire) {
        return;
    }
    BEATS.fetch_or(1 << task.index(), Ordering::Release);
}

/// 运行面（motion + ui 同居一线程）联合心跳：该线程的任一进度点都代表两任务在位。
pub(crate) fn rr3_beat_rt() {
    rr3_beat(Rr3Task::Motion);
    rr3_beat(Rr3Task::Ui);
}

/// SNTP 线程回写状态（供 `/health` 读）。
pub(crate) fn rr3_sntp_set(state: i8) {
    SNTP_STATE.store(state, Ordering::Relaxed);
}

/// 监督栅格：先排空心跳，再 tick 判死并发布快照，最后走去抖复位判定。
fn rr3_tick_loop(mut sup: Rr3Supervisor) {
    let mut ever = [false; RR3_TASK_COUNT];
    let mut gate = Rr3RestartGate::rr3_gate_new(DEAD_TICKS);
    loop {
        thread::sleep(Duration::from_millis(SUP_CFG.tick_ms));
        rr3_drain(&mut sup, &mut ever);
        let ev = sup.rr3_sup_tick(crate::state::now_ms());
        rr3_log_timeout(ev.timed_out, &ever);
        health_publish(sup.rr3_sup_health());
        rr3_dead_step(&mut gate, &sup.rr3_sup_health(), &ever);
    }
}

/// 心跳排空：位图一次性取走、逐位续命；`Dead → Running` 打恢复行（每次停摆
/// 恰一行，不刷屏）。
fn rr3_drain(sup: &mut Rr3Supervisor, ever: &mut [bool; RR3_TASK_COUNT]) {
    let bits = BEATS.swap(0, Ordering::AcqRel);
    if bits == 0 {
        return;
    }
    let now = crate::state::now_ms();
    for task in Rr3Task::ALL {
        if bits & (1 << task.index()) == 0 {
            continue;
        }
        ever[task.index()] = true;
        if sup.rr3_sup_beat(task, now) {
            println!("[autonomy] 心跳恢复 task={} now={now}ms", task.name());
        }
    }
}

/// 把快照写入静态槽（锁窗只做 Copy，不组串）。
fn health_publish(h: Rr3SupHealth) {
    if let Ok(mut slot) = SNAP.lock() {
        *slot = Some(h);
    }
}

/// 超时点名（仅在 Dead 迁移那一拍触发）。**按是否曾有过心跳分两行**：曾心跳
/// 的才是"失联"（连续 DEAD_TICKS 拍会复位）；从未心跳的是"未启动/降级"，
/// 只登记观测、不参与复位——旧版对两者都承诺复位，观测面失真。
fn rr3_log_timeout(bits: u8, ever: &[bool; RR3_TASK_COUNT]) {
    if bits == 0 {
        return;
    }
    let (lost, never) = rr3_split_bits(bits, ever);
    if lost != 0 {
        println!(
            "[autonomy] 心跳超时 task={}（曾有心跳，连续 {DEAD_TICKS} 拍不恢复即整机复位）",
            rr3_names(lost)
        );
    }
    if never != 0 {
        println!(
            "[autonomy] 任务未启动/降级 task={}（从未有心跳；不参与复位，查启动日志的 OOM/门禁点名）",
            rr3_names(never)
        );
    }
}

/// 位图按"曾心跳/从未心跳"拆成两半（顺序无关，仅用于点名）。
fn rr3_split_bits(bits: u8, ever: &[bool; RR3_TASK_COUNT]) -> (u8, u8) {
    let mut lost = 0u8;
    let mut never = 0u8;
    for task in Rr3Task::ALL {
        let i = task.index();
        if bits & (1 << i) == 0 {
            continue;
        }
        if ever[i] {
            lost |= 1 << i;
        } else {
            never |= 1 << i;
        }
    }
    (lost, never)
}

/// 复位去抖：已接线且**曾有过心跳**的 dead 任务连续 [`DEAD_TICKS`] 拍 → 复位；
/// 否则闸门清零。从未起过的任务留在 dead 供观测，不触发重启环。
fn rr3_dead_step(gate: &mut Rr3RestartGate, h: &Rr3SupHealth, ever: &[bool; RR3_TASK_COUNT]) {
    let bits = rr3_dead_bits(h, ever);
    if !gate.rr3_gate_step(bits != 0) {
        return;
    }
    println!(
        "[autonomy] 已接线任务持续 dead {DEAD_TICKS} 拍: {} → esp_restart",
        rr3_names(bits)
    );
    // SAFETY：esp_restart 是 IDF 全局复位调用，不返回到调用点；调用前日志与
    // 快照已落，不跨复位持有任何 Rust 状态。
    unsafe { esp_idf_sys::esp_restart() }
}

/// dead 且曾有心跳的已接线任务位图（0＝无需复位）。
fn rr3_dead_bits(h: &Rr3SupHealth, ever: &[bool; RR3_TASK_COUNT]) -> u8 {
    let mut bits = 0u8;
    for task in WIRED {
        let i = task.index();
        if h.state[i] == Rr3TaskState::Dead && ever[i] {
            bits |= 1 << i;
        }
    }
    bits
}

/// 任务位图 → 逗号分隔名串（顺序按任务表；空串＝空位图）。
fn rr3_names(bits: u8) -> String {
    let mut names = String::new();
    for task in Rr3Task::ALL {
        if bits & (1 << task.index()) == 0 {
            continue;
        }
        if !names.is_empty() {
            names.push(',');
        }
        names.push_str(task.name());
    }
    names
}

/// `/api/board/health` 的 JSON 体（无快照时 `shell=off`，不虚构任务状态）。
pub(crate) fn rr3_health_json() -> String {
    let sntp = SNTP_STATE.load(Ordering::Relaxed);
    let snap = SNAP.lock().ok().and_then(|s| *s);
    match snap {
        Some(h) => rr_autonomy::rr3_health_json(&h, sntp_word(sntp)),
        None => format!("{{\"shell\":\"off\",\"sntp\":\"{}\"}}", sntp_word(sntp)),
    }
}

fn sntp_word(v: i8) -> &'static str {
    match v {
        SNTP_SYNCED => "synced",
        SNTP_FAILED => "failed",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health_with(state: Rr3TaskState, timeouts: u32) -> Rr3SupHealth {
        Rr3SupHealth {
            state: [state; RR3_TASK_COUNT],
            timeouts: [timeouts; RR3_TASK_COUNT],
            ticks: 0,
            overruns: 0,
            restarts: 0,
        }
    }

    #[test]
    fn state_cover_variants() {
        assert_eq!(
            rr_autonomy::rr3_state_word(Rr3TaskState::Starting),
            "starting"
        );
        assert_eq!(
            rr_autonomy::rr3_state_word(Rr3TaskState::Running),
            "running"
        );
        assert_eq!(
            rr_autonomy::rr3_state_word(Rr3TaskState::Degraded),
            "degraded"
        );
        assert_eq!(rr_autonomy::rr3_state_word(Rr3TaskState::Dead), "dead");
    }

    #[test]
    fn sntp_three_states() {
        assert_eq!(sntp_word(SNTP_SYNCED), "synced");
        assert_eq!(sntp_word(SNTP_FAILED), "failed");
        assert_eq!(sntp_word(SNTP_UNKNOWN), "unknown");
    }

    #[test]
    fn health_json_off() {
        let j = rr3_health_json();
        assert!(j.contains("\"shell\":\"off\""), "{j}");
        assert!(!j.contains("tasks"), "无快照不得虚构任务状态");
    }

    /// 位图口径 = WIRED 任务集（feature 档不同，期望值从同一常量导出，不抄字面）。
    fn wired_mask() -> u8 {
        WIRED.iter().fold(0u8, |m, t| m | 1 << t.index())
    }

    #[test]
    fn dead_bits_real_beat() {
        let h = health_with(Rr3TaskState::Dead, 1);
        let all = rr3_dead_bits(&h, &[true; RR3_TASK_COUNT]);
        assert_eq!(all, wired_mask());
        let mut ever = [true; RR3_TASK_COUNT];
        ever[Rr3Task::AudioIn.index()] = false; // 从未起过 → 不触发复位
        let want = wired_mask() & !(1 << Rr3Task::AudioIn.index());
        assert_eq!(rr3_dead_bits(&h, &ever), want);
        let running = health_with(Rr3TaskState::Running, 0);
        assert_eq!(rr3_dead_bits(&running, &ever), 0);
    }

    #[test]
    fn dead_two_cons_ticks() {
        let h = health_with(Rr3TaskState::Dead, 1);
        let ever = [true; RR3_TASK_COUNT];
        let mut gate = Rr3RestartGate::rr3_gate_new(DEAD_TICKS);
        assert!(!gate.rr3_gate_step(rr3_dead_bits(&h, &ever) != 0));
        let alive = health_with(Rr3TaskState::Running, 0);
        assert!(!gate.rr3_gate_step(rr3_dead_bits(&alive, &ever) != 0));
        assert!(
            !gate.rr3_gate_step(rr3_dead_bits(&h, &ever) != 0),
            "清零后重新计数"
        );
    }
}
