//! esp32audio 的单测（自同名模块原样抽出，判据不变）。

use super::cmd::*;
use super::core::*;
use super::evt::*;
use super::ring::*;

use crate::esp32audio::*;
use crate::{Rr3AudioCfg, Rr3HalError};
use rr_ipc::{
    opcode, rr3_base64_encode, rr3_body_encode, rr3_body_split, Rr3Frame, Rr3Header, Rr3PayloadType,
};
use serde_json::Value;

/// 板面音频流事实：16k/双声道/s16le。
fn board_cfg() -> Rr3AudioCfg {
    Rr3AudioCfg {
        sample_rate_hz: RR3_ESP32_AUDIO_RATE,
        channels: RR3_ESP32_AUDIO_CHANNELS,
        bit_depth: 16,
    }
}

/// 重连失败打点节流：首败必打，其后每 100 次一条（板长期不在位不刷屏）。
#[test]
fn recon_log_is_rate_limited() {
    assert!(rr3_recon_log_now(1));
    assert!(!rr3_recon_log_now(2));
    assert!(!rr3_recon_log_now(99));
    assert!(rr3_recon_log_now(100));
    assert!(!rr3_recon_log_now(101));
    assert!(rr3_recon_log_now(1000));
}

/// 构造一条与板面 mic.rs 同形的 Event 帧（seal/还原链路与收流端同源 rr-ipc；
/// 与 tools/rr3-audio-rec 测试同款 helper——同值锚定的测试侧孪生）。
fn board_like_frame(seq: u32, pcm: &[u8], dst: u64) -> Rr3Frame {
    let body = serde_json::json!({
        "topic": RR3_AUDIO_TOPIC,
        "seq": seq,
        "format": "s16le",
        "rate": 16000u32,
        "channels": 2u32,
        "ms": 128u64,
        "pcm_b64": rr3_base64_encode(pcm),
    });
    let payload = rr3_body_encode(Rr3PayloadType::Event, body.to_string().as_bytes());
    let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
    hdr.trace_id = 0;
    hdr.timestamp_ns = 100;
    hdr.src_task = crate::esp32_uart::RR3_PERIPH_TASK;
    hdr.dst_task = dst;
    Rr3Frame {
        header: hdr,
        payload,
    }
}

#[test]
fn cfg_check_accepts_board_facts_and_rejects_drift() {
    assert!(rr3_esp32_cfg_check(board_cfg()).is_ok());
    // 单声道缺省配置（modules/rr3-sense/config.toml 现值）必须被拒——
    // 误吃 esp32 档在探活期点名，不静默出交织垃圾
    let mono = Rr3AudioCfg {
        sample_rate_hz: 16000,
        channels: 1,
        bit_depth: 16,
    };
    let err = rr3_esp32_cfg_check(mono).unwrap_err();
    assert!(matches!(err, Rr3HalError::InvalidParam(_)));
    assert!(
        err.to_string().contains("config-esp32"),
        "文案须指路 config-esp32.toml: {err}"
    );
    let fast = Rr3AudioCfg {
        sample_rate_hz: 48000,
        channels: 2,
        bit_depth: 16,
    };
    assert!(rr3_esp32_cfg_check(fast).is_err());
    let wide = Rr3AudioCfg {
        sample_rate_hz: 16000,
        channels: 2,
        bit_depth: 24,
    };
    assert!(rr3_esp32_cfg_check(wide).is_err());
}

#[test]
fn s16le_decode_pairs_and_drops_odd_byte() {
    assert_eq!(rr3_s16le_decode(&[0x00, 0x00]), vec![0i16]);
    assert_eq!(rr3_s16le_decode(&[0x00, 0x80]), vec![i16::MIN]); // LE 负数
    assert_eq!(rr3_s16le_decode(&[0x01, 0x80]), vec![-32767]);
    assert_eq!(rr3_s16le_decode(&[0xFF, 0x7F]), vec![i16::MAX]);
    assert_eq!(
        rr3_s16le_decode(&[0x34, 0x12, 0x78, 0x56]),
        vec![0x1234, 0x5678]
    );
    // 奇数尾字节弃
    assert_eq!(rr3_s16le_decode(&[0x34, 0x12, 0x99]), vec![0x1234]);
    assert!(rr3_s16le_decode(&[]).is_empty());
    assert!(rr3_s16le_decode(&[0x99]).is_empty());
}

