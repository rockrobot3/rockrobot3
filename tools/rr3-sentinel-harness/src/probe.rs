//! 健康探测与断言：帧形状门、round-trip 与 parity 对拍。

use std::time::Instant;

use rr_core::control::parity::{
    rr3_parity_run, Rr3ParityOutputs, RR3_PARITY_EXPECTED, RR3_PARITY_VERSION,
};
use rr_core::{rr3_frame_compose, RR3_TASK_RRD, RR3_TRACE_ID_DIRECT};
use rr_ipc::transport::uart::Rr3UartConn;
use rr_ipc::transport::Rr3TransportConn;
use rr_ipc::{opcode, rr3_body_encode, rr3_body_split, Rr3Frame, Rr3FrameBuf, Rr3PayloadType};
use serde_json::{json, Value};

use crate::run::rr3_tally;
use crate::{HARNESS_TASK, PARITY_CALL_ID, ROUND_TRIP_ID};

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
pub(super) fn parse_upstream(
    frame: Rr3Frame,
) -> Result<(Rr3PayloadType, u64, Option<String>, Value, Value), String> {
    rr3_up_dst_ok(&frame)?;
    let (ty, v) = rr3_upstream_value(&frame.payload)?;
    let (id, method, params) = rr3_upstream_rpc(&v)?;
    Ok((ty, id, method, params, v))
}

/// dst 门：上行帧必须寄给 rrd。
fn rr3_up_dst_ok(frame: &Rr3Frame) -> Result<(), String> {
    if frame.header.dst_task != RR3_TASK_RRD {
        return Err(format!(
            "dst_task 必须为 {RR3_TASK_RRD}（rrd），收到 {}",
            frame.header.dst_task
        ));
    }
    Ok(())
}

/// payload 拆前缀 + 解析 JSON 体。
fn rr3_upstream_value(payload: &[u8]) -> Result<(Rr3PayloadType, Value), String> {
    let (ty, body) = rr3_body_split(payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
    let v: Value = serde_json::from_slice(&body).map_err(|e| format!("体非 JSON: {e}"))?;
    Ok((ty, v))
}

/// JSON-RPC 信封门：jsonrpc 版本 + id（无符号整数）；method/params 原样取出。
fn rr3_upstream_rpc(v: &Value) -> Result<(u64, Option<String>, Value), String> {
    rr3_rpc_version_ok(v)?;
    let id = v
        .get("id")
        .and_then(Value::as_u64)
        .ok_or("缺 id（要求无符号整数）")?;
    Ok((id, rr3_rpc_method(v), rr3_rpc_params(v)))
}

/// method 原件（Response 帧合法无 method）。
fn rr3_rpc_method(v: &Value) -> Option<String> {
    v.get("method").and_then(Value::as_str).map(str::to_string)
}

/// params 原件（缺省 Null）。
fn rr3_rpc_params(v: &Value) -> Value {
    v.get("params").cloned().unwrap_or(Value::Null)
}

/// JSON-RPC 版本门。
fn rr3_rpc_version_ok(v: &Value) -> Result<(), String> {
    if v.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("jsonrpc 必须等于 \"2.0\"".into());
    }
    Ok(())
}

/// 注册应答（rrd rr3_handshake 同款：Response + id 回显 + result.src_task）。
pub(super) fn register_reply(id: u64, task: u64, now_ns: u64) -> Rr3Frame {
    bus_frame(
        Rr3PayloadType::Response,
        RR3_TASK_RRD,
        task,
        json!({"jsonrpc": "2.0", "id": id, "result": {"src_task": task}}),
        now_ns,
    )
}

/// 心跳应答（rrd D-6 Kept/Promoted 同款：OK + {"beat": "accepted"}）。
pub(super) fn heartbeat_reply(id: u64, task: u64, now_ns: u64) -> Rr3Frame {
    bus_frame(
        Rr3PayloadType::Response,
        RR3_TASK_RRD,
        task,
        json!({"jsonrpc": "2.0", "id": id, "result": {"beat": "accepted"}}),
        now_ns,
    )
}

