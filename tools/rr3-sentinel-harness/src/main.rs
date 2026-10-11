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
use std::thread;
use std::time::{Duration, Instant};

use rr_core::control::parity::{
    rr3_parity_run, Rr3ParityOutputs, RR3_PARITY_EXPECTED, RR3_PARITY_VERSION,
};
use rr_core::{rr3_frame_compose, RR3_TASK_RRD, RR3_TRACE_ID_DIRECT};
use rr_ipc::transport::uart::{Rr3UartConn, RR3_UART_BAUD_DEFAULT};
use rr_ipc::transport::Rr3TransportConn;
use rr_ipc::{opcode, rr3_body_encode, rr3_body_split, Rr3Frame, Rr3FrameBuf, Rr3PayloadType};
use serde_json::{json, Value};

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

/// 组一帧 rrd 位形的下行帧（SEND、直连 trace、时间戳入参注入）。src 由调用方给：
/// rrd 自答的应答帧（注册/心跳）src=rrd；`rr3.call` 转发帧按 M3 语义 src=调用方
/// task（conn.rs rr3_route_call：转发时改写 src=调用方、trace_id=调用方）——
/// harness 扮 rrd 就得扮全，下行点名 src=本 harness 的 task，哨兵应答若错把
/// dst 写成调用方 task，本件的 dst 门（parse_upstream）当场点名——E6 彩排实锤
/// 的盲区（原 src 恒为 1，reply dst=1 恰好蒙混过门）由此变红。
fn bus_frame(
    ty: Rr3PayloadType,
    src_task: u64,
    dst_task: u64,
    body: Value,
    now_ns: u64,
) -> Rr3Frame {
    rr3_frame_compose(
        opcode::SEND,
        0,
        RR3_TRACE_ID_DIRECT,
        now_ns,
        src_task,
        dst_task,
        rr3_body_encode(ty, body.to_string().as_bytes()),
    )
}

/// 拆上行帧 → (payload 类型, JSON-RPC id, method, params, 完整体)。
/// 形状不符即点名（rrd 握手门序同款：dst 门 / 前缀门 / jsonrpc 门 / id 门）。
/// method 为 `None`：Response 帧合法无 method（round-trip 应答即此形状）；
/// Request 必有——调用方按 ty 各自断言（批D1 板上实测修正：原强制门误杀 Response）。
#[allow(clippy::type_complexity)]
fn parse_upstream(
    frame: Rr3Frame,
) -> Result<(Rr3PayloadType, u64, Option<String>, Value, Value), String> {
    if frame.header.dst_task != RR3_TASK_RRD {
        return Err(format!(
            "dst_task 必须为 {RR3_TASK_RRD}（rrd），收到 {}",
            frame.header.dst_task
        ));
    }
    let (ty, body) =
        rr3_body_split(&frame.payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
    let v: Value = serde_json::from_slice(&body).map_err(|e| format!("体非 JSON: {e}"))?;
    if v.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("jsonrpc 必须等于 \"2.0\"".into());
    }
    let id = v
        .get("id")
        .and_then(Value::as_u64)
        .ok_or("缺 id（要求无符号整数）")?;
    let method = v.get("method").and_then(Value::as_str).map(str::to_string);
    let params = v.get("params").cloned().unwrap_or(Value::Null);
    Ok((ty, id, method, params, v))
}

/// 注册应答（rrd rr3_handshake 同款：Response + id 回显 + result.src_task）。
fn register_reply(id: u64, task: u64, now_ns: u64) -> Rr3Frame {
    bus_frame(
        Rr3PayloadType::Response,
        RR3_TASK_RRD,
        task,
        json!({"jsonrpc": "2.0", "id": id, "result": {"src_task": task}}),
        now_ns,
    )
}

/// 心跳应答（rrd D-6 Kept/Promoted 同款：OK + {"beat": "accepted"}）。
fn heartbeat_reply(id: u64, task: u64, now_ns: u64) -> Rr3Frame {
    bus_frame(
        Rr3PayloadType::Response,
        RR3_TASK_RRD,
        task,
        json!({"jsonrpc": "2.0", "id": id, "result": {"beat": "accepted"}}),
        now_ns,
    )
}

