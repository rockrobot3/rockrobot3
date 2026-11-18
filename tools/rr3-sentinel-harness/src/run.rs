//! 主流程与节拍：注册握手、心跳 N 拍、时序对表与看门狗。

use std::thread;
use std::time::{Duration, Instant};

use rr_ipc::transport::uart::{Rr3UartConn, RR3_UART_BAUD_DEFAULT};
use rr_ipc::transport::Rr3TransportConn;
use rr_ipc::{Rr3Frame, Rr3FrameBuf, Rr3PayloadType};
use serde_json::Value;

use crate::probe::{heartbeat_reply, parse_upstream, register_reply, rr3_recv_frame};
use crate::report::{histogram_text, verdict};
use crate::{
    BEAT_AVG_CEIL_SLACK_MS, BEAT_AVG_FLOOR_SLACK_MS, BEAT_FLOOR_SLACK_MS, HARNESS_TASK,
    HEARTBEAT_STALL_FAIL_MS, WATCHDOG_BASE_SECS, WATCHDOG_PER_BEAT_MS,
};

/// 全程看门狗：到点 exit 2（环境性失败），预算随拍数放宽。
pub(super) fn rr3_watchdog(t0: Instant, beats: usize) {
    let wd_budget = rr3_wd_budget(beats);
    thread::spawn(move || rr3_wd_loop(t0, wd_budget));
}

/// 看门狗循环体：每 200ms 查一次死线，到点 exit 2（环境性失败）。
fn rr3_wd_loop(t0: Instant, wd_budget: Duration) {
    loop {
        thread::sleep(Duration::from_millis(200));
        if t0.elapsed() > wd_budget {
            eprintln!("FAIL  harness 全程 {wd_budget:?} 看门狗到点（板侧在跑吗？线松了吗？）");
            std::process::exit(2);
        }
    }
}

/// 看门狗预算：基准 + 每拍放宽量。
fn rr3_wd_budget(beats: usize) -> Duration {
    Duration::from_secs(WATCHDOG_BASE_SECS + beats as u64 * WATCHDOG_PER_BEAT_MS / 1000)
}

/// 打开串口：打不开 = 环境性失败（exit 2，与判据 FAIL=1 分开）。
pub(super) fn rr3_open(device: &str) -> Rr3UartConn {
    match Rr3UartConn::open_with_baud(std::path::Path::new(device), RR3_UART_BAUD_DEFAULT) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("FAIL  打开 {device} 失败: {e}");
            std::process::exit(2);
        }
    }
}

/// ① 注册：首帧必须是 rr3.register，params.name/manifest 在场（rrd 握手门序同款）。
fn rr3_register(conn: &mut Rr3UartConn, now_ns: &dyn Fn() -> u64) -> (String, u64) {
    let frame = rr3_recv_frame(conn, "FAIL  首帧读取失败", "FAIL  首帧还原失败");
    let (id, params) = rr3_reg_params(frame);
    let (name, hb) = rr3_reg_fields(&params);
    rr3_register_ack(conn, id, now_ns());
    (name, hb)
}

/// 首帧必须 Request/rr3.register（形状/畸形退 1），返回 id 与 params。
fn rr3_reg_params(frame: Rr3Frame) -> (u64, Value) {
    match parse_upstream(frame) {
        Ok(up) => rr3_reg_shape(up),
        Err(e) => rr3_register_bad(&e),
    }
}

/// 已拆帧形状门：必须 Request/rr3.register，否则点名退 1。
fn rr3_reg_shape(up: (Rr3PayloadType, u64, Option<String>, Value, Value)) -> (u64, Value) {
    let (ty, id, m, params, _) = up;
    if ty == Rr3PayloadType::Request && m.as_deref() == Some("rr3.register") {
        return (id, params);
    }
    rr3_register_shape(ty, m)
}

/// 首帧形状错：点名收到什么，退 1。
fn rr3_register_shape(ty: Rr3PayloadType, m: Option<String>) -> ! {
    eprintln!(
        "FAIL  首帧必须是 Request/rr3.register，收到 {ty:?}/{}",
        m.as_deref().unwrap_or("-")
    );
    std::process::exit(1)
}

