//! 满箱饥饿路径回归（2026-11-22 Pi 真机实证）。

use super::super::conn::RR3_COUNTER_INBOX_STREAM_DROP;
use super::super::{rr3_manifest_ident, Rr3BusClient};
use super::{frame_send, tmp_bus_path, MANIFEST};
use rr_ipc::{rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3PayloadType};
use serde_json::{json, Value};
use std::os::unix::net::UnixListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// 主循环被长云调用（ASR 超时重试）占住、inbox 被音频流帧填满时，reader 不能
/// 卡在 send 上——否则心跳应答读不到，模块被自己的心跳宽限判死。流帧满箱丢弃
/// 要计数，心跳必须继续通。
#[test]
fn full_inbox_drops_stream_frames_and_keeps_heartbeat_alive() {
    let path = tmp_bus_path("stream-drop");
    let listener = UnixListener::bind(&path).unwrap();
    let (hb_tx, hb_rx) = mpsc::channel::<Value>();
    std::thread::spawn(move || {
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
                let payload =
                    rr3_body_encode(Rr3PayloadType::Response, reply.to_string().as_bytes());
                if rr3_frame_write(&mut stream, &frame_send(payload)).is_err() {
                    return;
                }
                // 灌 >256 帧流：客户端不 poll，旧实现 reader 会卡在第 257 帧
                for i in 0..320u32 {
                    let body = json!({"topic":"rr3-fake/pcm","seq":i,"pcm_b64":"AA=="});
                    let payload =
                        rr3_body_encode(Rr3PayloadType::Stream, body.to_string().as_bytes());
                    if rr3_frame_write(&mut stream, &frame_send(payload)).is_err() {
                        return;
                    }
                }
            } else if method == "rr3.heartbeat" {
                let _ = hb_tx.send(v["params"].clone());
                let reply = json!({"jsonrpc":"2.0","id":id,"result":{}});
                let payload =
                    rr3_body_encode(Rr3PayloadType::Response, reply.to_string().as_bytes());
                if rr3_frame_write(&mut stream, &frame_send(payload)).is_err() {
                    return;
                }
            }
        }
    });
    let client = Rr3BusClient::rr3_connect_with(
        path.to_str().unwrap(),
        rr3_manifest_ident(MANIFEST).unwrap(),
        MANIFEST,
    )
    .expect("连假 rrd 应成功");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut drops = 0u64;
    while Instant::now() < deadline {
        let Ok(params) = hb_rx.recv_timeout(Duration::from_millis(500)) else {
            continue;
        };
        drops = params["counters"][RR3_COUNTER_INBOX_STREAM_DROP]
            .as_u64()
            .unwrap_or(0);
        if drops > 0 {
            break;
        }
    }
    assert!(
        drops >= 64,
        "320 帧灌入 256 上限后应至少丢 64 帧流帧，实得 {drops}"
    );
    assert!(
        client.rr3_connected(),
        "满箱丢流帧期间心跳必须继续可应答，模块不得自杀"
    );
    drop(client);
    let _ = std::fs::remove_file(&path);
}
