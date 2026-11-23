// SPDX-License-Identifier: Apache-2.0
//! 整轮计账与唤醒用例。

use super::*;

// ---------- 整轮计账拆分（Python test_composite.TestFinishTurnLedger 对位） ----------

/// played_ms>0：两本账各记一次、点名超时经 config 注入（B-13①）、意图照发
#[test]
fn played_injected() {
    let (mut dp, _g) = mock_dispatcher(cfg_call_timeout(12.5));
    let bus = FakeTurnBus::reply(json!({"played_ms": 120}));
    dp.dialog.rr3_feed_vad("onset", 0);
    dp.dialog
        .rr3_feed_pcm(&rr3_base64_encode(&pcm_bytes(320)), 32)
        .unwrap();
    dp.rr3_finish_turn(&bus).unwrap();
    let intent = dp_published_intent(&bus);
    assert_eq!(intent["trace"], 1);
    assert_eq!(bus.bumps_of("rounds_replied"), 1, "应答一本账");
    assert_eq!(bus.bumps_of("rounds_audible"), 1, "听见一本账");
    assert_eq!(bus.bumps_of("chat_rounds"), 0, "旧键退役不再记");
    assert_eq!(
        bus.calls.borrow()[0].2,
        Duration::from_millis(12500),
        "点名超时经 config 注入（B-13①）"
    );
}

/// 播放点名失败：只进应答账，听见账不动——"完成一轮"与"用户听见"自此可分
/// （不许一个词盖两件事）；意图照发（动作不丢）
#[test]
fn call_fail_intent_pub() {
    let (mut dp, _g) = mock_dispatcher(cfg());
    let bus = FakeTurnBus::failing_call("tts 播出 0ms（…），不得记 ok（I-21①）");
    dp.dialog.rr3_feed_vad("onset", 0);
    dp.dialog
        .rr3_feed_pcm(&rr3_base64_encode(&pcm_bytes(320)), 32)
        .unwrap();
    dp.rr3_finish_turn(&bus).unwrap();
    assert_eq!(bus.bumps_of("rounds_replied"), 1);
    assert_eq!(bus.bumps_of("rounds_audible"), 0);
    assert!(
        !bus.published_of(RR3_TOPIC_INTENT).is_empty(),
        "点名失败意图照发（动作不丢）"
    );
}

/// played_ms=0 不算听见（interact 修后 0ms 折 error；此处防御异常形状仍不进账）
#[test]
fn zero_ms_not_audible() {
    let (mut dp, _g) = mock_dispatcher(cfg());
    let bus = FakeTurnBus::reply(json!({"played_ms": 0}));
    dp.dialog.rr3_feed_vad("onset", 0);
    dp.dialog
        .rr3_feed_pcm(&rr3_base64_encode(&pcm_bytes(320)), 32)
        .unwrap();
    dp.rr3_finish_turn(&bus).unwrap();
    assert_eq!(bus.bumps_of("rounds_replied"), 1);
    assert_eq!(bus.bumps_of("rounds_audible"), 0, "0ms 不算听见");
}

/// 云端 ASR 失败弃轮：两本账都不动（既有 asr_fail 计数语义不变）——real 档
/// 缺密钥即 ASR 确定性失败（不触网）
#[test]
fn asr_fail_ledger() {
    let (mut dp, g) = mock_dispatcher(cfg());
    // 整装后换 real 档并撤密钥：mock 判据现读 env，半途换档即生效
    std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "real");
    std::env::remove_var(crate::cloud::RR3_API_KEY_ENV);
    let bus = FakeTurnBus::reply(json!({"played_ms": 120}));
    dp.dialog.rr3_feed_vad("onset", 0);
    dp.dialog
        .rr3_feed_pcm(&rr3_base64_encode(&pcm_bytes(320)), 32)
        .unwrap();
    dp.rr3_finish_turn(&bus).unwrap();
    std::env::remove_var(crate::cloud::RR3_CLOUD_MODE_ENV);
    drop(g);
    assert_eq!(bus.bumps_of("rounds_replied"), 0);
    assert_eq!(bus.bumps_of("rounds_audible"), 0);
    assert_eq!(bus.bumps_of("asr_fail"), 1, "弃轮仍有失败计数");
}

