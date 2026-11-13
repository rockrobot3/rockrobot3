//! 面板服务端测试：夹具（共享面板/单次服务/自检回环/假 rrd）与三组用例。

mod routes;
mod selftest;
mod service;

use super::*;
use std::io::{Read, Write};

use rr_ipc::rr3_frame_read;

use crate::panel::exec::{
    rr3_bus_reply_status, rr3_leg_relay, rr3_relay_expect, rr3_selftest_verdict,
};

/// 面板测试底座（四处同款）：缺省限流/期望 + 无主循环 link 的共享面
fn panel_shared(
    bus: BusHandle,
    bus_path: String,
    actions_tx: mpsc::Sender<(Rr3PanelAction, mpsc::Sender<Result<Value, String>>)>,
) -> Arc<Rr3PanelShared> {
    Arc::new(Rr3PanelShared {
        bus,
        bus_path,
        actions: actions_tx,
        link: Arc::new(Mutex::new(None)),
        limits: Rr3PanelLimits::default(),
        selftest_face_expect: "happy",
        selftest_expect: Rr3SelftestExpect::default(),
    })
}

/// 单连接服务底座（三处同款）：一次 accept 交给真 handler，带回地址与线程句柄
fn panel_serve_once(
    shared: Arc<Rr3PanelShared>,
) -> (std::net::SocketAddr, std::thread::JoinHandle<()>) {
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = tcp.local_addr().unwrap();
    let jh = std::thread::spawn(move || {
        let (mut sock, _) = tcp.accept().unwrap();
        rr3_panel_handle(&mut sock, &shared)
    });
    (addr, jh)
}

/// 自检 HTTP roundtrip 底座（两处同款）：POST /api/selftest 单请求，
/// 读全应答等线程收摊，解析出 JSON 体
fn selftest_roundtrip(addr: std::net::SocketAddr, jh: std::thread::JoinHandle<()>) -> Value {
    let mut c = TcpStream::connect(addr).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    c.write_all(b"POST /api/selftest HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}")
        .unwrap();
    let _ = c.shutdown(std::net::Shutdown::Write);
    let mut buf = Vec::new();
    c.read_to_end(&mut buf).expect("自检应答必须完整可读");
    jh.join().unwrap();
    let text = String::from_utf8_lossy(&buf);
    let body = text.split("\r\n\r\n").nth(1).expect("应答带 body");
    serde_json::from_str(body).expect("自检应答是 JSON: {body}")
}

/// BusCall 应答状态码裁定（I-21③）：载荷含 error = 422 业务失败，其余 200

// ---------- 批M M-β I-4：spawn 失败不退 accept 循环 + 槽位 RAII ----------

/// 假 rrd（I-4 夹具）：注册回 src_task=7、心跳回 accepted，并把每拍心跳
/// params 原样记账——测试轮询 params.counters 断言 panel_spawn_fail 上报。
const I4_MANIFEST: &str = "[manifest]\nschema_version = 1\nname = \"rr3-interact\"\nversion = \"0.0.1\"\ntype = \"module\"\nlang = \"rust\"\nentry = \"/bin/true\"\n\n[events]\npublishes = []\nsubscribes = []\n\n[health]\nheartbeat_ms = 50\n";

#[cfg(test)]
fn rr3_i4_fake_rrd(
    listener: std::os::unix::net::UnixListener,
    beats: Arc<Mutex<Vec<Value>>>,
    wiring: Value,
    contracts: Value,
) {
    // 每连接一线程：总线心跳是长连接（占住内层读环），而自检腿四/腿五的
    // ctl.wiring / ctl.contracts 查询会另开短连接——串行 accept 会让短连接
    // 永远排在长连接后面，5s 读超时把整个自检拖挂（真 rrd 也是并发连接，
    // 假件不得更弱）
    while let Ok((stream, _)) = listener.accept() {
        let beats = beats.clone();
        let wiring = wiring.clone();
        let contracts = contracts.clone();
        std::thread::spawn(move || rr3_i4_fake_rrd_conn(stream, beats, wiring, contracts));
    }
}

/// 假 rrd 单连接处理体（注册回 src_task=7、心跳回 accepted，并把每拍心跳
/// params 原样记账；ctl.wiring / ctl.contracts 回各自预置应答）
#[cfg(test)]
fn rr3_i4_fake_rrd_conn(
    mut stream: std::os::unix::net::UnixStream,
    beats: Arc<Mutex<Vec<Value>>>,
    wiring: Value,
    contracts: Value,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    while let Ok(frame) = rr3_frame_read(&mut stream) {
        let Ok((ty, body)) = rr3_body_split_of(&frame) else {
            break;
        };
        let Ok(v) = serde_json::from_slice::<Value>(&body) else {
            break;
        };
        if ty != Rr3PayloadType::Request {
            continue;
        }
        let method = v["method"].as_str().unwrap_or("");
        let result = match method {
            "rr3.register" => json!({ "src_task": 7 }),
            "rr3.heartbeat" => {
                beats.lock().unwrap().push(v["params"].clone());
                json!({ "beat": "accepted" })
            }
            "rr3.ctl.wiring" => wiring.clone(),
            "rr3.ctl.contracts" => contracts.clone(),
            // 夹具按 classic 形态在册 rr3-act：relay 腿应走 on/off 真调用；
            // host-only 的 n-a 分支由纯函数单测覆盖。
            "rr3.ctl.status" => json!({
                "modules": [{"name": "rr3-act", "task": 5, "phase": "running"}],
                "denied_publish": 0,
            }),
            _ => json!({ "noop": true }),
        };
        let id = v["id"].as_u64().unwrap_or(0);
        let payload = rr3_body_encode(
            Rr3PayloadType::Response,
            json!({"jsonrpc": "2.0", "id": id, "result": result})
                .to_string()
                .as_bytes(),
        );
        let out = Rr3Frame {
            header: Rr3Header {
                opcode: opcode::SEND,
                flags: 0,
                trace_id: 0,
                timestamp_ns: rr3_now_ns(),
                src_task: rr_core::RR3_TASK_RRD,
                dst_task: 7,
                payload_len: payload.len() as u32,
                payload_crc32: rr_ipc::rr3_crc32(&payload),
            },
            payload,
        };
        if rr3_frame_write(&mut stream, &out).is_err() {
            break;
        }
    }
}

/// rr3_body_split 的 Result 化薄封装（夹具内让 `let Ok(..) .. else` 可用）
#[cfg(test)]
fn rr3_body_split_of(frame: &Rr3Frame) -> Result<(Rr3PayloadType, Vec<u8>), ()> {
    rr_ipc::rr3_body_split(&frame.payload).map_err(|_| ())
}
