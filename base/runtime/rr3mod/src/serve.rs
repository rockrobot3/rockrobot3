//! serve 主循环骨架（0.0.8 方案 §2.5，ADR-0017 裁定 1 序列的后三段：poll_wait →
//! inbox 分发 → 传输性失败上抛）。poll_wait 作实参——五模块现值不同（rr3-act /
//! rr3-interact 250ms、rr3-connect / rr3-power 100ms、rr3-sense 等价 0），不进
//! 公共配置。逐步骤规格见 [`serve`] 行内注（实施照抄；现行五模块主循环的逐字
//! 语义）。

use std::time::{Duration, Instant};

use rr_core::RR3_METHOD_CALL;
use rr_ipc::{opcode, Rr3PayloadType};
use serde_json::{json, Value};

use crate::{BusHandle, Dispatcher};

/// 主循环骨架：死亡探针 → 分帧 → RPC/订阅分发 → 节拍。`Dispatcher` 只在主线程
/// 被 `&mut` 回调；传输性失败（应答写不出去 / `on_event` / `on_tick` 的 `Err`）
/// 经 `?` 上抛——"模块不装活着，死交给 supervisor 按 restart_policy=always 重启"
/// 语义随上抛路径单点化（装配段 main 收尾 ≤3 行：`eprintln!("rr3-<module> 退出:
/// {e}")` + `exit(1)`）。
pub fn serve(
    bus: BusHandle,
    dispatcher: &mut impl Dispatcher,
    poll_wait: Duration,
) -> Result<(), String> {
    loop {
        // 步骤 1：总线死亡探针——断开后 poll_wait 立即返回空 Vec，与"暂时没帧"
        // 不可区分，不探测就满转成僵尸。文案逐字保留（现行五模块同款）。
        if !bus.client.rr3_connected() {
            return Err("总线已断开（reader EOF）：交 supervisor 按 restart_policy 裁定".into());
        }
        // 步骤 2：分帧——opcode != SEND 跳过；rr3_frame_body 解不开只排空。
        for frame in bus.client.rr3_poll_wait(poll_wait) {
            if frame.header.opcode != opcode::SEND {
                continue;
            }
            let Ok((ty, body)) = rr_bus_api::Rr3BusClient::rr3_frame_body(&frame) else {
                continue;
            };
            match ty {
                // 步骤 3：RPC 面——Request 且 method == rr3.call → 剥 id /
                // params.topic / params.args → on_rpc；Err 折 {"error": …}；
                // `?` 即传输性失败上抛——应答写不出去不装活着。
                Rr3PayloadType::Request => {
                    if body.get("method").and_then(Value::as_str) != Some(RR3_METHOD_CALL) {
                        continue;
                    }
                    let id = body.get("id").and_then(Value::as_u64).unwrap_or(0);
                    let params = body.get("params").cloned().unwrap_or(Value::Null);
                    let topic = params.get("topic").and_then(Value::as_str).unwrap_or("");
                    let args = params.get("args").cloned().unwrap_or(Value::Null);
                    let reply = dispatcher
                        .on_rpc(topic, &args, &bus)
                        .unwrap_or_else(|e| json!({ "error": e }));
                    bus.client.rr3_reply_call(frame.header.trace_id, id, reply)?;
                }
                // 步骤 4：订阅面——Event 交 on_event（topic 本地过滤是 Dispatcher
                // 的职责）；其余 payload（Notification/Stream 等）只排空。
                Rr3PayloadType::Event => {
                    let topic = body.get("topic").and_then(Value::as_str).unwrap_or("");
                    dispatcher.on_event(topic, &body, &bus)?;
                }
                _ => {}
            }
        }
        // 步骤 5：节拍——每圈一次（帧无关节拍）。
        dispatcher.on_tick(Instant::now(), &bus)?;
        // 步骤 6：回到 1。
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::test_support::{
        connect_client, rr3_wait_log, FakeMode, REQ_BAD_TOPIC, REQ_OK_TOPIC, RESP_ERROR_TEXT,
    };
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 记录型 Dispatcher（方案 §2.8-2 的 fake）：记回调序；`stop_after_ticks` 到点
    /// 由 on_tick 返 Err（测上抛路径）；`boom_event` 命中时 on_event 返 Err。
    struct RecordingDispatcher {
        events: Vec<String>,
        rpcs: Vec<(String, u64)>,
        ticks: AtomicU64,
        stop_after_ticks: u64,
        boom_event: &'static str,
    }

    impl RecordingDispatcher {
        fn new(stop_after_ticks: u64, boom_event: &'static str) -> Self {
            Self {
                events: Vec::new(),
                rpcs: Vec::new(),
                ticks: AtomicU64::new(0),
                stop_after_ticks,
                boom_event,
            }
        }
    }

    impl Dispatcher for RecordingDispatcher {
        fn on_rpc(&mut self, topic: &str, _args: &Value, _bus: &BusHandle) -> Result<Value, String> {
            self.rpcs.push((topic.to_string(), self.rpcs.len() as u64));
            if topic == REQ_BAD_TOPIC {
                return Err(RESP_ERROR_TEXT.into());
            }
            Ok(json!({ "ok": topic }))
        }

        fn on_event(&mut self, topic: &str, _body: &Value, _bus: &BusHandle) -> Result<(), String> {
            self.events.push(topic.to_string());
            if topic == self.boom_event {
                return Err("事件处理失败".into());
            }
            Ok(())
        }

        fn on_tick(&mut self, _now: Instant, _bus: &BusHandle) -> Result<(), String> {
            let n = self.ticks.fetch_add(1, Ordering::SeqCst) + 1;
            if self.stop_after_ticks > 0 && n >= self.stop_after_ticks {
                return Err("节拍止步".into());
            }
            Ok(())
        }
    }

    /// 方案 §2.8-2 主用例：回调序全钉——Request→on_rpc→reply 折返（Ok 与
    /// Err 折 {"error":…} 两形状）、Event→on_event、on_tick 每圈一次、
    /// on_tick 的 Err 上抛（serve 以该 Err 返回）。
    #[test]
    fn serve_dispatches_rpc_event_and_tick_in_order() {
        let bus = connect_client("serve-order", FakeMode::Primed);
        let mut d = RecordingDispatcher::new(2, "");
        let err = serve(bus.clone(), &mut d, Duration::ZERO).unwrap_err();
        assert_eq!(err, "节拍止步", "on_tick 的 Err 应原样上抛（方案 §2.5 步骤 5）");
        assert_eq!(
            d.events,
            vec!["rr3mod-t/echo".to_string(), "rr3mod-t/boom".to_string()],
            "Event 按 body.topic 进 on_event（本用例 boom 未标记，两条都进账）"
        );
        let topics: Vec<&str> = d.rpcs.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            topics,
            vec![REQ_OK_TOPIC, REQ_BAD_TOPIC],
            "两枚 rr3.call 按到帧序进 on_rpc"
        );
        assert!(d.ticks.load(Ordering::SeqCst) >= 2, "on_tick 每圈一次");
        // reply 折返两形状：Ok → result 原样；Err → {"error": …}（fake rrd 已记账：
        // 注册 req + 两枚 response）
        let log = rr3_wait_log("serve-order", 3);
        let ok_reply = log
            .iter()
            .find(|v| v["kind"] == "response" && v["id"] == 42)
            .expect("id=42 的应答应到 fake rrd");
        assert_eq!(
            ok_reply["result"],
            json!({ "ok": REQ_OK_TOPIC }),
            "on_rpc Ok → result 应答体"
        );
        let bad_reply = log
            .iter()
            .find(|v| v["kind"] == "response" && v["id"] == 43)
            .expect("id=43 的应答应到 fake rrd");
        assert_eq!(
            bad_reply["result"],
            json!({ "error": RESP_ERROR_TEXT }),
            "on_rpc Err 折 {{\"error\": …}}"
        );
    }

    /// 方案 §2.8-2 补充：on_event 的 Err 上抛（传输性/设备级档）。预推帧序
    /// echo → boom：echo 正常进账后 boom 处置失败，serve 以该 Err 返回
    /// （后到两枚 rr3.call 不再处理——上抛即停圈）。
    #[test]
    fn serve_propagates_event_handler_error() {
        let bus = connect_client("serve-boom", FakeMode::Primed);
        let mut d = RecordingDispatcher::new(0, "rr3mod-t/boom");
        let err = serve(bus, &mut d, Duration::ZERO).unwrap_err();
        assert_eq!(err, "事件处理失败", "on_event 的 Err 应原样上抛");
        assert_eq!(
            d.events,
            vec!["rr3mod-t/echo".to_string(), "rr3mod-t/boom".to_string()]
        );
        assert!(d.rpcs.is_empty(), "上抛即停圈，boom 之后的帧不再分发");
    }

    /// 方案 §2.8-2 死亡探针：断开后 serve 返回，文案逐字（现行五模块同款）。
    #[test]
    fn serve_death_probe_returns_exact_text() {
        let bus = connect_client("serve-dead", FakeMode::DropAfterRegister);
        std::thread::sleep(Duration::from_millis(100)); // 放大 reader EOF 先于 serve 的窗口
        let mut d = RecordingDispatcher::new(0, "");
        let err = serve(bus, &mut d, Duration::ZERO).unwrap_err();
        assert_eq!(
            err,
            "总线已断开（reader EOF）：交 supervisor 按 restart_policy 裁定",
            "死亡探针文案逐字（方案 §2.5 步骤 1）"
        );
    }

    /// 方案 §2.8-4：BusHandle Clone 冒烟——Clone 后同连接语义（双句柄 publish
    /// 各自可用，fake rrd 记到两条 Event）。
    #[test]
    fn bus_handle_clone_shares_connection() {
        let bus = connect_client("serve-clone", FakeMode::Quiet);
        let bus2 = bus.clone();
        assert!(bus.connected() && bus2.connected(), "双句柄同连接同活");
        bus.publish("rr3mod-t/one", json!({ "n": 1 })).unwrap();
        bus2.publish("rr3mod-t/two", json!({ "n": 2 })).unwrap();
        bus.counter_bump("clone_smoke", 1);
        // 等到注册 req + 两条 Event 都进账（reader 异步，publish 返回≠对端读到）
        let log = rr3_wait_log("serve-clone", 3);
        let topics: Vec<&str> = log
            .iter()
            .filter(|v| v["kind"] == "event")
            .filter_map(|v| v["topic"].as_str())
            .collect();
        assert!(
            topics.contains(&"rr3mod-t/one") && topics.contains(&"rr3mod-t/two"),
            "双句柄 publish 各自可达 fake rrd: {topics:?}"
        );
    }
}
