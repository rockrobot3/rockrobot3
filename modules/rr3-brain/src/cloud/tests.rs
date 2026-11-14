use super::testkit::{env_lock, FakeCloud};
use std::sync::atomic::Ordering;

use serde_json::Value;

#[cfg(target_os = "macos")]
use super::cfg::RR3_TTS_BACKEND_MAC_SAY;

use super::*;

// ---------- 纯函数面 ----------

/// 应答体策略帽（本轮审计）：帽内原样返回，超帽 1 字节即拒——云端/中间人
/// 返回超大响应时不得把板子内存顶穿。
#[test]
fn response_body_cap_bounds_memory() {
    let ok = vec![7u8; 16];
    assert_eq!(
        rr3_read_capped_body("u", std::io::Cursor::new(ok.clone()), 16).unwrap(),
        ok
    );
    let e = rr3_read_capped_body("u", std::io::Cursor::new(vec![7u8; 17]), 16)
        .expect_err("超帽 1 字节必须拒");
    assert!(e.contains("应答体超界"), "{e}");
}

/// boundary 32 位小写 hex、两次调用不同
#[test]
fn boundary_hex_shape() {
    let a = rr3_boundary_hex();
    let b = rr3_boundary_hex();
    assert_eq!(a.len(), 32);
    assert!(a
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(a, b);
}

/// 云档解析四径：缺席 real、mock、bogus 红、空串红（Python 键在空串 not in 同红）
#[test]
fn cloud_mode_parse() {
    let _g = env_lock();
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
    assert_eq!(rr3_cloud_mode().unwrap(), Rr3CloudMode::Real);
    std::env::set_var(RR3_CLOUD_MODE_ENV, "mock");
    assert_eq!(rr3_cloud_mode().unwrap(), Rr3CloudMode::Mock);
    std::env::set_var(RR3_CLOUD_MODE_ENV, "bogus");
    let e = rr3_cloud_mode().unwrap_err();
    assert_eq!(e, "RR3_BRAIN_CLOUD 取值不认: bogus（允许 real/mock）");
    std::env::set_var(RR3_CLOUD_MODE_ENV, "");
    let e = rr3_cloud_mode().unwrap_err();
    assert!(e.contains("取值不认"), "空串同红: {e}");
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// secrets 现读：缺文件 None、缺键 None、有值 trim、空串 None
#[test]
fn secrets_get_reads_file_lazily() {
    let _g = env_lock();
    let dir = std::env::temp_dir().join(format!("rr3-brain-secrets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("secrets.json");
    std::fs::write(
        &path,
        r#"{"keys": {"DeepSeek官方": "  sk-test  ", "空": "  "}}"#,
    )
    .unwrap();
    std::env::set_var(RR3_SECRETS_FILE_ENV, &path);
    assert_eq!(rr3_secrets_get("DeepSeek官方").as_deref(), Some("sk-test"));
    assert_eq!(rr3_secrets_get("空"), None, "空白值折 None");
    assert_eq!(rr3_secrets_get("没有的"), None, "缺键 None");
    std::env::set_var(RR3_SECRETS_FILE_ENV, dir.join("不存在.json"));
    assert_eq!(rr3_secrets_get("任意"), None, "缺文件 None");
    std::env::remove_var(RR3_SECRETS_FILE_ENV);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 传输异常收敛（B-16②）：端口 1 无监听 → 连接拒绝折「云端不可达」（Python
/// OSError 族折 RuntimeError 同语义）
#[test]
fn http_post_folds_unreachable_endpoint() {
    let e = rr3_http_post(
        "http://127.0.0.1:1/v1/audio/transcriptions",
        "k",
        b"x",
        "text/plain",
        1.0,
    )
    .unwrap_err();
    assert!(e.contains("云端不可达"), "传输失败折固定文案: {e}");
    assert!(!e.contains("Bearer"), "文案不得带出密钥");
}

/// 连接复用（B19）：keep-alive 服务端连续两请求只 accept 一次——进程级共享
/// Agent 的池化生效（改造前 `ureq::post` 每次新建 Agent，必然两条连接）。
#[test]
fn http_post_reuses_pooled_connection() {
    let srv = FakeCloud::start_keep_alive();
    let url = format!("{}/v1/chat/completions", srv.base);
    for _ in 0..2 {
        let (status, _) = rr3_http_post(&url, "k", b"{}", "application/json", 5.0).unwrap();
        assert_eq!(status, 200);
    }
    assert_eq!(srv.requests.lock().unwrap().len(), 2, "两请求都应到达");
    assert_eq!(
        srv.conns.load(Ordering::Relaxed),
        1,
        "两请求应复用同一条 TCP 连接"
    );
}

/// 云端重试判据（2026-10-31 有界重试，ASR/LLM/TTS 同轨）：429/5xx/传输层可重试；
/// 4xx/坏应答/缺密钥不重试（重试只会重复同一结局，还把 401 拖成 2 倍时延）。
#[test]
fn cloud_error_retry_classification() {
    for (iface, e) in [
        ("识别接口", "识别接口 HTTP 429"),
        ("识别接口", "识别接口 HTTP 500"),
        ("识别接口", "识别接口 HTTP 502"),
        ("识别接口", "识别接口 HTTP 503"),
        (
            "识别接口",
            "云端不可达 http://x: timed out reading response",
        ),
        ("合成接口", "合成接口 HTTP 429"),
        ("合成接口", "合成接口 HTTP 500"),
        ("合成接口", "合成接口 HTTP 503"),
        (
            "合成接口",
            "云端不可达 http://x: timed out reading response",
        ),
    ] {
        assert!(rr3_cloud_retryable(iface, e), "应重试: {e}");
    }
    for (iface, e) in [
        ("识别接口", "识别接口 HTTP 400"),
        ("识别接口", "识别接口 HTTP 401"),
        ("识别接口", "识别接口 HTTP 403"),
        ("识别接口", "识别应答非 JSON: expected value at line 1"),
        ("识别接口", "识别应答缺 text 字段"),
        ("识别接口", "缺少环境变量 RR3_CLOUD_API_KEY，云端链路不可用"),
        // 接口前缀词错配不重试：识别文案不能拿合成前缀解析，反之亦然
        ("合成接口", "识别接口 HTTP 500"),
        ("识别接口", "合成接口 HTTP 500"),
        ("合成接口", "合成应答非 JSON: expected value at line 1"),
        ("合成接口", "缺少环境变量 RR3_CLOUD_API_KEY，云端链路不可用"),
    ] {
        assert!(!rr3_cloud_retryable(iface, e), "不应重试: {e}");
    }
}

// ---------- 客户端面（asr/llm/tts 三家经本地假端点，对位 test_cloud.py
// TestRealPathAgainstFakeEndpoint / TestMockPath / B-16② 收敛三组） ----------

/// real 档 ASR 真调假端点：Bearer 鉴权、multipart 请求体带 model/WAV 头/
/// filename="a.wav"（对位 test_asr_sends_multipart_with_model_and_wav）
#[test]
fn asr_real_sends_multipart_with_model_and_wav() {
    let _g = env_lock();
    let srv = FakeCloud::start();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
    std::env::set_var(RR3_API_KEY_ENV, "test-key");
    let client = Rr3AsrClient::new(&srv.base, "iic/SenseVoiceSmall", 5.0);
    let text = client
        .rr3_transcribe(&b"\x00\x01".repeat(16), 16000)
        .unwrap();
    assert_eq!(text, "你好");
    let (path, auth, body) = srv.last();
    assert_eq!(path, "/v1/audio/transcriptions");
    assert_eq!(auth, "Bearer test-key");
    assert!(body.windows(12).any(|w| w == b"name=\"model\""));
    assert!(body.windows(19).any(|w| w == b"iic/SenseVoiceSmall"));
    assert!(body.windows(4).any(|w| w == b"RIFF"), "WAV 头仓内自建");
    assert!(body.windows(16).any(|w| w == b"filename=\"a.wav\""));
    std::env::remove_var(RR3_API_KEY_ENV);
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// mock 档 ASR 不触网不读 key：固定文本；RR3_BRAIN_MOCK_ASR_TEXT 注入覆盖
/// （批F2 判向-转向闭环 e2e 的注入面）、空白回落固定文本
#[test]
fn asr_mock_fixed_text_and_env_injection_without_key() {
    let _g = env_lock();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "mock");
    std::env::remove_var(RR3_API_KEY_ENV);
    let client = Rr3AsrClient::new("http://127.0.0.1:1", "m", 1.0);
    assert_eq!(
        client.rr3_transcribe(b"\x00\x00", 16000).unwrap(),
        "你好，今天天气怎么样"
    );
    std::env::set_var(RR3_MOCK_TEXT_ENV, "向左");
    assert_eq!(client.rr3_transcribe(b"\x00\x00", 16000).unwrap(), "向左");
    std::env::set_var(RR3_MOCK_TEXT_ENV, "   ");
    assert_eq!(
        client.rr3_transcribe(b"\x00\x00", 16000).unwrap(),
        "你好，今天天气怎么样",
        "空白注入回落固定文本"
    );
    std::env::remove_var(RR3_MOCK_TEXT_ENV);
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// real 档 ASR 200 + 非 JSON 体：折「识别应答非 JSON」不逃逸（B-16②，
/// 对位 test_asr_non_json_body_is_runtime_error_not_escape）
#[test]
fn asr_non_json_body_folds_error() {
    let _g = env_lock();
    let srv = FakeCloud::start();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
    std::env::set_var(RR3_API_KEY_ENV, "test-key");
    *srv.bad_json_path.lock().unwrap() = Some("/v1/audio/transcriptions".into());
    let client = Rr3AsrClient::new(&srv.base, "m", 5.0);
    let e = client
        .rr3_transcribe(&b"\x00\x00".repeat(100), 16000)
        .unwrap_err();
    assert!(e.contains("非 JSON"), "{e}");
    assert_eq!(
        srv.requests.lock().unwrap().len(),
        1,
        "坏应答不重试（只打一次）"
    );
    std::env::remove_var(RR3_API_KEY_ENV);
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// ASR 有界重试（2026-10-31）：首请求 500 → 500ms 后重试一次 → 第二次 200
/// 成功；请求数恰 2（有界，不无限重试）。
#[test]
fn asr_retries_5xx_once_then_succeeds() {
    let _g = env_lock();
    let srv = FakeCloud::start();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
    std::env::set_var(RR3_API_KEY_ENV, "test-key");
    *srv.fail_once_path.lock().unwrap() = Some("/v1/audio/transcriptions".into());
    let client = Rr3AsrClient::new(&srv.base, "m", 5.0);
    assert_eq!(
        client
            .rr3_transcribe(&b"\x00\x00".repeat(100), 16000)
            .unwrap(),
        "你好"
    );
    assert_eq!(
        srv.requests.lock().unwrap().len(),
        2,
        "首调 + 1 次重试恰两次（有界）"
    );
    std::env::remove_var(RR3_API_KEY_ENV);
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// 5xx 持续失败：有界重试只多打一次（总 2 次），最终错误回带尝试数。
#[test]
fn asr_5xx_exhausts_bounded_attempts() {
    let _g = env_lock();
    let srv = FakeCloud::start();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
    std::env::set_var(RR3_API_KEY_ENV, "test-key");
    *srv.fail_path.lock().unwrap() = Some("/v1/audio/transcriptions".into());
    let client = Rr3AsrClient::new(&srv.base, "m", 5.0);
    let e = client
        .rr3_transcribe(&b"\x00\x00".repeat(100), 16000)
        .unwrap_err();
    assert!(e.contains("识别接口 HTTP 500"), "{e}");
    assert!(e.contains("ASR 共尝试 2 次"), "{e}");
    assert_eq!(
        srv.requests.lock().unwrap().len(),
        2,
        "有界：总尝试 2 次后不再打"
    );
    std::env::remove_var(RR3_API_KEY_ENV);
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// LLM 有界重试（B19，与 ASR/TTS 同轨）：首请求 500 → 重试一次 → 200；
/// 请求数恰 2，历史只入账一轮（重试不产生额外对话轮）。
#[test]
fn llm_retries_5xx_once_then_succeeds() {
    let _g = env_lock();
    let srv = FakeCloud::start();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
    std::env::set_var(RR3_API_KEY_ENV, "test-key");
    *srv.fail_once_path.lock().unwrap() = Some("/v1/chat/completions".into());
    let llm_root = format!("{}/v1", srv.base);
    let mut client = Rr3LlmClient::new(&llm_root, "m", 5.0, None, None);
    assert_eq!(client.rr3_complete("在吗").unwrap(), "好的");
    assert_eq!(
        srv.requests.lock().unwrap().len(),
        2,
        "首调 + 1 次重试（有界）"
    );
    assert_eq!(
        client.rr3_history().len(),
        3,
        "system + user + assistant 各一条（重试不额外入账）"
    );
    std::env::remove_var(RR3_API_KEY_ENV);
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// real 档 LLM：多轮裁窗（system 恒守首位 + 最近 4 轮）、max_tokens=150、
/// 失败轮回滚（user 一并出列——裁剪 -2＋回滚 -1 = before-2，Python 同账）
#[test]
fn llm_real_history_trim_and_failure_rollback() {
    let _g = env_lock();
    let srv = FakeCloud::start();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
    std::env::set_var(RR3_API_KEY_ENV, "test-key");
    // 模型行 base_url = 完整 API 根（假端点在主机根之上挂 /v1）
    let llm_root = format!("{}/v1", srv.base);
    let mut client = Rr3LlmClient::new(&llm_root, "deepseek-ai/DeepSeek-V3", 5.0, None, None);
    for i in 0..crate::llm::RR3_LLM_HISTORY_ROUNDS + 2 {
        assert_eq!(client.rr3_complete(&format!("问{i}")).unwrap(), "好的");
    }
    let h = client.rr3_history();
    assert_eq!(h.len(), 1 + crate::llm::RR3_LLM_HISTORY_ROUNDS * 2);
    assert_eq!(h[0].0, "system");
    assert_eq!(h[1].1, "问2", "最旧一轮已裁");
    let (path, _, body) = srv.last();
    assert_eq!(path, "/v1/chat/completions");
    let req: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(req["max_tokens"], 150);
    // 失败轮不入账：user 先入列、失败后回滚（对位 legacy：只有成功才追加 assistant）
    let before = client.rr3_history().len();
    *srv.fail_path.lock().unwrap() = Some("/v1/chat/completions".into());
    assert!(client.rr3_complete("会失败").is_err());
    *srv.fail_path.lock().unwrap() = None;
    assert_eq!(client.rr3_history().len(), before - 2, "裁剪 + 回滚共 -2");
    std::env::remove_var(RR3_API_KEY_ENV);
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// mock 档 LLM 回显且入账（history 尾条 = 回显应答；不触网不读 key）
#[test]
fn llm_mock_echo_and_history() {
    let _g = env_lock();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "mock");
    let mut client = Rr3LlmClient::new("http://127.0.0.1:1", "m", 1.0, None, None);
    assert_eq!(client.rr3_complete("在吗").unwrap(), "（mock 回复）在吗");
    assert_eq!(client.rr3_history().last().unwrap().1, "（mock 回复）在吗");
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// real 档 LLM 200 + 非 JSON 体：形状不符一档折 Err 且 user 回滚
/// （B-16②，Python 单 except 同文案档）
#[test]
fn llm_non_json_body_folds_shape_error() {
    let _g = env_lock();
    let srv = FakeCloud::start();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
    std::env::set_var(RR3_API_KEY_ENV, "test-key");
    let llm_root = format!("{}/v1", srv.base);
    let mut client = Rr3LlmClient::new(&llm_root, "m", 5.0, None, None);
    *srv.bad_json_path.lock().unwrap() = Some("/v1/chat/completions".into());
    let e = client.rr3_complete("在吗").unwrap_err();
    assert!(e.contains("形状不符"), "{e}");
    assert_eq!(client.rr3_history().len(), 1, "失败轮 user 已回滚");
    std::env::remove_var(RR3_API_KEY_ENV);
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// real 档 LLM 200 + 缺字段体：形状不符同档（choices[0].message.content
/// 取不出即红，Python KeyError 同 except 折同文案）
#[test]
fn llm_missing_field_folds_shape_error() {
    let _g = env_lock();
    let srv = FakeCloud::start();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
    std::env::set_var(RR3_API_KEY_ENV, "test-key");
    let llm_root = format!("{}/v1", srv.base);
    let mut client = Rr3LlmClient::new(&llm_root, "m", 5.0, None, None);
    *srv.missing_field_path.lock().unwrap() = Some("/v1/chat/completions".into());
    let e = client.rr3_complete("在吗").unwrap_err();
    assert!(e.contains("形状不符"), "{e}");
    assert_eq!(client.rr3_history().len(), 1, "失败轮 user 已回滚");
    std::env::remove_var(RR3_API_KEY_ENV);
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// real 档 TTS 真调：PCM 原样回流、请求体 voice=:bella/response_format/
/// sample_rate（对位 test_tts_returns_pcm_stream）
#[test]
fn tts_real_returns_pcm_stream() {
    let _g = env_lock();
    let srv = FakeCloud::start();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
    std::env::set_var(RR3_API_KEY_ENV, "test-key");
    let client = Rr3TtsClient::new(&srv.base, "FunAudioLLM/CosyVoice2-0.5B", 5.0);
    let pcm = client.rr3_synthesize("好的", 16000).unwrap();
    assert_eq!(pcm, [1u8, 0].repeat(1600));
    let (path, _, body) = srv.last();
    assert_eq!(path, "/v1/audio/speech");
    let req: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(req["voice"], "FunAudioLLM/CosyVoice2-0.5B:bella");
    assert_eq!(req["response_format"], "pcm");
    assert_eq!(req["sample_rate"], 16000);
    std::env::remove_var(RR3_API_KEY_ENV);
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// 宿主兜底档：真云挂死时 `RR3_TTS_BACKEND=mac_say` 走本机 say，交出非空
/// s16le PCM（仅 macOS；say 缺失/无 data 块即 Err，不静默降级）
#[test]
#[cfg(target_os = "macos")]
fn tts_mac_say_backend_returns_pcm_without_cloud() {
    let _g = env_lock();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "real");
    std::env::set_var(RR3_TTS_BACKEND_ENV, RR3_TTS_BACKEND_MAC_SAY);
    std::env::remove_var(RR3_API_KEY_ENV);
    let client = Rr3TtsClient::new("http://127.0.0.1:9", "unused", 1.0);
    let pcm = client.rr3_synthesize("你好", 16000).unwrap();
    assert!(!pcm.is_empty());
    assert_eq!(pcm.len() % 2, 0);
    std::env::remove_var(RR3_TTS_BACKEND_ENV);
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}

/// mock 档 TTS 不触网不读 key：正弦 PCM 非空（机制在 atoms/tts）
#[test]
fn tts_mock_sine_without_key() {
    let _g = env_lock();
    std::env::set_var(RR3_CLOUD_MODE_ENV, "mock");
    std::env::remove_var(RR3_API_KEY_ENV);
    let client = Rr3TtsClient::new("http://127.0.0.1:1", "m", 1.0);
    assert!(!client.rr3_synthesize("hi", 16000).unwrap().is_empty());
    std::env::remove_var(RR3_CLOUD_MODE_ENV);
}
