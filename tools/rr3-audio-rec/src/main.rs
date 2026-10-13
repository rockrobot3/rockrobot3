//! rr3-audio-rec —— 批D③ 音频上行收流端（宿主；0.0.5 增补轮五 全案 §4 批D 判据①）。
//!
//! 链路：板面 INMP441 双麦（mic.rs 分片组帧，`rr3-sense/audio/pcm` Event @
//! trace_id=0 / src=2 / dst=1）→ UART2 921600（CP2102 `/dev/cu.usbserial-0001`）→
//! 本件收流：`AUDIO ON` 行命令起流 → dst=1 门 → Event payload → topic 门 →
//! b64 解码 → 累积 PCM → `AUDIO OFF` → 按**实测采样率**写 wav（双通道 s16le）。
//!
//! 纪律：
//! - 帧语义/串口/b64 全部 rr-ipc 单点（`Rr3UartConn` + blanket impl
//!   `Rr3TransportConn` + `rr3_base64_decode`），零第二份帧层；
//! - 载荷形状事实源 modules/rr3-sense composite::`rr3_pcm_event_body`（seq/format/
//!   rate/channels/ms/pcm_b64），topic 键并入体（rr-bus `rr3_publish` 同形——板无
//!   rr-bus 客户端，按 `body["topic"]` 形状自行并入）；
//! - 开口 settle 纪律（批C 实锤）：CP2102 DTR/RTS 翻转会复位板子，open 后停 1.2s
//!   再 `rr3_flush_input` 净化复位 banner 残留，然后才发 `AUDIO ON`；
//! - seq 连续性记账（wrapping u32）：断档即板侧背压丢片（`INFO` 的 `drop` 互为
//!   对账），收流端只对账不补救（fail-honest：wav 是真实收到的流）。
//!
//! 用法：`rr3-audio-rec <device> [--out <path>] [--secs <n>] [--baud <n>]`
//! 缺省 out=`rr3-audio-<unix秒>.wav`、secs=30、baud=921600（板面批D② 落板档；
//! 波特率仍过 rr-ipc 档位表白名单，不预设动物园）。

use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rr_ipc::{
    rr3_base64_decode, rr3_body_split, rr3_uart_baud_speed, Rr3ByteStream, Rr3Frame,
    Rr3PayloadType, Rr3TransportConn, Rr3UartConn,
};
use serde_json::Value;

// ---------------------------------------------------------------------------
// 两端帧头约定（板不依赖 rr-core，板/收流端常量自证——board::RR3_*_TASK 同纪律；
// 与宿主 rr-hal::esp32_uart 的同值锚定测试同一防静默分叉口径）
// ---------------------------------------------------------------------------

/// 宿主 task 位形（帧头 dst 门：只收发给宿主的 Event）。
const RR3_HOST_TASK: u64 = 1;
/// 板面外设执行面 task 位形（帧头 src，诊断显示用）。
const RR3_PERIPH_TASK: u64 = 2;
/// 音频 PCM 上行主题（在册 stream L1：contracts/topics.toml）。
const RR3_AUDIO_TOPIC: &str = "rr3-sense/audio/pcm";
/// 开口 settle（CP2102 DTR/RTS 翻转复位板子的既定时序，批C 实锤）。
const SETTLE: Duration = Duration::from_millis(1200);

// ---------------------------------------------------------------------------

struct Args {
    device: String,
    out: PathBuf,
    secs: u64,
    baud: u32,
}

