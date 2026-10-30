//! 测试共享夹具（cfg(test)）：假 rrd（体例先例＝rr-bus-api client.rs 测试段，
//! 行为写死的测试夹具不是通用服务器）+ env 串扰锁 + 临时目录助手。
//!
//! env 纪律：`RR3_BUS_PATH_ENV` 只在 [`connect_client`] 内 set→connect→remove，
//! 全程持 [`ENV_LOCK`]；boot.rs 的 env 用例同锁互斥——crate 内所有 env 变更
//! 单点串行，杜绝并行测试的进程级 env 竞态。

use std::collections::HashMap;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rr_core::RR3_TASK_RRD;
use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame,
    Rr3Header, Rr3PayloadType, RR3_BUS_PATH_ENV,
};
use serde_json::{json, Value};

use crate::BusHandle;

/// crate 内 env 变更互斥锁（set_var/remove_var 与并行读的竞态面单点串行）
pub static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// 测试 manifest（heartbeat_ms 拉大＝测试窗口内心跳不拍；reader EOF 走 Down
/// 停拍语义，不触喂狗失败退出）
pub const MANIFEST: &str = "[manifest]\nschema_version = 1\nname = \"rr3mod-t\"\nversion = \"0.0.1\"\ntype = \"module\"\nlang = \"rust\"\nentry = \"/bin/true\"\n\n[events]\npublishes = []\nsubscribes = []\n\n[health]\nheartbeat_ms = 50000\n";

/// 夹具词汇：on_rpc 的 Ok 路 / Err 路点名（RecordingDispatcher 按此折返）
pub const REQ_OK_TOPIC: &str = "rr3mod-t/ok";
pub const REQ_BAD_TOPIC: &str = "rr3mod-t/bad";
/// Err 路的折返文案（serve 折 {"error": …} 后 fake rrd 记到的原文）
pub const RESP_ERROR_TEXT: &str = "业务拒绝";

/// 假 rrd 行为三形状
pub enum FakeMode {
    /// 注册应答后预推回调序帧（echo/boom Event + 两枚 rr3.call）
    Primed,
    /// 只应答注册与心跳（Clone 冒烟用）
    Quiet,
    /// 注册应答后立即关流（死亡探针形状：reader EOF → stop 位 → serve 探针返）
    DropAfterRegister,
}

/// 各测试独立的 log 槽：tag → 收帧记账（kind=req/response/event）
pub fn log_slot(tag: &str) -> Arc<Mutex<Vec<Value>>> {
    static SLOTS: OnceLock<Mutex<HashMap<String, Arc<Mutex<Vec<Value>>>>>> = OnceLock::new();
    let slots = SLOTS.get_or_init(|| Mutex::new(HashMap::new()));
    slots
        .lock()
        .unwrap()
        .entry(tag.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(Vec::new())))
        .clone()
}

/// env 锁取用（boot.rs 的 env 用例同锁互斥）
pub fn env_lock() -> &'static Mutex<()> {
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

/// 唯一临时 socket/目录路径（体例同 rr-bus-api tmp_bus_path）
fn tmp_path(tag: &str, suffix: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "rr3mod-t-{tag}-{}-{}{suffix}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    p
}

/// 唯一临时目录（load_config_text 用例）
pub fn rr3_tmp_dir(tag: &str) -> PathBuf {
    tmp_path(tag, ".d")
}

/// 连假 rrd 取裸客户端（`RR3_BUS_PATH_ENV` set→connect→remove 全程持锁）——
/// [`connect_client`] 与 wrap 冒烟测共用的连接段，client 层真值单点。
pub fn connect_raw(tag: &str, mode: FakeMode) -> rr_bus_api::Rr3BusClient {
    let path = tmp_path(tag, ".sock");
    let listener = UnixListener::bind(&path).unwrap();
    let log = log_slot(tag);
    std::thread::spawn(move || fake_rrd(listener, log, mode));
    let _g = env_lock().lock().unwrap();
    std::env::set_var(RR3_BUS_PATH_ENV, path.to_str().unwrap());
    let client = rr_bus_api::Rr3BusClient::rr3_connect(MANIFEST).expect("连假 rrd 应成功");
    std::env::remove_var(RR3_BUS_PATH_ENV);
    client
}

