//! manifest 文本与帧构造助手（与线上同一 rr-ipc 编码）。

use super::*;

// ---------- manifest 文本 ----------

/// 合法的最小可用 manifest。改这里的字面量必须同步改测试里的 `.replace(...)` 目标。
pub fn rr3_manifest(name: &str, hb: u64, wd: u64, policy: &str) -> String {
    format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/bin/{name}"

[events]
publishes = []
subscribes = ["rr3-runtime/module/state"]

[resources]
restart_policy = "{policy}"

[health]
heartbeat_ms = {hb}
watchdog_timeout_ms = {wd}
"#
    )
}

// ---------- 帧 ----------

pub fn rr3_request(method: &str, id: u64, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

pub fn rr3_json_frame(ty: Rr3PayloadType, body: &Value, src_task: u64, dst_task: u64) -> Rr3Frame {
    let payload = rr3_body_encode(ty, body.to_string().as_bytes());
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: 0,
            src_task,
            dst_task,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// 上行 RPC（默认 dst=rrd）
pub fn rr3_rpc_frame(method: &str, id: u64, params: Value, src_task: u64) -> Rr3Frame {
    rr3_json_frame(
        Rr3PayloadType::Request,
        &rr3_request(method, id, params),
        src_task,
        rr_bus::RR3_TASK_RRD,
    )
}

/// 注册帧（此时模块还没有 task，按契约填 0——"用户态不得自设"的另一种写法）
pub fn rr3_register_frame(manifest: &str, name: &str) -> Rr3Frame {
    rr3_rpc_frame(
        "rr3.register",
        1,
        json!({"name": name, "manifest": manifest}),
        0,
    )
}