/// 参数解析（device 必给；波特率过 rr-ipc 档位表白名单——真值单点）。
fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut device: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut secs = 30u64;
    let mut baud = 921600u32;
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(
                    argv.get(i).ok_or("--out 缺路径参数")?.clone(),
                ));
            }
            "--secs" => {
                i += 1;
                secs = argv
                    .get(i)
                    .ok_or("--secs 缺秒数参数")?
                    .parse()
                    .map_err(|_| format!("--secs 非法: \"{}\"", argv[i]))?;
                if secs == 0 {
                    return Err("--secs 须 ≥1".into());
                }
            }
            "--baud" => {
                i += 1;
                baud = argv
                    .get(i)
                    .ok_or("--baud 缺波特率参数")?
                    .parse()
                    .map_err(|_| format!("--baud 非法: \"{}\"", argv[i]))?;
            }
            other if other.starts_with("--") => {
                return Err(format!("未知参数: {other}"));
            }
            other => {
                if device.is_some() {
                    return Err(format!("device 重复: {other}"));
                }
                device = Some(other.to_string());
            }
        }
        i += 1;
    }
    let device = device.ok_or("缺 <device> 参数（如 /dev/cu.usbserial-0001）")?;
    rr3_uart_baud_speed(baud).map_err(|e| format!("--baud {baud} 不在档位表: {e}"))?;
    Ok(Args {
        device,
        out: out.unwrap_or_else(default_out),
        secs,
        baud,
    })
}

/// 缺省输出名（unix 秒时戳防覆盖；chrono 不进依赖闭包）。
fn default_out() -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    PathBuf::from(format!("rr3-audio-{ts}.wav"))
}

/// wav 头（44B PCM RIFF；s16le 双通道，rate 由调用方按**实测率**给定）。
fn wav_header(data_len: u32, channels: u16, rate: u32, bits: u16) -> [u8; 44] {
    let byte_rate = rate as u32 * channels as u32 * bits as u32 / 8;
    let block_align = channels * bits / 8;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    h[32..34].copy_from_slice(&block_align.to_le_bytes());
    h[34..36].copy_from_slice(&bits.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_len.to_le_bytes());
    h
}

/// 一条音频事件（自 Board Event 帧解析出）。
struct AudioChunk {
    seq: u32,
    rate: u32,
    ms: u64,
    /// s16le 交错原始字节（解码后）
    samples: Vec<u8>,
}

/// Event 帧解析（四道门全过才 Ok，任一失败 Err 点名——与板侧 frame_recv 同款
/// fail-closed 口径）：dst=宿主门 → payload Event 类型门 → topic 门 → 字段/b64 门。
fn parse_audio_event(frame: &Rr3Frame) -> Result<AudioChunk, String> {
    if frame.header.dst_task != RR3_HOST_TASK {
        return Err(format!(
            "dst_task={} 非宿主位形 {RR3_HOST_TASK}",
            frame.header.dst_task
        ));
    }
    let (ty, body) =
        rr3_body_split(&frame.payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
    if ty != Rr3PayloadType::Event {
        return Err(format!("payload 类型非 Event: {ty:?}"));
    }
    let v: Value = serde_json::from_slice(&body).map_err(|e| format!("事件体非 JSON: {e}"))?;
    if v.get("topic").and_then(Value::as_str) != Some(RR3_AUDIO_TOPIC) {
        return Err(format!(
            "topic 非 {}: {:?}",
            RR3_AUDIO_TOPIC,
            v.get("topic")
        ));
    }
    if v.get("format").and_then(Value::as_str) != Some("s16le") {
        return Err("format 非 s16le".into());
    }
    let seq = v.get("seq").and_then(Value::as_u64).ok_or("缺 seq")? as u32;
    let rate = v.get("rate").and_then(Value::as_u64).ok_or("缺 rate")? as u32;
    let ms = v.get("ms").and_then(Value::as_u64).ok_or("缺 ms")?;
    let b64 = v
        .get("pcm_b64")
        .and_then(Value::as_str)
        .ok_or("缺 pcm_b64")?;
    let samples = rr3_base64_decode(b64).map_err(|e| format!("pcm_b64 解码失败: {e:?}"))?;
    Ok(AudioChunk {
        seq,
        rate,
        ms,
        samples,
    })
}

/// seq 连续性记账（wrapping u32）：断档数 = (seq - prev) - 1（wrapping 域），
/// 首片记账不断档。
struct SeqTracker {
    prev: Option<u32>,
    gaps: u64,
}

impl SeqTracker {
    fn feed(&mut self, seq: u32) -> u64 {
        if let Some(prev) = self.prev {
            let step = seq.wrapping_sub(prev);
            if step > 1 {
                self.gaps += (step - 1) as u64;
            }
        }
        self.prev = Some(seq);
        self.gaps
    }
}

/// 收流期的帧面字节流：行命令阶段的**尾部残留**（OK 行之后已到的帧前缀字节）
/// 经 pre 回放进帧层——批次边界不吞字节。实现 [`Rr3ByteStream`] 两 `usize` 方法，
/// 经 blanket impl 免费获得 rr3_recv_buf（板侧 UartIo 同款接缝）。
struct FrameStream<'a> {
    conn: &'a mut Rr3UartConn,
    pre: Vec<u8>,
    pos: usize,
}

