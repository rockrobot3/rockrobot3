//! 上行调用、业务发布与点名应答路径。

use std::time::Duration;

use rr_core::{rr3_frame_compose, RR3_METHOD_CALL, RR3_TASK_RRD};
use rr_ipc::{opcode, rr3_body_encode, Rr3PayloadType};
use serde_json::{json, Value};

use super::conn::{rr3_now_ns, Rr3BusClient};

impl Rr3BusClient {
    /// 运维计数自报（0.0.4 批B，Q1 裁定；b3c 2026-10-21 增量语义钉死）：给键累加
    /// `by`，心跳线程每拍把「自上一拍以来」的增量塞进 `rr3.heartbeat` 的可选
    /// `counters` 字段（rrd 侧按增量归并，`rr3.ctl.counters` 聚合表 = 生命周期
    /// 累计）。键是模块内部运维词汇（不逐键 pin，归属表落各模块 README）；无上报
    /// 的模块字段缺省，rrd 侧未知字段忽略——旧端 / 新端双向兼容。
    pub fn rr3_counter_bump(&self, key: &str, by: u64) {
        let mut t = self
            .inner
            .counters
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        *t.entry(key.to_string()).or_insert(0) += by;
    }

    /// 上行 RPC 一次一答（rr3.heartbeat 由心跳线程用，模块业务也可用）
    pub fn rr3_call(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        self.inner
            .rr3_call_inner(self.inner.rr3_task(), method, params, timeout)
    }

    /// 模块间请求：`rr3.call`（第 4 个上行 RPC 方法）。`args` 原样进 params.args，
    /// 由 rrd 路由到 exposes 该 topic 的在册模块，trace_id = 调用方 task。
    pub fn rr3_call_topic(
        &self,
        topic: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        self.rr3_call(
            RR3_METHOD_CALL,
            json!({"topic": topic, "args": args}),
            timeout,
        )
    }

    /// 业务发布：Event 帧，topic 并入体（conn.rs 按 body["topic"] 归属检查）。
    /// `body` 必须是 JSON 对象。
    pub fn rr3_publish(&self, topic: &str, body: Value) -> Result<(), String> {
        let mut body = match body {
            Value::Object(map) => Value::Object(map),
            other => return Err(format!("Event 体必须是 JSON 对象，收到 {other}")),
        };
        body["topic"] = json!(topic);
        self.inner
            .rr3_write_frame(rr_core::rr3_event_compose(
                rr3_now_ns(),
                self.inner.rr3_task(),
                rr3_body_encode(Rr3PayloadType::Event, body.to_string().as_bytes()),
            ))
            .map_err(|e| format!("发布 {topic} 失败: {e}"))
    }

    /// 应答一条点名到自己的 `rr3.call`：Response 上行，trace_id = 调用方 task，
    /// rrd 据此把应答改道回调用方（conn.rs 的 rr3_route_response）。
    /// `result` 是调用方在 `rr3_call_topic` 返回值里看到的对象。
    pub fn rr3_reply_call(&self, trace_id: u64, id: u64, result: Value) -> Result<(), String> {
        if trace_id == 0 {
            return Err("trace_id=0 的请求没有回路，无从应答".into());
        }
        let body = json!({"jsonrpc": "2.0", "id": id, "result": result});
        self.inner
            .rr3_write_frame(rr3_frame_compose(
                opcode::SEND,
                0,
                trace_id,
                rr3_now_ns(),
                self.inner.rr3_task(),
                RR3_TASK_RRD,
                rr3_body_encode(Rr3PayloadType::Response, body.to_string().as_bytes()),
            ))
            .map_err(|e| format!("应答 rr3.call 失败: {e}"))
    }
}