/// round-trip 下行请求（rrd rr3.call 转发同款形状：Request + params{topic,args}）。
pub(super) fn round_trip_request(task: u64, now_ns: u64) -> Rr3Frame {
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
pub(super) fn parity_request(task: u64, now_ns: u64) -> Rr3Frame {
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
pub(super) fn parity_expect_json(o: &Rr3ParityOutputs) -> Value {
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
pub(super) fn parity_first_diff(board: &Value, expect: &Value) -> Option<String> {
    ["v", "rate", "low", "sat", "plan", "wsat", "rhz", "clamp"]
        .iter()
        .find_map(|key| parity_key_diff(board, expect, key))
}

/// 单字段差异（`None` = 该字段一致）。
fn parity_key_diff(board: &Value, expect: &Value, key: &str) -> Option<String> {
    match (board.get(key), expect.get(key)) {
        (Some(b), Some(e)) => (b != e).then(|| format!("字段 {key} 不一致：板={b} 期望/宿主={e}")),
        (None, _) => Some(format!("板回缺字段 {key}")),
        (_, None) => Some(format!("期望侧缺字段 {key}（形状漂移？）")),
    }
}

/// 收一帧或点名退出（读取/还原失败按调用方给定文案 FAIL 并 exit 1）。
pub(super) fn rr3_recv_frame(conn: &mut Rr3UartConn, on_recv: &str, on_parse: &str) -> Rr3Frame {
    let buf = rr3_recv_buf(conn, on_recv);
    rr3_frame_parse(buf, on_parse)
}

/// 收一帧裸帧；读取失败按 `on_recv` 点名后退出 1。
fn rr3_recv_buf(conn: &mut Rr3UartConn, on_recv: &str) -> Rr3FrameBuf {
    match conn.rr3_recv_buf() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{on_recv}: {e:?}");
            std::process::exit(1);
        }
    }
}

/// 还原一帧（失败点名后退出 1）。
fn rr3_frame_parse(buf: Rr3FrameBuf, on_parse: &str) -> Rr3Frame {
    match Rr3Frame::try_from(buf) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{on_parse}: {e:?}");
            std::process::exit(1);
        }
    }
}

/// round-trip 单帧判定（命中 / 心跳跳过 / 终态失败；失败已在此点名）。
enum RtFrame {
    Hit { echo_ok: bool },
    Skip,
    Fail,
}

/// 单帧 verdict：echo 语义判据＝哨兵把 params 原样回带（result.echo == 发出的 params）。
fn rt_verdict(frame: Rr3Frame) -> RtFrame {
    match parse_upstream(frame) {
        Ok((ty, id, m, _, v)) => rt_frame_kind(ty, id, m.as_deref(), &v),
        Err(e) => {
            eprintln!("FAIL  round-trip 应答畸形: {e}");
            RtFrame::Fail
        }
    }
}

/// 已还原帧归类：命中 / 心跳跳过 / 形状错点名失败。
fn rt_frame_kind(ty: Rr3PayloadType, id: u64, m: Option<&str>, v: &Value) -> RtFrame {
    if ty == Rr3PayloadType::Response && id == ROUND_TRIP_ID {
        return RtFrame::Hit {
            echo_ok: rt_echo_ok(v),
        };
    }
    if ty == Rr3PayloadType::Request {
        return RtFrame::Skip; // 心跳并发到达，跳过再等
    }
    rt_shape_fail(ty, id, m)
}

/// 既非命中亦非跳过：形状错点名后记失败。
fn rt_shape_fail(ty: Rr3PayloadType, id: u64, m: Option<&str>) -> RtFrame {
    eprintln!(
        "FAIL  round-trip 应答形状错: {ty:?}/id={id}/{}",
        m.unwrap_or("-")
    );
    RtFrame::Fail
}

/// echo 对拍：ping 回 `pong` 且 topic 原样。
fn rt_echo_ok(v: &Value) -> bool {
    v.pointer("/result/echo/args/ping").and_then(Value::as_str) == Some("pong")
        && v.pointer("/result/echo/topic").and_then(Value::as_str)
            == Some("rr3-sentinel/diag/probe")
}

/// 收一帧并还原；读/还原失败点名后回 `None`。
fn rt_recv(conn: &mut Rr3UartConn) -> Option<Rr3Frame> {
    let buf = rt_buf(conn)?;
    rt_parse(buf)
}