impl Rr3ByteStream for FrameStream<'_> {
    fn rr3_read_into(&mut self, buf: &mut [u8]) -> usize {
        if self.pos < self.pre.len() {
            let n = (self.pre.len() - self.pos).min(buf.len());
            buf[..n].copy_from_slice(&self.pre[self.pos..self.pos + n]);
            self.pos += n;
            return n;
        }
        self.conn.read(buf).unwrap_or(0)
    }

    fn rr3_write_all_bytes(&mut self, buf: &[u8]) -> usize {
        match self.conn.write_all(buf) {
            Ok(()) => buf.len(),
            Err(_) => 0,
        }
    }
}

/// 收一行（阻塞逐字节；'\r' 剥离；换行即返回）。
fn read_reply_line(conn: &mut Rr3UartConn) -> Result<String, std::io::Error> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = conn.read(&mut byte)?;
        if n == 0 {
            continue;
        }
        match byte[0] {
            b'\n' => {
                while matches!(line.last(), Some(b'\r')) {
                    line.pop();
                }
                return Ok(String::from_utf8_lossy(&line).into_owned());
            }
            b => line.push(b),
        }
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("rr3-audio-rec: {e}\n\n用法: rr3-audio-rec <device> [--out <path>] [--secs <n>] [--baud <n>]\n缺省 out=rr3-audio-<unix秒>.wav secs=30 baud=921600（板面批D② 落板档）");
            std::process::exit(2);
        }
    };
    let mut conn = match Rr3UartConn::open_with_baud(std::path::Path::new(&args.device), args.baud)
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rr3-audio-rec: 开口失败 {}: {e}", args.device);
            std::process::exit(1);
        }
    };
    // settle 纪律：开口翻转复位板子 → 板 reboot banner 在开口 flush 之后才进队，
    // 停 1.2s 让复位走完再冲一次（批C 实锤），随后才发 AUDIO ON。
    std::thread::sleep(SETTLE);
    if let Err(e) = conn.rr3_flush_input() {
        eprintln!("rr3-audio-rec: flush 失败: {e}");
        std::process::exit(1);
    }
    if let Err(e) = conn.write_all(b"AUDIO ON\n") {
        eprintln!("rr3-audio-rec: 发 AUDIO ON 失败: {e}");
        std::process::exit(1);
    }
    let leftover = Vec::new();
    let reply = match read_reply_line(&mut conn) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("rr3-audio-rec: 读应答失败: {e}");
            std::process::exit(1);
        }
    };
    println!("[rec] board reply: {reply:?}");
    if !reply.starts_with("OK") {
        eprintln!("rr3-audio-rec: 板面拒收（{reply:?}），不收流");
        std::process::exit(1);
    }

    // 帧收流循环：dst/Event/topic/b64 四道门 → 累积 PCM + seq 对账
    let mut pcm: Vec<u8> = Vec::new();
    let mut tracker = SeqTracker {
        prev: None,
        gaps: 0,
    };
    let mut chunks = 0u64;
    let mut errors = 0u64;
    let mut first_frame: Option<Instant> = None;
    let mut last_frame: Option<Instant> = None;
    let deadline = Instant::now() + Duration::from_secs(args.secs);
    let mut fs = FrameStream {
        conn: &mut conn,
        pre: leftover,
        pos: 0,
    };
    println!(
        "[rec] streaming {}s @ {} ({})…",
        args.secs, args.baud, args.device
    );
    while Instant::now() < deadline {
        match fs.rr3_recv_buf() {
            Ok(buf) => match Rr3Frame::try_from(buf) {
                Ok(frame) => match parse_audio_event(&frame) {
                    Ok(chunk) => {
                        let now = Instant::now();
                        first_frame.get_or_insert(now);
                        last_frame = Some(now);
                        tracker.feed(chunk.seq);
                        chunks += 1;
                        pcm.extend_from_slice(&chunk.samples);
                        if chunks % 8 == 0 {
                            println!(
                                "[rec] chunk={chunks} seq={} ms={} bytes={} gaps={} err={errors} src_task={}",
                                chunk.seq,
                                chunk.ms,
                                pcm.len(),
                                tracker.gaps,
                                frame.header.src_task
                            );
                        }
                    }
                    Err(e) => {
                        errors += 1;
                        eprintln!("[rec] 事件门拒收: {e}");
                    }
                },
                Err(e) => {
                    errors += 1;
                    eprintln!("[rec] 帧还原失败: {e:?}");
                }
            },
            Err(e) => {
                errors += 1;
                eprintln!("[rec] 收帧失败: {e:?}");
            }
        }
    }
    // 收兵：先还 conn 可变借用（fs 持有 &mut conn），再发 OFF——板面队列余片
    // 照发，本端不再读，conn 关闭即弃。
    drop(fs);
    let _ = conn.write_all(b"AUDIO OFF\n");

    if pcm.is_empty() {
        eprintln!("[rec] 0 分片（查板面 AUDIO ON 应答/I2S 面波特率档）");
        std::process::exit(1);
    }
    // 实测采样率：首末分片墙钟 × 总帧数（板面 I2S 实际率 ~16.6kHz 与标称 16k 有
    // 差，wav 头按实测率写回放音高才对）。单分片（不足测）退标称 rate。
    let (frames_total, nominal_rate) = (pcm.len() / 4, 16000u32);
    let measured = match (first_frame, last_frame) {
        (Some(a), Some(b)) if b.duration_since(a).as_secs_f64() > 0.5 => {
            (frames_total as f64 / b.duration_since(a).as_secs_f64()).round() as u32
        }
        _ => nominal_rate,
    };
    let data_len = pcm.len() as u32;
    let out = args.out;
    match std::fs::File::create(&out).and_then(|mut f| {
        f.write_all(&wav_header(data_len, 2, measured, 16))?;
        f.write_all(&pcm)?;
        f.flush()
    }) {
        Ok(()) => println!(
            "[rec] ✓ {}：chunks={chunks} gaps={} err={errors} frames={frames_total} rate={}Hz（实测）dur≈{:.1}s size={data_len}B",
            out.display(),
            tracker.gaps,
            measured,
            frames_total as f64 / measured as f64,
        ),
        Err(e) => {
            eprintln!("[rec] wav 落盘失败 {}: {e}", out.display());
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rr_ipc::{opcode, rr3_base64_encode, rr3_body_encode, Rr3FrameBuf, Rr3Header};

    /// 构造一条与板面 mic.rs 同形的 Event 帧（seal/还原链路与收流端同源 rr-ipc）。
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

    #[test]
    fn seq_tracker_gap_accounting() {
        let mut t = SeqTracker {
            prev: None,
            gaps: 0,
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
        };
        assert_eq!(w.feed(1), 1);
    }

    #[test]
    fn parse_args_tier_table() {
        let ok = |argv: &[&str]| -> Result<Args, String> {
            parse_args(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        };
        let a = ok(&["/dev/cu.usbserial-0001"]).unwrap();
        assert_eq!(a.baud, 921600, "缺省落板档");
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
}
