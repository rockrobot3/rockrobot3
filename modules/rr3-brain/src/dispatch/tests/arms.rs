//! 臂位用例：未知 topic、completion/ops/event/tick 四臂。

use super::*;

/// 未知 topic 兜底文案逐字（Python f"rr3-brain 只 exposes {RPC_EXPOSES}，收到
/// {rpc_topic}" 同形——列表 repr 是 Python 形状）
#[test]
fn unknown_topic_error_names_exposes_list() {
    let mut c = cfg();
    let (mut cl, _g) = mock_clients(&c);
    let mut d = Rr3Dialog::rr3_new(8000);
    let e = rr3_rpc_arm(&mut c, &mut cl, &mut d, "rr3-brain/other", &Value::Null).unwrap_err();
    assert_eq!(
            e,
            "rr3-brain 只 exposes ['rr3-brain/llm/completion', 'rr3-brain/ops/list', 'rr3-brain/ops/set', 'rr3-brain/model/list', 'rr3-brain/model/set', 'rr3-brain/model/test']，收到 rr3-brain/other"
        );
}

/// completion 臂激活（迁移件2）：mock 档回显＋{"text": …} 应答体（Python
/// result = {"text": llm.complete(text)} 同 wire）
#[test]
fn completion_arm_mocks_echo_reply() {
    let mut c = cfg();
    let (mut cl, _g) = mock_clients(&c);
    let mut d = Rr3Dialog::rr3_new(8000);
    let v = rr3_rpc_arm(
        &mut c,
        &mut cl,
        &mut d,
        RR3_COMPLETION_TOPIC,
        &serde_json::json!({"text": "在吗"}),
    )
    .unwrap();
    assert_eq!(v, serde_json::json!({"text": "（mock 回复）在吗"}));
}

/// completion 臂 text 参数 str() coercion（Python str(args.get("text", ""))：
/// 缺键回落空串、非串折词——mock 回显即观测面）
#[test]
fn completion_arm_coerces_text_like_python_str() {
    let mut c = cfg();
    let (mut cl, _g) = mock_clients(&c);
    let mut d = Rr3Dialog::rr3_new(8000);
    let v = rr3_rpc_arm(
        &mut c,
        &mut cl,
        &mut d,
        RR3_COMPLETION_TOPIC,
        &serde_json::json!({}),
    )
    .unwrap();
    assert_eq!(
        v,
        serde_json::json!({"text": "（mock 回复）"}),
        "缺 text 回落空串"
    );
    let v = rr3_rpc_arm(
        &mut c,
        &mut cl,
        &mut d,
        RR3_COMPLETION_TOPIC,
        &serde_json::json!({"text": 5}),
    )
    .unwrap();
    assert_eq!(
        v,
        serde_json::json!({"text": "（mock 回复）5"}),
        "str(5)='5' coercion"
    );
}

/// completion 臂 real 档缺密钥：业务失败 Err 折 error 应答（Python try/except
/// RuntimeError 同 wire——文案点名环境变量，不静默降级）
#[test]
fn completion_arm_real_without_key_folds_error() {
    let mut c = cfg();
    let _g = env_lock();
    let mut cl = Rr3CloudClients::from_cfg(&c);
    let mut d = Rr3Dialog::rr3_new(8000);
    std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "real");
    std::env::remove_var(crate::cloud::RR3_API_KEY_ENV);
    let e = rr3_rpc_arm(
        &mut c,
        &mut cl,
        &mut d,
        RR3_COMPLETION_TOPIC,
        &serde_json::json!({"text": "hi"}),
    )
    .unwrap_err();
    assert_eq!(e, "缺少环境变量 RR3_CLOUD_API_KEY，云端链路不可用");
    std::env::remove_var(crate::cloud::RR3_CLOUD_MODE_ENV);
}

/// ops/list 臂经 rr3_rpc_arm 全链路可达（快照组体在 rpc.rs 逐键钉，此处钉臂位）
#[test]
fn ops_list_arm_reachable_via_rpc_arm() {
    let mut c = cfg();
    let (mut cl, _g) = mock_clients(&c);
    let mut d = Rr3Dialog::rr3_new(8000);
    let v = rr3_rpc_arm(&mut c, &mut cl, &mut d, RR3_OPS_LIST_TOPIC, &Value::Null).unwrap();
    assert_eq!(v["ops"]["dialog"]["max_utterance_ms"], 8000);
}

/// ops/set 臂写后按组重建：llm 组写 model_id 后重建不红（重建取数在
/// rpc.rs model_row_of_reflects_live_write 钉真值），mock 档 complete 照走
#[test]
fn ops_set_arm_rebuilds_llm_client_after_write() {
    let mut c = cfg();
    let (mut cl, _g) = mock_clients(&c);
    let mut d = Rr3Dialog::rr3_new(8000);
    let v = rr3_rpc_arm(
        &mut c,
        &mut cl,
        &mut d,
        RR3_OPS_SET_TOPIC,
        &serde_json::json!({"group": "llm", "slot": "llm_model", "value": "m2"}),
    )
    .unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["new"], "m2");
    // 重建后 completion 臂照常可用（mock 档回显不依赖模型行，行为连续性观测）
    let v = rr3_rpc_arm(
        &mut c,
        &mut cl,
        &mut d,
        RR3_COMPLETION_TOPIC,
        &serde_json::json!({"text": "hi"}),
    )
    .unwrap();
    assert_eq!(v, serde_json::json!({"text": "（mock 回复）hi"}));
}

