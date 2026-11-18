//! 单元测试：wav 头字段、事件四道门、seq 记账、DoA 符号分离、参数档位门。

use rr_ipc::{
    opcode, rr3_base64_encode, rr3_body_encode, Rr3Frame, Rr3FrameBuf, Rr3Header, Rr3PayloadType,
};

use crate::args::{parse_args, Args};
use crate::event::parse_audio_event;
use crate::track::{DoaTracker, SeqTracker, DOA_WIN_FRAMES};
use crate::wav::wav_header;
use crate::{NOMINAL_RATE_FALLBACK_HZ, RR3_AUDIO_TOPIC, RR3_HOST_TASK, RR3_PERIPH_TASK};

/// 构造一条与板面 mic.rs 同形的 Event 帧（seal/还原链路与收流端同源 rr-ipc）。
fn board_like_frame(seq: u32, pcm: &[u8], dst: u64) -> Rr3Frame {
    let body = serde_json::json!({
        "topic": RR3_AUDIO_TOPIC,
        "seq": seq,
        "format": "s16le",
        "rate": NOMINAL_RATE_FALLBACK_HZ,
        "channels": 2u32,
        "ms": 128u64,
        "pcm_b64": rr3_base64_encode(pcm),
    });
    let payload = rr3_body_encode(Rr3PayloadType::Event, body.to_string().as_bytes());
    let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
    hdr.trace_id = 0;
    hdr.timestamp_ns = 100;
    hdr.src_task = RR3_PERIPH_TASK;
    hdr.dst_task = dst;
    Rr3Frame {
        header: hdr,
        payload,
    }
}