#[test]
fn ring_push_take_whole_frame_semantics() {
    let mut ring = Rr3AudioRing::rr3_ring_new(16);
    // stereo（ch=2）：推 3 帧，整帧弹出
    ring.rr3_ring_push(&[1, 2, 3, 4, 5, 6], 2);
    let mut out = [0i16; 4]; // 请求 2 帧
    assert_eq!(ring.rr3_ring_take(&mut out, 2), 4);
    assert_eq!(out, [1, 2, 3, 4]);
    // 余 1 帧：请求 2 帧只给 1 帧（不拆帧不欠配）
    let mut out2 = [0i16; 4];
    assert_eq!(ring.rr3_ring_take(&mut out2, 2), 2);
    assert_eq!(out2[..2], [5, 6]);
    // 空环欠载回 0
    assert_eq!(ring.rr3_ring_take(&mut out2, 2), 0);
    // 推奇数采样（防御对齐）：奇尾巴不入环
    ring.rr3_ring_push(&[7, 8, 9], 2);
    let mut out3 = [0i16; 2];
    assert_eq!(ring.rr3_ring_take(&mut out3, 2), 2);
    assert_eq!(out3, [7, 8]);
    assert_eq!(ring.rr3_ring_dropped(), 0, "未溢出不计丢");
}

#[test]
fn ring_overflow_drops_oldest_frame_aligned_and_counts() {
    let mut ring = Rr3AudioRing::rr3_ring_new(8); // 4 帧 @ch=2
    ring.rr3_ring_push(&[1, 2, 3, 4, 5, 6, 7, 8], 2);
    // 溢出 2 采样（1 帧）：丢最旧一帧 [1,2]，环内 [3..8] + 新样 [9,10]
    ring.rr3_ring_push(&[9, 10], 2);
    assert_eq!(ring.rr3_ring_dropped(), 2, "溢丢按整帧记账");
    let mut out = [0i16; 10];
    let n = ring.rr3_ring_take(&mut out, 2);
    assert_eq!(n, 8, "环 4 帧（丢最旧 1 帧后）；out 只填实有采样");
    assert_eq!(&out[..8], &[3, 4, 5, 6, 7, 8, 9, 10]);
    // 奇数溢出量对齐：cap=8，环满 8 再推 3 采样（1 帧半）→ 对齐丢 2（1 帧）
    let mut ring2 = Rr3AudioRing::rr3_ring_new(8);
    ring2.rr3_ring_push(&[1, 2, 3, 4, 5, 6, 7, 8], 2);
    ring2.rr3_ring_push(&[9, 10, 11], 2); // 防御对齐后只推 [9,10]
    assert_eq!(ring2.rr3_ring_dropped(), 2, "奇数溢出对齐到整帧");
    let mut out2 = [0i16; 8];
    assert_eq!(ring2.rr3_ring_take(&mut out2, 2), 8);
    assert_eq!(&out2[..8], &[3, 4, 5, 6, 7, 8, 9, 10]);
}

#[test]
fn parse_audio_event_roundtrip_and_gates() {
    let pcm: Vec<u8> = (0..16u8).collect();
    // 整链路：板形帧 → 四道门全过
    let frame = board_like_frame(7, &pcm, RR3_HOST_TASK);
    let chunk = rr3_esp32_evt_parse(&frame.header, &frame.payload).expect("板形帧必须全过");
    assert_eq!(chunk.rate, 16000);
    assert_eq!(chunk.samples, pcm);

    // dst 门：非宿主位形拒收
    let other = board_like_frame(8, &pcm, 9);
    assert!(
        rr3_esp32_evt_parse(&other.header, &other.payload).is_err(),
        "dst=9 必须被拒"
    );
}

/// 板形应答帧（分类面测试孪生）：src=板 2 / dst=宿主 1，Response payload。
fn board_reply_frame(id: u64, error: Option<&str>) -> Rr3Frame {
    let result = match error {
        Some(e) => serde_json::json!({ "error": e }),
        None => serde_json::json!({ "angle_deg": 90 }),
    };
    let body = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result });
    let payload = rr3_body_encode(Rr3PayloadType::Response, body.to_string().as_bytes());
    let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
    hdr.trace_id = 0;
    hdr.timestamp_ns = 100;
    hdr.src_task = crate::esp32_uart::RR3_PERIPH_TASK;
    hdr.dst_task = RR3_HOST_TASK;
    Rr3Frame {
        header: hdr,
        payload,
    }
}