/// ops/set dialog 组就地改参（迁移件四接线）：写臂应答后 max_utterance_ms
/// live 生效（doc 真值同拍写入——重启回 config.toml 真值）
#[test]
fn ops_set_arm_dialog_group_updates_live_param() {
    let mut c = cfg();
    let (mut cl, _g) = mock_clients(&c);
    let mut d = Rr3Dialog::rr3_new(8000);
    let v = rr3_rpc_arm(
        &mut c,
        &mut cl,
        &mut d,
        RR3_OPS_SET_TOPIC,
        &serde_json::json!({"group": "dialog", "slot": "max_utterance_ms", "value": 9000}),
    )
    .unwrap();
    assert_eq!(v["new"], 9000);
    assert_eq!(d.max_utterance_ms, 9000, "状态机话轮封顶 live 生效");
    assert_eq!(
        c.doc["rr3-brain"]["dialog"]["max_utterance_ms"],
        serde_json::json!(9000),
        "doc 真值已写"
    );
}

/// 事件臂订阅面外事件静默过滤（Python 主循环 if/elif 兜底同形状）
#[test]
fn event_arm_ignores_unsubscribed_topics() {
    let (mut dp, _g) = mock_dispatcher(cfg());
    let bus = FakeTurnBus::reply(json!({"played_ms": 120}));
    dp.rr3_event_arm("rr3-other/topic", &json!({"x": 1}), &bus)
        .unwrap();
    assert!(bus.published.borrow().is_empty());
    assert!(bus.bumps.borrow().is_empty());
}

/// 事件臂全链路：pcm 预滚/积累 + vad onset/offset 判停 → 触发整轮
/// （mock 档三客户端走通 ASR→LLM→TTS→分帧→点名→intent 发布）
#[test]
fn event_arm_vad_offset_triggers_full_turn() {
    let (mut dp, _g) = mock_dispatcher(cfg());
    let bus = FakeTurnBus::reply(json!({"played_ms": 120}));
    dp.rr3_event_arm(RR3_TOPIC_PCM, &pcm_event_body(b"\x01\x02", 200), &bus)
        .unwrap(); // onset 前判据段（预滚）
    dp.rr3_event_arm(RR3_TOPIC_VAD, &json!({"edge": "onset"}), &bus)
        .unwrap();
    dp.rr3_event_arm(RR3_TOPIC_PCM, &pcm_event_body(b"\x03\x04", 32), &bus)
        .unwrap();
    dp.rr3_event_arm(RR3_TOPIC_VAD, &json!({"edge": "offset"}), &bus)
        .unwrap();
    // intent 发布 = 整轮走通的观测面（trace 从 1 起）
    let intent = &dp_published_intent(&bus);
    assert_eq!(intent["trace"], 1);
    assert!(
        intent["user_text"].as_str().unwrap().contains("你好"),
        "mock ASR 文本入轮"
    );
    // tts_pcm 流已分帧发布（载荷通路），点名恰好一次
    assert!(!bus.published_of(RR3_TOPIC_TTS_PCM).is_empty());
    assert_eq!(bus.calls.borrow().len(), 1);
    assert_eq!(bus.calls.borrow()[0].0, RR3_TOPIC_TTS);
    // 拆账两本都有（played_ms=120 > 0）
    assert_eq!(bus.bumps_of("rounds_replied"), 1);
    assert_eq!(bus.bumps_of("rounds_audible"), 1);
}

/// intent 发布体取用（整轮走通的观测面；恰一次发布）
fn dp_published_intent(bus: &FakeTurnBus) -> Value {
    let intents = bus.published_of(RR3_TOPIC_INTENT);
    assert_eq!(intents.len(), 1, "整轮恰发布一次 intent");
    intents.into_iter().next().unwrap()
}

/// 多轮用例取**最后一条** intent（前轮已发布过，不能用恰一次断言）
fn dp_last_intent(bus: &FakeTurnBus) -> Value {
    bus.published_of(RR3_TOPIC_INTENT)
        .last()
        .cloned()
        .expect("至少一轮 intent")
}

/// 节拍臂时长判停触发整轮：0 封顶 onset 即刻到点（Python
/// test_duration_cap_expires 同款零等待技巧）
#[test]
fn tick_arm_expired_triggers_full_turn() {
    let mut c = cfg();
    c.max_utterance_ms = 0;
    let (mut dp, _g) = mock_dispatcher(c);
    let bus = FakeTurnBus::reply(json!({"played_ms": 120}));
    dp.rr3_event_arm(RR3_TOPIC_PCM, &pcm_event_body(b"\x01\x02", 32), &bus)
        .unwrap();
    dp.rr3_event_arm(RR3_TOPIC_VAD, &json!({"edge": "onset"}), &bus)
        .unwrap();
    dp.rr3_tick_arm(&bus).unwrap();
    assert!(
        !bus.published_of(RR3_TOPIC_INTENT).is_empty(),
        "时长判停应触发整轮"
    );
    // 复位后同一拍不再触发（take 交空直接返回）
    dp.rr3_tick_arm(&bus).unwrap();
    assert_eq!(bus.bumps_of("rounds_replied"), 1, "一拍只走一轮");
}
