//! 客户端连接、发布、心跳与派发路径的单测。

use super::conn::rr3_now_ns;
use super::*;
use rr_core::{RR3_METHOD_CALL, RR3_TASK_RRD};
use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame, Rr3Header,
    Rr3PayloadType, RR3_FRAMING_PLEN_CAP_DEFAULT,
};
use serde_json::{json, Value};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MANIFEST: &str = "[manifest]\nschema_version = 1\nname = \"rr3-bus-t\"\nversion = \"0.0.1\"\ntype = \"module\"\nlang = \"rust\"\nentry = \"/bin/true\"\n\n[events]\npublishes = []\nsubscribes = []\n\n[health]\nheartbeat_ms = 50\n";

/// 假 rrd：收注册回 src_task=7；心跳回 accepted；Event 记账并回播一条广播；
/// 点名 Request 原样记账。行为写死（测试夹具，不是通用服务器）。
fn fake_rrd(listener: UnixListener, log: Arc<Mutex<Vec<(String, Value)>>>) {
    let (mut stream, _) = listener.accept().expect("假 rrd 应接受到连接");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    loop {
        let Some(frame) = rr3_frame_read(&mut stream).ok() else {
            return;
        };
        let Ok((ty, body)) = rr3_body_split(&frame.payload) else {
            return;
        };
        let Ok(v) = serde_json::from_slice::<Value>(&body) else {
            return;
        };
        match ty {
            Rr3PayloadType::Request => {
                let method = v["method"].as_str().unwrap_or("").to_string();
                let id = v["id"].as_u64().unwrap_or(0);
                let (result, ok): (Value, bool) = match method.as_str() {
                    "rr3.register" => (json!({"src_task": 7}), true),
                    "rr3.heartbeat" => (json!({"beat": "accepted"}), true),
                    _ => (json!({"noop": true}), true),
                };
                log.lock()
                    .unwrap()
                    .push((method.clone(), v["params"].clone()));
                if method != RR3_METHOD_CALL {
                    let reply = if ok {
                        json!({"jsonrpc": "2.0", "id": id, "result": result})
                    } else {
                        v.clone()
                    };
                    let payload =
                        rr3_body_encode(Rr3PayloadType::Response, reply.to_string().as_bytes());
                    let out = frame_send(payload);
                    if rr3_frame_write(&mut stream, &out).is_err() {
                        return;
                    }
                }
            }
            Rr3PayloadType::Event => {
                log.lock().unwrap().push(("EVENT".into(), v.clone()));
                let payload = rr3_body_encode(
                    Rr3PayloadType::Event,
                    json!({"topic": "rr3-fake/echo", "echo": true})
                        .to_string()
                        .as_bytes(),
                );
                let out = frame_send(payload);
                if rr3_frame_write(&mut stream, &out).is_err() {
                    return;
                }
            }
            _ => return,
        }
    }
}

fn tmp_bus_path(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "rr3-bus-t-{tag}-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    p
}