/// round-trip 下行请求（rrd rr3.call 转发同款形状：Request + params{topic,args}）。
fn round_trip_request(task: u64, now_ns: u64) -> Rr3Frame {
    bus_frame(
        Rr3PayloadType::Request,
        HARNESS_TASK,
        task,
        json!({"jsonrpc": "2.0", "id": ROUND_TRIP_ID, "method": "rr3.call",
               "params": {"topic": "rr3-sentinel/diag/probe", "args": {"ping": "pong"}}}),
        now_ns,
    )
}

/// parity 下行请求（批G ④）：同款 rr3.call 形状点名 diag/parity——method 不改写
/// 是 rrd 转发的实锤形状（conn.rs rr3_route_call 原样 body），不是"method=主题名"。
fn parity_request(task: u64, now_ns: u64) -> Rr3Frame {
    bus_frame(
        Rr3PayloadType::Request,
        HARNESS_TASK,
        task,
        json!({"jsonrpc": "2.0", "id": PARITY_CALL_ID, "method": "rr3.call",
               "params": {"topic": "rr3-sentinel/diag/parity", "args": {}}}),
        now_ns,
    )
}

/// parity 输出 → 板侧同形状 JSON（哨兵 parity_result_json 的宿主镜像，逐字段
/// 同名同型——两端的比较基准就是"同一形状下的逐值相等"）。
fn parity_expect_json(o: &Rr3ParityOutputs) -> Value {
    json!({
        "v": RR3_PARITY_VERSION,
        "rate": o.rate,
        "low": o.low,
        "sat": o.sat,
        "plan": o.plan,
        "wsat": o.wsat,
        "rhz": o.rhz,
        "clamp": o.clamp,
    })
}

/// 板回 JSON 逐字段点名差分：返回首个不匹配/缺字段的说明（全等则 None）。
fn parity_first_diff(board: &Value, expect: &Value) -> Option<String> {
    for key in ["v", "rate", "low", "sat", "plan", "wsat", "rhz", "clamp"] {
        let b = board.get(key);
        let e = expect.get(key);
        match (b, e) {
            (Some(b), Some(e)) if b == e => {}
            (Some(b), Some(e)) => {
                return Some(format!("字段 {key} 不一致：板={b} 期望/宿主={e}"));
            }
            (None, _) => return Some(format!("板回缺字段 {key}")),
            (_, None) => return Some(format!("期望侧缺字段 {key}（形状漂移？）")),
        }
    }
    None
}

/// ⑦ 直方图（10ms 桶）紧凑串：`[200-209ms)=12` 按桶升序空格连接。
fn histogram_text(intervals: &[u128]) -> String {
    let mut buckets: std::collections::BTreeMap<u128, usize> = Default::default();
    for &ms in intervals {
        *buckets.entry(ms / 10).or_insert(0) += 1;
    }
    let parts: Vec<String> = buckets
        .iter()
        .map(|(k, n)| format!("[{}-{}ms)={n}", k * 10, k * 10 + 9))
        .collect();
    parts.join(" ")
}

/// 目标三元组（host 侧拼装；与 matrix.sh C12 证据门的正则同口径）。
fn target_triple() -> String {
    match std::env::consts::OS {
        "macos" => format!("{}-apple-darwin", std::env::consts::ARCH),
        "linux" => format!("{}-unknown-linux-gnu", std::env::consts::ARCH),
        other => format!("{}-unknown-{other}", std::env::consts::ARCH),
    }
}