/// 固定唤醒应答在启动时预合成；第一轮 Ack 只请求 ASR，不再请求 TTS。
#[test]
fn wake_ack_pcm_reused() {
    let _g = env_lock();
    // keep-alive 档：本用例在同一宿主上先 TTS 预合成、再 ASR 两请求；默认 close 档
    // 会让共享 Agent 在 macOS 上复用对端已关的连接（写成功、读 EOF），两次重试都
    // 打不出去（2026-11-24 host-macos 连红，Linux 绿）。真实云端点保持连接，
    // keep-alive 更贴近被测形态。
    let srv = crate::cloud::testkit::FakeCloud::start_keep_alive();
    std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "real");
    std::env::set_var(crate::cloud::RR3_API_KEY_ENV, "test-key");
    std::env::remove_var(crate::cloud::RR3_TTS_BACKEND_ENV);
    let cfg = rr3_brain_cfg_parse(&format!(
            // 本地假端点的超时给足：本用例钉的是"复用预合成 PCM"，
            // 不是超时行为——1s 在共享 macOS runner 上会偶发 ASR 未发出即超时
            // （2026-11-24 实测 host-macos 连红，Linux 绿）。
            "[rr3-brain.cloud]\nbase_url = \"{}\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 30\n[rr3-brain.dialog]\n[rr3-brain.wake]\nenable = true\nphrases = [\"你好洛克\"]\nack = \"我在呢\"\nfollowup_ms = 30000\n",
            srv.base
        ))
        .unwrap();
    let mut dp = Rr3BrainDispatcher::new(cfg).unwrap();
    assert!(dp.wake_ack_pcm.is_some(), "启动即备好固定应答 PCM");
    assert_eq!(srv.requests.lock().unwrap().len(), 1, "预合成只打一次 TTS");

    dp.dialog.rr3_feed_vad("onset", 0);
    dp.dialog
        .rr3_feed_pcm(&rr3_base64_encode(&pcm_bytes(320)), 32)
        .unwrap();
    let bus = FakeTurnBus::reply(json!({"played_ms": 120}));
    let utt_len = dp.dialog.rr3_utt_len();
    assert_eq!(utt_len, 320, "onset + pcm 后轮缓冲应有 320 B");
    dp.rr3_finish_turn(&bus).unwrap();

    let paths: Vec<String> = srv
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|(path, _, _)| path.clone())
        .collect();
    // 归因面：ASR 失败计数与云档一并进断言文案（macOS 连红时只看到 paths 缺 ASR，
    // 分不清"没走真云"还是"走了但打不出去"）。
    assert_eq!(
        bus.bumps_of("asr_fail"),
        0,
        "ASR 失败（mode={:?}，paths={paths:?}）",
        crate::cloud::rr3_cloud_mode()
    );
    assert_eq!(
        paths,
        vec!["/v1/audio/speech", "/v1/audio/transcriptions"],
        "首个唤醒轮复用预合成 PCM（mode={:?} asr_fail={} utt_len={utt_len} paths={paths:?}）",
        crate::cloud::rr3_cloud_mode(),
        bus.bumps_of("asr_fail")
    );
    assert_eq!(dp_last_intent(&bus)["reply_text"], "我在呢");
    std::env::remove_var(crate::cloud::RR3_API_KEY_ENV);
    std::env::remove_var(crate::cloud::RR3_CLOUD_MODE_ENV);
}

/// 唤醒词整轮（mock 档）：无唤醒窗外 → 静默不发布；仅唤醒 → 固定应答且
/// 不走 LLM；唤醒后窗内追问 → 正常应答；唤醒带命令 → 去掉唤醒词后的
/// 命令进 LLM。四条路径一轮内跑完。
#[test]
fn wake_ack_command() {
    let (mut dp, _g) = mock_dispatcher(cfg_wake(15_000));
    let bus = FakeTurnBus::reply(json!({"played_ms": 120}));
    let feed = |dp: &mut Rr3BrainDispatcher| {
        dp.dialog.rr3_feed_vad("onset", 0);
        dp.dialog
            .rr3_feed_pcm(&rr3_base64_encode(&pcm_bytes(320)), 32)
            .unwrap();
    };

    // 1) 从未唤醒：普通文本在窗外 → 静默（不发布 intent、不回复）
    std::env::set_var("RR3_BRAIN_MOCK_ASR_TEXT", "今天天气怎么样");
    feed(&mut dp);
    dp.rr3_finish_turn(&bus).unwrap();
    assert!(
        bus.published_of(RR3_TOPIC_INTENT).is_empty(),
        "窗外未命中唤醒词必须静默"
    );
    assert_eq!(bus.bumps_of("rounds_replied"), 0);

    // 2) 仅唤醒 → 固定应答（不进 LLM：mock LLM 会回 “（mock 回复）…”）
    std::env::set_var("RR3_BRAIN_MOCK_ASR_TEXT", "你好洛克");
    feed(&mut dp);
    dp.rr3_finish_turn(&bus).unwrap();
    let intent = dp_last_intent(&bus);
    assert_eq!(intent["reply_text"], "我在呢，有什么可以帮您？");
    assert_eq!(intent["user_text"], "你好洛克");

    // 3) 唤醒后窗内追问 → 正常进 LLM（原文本不被改写）
    std::env::set_var("RR3_BRAIN_MOCK_ASR_TEXT", "那明天呢");
    feed(&mut dp);
    dp.rr3_finish_turn(&bus).unwrap();
    let intent = dp_last_intent(&bus);
    assert_eq!(intent["reply_text"], "（mock 回复）那明天呢");

    // 4) 唤醒带命令 → 去唤醒词后的命令进 LLM
    std::env::set_var("RR3_BRAIN_MOCK_ASR_TEXT", "你好洛克，今天天气怎么样？");
    feed(&mut dp);
    dp.rr3_finish_turn(&bus).unwrap();
    let intent = dp_last_intent(&bus);
    assert_eq!(intent["reply_text"], "（mock 回复）今天天气怎么样");
    assert_eq!(intent["user_text"], "你好洛克，今天天气怎么样？");
    std::env::remove_var("RR3_BRAIN_MOCK_ASR_TEXT");
}