/// 假 rrd 应答帧（三处同一构造）：SEND 头，时间戳与 CRC 按 payload 现算
fn frame_send(payload: Vec<u8>) -> Rr3Frame {
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: rr3_now_ns(),
            src_task: RR3_TASK_RRD,
            dst_task: 7,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

fn connect_on(tag: &str, log: Arc<Mutex<Vec<(String, Value)>>>) -> (Rr3BusClient, PathBuf) {
    let path = tmp_bus_path(tag);
    let listener = UnixListener::bind(&path).unwrap();
    std::thread::spawn(move || fake_rrd(listener, log));
    let client = Rr3BusClient::rr3_connect_with(
        path.to_str().unwrap(),
        rr3_manifest_ident(MANIFEST).unwrap(),
        MANIFEST,
    )
    .expect("连假 rrd 应成功");
    (client, path)
}

#[test]
fn manifest_identity_takes_name_and_heartbeat() {
    let id = rr3_manifest_ident(MANIFEST).unwrap();
    assert_eq!(id.name, "rr3-bus-t");
    assert_eq!(id.heartbeat_ms, 50);
    assert!(rr3_manifest_ident("no-toml-here [").is_err());
    assert!(
        rr3_manifest_ident("[manifest]\nname=\"x\"\n").is_err(),
        "缺 heartbeat_ms 必须报错"
    );
}

#[test]
fn entry_absolutize_rewrites_entry_and_keeps_rest() {
    let out = rr3_entry_absolutize(MANIFEST, Path::new("/opt/rr3/rr3-sense")).unwrap();
    let root: toml::Value = out.parse().unwrap();
    assert_eq!(
        root["manifest"]["entry"].as_str(),
        Some("/opt/rr3/rr3-sense")
    );
    assert_eq!(root["manifest"]["name"].as_str(), Some("rr3-bus-t"));
    assert_eq!(root["health"]["heartbeat_ms"].as_integer(), Some(50));
    assert!(rr3_entry_absolutize(MANIFEST, Path::new("rel/bin")).is_err());
}

#[test]
fn connect_registers_and_call_round_trips() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (client, path) = connect_on("call", log.clone());
    assert_eq!(client.rr3_task(), 7);
    assert_eq!(client.rr3_name(), "rr3-bus-t");
    let r = client
        .rr3_call("rr3.heartbeat", json!({}), Duration::from_secs(2))
        .unwrap();
    assert_eq!(r["beat"], "accepted");
    let log = log.lock().unwrap();
    assert!(
        log.iter().any(|(m, _)| m == "rr3.register"),
        "注册帧必须到过假 rrd"
    );
    assert!(log.iter().any(|(m, _)| m == "rr3.heartbeat"));
    drop(client);
    let _ = std::fs::remove_file(&path);
}

/// B-8（本轮审计）：载荷超总线读侧策略帽时写侧必须先拒——旧行为是把帧写进
/// socket，rrd 读侧拒收后断链，模块只看到"总线已断开"，根因丢失。
#[test]
fn publish_rejects_payload_over_bus_cap() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (client, path) = connect_on("cap", log);
    let big = "x".repeat(RR3_FRAMING_PLEN_CAP_DEFAULT);
    let e = client
        .rr3_publish("rr3-t/big", json!({"blob": big}))
        .expect_err("超帽载荷必须在写侧被拒");
    assert!(e.contains("超总线帧上界"), "{e}");
    drop(client);
    let _ = std::fs::remove_file(&path);
}

/// B-9（本轮审计）：对端接受连接但静默不回注册应答时，连接必须按超时失败，
/// 且失败路径关 socket 收掉 reader——旧写法只置 stop 再 join，reader 卡在
/// read 上会把启动超时拖成永久挂死。
#[test]
fn connect_times_out_and_aborts_reader_when_peer_is_silent() {
    use std::time::Instant;
    let path = tmp_bus_path("silent");
    let listener = UnixListener::bind(&path).unwrap();
    let (hold_tx, hold_rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let (_stream, _) = listener.accept().expect("假 rrd 应接受连接");
        let _ = hold_rx.recv(); // 持有连接保持静默：不回注册应答
    });
    let t0 = Instant::now();
    let e = match Rr3BusClient::rr3_connect_timeout(
        path.to_str().unwrap(),
        rr3_manifest_ident(MANIFEST).unwrap(),
        MANIFEST,
        Duration::from_millis(150),
    ) {
        Ok(_) => panic!("静默对端必须超时失败"),
        Err(e) => e,
    };
    assert!(e.contains("注册失败") && e.contains("超时"), "{e}");
    assert!(
        t0.elapsed() < Duration::from_secs(3),
        "reader 必须被收掉，连接失败不得挂死：{:?}",
        t0.elapsed()
    );
    let _ = hold_tx.send(());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn publish_reaches_bus_and_broadcast_lands_in_inbox() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (client, path) = connect_on("pub", log.clone());
    client.rr3_publish("rr3-t/pcm", json!({"seq": 1})).unwrap();
    assert!(
        client.rr3_publish("bad", json!([])).is_err(),
        "非对象体必须被拒"
    );
    let frames = client.rr3_poll_wait(Duration::from_secs(2));
    assert_eq!(frames.len(), 1, "回播的广播应落进 inbox");
    let (ty, v) = Rr3BusClient::rr3_frame_body(&frames[0]).unwrap();
    assert_eq!(ty, Rr3PayloadType::Event);
    assert_eq!(v["topic"], "rr3-fake/echo");
    let log = log.lock().unwrap();
    let pubed = log
        .iter()
        .find(|(m, _)| *m == "EVENT")
        .expect("发布应到达假 rrd");
    assert_eq!(pubed.1["topic"], "rr3-t/pcm");
    assert_eq!(pubed.1["seq"], 1);
    drop(client);
    let _ = std::fs::remove_file(&path);
}