#[test]
fn cmd_queue_bounded_drops_oldest_and_counts() {
    let (producer, consumer) = rr3_audio_cmd_pair();
    assert_eq!(producer.rr3_cmd_dropped(), 0);
    producer.rr3_cmd_push(Rr3PeriphCmd::ServoPan(60));
    producer.rr3_cmd_push(Rr3PeriphCmd::ServoPan(90));
    assert_eq!(producer.rr3_cmd_dropped(), 0, "未满不计丢");
    // 队深 2：第三条挤掉最旧（60），最新意图（120）保住
    producer.rr3_cmd_push(Rr3PeriphCmd::ServoPan(120));
    assert_eq!(producer.rr3_cmd_dropped(), 1, "满丢最旧记账");
    assert_eq!(consumer.rr3_cmd_pop(), Some(Rr3PeriphCmd::ServoPan(90)));
    assert_eq!(consumer.rr3_cmd_pop(), Some(Rr3PeriphCmd::ServoPan(120)));
    assert_eq!(consumer.rr3_cmd_pop(), None, "排空回 None");
    // 回插队首：io 失败保命路径（重连后补写）
    consumer.rr3_cmd_push_front(Rr3PeriphCmd::ServoPan(120));
    assert_eq!(consumer.rr3_cmd_pop(), Some(Rr3PeriphCmd::ServoPan(120)));
}

#[test]
fn cmd_frame_matches_bridge_shape() {
    let frame = rr3_esp32_cmd_frame(7, &Rr3PeriphCmd::ServoPan(105));
    // 帧头：src=宿主 1 / dst=板 2 / direct trace（与舵机/显示桥同形）
    assert_eq!(frame.header.src_task, RR3_HOST_TASK);
    assert_eq!(frame.header.dst_task, crate::esp32_uart::RR3_PERIPH_TASK);
    assert_eq!(frame.header.trace_id, 0);
    // 信封：rr3.call → params.topic = 在册舵机主题，args 同舵机桥 payload
    let (ty, body) = rr3_body_split(&frame.payload).expect("payload 前缀合法");
    assert_eq!(ty, Rr3PayloadType::Request);
    let v: Value = serde_json::from_slice(&body).expect("体为 JSON");
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], 7);
    assert_eq!(v["method"], "rr3.call");
    assert_eq!(v["params"]["topic"], crate::esp32_uart::RR3_SERVO_TOPIC);
    assert_eq!(v["params"]["args"]["angle_deg"], 105);
    assert_eq!(v["params"]["args"]["axis"], "pan");
    // tilt 轴同形（点头示意批）：仅 axis 键换值，其余逐字同源
    let tilt_frame = rr3_esp32_cmd_frame(8, &Rr3PeriphCmd::ServoTilt(98));
    let (ty, body) = rr3_body_split(&tilt_frame.payload).expect("payload 前缀合法");
    assert_eq!(ty, Rr3PayloadType::Request);
    let tv: Value = serde_json::from_slice(&body).expect("体为 JSON");
    assert_eq!(tv["id"], 8);
    assert_eq!(tv["params"]["topic"], crate::esp32_uart::RR3_SERVO_TOPIC);
    assert_eq!(tv["params"]["args"]["angle_deg"], 98);
    assert_eq!(tv["params"]["args"]["axis"], "tilt");
    let tilt_bridge = crate::esp32_uart::rr3_bridge_call(
        8,
        crate::esp32_uart::RR3_SERVO_TOPIC,
        &serde_json::json!({ "angle_deg": 98, "axis": "tilt" }),
    );
    assert_eq!(
        tilt_frame.payload, tilt_bridge.payload,
        "tilt payload 字节级同形"
    );
    // 与桥侧 golden 同值锚定（esp32_uart 测试 rr3_bridge_request_body(7, ...) 同形）；
    // timestamp_ns 是构造时刻、两帧天然不同，不参与对拍
    let bridge = crate::esp32_uart::rr3_bridge_call(
        7,
        crate::esp32_uart::RR3_SERVO_TOPIC,
        &serde_json::json!({ "angle_deg": 105, "axis": "pan" }),
    );
    assert_eq!(frame.payload, bridge.payload, "payload 字节级同形");
    assert_eq!(frame.header.src_task, bridge.header.src_task);
    assert_eq!(frame.header.dst_task, bridge.header.dst_task);
    assert_eq!(frame.header.trace_id, bridge.header.trace_id);
    assert_eq!(frame.header.payload_len, bridge.header.payload_len);
}

