//! rr3-sentinel-harness —— 批D1 层③ 哨兵的宿主对手件。
//!
//! 扮演**总线侧**（rrd 位形，走 rr-ipc 的 Rr3UartConn + blanket conn 同源帧语义）：
//! ① 收哨兵的 `rr3.register` → 校验形状 → 回 `{"src_task": 2}`；② 收 N 拍
//! `rr3.heartbeat` → 逐拍回 `{"beat": "accepted"}` 并记间隔；③ 发一条 `rr3.call`
//! Request（rrd→模块下行转发同款形状）→ 校验哨兵 Response 的 id 回显与
//! result.echo 原样回带。判据输出 PASS/FAIL 逐条点名，退出码 0/1（2 = 环境性
//! 失败：设备打不开 / 看门狗到点）。D1 层③ 板上判据的宿主半边。
//!
//! 用法：`cargo run -p rr3-sentinel-harness -- <串口设备> [拍数，缺省 8]`
//! （macOS：/dev/tty.usbserial-*；树莓派：/dev/ttyUSB0。板侧 monitor 先行，
//! 再跑本件；全程 60s 看门狗。）

use std::env;
use std::thread;
use std::time::{Duration, Instant};

use rr_core::{rr3_frame_compose, RR3_TASK_RRD, RR3_TRACE_ID_DIRECT};
use rr_ipc::transport::uart::{Rr3UartConn, RR3_UART_BAUD_DEFAULT};
use rr_ipc::transport::Rr3TransportConn;
use rr_ipc::{opcode, rr3_body_encode, rr3_body_split, Rr3Frame, Rr3FrameBuf, Rr3PayloadType};
use serde_json::{json, Value};

const ROUND_TRIP_ID: u64 = 9001;
/// 全程看门狗：任一步卡死（板侧复位/线松）即 FAIL 退出，不挂死操作面。
const WATCHDOG: Duration = Duration::from_secs(60);
/// 心跳停摆判据（单拍间隔上界；200ms 名义值的数量级护栏，抖动不误伤）。
const HEARTBEAT_STALL_FAIL_MS: u128 = 5_000;
/// harness 分配给哨兵的 task 号（模块池起点，rr-core::RR3_TASK_FIRST_MODULE）。
const HARNESS_TASK: u64 = 2;

/// 组一帧 rrd 位形的下行帧（src=RR3_TASK_RRD、SEND、直连 trace、时间戳入参注入）。
fn bus_frame(ty: Rr3PayloadType, dst_task: u64, body: Value, now_ns: u64) -> Rr3Frame {
    rr3_frame_compose(
        opcode::SEND,
        0,
        RR3_TRACE_ID_DIRECT,
        now_ns,
        RR3_TASK_RRD,
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
        task,
        json!({"jsonrpc": "2.0", "id": id, "result": {"src_task": task}}),
        now_ns,
    )
}

/// 心跳应答（rrd D-6 Kept/Promoted 同款：OK + {"beat": "accepted"}）。
fn heartbeat_reply(id: u64, task: u64, now_ns: u64) -> Rr3Frame {
    bus_frame(
        Rr3PayloadType::Response,
        task,
        json!({"jsonrpc": "2.0", "id": id, "result": {"beat": "accepted"}}),
        now_ns,
    )
}

/// round-trip 下行请求（rrd rr3.call 转发同款形状：Request + params{topic,args}）。
fn round_trip_request(task: u64, now_ns: u64) -> Rr3Frame {
    bus_frame(
        Rr3PayloadType::Request,
        task,
        json!({"jsonrpc": "2.0", "id": ROUND_TRIP_ID, "method": "rr3.call",
               "params": {"topic": "rr3-espidf-sentinel/probe", "args": {"ping": "pong"}}}),
        now_ns,
    )
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
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: rr3-sentinel-harness <串口设备> [拍数，缺省 8]");
        std::process::exit(2);
    }
    let device = args[1].clone();
    let beats: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    // 全程看门狗：到点 FAIL 退出（exit 2 = 环境性失败，与判据 FAIL 的 1 分开）
    {
        let wd_start = t0;
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(200));
            if wd_start.elapsed() > WATCHDOG {
                eprintln!("FAIL  harness 全程 {WATCHDOG:?} 看门狗到点（板侧在跑吗？线松了吗？）");
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
                        == Some("rr3-espidf-sentinel/probe");
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
        let manifest = "[manifest]\nname = \"rr3-espidf-sentinel\"\n[health]\nheartbeat_ms = 200\n";
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
                       "params": {"name": "rr3-espidf-sentinel", "manifest": manifest}})
                .to_string()
                .as_bytes(),
            ),
        );
        let (ty, id, method, params, _) = parse_upstream(f).unwrap();
        assert_eq!(ty, Rr3PayloadType::Request);
        assert_eq!(id, 1);
        assert_eq!(method.as_deref(), Some("rr3.register"));
        assert_eq!(params["name"], "rr3-espidf-sentinel");
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
        assert_eq!(f.header.src_task, RR3_TASK_RRD);
        let (ty, body) = rr3_body_split(&f.payload).unwrap();
        assert_eq!(ty, Rr3PayloadType::Request);
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["id"], ROUND_TRIP_ID);
        assert_eq!(v["method"], "rr3.call");
        assert_eq!(v["params"]["args"]["ping"], "pong");
    }
}