/// 计数自报 → 心跳载荷（0.0.4 批B，Q1 裁定的双端兼容面；b3c 2026-10-21
/// 增量语义钉死）：无上报的拍 params 维持 `{}`；bump 后的拍 params 带非空
/// `counters`，值是「自上一拍以来」的增量（首拍水位 0，增量恰为累计值；
/// 之后跨拍只发新增量——rrd 归并按增量累加，聚合表才是生命周期累计而非
/// 二次方膨胀）。有界轮询不猜时刻（hb=50ms，2s 上界足够）。
#[test]
fn heartbeat_carries_counters_only_after_bump() {
    use std::time::Instant;
    let log = Arc::new(Mutex::new(Vec::new()));
    let (client, path) = connect_on("cnt", log.clone());
    // 第一段：等一个不带 counters 的心跳（空增量 → params = {}）
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let has_empty = log
            .lock()
            .unwrap()
            .iter()
            .any(|(m, p)| m == "rr3.heartbeat" && p.get("counters").is_none());
        assert!(
            has_empty || Instant::now() < deadline,
            "bump 前应出现过 params={{}} 的心跳"
        );
        if has_empty {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // bump 两次同键（累加）+ 一次异键
    client.rr3_counter_bump("relay_on", 2);
    client.rr3_counter_bump("relay_on", 1);
    client.rr3_counter_bump("chat_rounds", 1);
    // 第二段：等带 counters 的心跳，值逐键核对（首拍增量 = 累计值）
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let hit = {
            let log = log.lock().unwrap();
            log.iter()
                .find(|(m, p)| m == "rr3.heartbeat" && p.get("counters").is_some())
                .map(|(_, p)| p["counters"].clone())
        };
        if let Some(c) = hit {
            assert_eq!(c["relay_on"], 3, "首拍增量 = 水位 0 起的累计");
            assert_eq!(c["chat_rounds"], 1);
            break;
        }
        assert!(Instant::now() < deadline, "bump 后应出现带 counters 的心跳");
        std::thread::sleep(Duration::from_millis(10));
    }
    // 观测点游标：第三段只看此后的新增日志（旧拍仍在账上，整表倒查会撞旧值）
    let observed_len = log.lock().unwrap().len();
    // 第三段（b3c 语义钉）：水位已到 3，再 bump 1 → 下一拍只发增量 1（旧
    // 快照语义会发全量 4，被 rrd 当增量再收一遍即 40 亿病灶）
    client.rr3_counter_bump("relay_on", 1);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let hit = {
            let log = log.lock().unwrap();
            log[observed_len..]
                .iter()
                .rev()
                .find(|(m, p)| m == "rr3.heartbeat" && p.get("counters").is_some())
                .map(|(_, p)| p["counters"].clone())
        };
        if let Some(c) = hit {
            assert_eq!(c["relay_on"], 1, "跨拍只发自上一拍以来的增量，不发全量快照");
            assert!(
                c.get("chat_rounds").is_none(),
                "零增量键不上线（空闲键不刷拍）: {c}"
            );
            break;
        }
        assert!(Instant::now() < deadline, "再 bump 后应出现带增量的心跳");
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(client);
    let _ = std::fs::remove_file(&path);
}

