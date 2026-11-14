//! rr3-sentinel-harness —— 批D1 层③ 哨兵的宿主对手件。
//!
//! 扮演**总线侧**（rrd 位形，走 rr-ipc 的 Rr3UartConn + blanket conn 同源帧语义）：
//! ① 收哨兵的 `rr3.register` → 校验形状 → 回 `{"src_task": 2}`；② 收 N 拍
//! `rr3.heartbeat` → 逐拍回 `{"beat": "accepted"}` 并记间隔；③ 发一条 `rr3.call`
//! Request（rrd→模块下行转发同款形状）→ 校验哨兵 Response 的 id 回显与
//! result.echo 原样回带。判据输出 PASS/FAIL 逐条点名，退出码 0/1（2 = 环境性
//! 失败：设备打不开 / 看门狗到点）。D1 层③ 板上判据的宿主半边。
//!
//! 批G 扩面（④⑦ 列）：④ 安全核逐拍对拍——下行 `rr3.call` 点名
//! `rr3-sentinel/diag/parity`（rrd 转发实锤形状：method 不改写、topic 在
//! params，conn.rs rr3_route_call），板回向量与**宿主同一 runner**
//! `rr_core::control::parity::rr3_parity_run()` 的输出（确定性腿）及
//! `RR3_PARITY_EXPECTED`（契约期望腿）三方两腿对拍；⑦ 时序达标——心跳间隔
//! 直方图与 `[supplies]` 名义值对表（tick_period_ms=heartbeat_ms /
//! tick_jitter_ms=100 出自 profiles/esp32-mini.toml），可留证据文件（C12 三样
//! 契约：环境指纹/采样窗口/样本数/原始数据，rustc -Vv 由操作面追记）。
//!
//! 用法：`cargo run -p rr3-sentinel-harness -- <串口设备> [拍数，缺省 8] [证据文件路径]`
//! （macOS：/dev/tty.usbserial-*；树莓派：/dev/ttyUSB0。板侧 monitor 先行，
//! 再跑本件；看门狗随拍数动态放宽。）

use std::env;
use std::time::Instant;

use args::rr3_args;
use probe::{rr3_parity_judge, rr3_parity_wait, rr3_rt_checks};
use report::{rr3_evidence_write, rr3_exit_summary, rr3_now_unix};
use run::{rr3_beats, rr3_open, rr3_register_checks, rr3_timing, rr3_watchdog};

mod args;
mod probe;
mod report;
mod run;
#[cfg(test)]
mod tests;

const ROUND_TRIP_ID: u64 = 9001;
/// parity 点名的 JSON-RPC id（与 ③ probe 的 9001 区分，两段可独立点名）。
const PARITY_CALL_ID: u64 = 9002;
/// 全程看门狗基数：在拍数动态项之外的基础开销（注册 + round-trip + parity 对拍）。
const WATCHDOG_BASE_SECS: u64 = 30;
/// 每拍看门狗预算（hb 名义 + 传输/调度余量的宽松上界）。
const WATCHDOG_PER_BEAT_MS: u64 = 1_000;
/// ⑦ 对表判据（profiles/esp32-mini.toml `[supplies]` 声明：tick_period_ms=200
/// 即 heartbeat_ms、tick_jitter_ms=100）：单拍间隔允许窗 = [period-50, period+jitter]；
/// 均值窗 = [period-20, period+50]。下限防"拍点漂移早发"，上限就是声明的抖动预算。
const BEAT_FLOOR_SLACK_MS: i64 = 50;
const BEAT_AVG_FLOOR_SLACK_MS: i64 = 20;
const BEAT_AVG_CEIL_SLACK_MS: i64 = 50;
/// 心跳停摆判据（单拍间隔上界；200ms 名义值的数量级护栏，抖动不误伤）。
const HEARTBEAT_STALL_FAIL_MS: u128 = 5_000;
/// harness 分配给哨兵的 task 号（模块池起点，rr-core::RR3_TASK_FIRST_MODULE）。
const HARNESS_TASK: u64 = 2;

fn main() {
    let t0 = Instant::now();
    let t0_unix = rr3_now_unix();
    let args: Vec<String> = env::args().collect();
    let (device, beats, evidence_path) = rr3_args(&args);
    rr3_watchdog(t0, beats);
    let now_ns = || t0.elapsed().as_nanos() as u64;
    let mut fail = 0u32;
    let mut conn = rr3_open(&device);

    let (reg_fail, hb_ms) = rr3_register_checks(&mut conn, &now_ns);
    fail += reg_fail;

    let intervals = rr3_beats(&mut conn, beats, &now_ns);
    let (hist, timing_fail) = rr3_timing(&intervals, beats, hb_ms);
    fail += timing_fail;

    fail += rr3_rt_checks(&mut conn, &now_ns);

    let (board_parity, parity_ms) = rr3_parity_wait(&mut conn, &now_ns);
    fail += rr3_parity_judge(board_parity.as_ref(), parity_ms);

    // C12 证据文件（三样契约；rustc -Vv 由操作面追记）——fail 也写，原始数据留痕。
    rr3_evidence_write(
        evidence_path.as_ref(),
        &device,
        &intervals,
        &hist,
        board_parity.as_ref(),
        t0_unix,
    );

    rr3_exit_summary(fail);
}