#[test]
fn classify_routes_reply_and_audio() {
    // 音频 Event → Audio（原四道门语义不变）
    let pcm: Vec<u8> = (0..8u8).collect();
    let audio = board_like_frame(3, &pcm, RR3_HOST_TASK);
    match rr3_esp32_classify(&audio.header, &audio.payload) {
        Ok(Rr3BoardFrameIn::Audio(c)) => assert_eq!(c.samples, pcm),
        other => panic!("音频帧须落 Audio 臂: {other:?}"),
    }
    // 板侧应答 ok → Reply{id, err=None}
    let ok = board_reply_frame(7, None);
    match rr3_esp32_classify(&ok.header, &ok.payload) {
        Ok(Rr3BoardFrameIn::Reply { id, err }) => {
            assert_eq!(id, 7);
            assert_eq!(err, None);
        }
        other => panic!("应答帧须落 Reply 臂: {other:?}"),
    }
    // 板侧业务失败（越程拒收）→ Reply{err=Some}（fail-visible，不静默）
    let bad = board_reply_frame(8, Some("越程拒收"));
    match rr3_esp32_classify(&bad.header, &bad.payload) {
        Ok(Rr3BoardFrameIn::Reply { id, err }) => {
            assert_eq!(id, 8);
            assert_eq!(err.as_deref(), Some("越程拒收"));
        }
        other => panic!("失败应答须落 Reply 臂带 err: {other:?}"),
    }
    // dst 门：非宿主位形拒收（与音频门同口径）
    let mut other_dst = board_reply_frame(9, None);
    other_dst.header.dst_task = 9;
    assert!(rr3_esp32_classify(&other_dst.header, &other_dst.payload).is_err());
}

/// 批N12 件二（敞口 16 对表 tee 工具面）：tee 关（None）＝no-op 零副作用；
/// tee 开＝多笔写入**逐字节一致**且按序追加（tee 流 == 消费流判据的单测
/// 锚点）。关闭态"零落盘"由构造面保证（env 未设不 open，见
/// [`rr3_tee_open`](super::core::rr3_tee_open)），无路径可断言，不在此冒领。
#[test]
fn tee_disabled_noop_and_enabled_mirrors_bytes_in_order() {
    // 关闭态：喂两笔零副作用（无 panic 即 no-op；无 fd 无文件可证）
    let mut off: Option<std::fs::File> = None;
    rr3_tee_write(&mut off, b"abc");
    rr3_tee_write(&mut off, b"de");
    assert!(off.is_none());

    // 开启态：临时文件逐字节镜像 + 追加序
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "rr3-tee-test-{}-{}.bin",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("tee 测试临时文件打开失败");
    let mut on: Option<std::fs::File> = Some(f);
    rr3_tee_write(&mut on, b"abc");
    rr3_tee_write(&mut on, b"de");
    assert!(on.is_some(), "正常写不应停 tee");
    let got = std::fs::read(&path).expect("tee 测试临时文件回读失败");
    assert_eq!(got, b"abcde", "tee 流须与消费流逐字节一致且按序追加");
    let _ = std::fs::remove_file(&path);
}

/// 坏帧率告警：严格 >0.1%（千分之一）才置位；空窗不告警。
#[test]
fn bad_rate_alert_threshold() {
    assert!(!rr3_bad_alert(0, 0));
    assert!(!rr3_bad_alert(1000, 1), "恰 0.1% 不告警");
    assert!(rr3_bad_alert(1000, 2));
    assert!(rr3_bad_alert(4687, 5));
    assert!(!rr3_bad_alert(4687, 4), "4/4687 < 0.1%");
}

/// 坏帧打点：首条必打，其后按累计周期抽样（持续误码不刷屏）。
#[test]
fn err_log_first_then_sampled() {
    assert!(rr3_err_log_now(1));
    assert!(!rr3_err_log_now(2));
    assert!(!rr3_err_log_now(RR3_ERR_LOG_EVERY - 1));
    assert!(rr3_err_log_now(RR3_ERR_LOG_EVERY));
    assert!(!rr3_err_log_now(RR3_ERR_LOG_EVERY + 1));
    assert!(rr3_err_log_now(RR3_ERR_LOG_EVERY * 2));
}

/// 上限闸：文件到 64MiB 后下一笔停 tee（观测面降级），不 panic 不反噬链路。
#[test]
fn tee_stops_at_size_cap() {
    let path = std::env::temp_dir().join(format!(
        "rr3-tee-cap-test-{}-{}.bin",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)
        .expect("tee 上限测试文件打开失败");
    // 稀疏文件预置到上限（不实际占盘），下一笔即应停
    f.set_len(RR3_TEE_MAX_BYTES).expect("预置长度失败");
    let mut sink: Option<std::fs::File> = Some(f);
    rr3_tee_write(&mut sink, b"x");
    assert!(sink.is_none(), "达上限后 tee 必须停用");
    let _ = std::fs::remove_file(&path);
}