/// C12 证据文件（三样契约：环境指纹/采样窗口/样本数/原始数据，四标记词逐一
/// 在文；目标三元组正则由 target_triple 满足）。rustc -Vv 由操作面追记——
/// harness 不做进程外调用。
fn write_evidence(
    path: &str,
    device: &str,
    intervals: &[u128],
    hist: &str,
    parity_board: Option<&Value>,
    t0_unix: u64,
    t1_unix: u64,
) -> std::io::Result<()> {
    let mut text = String::new();
    text.push_str("=== 环境指纹 ===\n");
    text.push_str(&format!(
        "harness 编译目标: {}（{} / {}）\n",
        target_triple(),
        std::env::consts::ARCH,
        std::env::consts::OS
    ));
    text.push_str(&format!("串口设备: {device}\n"));
    text.push_str("对端固件: rr3-sentinel（xtensa-esp32-espidf，espidf-sentinel crate）\n");
    text.push_str("rustc -Vv: 由操作面在本文件追记（harness 不做进程外调用）\n");
    text.push_str("=== 采样窗口 ===\n");
    text.push_str(&format!(
        "UNIX 起止: {t0_unix} .. {t1_unix}（{}s）\n",
        t1_unix.saturating_sub(t0_unix)
    ));
    text.push_str("对表名义: heartbeat_ms（注册 manifest 在场值）/ tick_jitter_ms=100（profiles/esp32-mini.toml）\n");
    text.push_str("=== 样本数 ===\n");
    text.push_str(&format!(
        "心跳间隔 {} 拍；parity 逐拍对拍 1 次（{} 八组原语）\n",
        intervals.len(),
        if parity_board.is_some() {
            "板回向量"
        } else {
            "未完成——证据无效"
        }
    ));
    text.push_str("=== 原始数据 ===\n");
    text.push_str(&format!("逐拍间隔 ms: {intervals:?}\n"));
    text.push_str(&format!("直方图(10ms桶): {hist}\n"));
    if let Some(p) = parity_board {
        text.push_str(&format!("板回 parity 向量: {p}\n"));
    }
    std::fs::write(path, text)
}

/// 判据输出：PASS/FAIL 逐条点名，返回 ok 供调用方累计。
fn verdict(ok: bool, label: &str) -> bool {
    if ok {
        println!("PASS  {label}");
    } else {
        eprintln!("FAIL  {label}");
    }
    ok
}