/// round-trip 应答读一帧（读失败点名后回 `None`）。
fn rt_buf(conn: &mut Rr3UartConn) -> Option<Rr3FrameBuf> {
    match conn.rr3_recv_buf() {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("FAIL  round-trip 应答读取失败: {e:?}");
            None
        }
    }
}

/// 裸帧还原（还原失败点名后回 `None`）。
fn rt_parse(buf: Rr3FrameBuf) -> Option<Rr3Frame> {
    match Rr3Frame::try_from(buf) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("FAIL  round-trip 应答还原失败: {e:?}");
            None
        }
    }
}

/// ③ round-trip：下行 rr3.call → Response id 回显 + result.echo 原样回带。
/// 返回 (应答到达, echo 对拍通过, 耗时 ms)。
fn rr3_round_trip(conn: &mut Rr3UartConn, now_ns: &dyn Fn() -> u64) -> (bool, bool, u128) {
    rr3_rt_send(conn, now_ns);
    let rt_start = Instant::now();
    let (rt_ok, rt_echo_ok) = rr3_rt_scan(conn);
    (rt_ok, rt_echo_ok, rt_start.elapsed().as_millis())
}

/// 逐帧等命中：Hit 记结果收兵，Skip 继续等，Fail 收兵。
fn rr3_rt_scan(conn: &mut Rr3UartConn) -> (bool, bool) {
    while let Some(frame) = rt_recv(conn) {
        match rt_verdict(frame) {
            RtFrame::Hit { echo_ok } => return (true, echo_ok),
            RtFrame::Skip => continue,
            RtFrame::Fail => break,
        }
    }
    (false, false)
}

/// round-trip 请求写出（失败即点名退场）。
fn rr3_rt_send(conn: &mut Rr3UartConn, now_ns: &dyn Fn() -> u64) {
    let req = round_trip_request(HARNESS_TASK, now_ns());
    if let Err(e) = conn.rr3_send_buf(Rr3FrameBuf::from(&req)) {
        eprintln!("FAIL  round-trip 请求写出失败: {e:?}");
        std::process::exit(1);
    }
}

/// ④ parity 应答等待：返回 (板回 result, 耗时 ms)。
pub(super) fn rr3_parity_wait(
    conn: &mut Rr3UartConn,
    now_ns: &dyn Fn() -> u64,
) -> (Option<Value>, u128) {
    let req = parity_request(HARNESS_TASK, now_ns());
    if let Err(e) = conn.rr3_send_buf(Rr3FrameBuf::from(&req)) {
        eprintln!("FAIL  parity 请求写出失败: {e:?}");
        std::process::exit(1);
    }
    let parity_start = Instant::now();
    let board_parity = rr3_parity_recv(conn);
    (board_parity, parity_start.elapsed().as_millis())
}

/// parity 应答收帧（读到 id 匹配的 Response 为止；并发心跳 Request 跳过再等；
/// 读取/还原/形状/畸形违约点名后返回 None——此面不 exit，交由判据面记 FAIL）。
fn rr3_parity_recv(conn: &mut Rr3UartConn) -> Option<Value> {
    while let Some(frame) = rr3_parity_frame(conn) {
        match rr3_parity_verdict(frame) {
            Rr3ParityStep::Hit(v) => return v,
            Rr3ParityStep::Skip => continue,
            Rr3ParityStep::Fail => return None,
        }
    }
    None
}

/// parity 单帧读取与还原（失败点名后返回 None——此面不 exit）。
fn rr3_parity_frame(conn: &mut Rr3UartConn) -> Option<Rr3Frame> {
    let buf = rr3_parity_buf(conn)?;
    rr3_parity_parse(buf)
}

/// parity 应答读一帧（读失败点名后回 `None`——此面不 exit）。
fn rr3_parity_buf(conn: &mut Rr3UartConn) -> Option<Rr3FrameBuf> {
    match conn.rr3_recv_buf() {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("FAIL  parity 应答读取失败: {e:?}");
            None
        }
    }
}

/// parity 裸帧还原（还原失败点名后回 `None`——此面不 exit）。
fn rr3_parity_parse(buf: Rr3FrameBuf) -> Option<Rr3Frame> {
    match Rr3Frame::try_from(buf) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("FAIL  parity 应答还原失败: {e:?}");
            None
        }
    }
}

