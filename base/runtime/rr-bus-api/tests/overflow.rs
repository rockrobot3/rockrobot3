#![cfg(unix)]
// SPDX-License-Identifier: Apache-2.0
//! 满箱饥饿路径回归（2026-11-22 Pi 真机实证，集成测试面：不进 ⑫ 生产度量）。

use rr_bus_api::Rr3BusClient;
use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame, Rr3Header,
    Rr3PayloadType,
};
use serde_json::{json, Value};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MANIFEST: &str = "[manifest]\nschema_version = 1\nname = \"rr3-bus-it\"\nversion = \"0.0.1\"\ntype = \"module\"\nlang = \"rust\"\nentry = \"/bin/true\"\n\n[events]\npublishes = []\nsubscribes = []\n\n[health]\nheartbeat_ms = 50\n";

fn tmp_bus_path() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "rr3-bus-it-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    p
}

fn frame_send(payload: Vec<u8>) -> Rr3Frame {
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: 0,
            src_task: 1,
            dst_task: 7,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// 假 rrd：接受连接、应答注册后灌 320 帧广播，心跳转发到测试侧。
fn fake_rrd(listener: UnixListener, hb_tx: mpsc::Sender<Value>) {
    let (mut stream, _) = listener.accept().expect("假 rrd 应接受到连接");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    loop {
        let Ok(frame) = rr3_frame_read(&mut stream) else {
            return;
        };
        let Ok((ty, body)) = rr3_body_split(&frame.payload) else {
            return;
        };
        let Ok(v) = serde_json::from_slice::<Value>(&body) else {
            return;
        };
        if ty != Rr3PayloadType::Request {
            continue;
        }
        let id = v["id"].as_u64().unwrap_or(0);
        let method = v["method"].as_str().unwrap_or("");
        if method == "rr3.register" {
            let reply = json!({"jsonrpc":"2.0","id":id,"result":{"src_task":7}});
            let payload = rr3_body_encode(Rr3PayloadType::Response, reply.to_string().as_bytes());
            rr3_frame_write(&mut stream, &frame_send(payload)).unwrap();
            // 灌 >256 帧广播（当前线上全是 Event）：客户端不 poll，旧实现 reader 会卡在第 257 帧
            for i in 0..320u32 {
                let body = json!({"topic":"rr3-fake/pcm","seq":i,"pcm_b64":"AA=="});
                let payload = rr3_body_encode(Rr3PayloadType::Event, body.to_string().as_bytes());
                rr3_frame_write(&mut stream, &frame_send(payload)).unwrap();
            }
        } else if method == "rr3.heartbeat" {
            hb_tx.send(v["params"].clone()).unwrap();
            let reply = json!({"jsonrpc":"2.0","id":id,"result":{}});
            let payload = rr3_body_encode(Rr3PayloadType::Response, reply.to_string().as_bytes());
            rr3_frame_write(&mut stream, &frame_send(payload)).unwrap();
        }
    }
}

/// 有界等待心跳自报的 inbox_backlog_drop 越过 64（3s 上界）。
fn await_drops(hb_rx: &mpsc::Receiver<Value>) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut drops = 0u64;
    while Instant::now() < deadline {
        let Ok(params) = hb_rx.recv_timeout(Duration::from_millis(500)) else {
            continue;
        };
        drops = params["counters"]["inbox_backlog_drop"]
            .as_u64()
            .unwrap_or(0);
        if drops > 0 {
            break;
        }
    }
    drops
}

/// 主循环被长动作（ASR 超时重试、整段 TTS 播放）占住、inbox 被广播帧填满时，
/// reader 不能卡在 send 上——否则心跳应答读不到，模块被自己的心跳宽限判死。
#[test]
fn inbox_full_keeps_heartbeat() {
    let path = tmp_bus_path();
    let listener = UnixListener::bind(&path).unwrap();
    let (hb_tx, hb_rx) = mpsc::channel::<Value>();
    std::thread::spawn(move || fake_rrd(listener, hb_tx));
    let client =
        Rr3BusClient::rr3_connect_at(path.to_str().unwrap(), MANIFEST).expect("连假 rrd 应成功");
    let drops = await_drops(&hb_rx);
    assert!(
        drops >= 64,
        "320 帧灌入 256 上限后应至少丢 64 帧事件面广播，实得 {drops}"
    );
    assert!(
        client.rr3_connected(),
        "满箱丢事件面广播期间心跳必须继续可应答，模块不得自杀"
    );
    drop(client);
    let _ = std::fs::remove_file(&path);
}