fn main() {
    let t0 = Instant::now();
    let t0_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "用法: rr3-sentinel-harness <串口设备> [拍数，缺省 8] [证据文件路径（C12 三样）]"
        );
        std::process::exit(2);
    }
    let device = args[1].clone();
    let beats: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    let evidence_path = args.get(3).cloned();
    // 全程看门狗：到点 FAIL 退出（exit 2 = 环境性失败，与判据 FAIL 的 1 分开）。
    // 预算随拍数放宽（base + 每拍 1s 宽松上界）——⑦ 列 200 拍 ≈ 40s 实跑，固定
    // 60s 会把合法长采样误杀成环境失败。
    {
        let wd_budget =
            Duration::from_secs(WATCHDOG_BASE_SECS + beats as u64 * WATCHDOG_PER_BEAT_MS / 1000);
        let wd_start = t0;
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(200));
            if wd_start.elapsed() > wd_budget {
                eprintln!("FAIL  harness 全程 {wd_budget:?} 看门狗到点（板侧在跑吗？线松了吗？）");
                std::process::exit(2);
            }
        });
    }
    let now_ns = || t0.elapsed().as_nanos() as u64;
    let mut fail = 0u32;

    let mut conn =
        match Rr3UartConn::open_with_baud(std::path::Path::new(&device), RR3_UART_BAUD_DEFAULT) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("FAIL  打开 {device} 失败: {e}");
                std::process::exit(2);
            }
        };

    // ① 注册：首帧必须是 rr3.register，params.name/manifest 在场（rrd 握手门序同款）
    let (reg_name, hb_ms) = match conn.rr3_recv_buf() {
        Ok(buf) => match Rr3Frame::try_from(buf) {
            Ok(f) => match parse_upstream(f) {
                Ok((Rr3PayloadType::Request, id, m, params, _))
                    if m.as_deref() == Some("rr3.register") =>
                {
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
                    let hb = manifest
                        .lines()
                        .find_map(|l| l.strip_prefix("heartbeat_ms = "))
                        .and_then(|v| v.trim().parse::<u64>().ok())
                        .unwrap_or(0);
                    println!("---- 哨兵 manifest ----\n{manifest}------------------------");
                    if let Err(e) = conn.rr3_send_buf(Rr3FrameBuf::from(&register_reply(
                        id,
                        HARNESS_TASK,
                        now_ns(),
                    ))) {
                        eprintln!("FAIL  注册应答写出失败: {e:?}");
                        std::process::exit(1);
                    }
                    (name, hb)
                }
                Ok((ty, _, m, _, _)) => {
                    eprintln!(
                        "FAIL  首帧必须是 Request/rr3.register，收到 {ty:?}/{}",
                        m.as_deref().unwrap_or("-")
                    );
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("FAIL  首帧畸形: {e}");
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("FAIL  首帧还原失败: {e:?}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("FAIL  首帧读取失败: {e:?}");
            std::process::exit(1);
        }
    };
    if !verdict(
        !reg_name.is_empty(),
        &format!("① 注册（Request/rr3.register，name={reg_name}）"),
    ) {
        fail += 1;
    }
    if !verdict(
        hb_ms > 0,
        &format!("① manifest [health].heartbeat_ms 在场（{hb_ms}ms）"),
    ) {
        fail += 1;
    }

    // ② 心跳 N 拍：逐拍回 accepted，记间隔（相对节拍的外部观测；首拍间隔含
    // 注册→首拍的排拍 = heartbeat_ms）
    let mut intervals: Vec<u128> = Vec::new();
    let mut last = Instant::now();
    for i in 0..beats {
        let frame = match conn.rr3_recv_buf() {
            Ok(buf) => Rr3Frame::try_from(buf),
            Err(e) => {
                eprintln!("FAIL  第{}拍读取失败: {e:?}", i + 1);
                std::process::exit(1);
            }
        };
        let frame = match frame {
            Ok(f) => f,
            Err(e) => {
                eprintln!("FAIL  第{}拍还原失败: {e:?}", i + 1);
                std::process::exit(1);
            }
        };
        let now = Instant::now();
        let dt = now.duration_since(last).as_millis();
        last = now;
        match parse_upstream(frame) {
            Ok((Rr3PayloadType::Request, id, m, _, _)) if m.as_deref() == Some("rr3.heartbeat") => {
                if let Err(e) = conn.rr3_send_buf(Rr3FrameBuf::from(&heartbeat_reply(
                    id,
                    HARNESS_TASK,
                    now_ns(),
                ))) {
                    eprintln!("FAIL  第{}拍应答写出失败: {e:?}", i + 1);
                    std::process::exit(1);
                }
                println!("PASS  ② 第{}拍 rr3.heartbeat id={id} 间隔={dt}ms", i + 1);
                intervals.push(dt);
            }
            Ok((ty, _, m, _, _)) => {
                eprintln!(
                    "FAIL  第{}拍应为 rr3.heartbeat，收到 {ty:?}/{}",
                    i + 1,
                    m.as_deref().unwrap_or("-")
                );
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("FAIL  第{}拍畸形: {e}", i + 1);
                std::process::exit(1);
            }
        }
    }
    let max_dt = intervals.iter().copied().max().unwrap_or(0);
    let avg_dt = if intervals.is_empty() {
        0
    } else {
        intervals.iter().sum::<u128>() / intervals.len() as u128
    };
    if !verdict(
        intervals.len() == beats,
        &format!("② 心跳 {beats} 拍全收全应"),
    ) {
        fail += 1;
    }
    if !verdict(
        max_dt <= HEARTBEAT_STALL_FAIL_MS,
        &format!(
            "② 心跳无停摆（最大间隔 {max_dt}ms ≤ {HEARTBEAT_STALL_FAIL_MS}ms；平均 {avg_dt}ms）"
        ),
    ) {
        fail += 1;
    }
    // ⑦ 时序达标对表（批G；判据窗出处见 BEAT_* 常量注记）：名义值 = 注册
    // manifest 的 heartbeat_ms，抖动预算 = profiles/esp32-mini.toml
    // tick_jitter_ms=100。真值只认真板直方图（architecture §12.6 ⑦ 列）。
    let hist = histogram_text(&intervals);
    println!("② 直方图(10ms桶): {hist}");
    let hb = hb_ms as i64;
    let floor_ms = hb - BEAT_FLOOR_SLACK_MS;
    let ceil_ms = hb + 100;
    let avg_floor = hb - BEAT_AVG_FLOOR_SLACK_MS;
    let avg_ceil = hb + BEAT_AVG_CEIL_SLACK_MS;
    let min_i = intervals.iter().copied().min().unwrap_or(0) as i64;
    let max_i = max_dt as i64;
    let avg_i = avg_dt as i64;
    if !verdict(
        !intervals.is_empty()
            && min_i >= floor_ms
            && max_i <= ceil_ms
            && avg_i >= avg_floor
            && avg_i <= avg_ceil,
        &format!(
            "⑦ 间隔对表（[{floor_ms},{ceil_ms}]ms 均值[{avg_floor},{avg_ceil}]ms；\
             实测 min={min_i} avg={avg_i} max={max_i}，名义 hb={hb}ms）"
        ),
    ) {
        fail += 1;
    }

    // ③ round-trip：下行 rr3.call → Response id 回显 + result.echo 原样回带
    let req = round_trip_request(HARNESS_TASK, now_ns());
    if let Err(e) = conn.rr3_send_buf(Rr3FrameBuf::from(&req)) {
        eprintln!("FAIL  round-trip 请求写出失败: {e:?}");
        std::process::exit(1);
    }
    let rt_start = Instant::now();
    let mut rt_ok = false;
    let mut rt_echo_ok = false;
    loop {
        let buf = match conn.rr3_recv_buf() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("FAIL  round-trip 应答读取失败: {e:?}");
                break;
            }
        };
        let frame = match Rr3Frame::try_from(buf) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("FAIL  round-trip 应答还原失败: {e:?}");
                break;
            }
        };
        match parse_upstream(frame) {
            Ok((Rr3PayloadType::Response, id, _, _, v)) if id == ROUND_TRIP_ID => {
                rt_ok = true;
                // echo 语义判据：哨兵把 params 原样回带（result.echo == 发出的 params）
                rt_echo_ok = v.pointer("/result/echo/args/ping").and_then(Value::as_str)
                    == Some("pong")
                    && v.pointer("/result/echo/topic").and_then(Value::as_str)
                        == Some("rr3-sentinel/diag/probe");
                break;
            }
            Ok((Rr3PayloadType::Request, _, _, _, _)) => continue, // 心跳并发到达，跳过再等
            Ok((ty, id, m, _, _)) => {
                eprintln!(
                    "FAIL  round-trip 应答形状错: {ty:?}/id={id}/{}",
                    m.as_deref().unwrap_or("-")
                );
                break;
            }
            Err(e) => {
                eprintln!("FAIL  round-trip 应答畸形: {e}");
                break;
            }
        }
    }
    if !verdict(
        rt_ok,
        &format!(
            "③ round-trip 应答（id={ROUND_TRIP_ID} 回显，耗时 {}ms）",
            rt_start.elapsed().as_millis()
        ),
    ) {
        fail += 1;
    }
    if !verdict(rt_echo_ok, "③ result.echo 原样回带（params→echo 逐字对拍）") {
        fail += 1;
    }

    // ④ parity 对拍（批G ⑤列板上腿）：下行 rr3.call 点名 diag/parity——板回
    // 向量、宿主同一 runner（确定性腿）、契约期望向量（契约腿）三方两腿。板侧
    // 跑真实原语、不回显期望（哨兵 bf20d15/勘误），两腿在宿主汇合才算对拍。
    let req = parity_request(HARNESS_TASK, now_ns());
    if let Err(e) = conn.rr3_send_buf(Rr3FrameBuf::from(&req)) {
        eprintln!("FAIL  parity 请求写出失败: {e:?}");
        std::process::exit(1);
    }
    let parity_start = Instant::now();
    let mut board_parity: Option<Value> = None;
    loop {
        let buf = match conn.rr3_recv_buf() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("FAIL  parity 应答读取失败: {e:?}");
                break;
            }
        };
        let frame = match Rr3Frame::try_from(buf) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("FAIL  parity 应答还原失败: {e:?}");
                break;
            }
        };
        match parse_upstream(frame) {
            Ok((Rr3PayloadType::Response, id, _, _, v)) if id == PARITY_CALL_ID => {
                board_parity = v.get("result").cloned();
                break;
            }
            Ok((Rr3PayloadType::Request, _, _, _, _)) => continue, // 心跳并发到达，跳过再等
            Ok((ty, id, m, _, _)) => {
                eprintln!(
                    "FAIL  parity 应答形状错: {ty:?}/id={id}/{}",
                    m.as_deref().unwrap_or("-")
                );
                break;
            }
            Err(e) => {
                eprintln!("FAIL  parity 应答畸形: {e}");
                break;
            }
        }
    }
    match &board_parity {
        Some(bv) => {
            if !verdict(
                bv.get("v").and_then(Value::as_u64) == Some(RR3_PARITY_VERSION as u64),
                &format!(
                    "④ 板回 parity 向量在册（v={}，耗时 {}ms）",
                    RR3_PARITY_VERSION,
                    parity_start.elapsed().as_millis()
                ),
            ) {
                fail += 1;
            }
            // 契约腿（宿主自证）：runner 与期望向量的一致性漂移在宿主先红。
            let host = parity_expect_json(&rr3_parity_run());
            let expected = parity_expect_json(&RR3_PARITY_EXPECTED);
            if !verdict(
                host == expected,
                "④ 契约腿：宿主 runner == RR3_PARITY_EXPECTED（手推期望守门）",
            ) {
                fail += 1;
            }
            // 确定性腿：板输出 == 宿主 runner（同 runner 同输入，bit-for-bit）。
            let diff = parity_first_diff(bv, &host);
            if !verdict(
                diff.is_none(),
                &format!(
                    "④ 确定性腿：板 == 宿主 runner（xtensa LX6 bit-for-bit）{}",
                    diff.map(|d| format!("——{d}")).unwrap_or_default()
                ),
            ) {
                fail += 1;
            }
        }
        None => {
            eprintln!("FAIL  ④ 板回 parity 应答未收到（点名 diag/parity 无应答）");
            fail += 1;
        }
    }

    // C12 证据文件（三样契约；rustc -Vv 由操作面追记）——fail 也写，原始数据留痕。
    if let Some(ep) = &evidence_path {
        let t1_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match write_evidence(
            ep,
            &device,
            &intervals,
            &hist,
            board_parity.as_ref(),
            t0_unix,
            t1_unix,
        ) {
            Ok(_) => println!("OK    证据文件已写: {ep}"),
            Err(e) => eprintln!("FAIL  证据文件写出失败: {e}"),
        }
    }

    println!("---- D1 harness 判据汇总: {fail} 项 FAIL ----");
    if fail == 0 {
        println!("OK    层③ 宿主半边判据全过（编译面/三点堆采样见板侧 monitor 日志）");
        std::process::exit(0);
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 注册应答帧形状：src=rrd、dst=分配 task、Response 前缀 + JSON-RPC id 回显 +
    /// result.src_task（rrd rr3_handshake 线上形状的对拍锚）。
    #[test]
    fn register_reply_frame_shape() {
        let f = register_reply(1, 2, 777);
        assert_eq!(f.header.opcode, opcode::SEND);
        assert_eq!(f.header.src_task, RR3_TASK_RRD);
        assert_eq!(f.header.dst_task, 2);
        assert_eq!(f.header.payload_len as usize, f.payload.len());
        let (ty, body) = rr3_body_split(&f.payload).unwrap();
        assert_eq!(ty, Rr3PayloadType::Response);
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["src_task"], 2);
    }

    /// 心跳应答形状：{"beat": "accepted"}（rrd D-6 OK 臂同款）。
    #[test]
    fn heartbeat_reply_frame_shape() {
        let f = heartbeat_reply(5, 2, 1);
        let (ty, body) = rr3_body_split(&f.payload).unwrap();
        assert_eq!(ty, Rr3PayloadType::Response);
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["id"], 5);
        assert_eq!(v["result"]["beat"], "accepted");
        assert!(v.get("error").is_none());
    }

    /// parse_upstream：对哨兵同款组帧（Request + rr3.register + params{name,manifest}）
    /// 逐字段还原；dst_task 非法被点名（rrd 握手 dst 门同口径）。
    #[test]
    fn parse_upstream_round_trip_register() {
        let manifest = "[manifest]\nname = \"rr3-sentinel\"\n[health]\nheartbeat_ms = 200\n";
        let f = rr3_frame_compose(
            opcode::SEND,
            0,
            RR3_TRACE_ID_DIRECT,
            42,
            0, // 注册前 src=0（rr-bus client 同款）
            RR3_TASK_RRD,
            rr3_body_encode(
                Rr3PayloadType::Request,
                json!({"jsonrpc": "2.0", "id": 1, "method": "rr3.register",
                       "params": {"name": "rr3-sentinel", "manifest": manifest}})
                .to_string()
                .as_bytes(),
            ),
        );
        let (ty, id, method, params, _) = parse_upstream(f).unwrap();
        assert_eq!(ty, Rr3PayloadType::Request);
        assert_eq!(id, 1);
        assert_eq!(method.as_deref(), Some("rr3.register"));
        assert_eq!(params["name"], "rr3-sentinel");
        assert!(params["manifest"]
            .as_str()
            .unwrap()
            .contains("heartbeat_ms = 200"));

        // dst 门：dst≠1 的上行帧必须被点名拒（fail-closed）
        let bad = rr3_frame_compose(
            opcode::SEND,
            0,
            0,
            0,
            3,
            9,
            rr3_body_encode(Rr3PayloadType::Request, b"{}"),
        );
        assert!(parse_upstream(bad).unwrap_err().contains("dst_task"));
    }

    /// round-trip 下行请求形状：Request + rr3.call + params{topic,args}（rrd 转发同款）。
    #[test]
    fn round_trip_request_shape() {
        let f = round_trip_request(2, 9);
        assert_eq!(f.header.dst_task, 2);
        // M3 忠实形状：转发帧 src=调用方 task（conn.rs rr3_route_call 实锤：
        // trace_id=src=调用方、dst=被调方）——调用方即本 harness，非 rrd
        assert_eq!(f.header.src_task, HARNESS_TASK);
        let (ty, body) = rr3_body_split(&f.payload).unwrap();
        assert_eq!(ty, Rr3PayloadType::Request);
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["id"], ROUND_TRIP_ID);
        assert_eq!(v["method"], "rr3.call");
        assert_eq!(v["params"]["args"]["ping"], "pong");
    }

    /// parity 下行请求形状（批G ④）：同款 rr3.call 形状，topic=diag/parity，
    /// id=9002 与 probe 区分。method 不改写是 rrd 转发实锤（conn.rs L779/L816）。
    #[test]
    fn parity_request_shape() {
        let f = parity_request(2, 9);
        assert_eq!(f.header.dst_task, 2);
        assert_eq!(f.header.src_task, HARNESS_TASK);
        let (ty, body) = rr3_body_split(&f.payload).unwrap();
        assert_eq!(ty, Rr3PayloadType::Request);
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["id"], PARITY_CALL_ID);
        assert_eq!(v["method"], "rr3.call");
        assert_eq!(v["params"]["topic"], "rr3-sentinel/diag/parity");
    }

    /// 期望形状函数与差分函数的自证：宿主 runner == 契约期望（契约腿在测试面
    /// 也钉一道）；差分函数对逐字段篡改逐个点名。
    #[test]
    fn parity_expect_json_and_diff() {
        let host = parity_expect_json(&rr3_parity_run());
        let expected = parity_expect_json(&RR3_PARITY_EXPECTED);
        assert_eq!(host, expected, "runner 与期望向量漂移（契约腿红）");
        assert_eq!(host["v"], RR3_PARITY_VERSION);
        assert_eq!(host["rate"].as_array().map(Vec::len), Some(8));
        assert_eq!(host["plan"].as_array().map(Vec::len), Some(3));

        assert_eq!(parity_first_diff(&host, &expected), None);
        let mut bad = host.clone();
        bad["clamp"] = json!([0, 0, 0, 0, 0]);
        let d = parity_first_diff(&bad, &expected).unwrap();
        assert!(d.contains("clamp") && d.contains("不一致"), "{d}");
        let mut missing = host.clone();
        missing.as_object_mut().unwrap().remove("sat");
        let d = parity_first_diff(&missing, &expected).unwrap();
        assert!(d.contains("sat") && d.contains("缺"), "{d}");
    }

    /// ⑦ 直方图分桶标签：10ms 粒度、桶区间首尾正确（[190-199ms) 形状）。
    #[test]
    fn histogram_buckets_are_labeled_by_ms_range() {
        let text = histogram_text(&[195, 200, 205, 205, 310]);
        assert!(text.contains("[190-199ms)=1"), "{text}");
        assert!(text.contains("[200-209ms)=3"), "{text}");
        assert!(text.contains("[310-319ms)=1"), "{text}");
    }
}