/// parity 单帧裁决：id 匹配 Response = 命中；并发心跳 Request = 跳过；其余点名即失败。
enum Rr3ParityStep {
    Hit(Option<Value>),
    Skip,
    Fail,
}

fn rr3_parity_verdict(frame: Rr3Frame) -> Rr3ParityStep {
    let Some((ty, id, m, _, v)) = rr3_parity_parsed(frame) else {
        return Rr3ParityStep::Fail;
    };
    rr3_parity_step(ty, id, m, &v)
}

/// 已拆帧归类：命中 / 心跳跳过 / 形状错。
fn rr3_parity_step(ty: Rr3PayloadType, id: u64, m: Option<String>, v: &Value) -> Rr3ParityStep {
    if ty == Rr3PayloadType::Response && id == PARITY_CALL_ID {
        return Rr3ParityStep::Hit(v.get("result").cloned());
    }
    if ty == Rr3PayloadType::Request {
        return Rr3ParityStep::Skip;
    }
    rr3_parity_shape(ty, id, m)
}

/// 拆帧；畸形即点名（本次裁决记 Fail）。
fn rr3_parity_parsed(
    frame: Rr3Frame,
) -> Option<(Rr3PayloadType, u64, Option<String>, Value, Value)> {
    match parse_upstream(frame) {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("FAIL  parity 应答畸形: {e}");
            None
        }
    }
}

/// 既非命中亦非跳过：形状错点名后记 Fail。
fn rr3_parity_shape(ty: Rr3PayloadType, id: u64, m: Option<String>) -> Rr3ParityStep {
    eprintln!(
        "FAIL  parity 应答形状错: {ty:?}/id={id}/{}",
        m.as_deref().unwrap_or("-")
    );
    Rr3ParityStep::Fail
}

/// ④ 三方两腿对拍：板回向量在册 / 契约腿（宿主 runner == 期望）/ 确定性腿
/// （板 == 宿主 runner，bit-for-bit）。返回 FAIL 数。
pub(super) fn rr3_parity_judge(board_parity: Option<&Value>, parity_ms: u128) -> u32 {
    let Some(bv) = board_parity else {
        eprintln!("FAIL  ④ 板回 parity 应答未收到（点名 diag/parity 无应答）");
        return 1;
    };
    rr3_parity_legs(bv, parity_ms)
}

/// 板回后的三条腿：向量在册 / 契约腿 / 确定性腿。返回 FAIL 数。
fn rr3_parity_legs(bv: &Value, parity_ms: u128) -> u32 {
    let mut fail = 0u32;
    fail += rr3_tally(
        bv.get("v").and_then(Value::as_u64) == Some(RR3_PARITY_VERSION as u64),
        &format!(
            "④ 板回 parity 向量在册（v={}，耗时 {parity_ms}ms）",
            RR3_PARITY_VERSION
        ),
    );
    // 契约腿（宿主自证）：runner 与期望向量的一致性漂移在宿主先红。
    let host = parity_expect_json(&rr3_parity_run());
    let expected = parity_expect_json(&RR3_PARITY_EXPECTED);
    fail += rr3_tally(
        host == expected,
        "④ 契约腿：宿主 runner == RR3_PARITY_EXPECTED（手推期望守门）",
    );
    // 确定性腿：板输出 == 宿主 runner（同 runner 同输入，bit-for-bit）。
    let diff = parity_first_diff(bv, &host);
    fail += rr3_tally(
        diff.is_none(),
        &format!(
            "④ 确定性腿：板 == 宿主 runner（xtensa LX6 bit-for-bit）{}",
            diff.map(|d| format!("——{d}")).unwrap_or_default()
        ),
    );
    fail
}

/// ③ round-trip 两项判据：返回 FAIL 数。
pub(super) fn rr3_rt_checks(conn: &mut Rr3UartConn, now_ns: &dyn Fn() -> u64) -> u32 {
    let (rt_ok, rt_echo_ok, rt_ms) = rr3_round_trip(conn, now_ns);
    rr3_tally(
        rt_ok,
        &format!("③ round-trip 应答（id={ROUND_TRIP_ID} 回显，耗时 {rt_ms}ms）"),
    ) + rr3_tally(rt_echo_ok, "③ result.echo 原样回带（params→echo 逐字对拍）")
}