#[test]
fn wav_header_fields() {
    let h = wav_header(16, 2, 16000, 16);
    assert_eq!(&h[0..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(h[4..8].try_into().unwrap()), 52); // 36+16
    assert_eq!(&h[8..12], b"WAVE");
    assert_eq!(u32::from_le_bytes(h[16..20].try_into().unwrap()), 16);
    assert_eq!(u16::from_le_bytes(h[20..22].try_into().unwrap()), 1); // PCM
    assert_eq!(u16::from_le_bytes(h[22..24].try_into().unwrap()), 2);
    assert_eq!(u32::from_le_bytes(h[24..28].try_into().unwrap()), 16000);
    assert_eq!(u32::from_le_bytes(h[28..32].try_into().unwrap()), 64000); // byte_rate
    assert_eq!(u16::from_le_bytes(h[32..34].try_into().unwrap()), 4); // block_align
    assert_eq!(u16::from_le_bytes(h[34..36].try_into().unwrap()), 16);
    assert_eq!(&h[36..40], b"data");
    assert_eq!(u32::from_le_bytes(h[40..44].try_into().unwrap()), 16);
}

#[test]
fn parse_audio_event_roundtrip_and_gates() {
    let pcm: Vec<u8> = (0..16u8).collect();
    // 整链路：板形帧 → seal → 还原（rr-ipc 同源路径）→ 四道门全过
    let sealed = Rr3FrameBuf::from(&board_like_frame(7, &pcm, RR3_HOST_TASK));
    let frame = Rr3Frame::try_from(sealed).unwrap();
    let chunk = parse_audio_event(&frame).expect("板形帧必须全过");
    assert_eq!(chunk.seq, 7);
    assert_eq!(chunk.rate, 16000);
    assert_eq!(chunk.ms, 128);
    assert_eq!(chunk.samples, pcm);

    // dst 门：非宿主位形拒收
    let other = board_like_frame(8, &pcm, 9);
    assert!(parse_audio_event(&other).is_err(), "dst=9 必须被拒");
}

// 拆帧器用例已随机制上提 rr-ipc（`rr_ipc::framing` tests，四类：分块还原/
// 垃圾跳过/坏头弃 magic 续扫/plen 越栏），本件不再持第二份。

// ON 应答判据用例已随机制上提 rr-ipc（`rr_ipc::audio_session` tests：
// ok_reply_line_accepts_stream_prefixed_mixed_line），本件不再持第二份。

#[test]
fn seq_tracker_gap_accounting() {
    let mut t = SeqTracker {
        prev: None,
        gaps: 0,
        restarts: 0,
    };
    assert_eq!(t.feed(1), 0, "首片记账不断档");
    assert_eq!(t.feed(2), 0);
    assert_eq!(t.feed(3), 0);
    assert_eq!(t.feed(6), 2, "3→6 缺 4/5 两片");
    assert_eq!(t.gaps, 2);
    // wrapping 域断档（u32 回绕：FFFFFFFF→1 缺 0 一片）
    let mut w = SeqTracker {
        prev: Some(u32::MAX),
        gaps: 0,
        restarts: 0,
    };
    assert_eq!(w.feed(1), 1);
    // 板侧重启（重连开口翻转复位，seq 归零重走）：大步进记 restarts，
    // 不按 wrapping 补天文 gap
    let mut r = SeqTracker {
        prev: Some(9182),
        gaps: 0,
        restarts: 0,
    };
    assert_eq!(r.feed(0), 0, "重启不记 gap");
    assert_eq!(r.restarts, 1);
    r.feed(1);
    assert_eq!(r.gaps, 0, "重启后续流正常记账");
    assert_eq!(r.restarts, 1);
}

/// 1s 窗合成 PCM（全窗同幅值交错对）。
fn synth_stereo(l: i16, r: i16) -> Vec<u8> {
    let mut v = Vec::with_capacity(DOA_WIN_FRAMES * 4);
    for _ in 0..DOA_WIN_FRAMES {
        v.extend_from_slice(&l.to_le_bytes());
        v.extend_from_slice(&r.to_le_bytes());
    }
    v
}

#[test]
fn doa_ild_sign_separation_and_silence_gate() {
    let mut d = DoaTracker::new();
    // 左占优（20dB）：az 正满幅钳位
    let line = d.feed(&synth_stereo(8000, 800)).expect("窗满出行");
    assert!(line.contains("az=+90°"), "{line}");
    // 右占优：负向
    let line = d.feed(&synth_stereo(800, 8000)).expect("窗满出行");
    assert!(line.contains("az=-90°"), "{line}");
    // 均衡（peak 足）：az≈0
    let line = d.feed(&synth_stereo(2000, 2000)).expect("窗满出行");
    assert!(line.contains("az=+0°") || line.contains("az=-0°"), "{line}");
    // 静默窗（peak 不足）：不出角不进账
    let line = d.feed(&synth_stereo(100, 100)).expect("窗满出行");
    assert!(line.contains("静默"), "{line}");
    assert_eq!(d.azs.len(), 3, "静默窗不进 active 账");
    let s = d.summary();
    assert!(s.contains("左右符号分离 ✓"), "{s}");
    // 跨窗边界：半窗不回行（None），补满才出
    let mut d2 = DoaTracker::new();
    assert!(d2
        .feed(&synth_stereo(8000, 800)[..DOA_WIN_FRAMES * 2])
        .is_none());
    assert!(d2.feed(&synth_stereo(8000, 800)).is_some(), "补满即出");
}

#[test]
fn parse_args_tier_table() {
    let ok = |argv: &[&str]| -> Result<Args, String> {
        parse_args(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    };
    let a = ok(&["/dev/cu.usbserial-0001"]).unwrap();
    assert_eq!(a.baud, 1_500_000, "缺省与固件 board::UART2_BAUD_HZ 同档");
    assert_eq!(a.secs, 30);
    let a = ok(&["/dev/tty", "--baud", "115200", "--secs", "5"]).unwrap();
    assert_eq!(a.baud, 115200);
    assert_eq!(a.secs, 5);
    assert!(
        ok(&["/dev/tty", "--baud", "230400"]).is_err(),
        "档位动物园拒收"
    );
    assert!(ok(&["/dev/tty", "--baud", "abc"]).is_err());
    assert!(ok(&[]).is_err(), "device 必给");
    assert!(ok(&["/dev/tty", "--secs", "0"]).is_err());
    assert!(ok(&["/dev/tty", "--wat"]).is_err(), "未知参数拒收");
}