/// topic 参数的写入形状钉死：rrd 的 rr3.call 解析器按 params.topic/args 取值
#[test]
fn call_topic_sends_method_and_params_shape() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (client, path) = connect_on("ct", log.clone());
    // 假 rrd 对 rr3.call 不回标准应答（走 _ 分支回了 {"noop":true}）——够用来核对线上形状
    let _ = client.rr3_call_topic("rr3-t/say", json!({"text": "hi"}), Duration::from_secs(2));
    let log = log.lock().unwrap();
    let (_, params) = log
        .iter()
        .find(|(m, _)| m == RR3_METHOD_CALL)
        .expect("rr3.call 应到达");
    assert_eq!(params["topic"], "rr3-t/say");
    assert_eq!(params["args"]["text"], "hi");
    drop(client);
    let _ = std::fs::remove_file(&path);
}

/// 总线死亡探针：对端在注册后掉线 → reader EOF → `rr3_connected` 翻假、
/// `rr3_poll_wait` 立即返回空。心跳用大间隔（3600s）：否则心跳线程先撞上
/// 断连走 `exit(1)`，测试进程自己先死，探针根本轮不到被观察。
#[test]
fn disconnect_flips_connected_false_and_poll_wait_returns_promptly() {
    use std::time::Instant;
    let path = tmp_bus_path("dead");
    let listener = UnixListener::bind(&path).unwrap();
    // 假 rrd：完成注册握手后**挂起等信号**，测试先观察"活着"，再 drop 发送端
    // 让 stream 掉线（FIN）——直接退场会和 connect 返回赛跑，探针的初值就成了赌运气
    let (close_tx, close_rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let Ok(mut stream) = listener.accept().map(|(s, _)| s) else {
            return;
        };
        if rr3_frame_read(&mut stream).is_err() {
            return;
        }
        let payload = rr3_body_encode(
            Rr3PayloadType::Response,
            json!({"jsonrpc": "2.0", "id": 1, "result": {"src_task": 7}})
                .to_string()
                .as_bytes(),
        );
        let out = frame_send(payload);
        if rr3_frame_write(&mut stream, &out).is_err() {
            return;
        }
        let _ = close_rx.recv(); // 挂到测试发令再退场（drop stream → 对端 EOF）
    });
    // 心跳用大间隔：否则心跳线程先撞上断连走 exit(1)，测试进程自己先死
    let manifest = MANIFEST.replace("heartbeat_ms = 50", "heartbeat_ms = 3600000");
    let client = Rr3BusClient::rr3_connect_with(
        path.to_str().unwrap(),
        rr3_manifest_ident(&manifest).unwrap(),
        &manifest,
    )
    .expect("注册握手应成功");
    assert_eq!(client.rr3_task(), 7);
    assert!(client.rr3_connected(), "握手刚完成时必须活着");

    // 发令掉线：对端 stream drop → 客户端 reader 撞上 EOF
    drop(close_tx);
    // 有界轮询，不猜时刻
    let deadline = Instant::now() + Duration::from_secs(2);
    while client.rr3_connected() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!client.rr3_connected(), "对端掉线后探针必须翻假");
    // 断开后的 poll_wait 立即返回空（满转僵尸的根源，这里钉住它是"可检测的"）
    let started = Instant::now();
    assert!(client.rr3_poll_wait(Duration::from_secs(5)).is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "断开后 poll_wait 必须立即返回"
    );
    let _ = std::fs::remove_file(&path);
}