/// 首帧畸形：点名后按判据失败退 1。
fn rr3_register_bad(e: &str) -> ! {
    eprintln!("FAIL  首帧畸形: {e}");
    std::process::exit(1)
}

/// 注册应答写出（失败即点名退场）。
fn rr3_register_ack(conn: &mut Rr3UartConn, id: u64, now: u64) {
    if let Err(e) = conn.rr3_send_buf(Rr3FrameBuf::from(&register_reply(id, HARNESS_TASK, now))) {
        eprintln!("FAIL  注册应答写出失败: {e:?}");
        std::process::exit(1);
    }
}

/// 注册参数取数：manifest 必填（空即拒），name 可空；heartbeat_ms 从 manifest
/// 行面取（缺省 0）。manifest 原文先打印（现场取证）。
fn rr3_reg_fields(params: &Value) -> (String, u64) {
    let manifest = params
        .get("manifest")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if manifest.is_empty() {
        eprintln!("FAIL  注册缺 params.manifest");
        std::process::exit(1);
    }
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let hb = rr3_hb_ms(&manifest);
    println!("---- 哨兵 manifest ----\n{manifest}------------------------");
    (name, hb)
}

/// manifest 行面的 heartbeat_ms（缺省 0）。
fn rr3_hb_ms(manifest: &str) -> u64 {
    manifest
        .lines()
        .find_map(|l| l.strip_prefix("heartbeat_ms = "))
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// ② 心跳 N 拍：逐拍回 accepted，记间隔（相对节拍的外部观测；首拍间隔含
/// 注册→首拍的排拍 = heartbeat_ms）。
pub(super) fn rr3_beats(
    conn: &mut Rr3UartConn,
    beats: usize,
    now_ns: &dyn Fn() -> u64,
) -> Vec<u128> {
    let mut intervals: Vec<u128> = Vec::new();
    let mut last = Instant::now();
    for i in 0..beats {
        if let Some(dt) = rr3_beat(conn, i, &mut last, now_ns) {
            intervals.push(dt);
        }
    }
    intervals
}

/// 单拍：收帧 → 必须 Request/rr3.heartbeat → 回应答并打 PASS；返回本拍间隔
/// （ms，相对上一拍）。形状/io 违约即点名 exit 1。
fn rr3_beat(
    conn: &mut Rr3UartConn,
    i: usize,
    last: &mut Instant,
    now_ns: &dyn Fn() -> u64,
) -> Option<u128> {
    let n = i + 1;
    let frame = rr3_recv_frame(
        conn,
        &format!("FAIL  第{n}拍读取失败"),
        &format!("FAIL  第{n}拍还原失败"),
    );
    let now = Instant::now();
    let dt = now.duration_since(*last).as_millis();
    *last = now;
    rr3_beat_judge(conn, n, frame, dt, now_ns)
}

/// 单拍判据：必须 Request/rr3.heartbeat → 回应答并打 PASS；形状/畸形违约 exit 1。
fn rr3_beat_judge(
    conn: &mut Rr3UartConn,
    n: usize,
    frame: Rr3Frame,
    dt: u128,
    now_ns: &dyn Fn() -> u64,
) -> Option<u128> {
    let id = rr3_beat_id(n, frame);
    rr3_beat_ack(conn, n, id, now_ns());
    println!("PASS  ② 第{n}拍 rr3.heartbeat id={id} 间隔={dt}ms");
    Some(dt)
}

/// 单拍帧必须是 Request/rr3.heartbeat（形状/畸形违约 exit 1），返回应答 id。
fn rr3_beat_id(n: usize, frame: Rr3Frame) -> u64 {
    match parse_upstream(frame) {
        Ok((ty, id, m, _, _)) => rr3_beat_ok(n, ty, id, m),
        Err(e) => rr3_beat_bad(n, &e),
    }
}

/// 已拆帧形状门：必须 Request/rr3.heartbeat，否则点名退 1。
fn rr3_beat_ok(n: usize, ty: Rr3PayloadType, id: u64, m: Option<String>) -> u64 {
    if ty == Rr3PayloadType::Request && m.as_deref() == Some("rr3.heartbeat") {
        return id;
    }
    rr3_beat_shape(n, ty, m)
}

/// 单拍形状错：点名收到什么，退 1。
fn rr3_beat_shape(n: usize, ty: Rr3PayloadType, m: Option<String>) -> ! {
    eprintln!(
        "FAIL  第{n}拍应为 rr3.heartbeat，收到 {ty:?}/{}",
        m.as_deref().unwrap_or("-")
    );
    std::process::exit(1)
}

/// 单拍畸形：点名后按判据失败退 1。
fn rr3_beat_bad(n: usize, e: &str) -> ! {
    eprintln!("FAIL  第{n}拍畸形: {e}");
    std::process::exit(1)
}

/// 单拍应答写出（失败即点名退场）。
fn rr3_beat_ack(conn: &mut Rr3UartConn, n: usize, id: u64, now: u64) {
    if let Err(e) = conn.rr3_send_buf(Rr3FrameBuf::from(&heartbeat_reply(id, HARNESS_TASK, now))) {
        eprintln!("FAIL  第{n}拍应答写出失败: {e:?}");
        std::process::exit(1);
    }
}

/// ② 全收/无停摆 + ⑦ 时序对表（判据窗出处见 BEAT_* 常量注记）。返回
/// (直方图文本, FAIL 数)。
pub(super) fn rr3_timing(intervals: &[u128], beats: usize, hb_ms: u64) -> (String, u32) {
    let mut fail = 0u32;
    let max_dt = intervals.iter().copied().max().unwrap_or(0);
    let avg_dt = if intervals.is_empty() {
        0
    } else {
        intervals.iter().sum::<u128>() / intervals.len() as u128
    };
    fail += rr3_tally(
        intervals.len() == beats,
        &format!("② 心跳 {beats} 拍全收全应"),
    );
    fail += rr3_tally(
        max_dt <= HEARTBEAT_STALL_FAIL_MS,
        &format!(
            "② 心跳无停摆（最大间隔 {max_dt}ms ≤ {HEARTBEAT_STALL_FAIL_MS}ms；平均 {avg_dt}ms）"
        ),
    );
    let hist = histogram_text(intervals);
    println!("② 直方图(10ms桶): {hist}");
    fail += rr3_beat_window(intervals, hb_ms, max_dt as i64, avg_dt as i64);
    (hist, fail)
}

/// 单条判据计数：不过即记 1（打印与文案仍归 `verdict` 单点）。
pub(super) fn rr3_tally(ok: bool, msg: &str) -> u32 {
    if verdict(ok, msg) {
        0
    } else {
        1
    }
}

/// ⑦ 间隔对表：区间与均值窗判定，返回 FAIL 计数。
fn rr3_beat_window(intervals: &[u128], hb_ms: u64, max_i: i64, avg_i: i64) -> u32 {
    let hb = hb_ms as i64;
    let floor_ms = hb - BEAT_FLOOR_SLACK_MS;
    let ceil_ms = hb + 100;
    let avg_floor = hb - BEAT_AVG_FLOOR_SLACK_MS;
    let avg_ceil = hb + BEAT_AVG_CEIL_SLACK_MS;
    let min_i = intervals.iter().copied().min().unwrap_or(0) as i64;
    let in_window = !intervals.is_empty()
        && [
            min_i >= floor_ms,
            max_i <= ceil_ms,
            avg_i >= avg_floor,
            avg_i <= avg_ceil,
        ]
        .iter()
        .all(|ok| *ok);
    rr3_tally(
        in_window,
        &format!(
            "⑦ 间隔对表（[{floor_ms},{ceil_ms}]ms 均值[{avg_floor},{avg_ceil}]ms；\
             实测 min={min_i} avg={avg_i} max={max_i}，名义 hb={hb}ms）"
        ),
    )
}

/// ① 注册两项判据：返回 (FAIL 数, heartbeat_ms)。
pub(super) fn rr3_register_checks(conn: &mut Rr3UartConn, now_ns: &dyn Fn() -> u64) -> (u32, u64) {
    let (reg_name, hb_ms) = rr3_register(conn, now_ns);
    let fail = rr3_tally(
        !reg_name.is_empty(),
        &format!("① 注册（Request/rr3.register，name={reg_name}）"),
    ) + rr3_tally(
        hb_ms > 0,
        &format!("① manifest [health].heartbeat_ms 在场（{hb_ms}ms）"),
    );
    (fail, hb_ms)
}