/// 连假 rrd 并包进 [`BusHandle`]（构造走 [`BusHandle::wrap`]——pub(crate) 字段
/// 直构形状随批N29 退役，夹具与生产消费方同一构造口）
pub fn connect_client(tag: &str, mode: FakeMode) -> BusHandle {
    BusHandle::wrap(connect_raw(tag, mode))
}

/// 等待 fake rrd 记账到 `n` 条（至多 ~2s；reader 异步，不忙等断言）
pub fn rr3_wait_log(tag: &str, n: usize) -> Vec<Value> {
    let log = log_slot(tag);
    for _ in 0..200 {
        if log.lock().unwrap().len() >= n {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let snapshot = log.lock().unwrap().clone();
    snapshot
}

fn rr3_write(stream: &mut UnixStream, ty: Rr3PayloadType, trace_id: u64, body: Value) {
    let payload = rr3_body_encode(ty, body.to_string().as_bytes());
    let out = Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id,
            timestamp_ns: 0,
            src_task: RR3_TASK_RRD,
            dst_task: 7,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    };
    let _ = rr3_frame_write(stream, &out);
}

/// 注册应答后的预推帧：echo/boom Event（on_event 路）+ 两枚 rr3.call（Ok / Err
/// 折返两形状；trace_id=9 ≠ 0，serve 的 rr3_reply_call 才有回路）
fn prime_dispatch_frames(stream: &mut UnixStream) {
    rr3_write(
        stream,
        Rr3PayloadType::Event,
        0,
        json!({ "topic": "rr3mod-t/echo", "n": 1 }),
    );
    rr3_write(
        stream,
        Rr3PayloadType::Event,
        0,
        json!({ "topic": "rr3mod-t/boom" }),
    );
    rr3_write(
        stream,
        Rr3PayloadType::Request,
        9,
        json!({"jsonrpc": "2.0", "id": 42, "method": "rr3.call",
               "params": {"topic": REQ_OK_TOPIC, "args": {"x": 1}}}),
    );
    rr3_write(
        stream,
        Rr3PayloadType::Request,
        9,
        json!({"jsonrpc": "2.0", "id": 43, "method": "rr3.call",
               "params": {"topic": REQ_BAD_TOPIC, "args": {}}}),
    );
}

/// 假 rrd：收注册回 src_task=7；心跳回 accepted；上行 Event / Response 记账；
/// 行为按 [`FakeMode`] 三形状（测试夹具写死，不是通用服务器）。
fn fake_rrd(listener: UnixListener, log: Arc<Mutex<Vec<Value>>>, mode: FakeMode) {
    let drop_after_register = matches!(mode, FakeMode::DropAfterRegister);
    let primed = matches!(mode, FakeMode::Primed);
    let Ok((mut stream, _)) = listener.accept() else {
        return;
    };
    if stream.set_read_timeout(Some(Duration::from_secs(5))).is_err() {
        return;
    }
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
                log.lock().unwrap().push(json!({
                    "kind": "req", "method": method, "id": id, "params": v["params"],
                }));
                if method == "rr3.register" {
                    rr3_write(
                        &mut stream,
                        Rr3PayloadType::Response,
                        0,
                        json!({"jsonrpc": "2.0", "id": id, "result": {"src_task": 7}}),
                    );
                    if drop_after_register {
                        return; // 关流 → reader EOF → stop 位 → serve 探针返
                    }
                    if primed {
                        prime_dispatch_frames(&mut stream);
                    }
                }
                // 心跳与其他上行方法只记账不应答（heartbeat_ms=50000 测试窗口内不拍）
            }
            Rr3PayloadType::Response => {
                log.lock().unwrap().push(json!({
                    "kind": "response", "id": v["id"], "result": v["result"],
                }));
            }
            Rr3PayloadType::Event => {
                log.lock().unwrap().push(json!({
                    "kind": "event", "topic": v["topic"], "body": v,
                }));
            }
            _ => return,
        }
    }
}
