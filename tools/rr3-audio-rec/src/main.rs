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
//! - 开口 settle 纪律（批C 实锤）：open 后停 1.2s 让开口翻转的板复位走完，再
//!   `rr3_flush_input` 净化残留；握手先发**防御 AUDIO OFF**（上一会话看门狗/
//!   异常退出的板面残流会抢在 OK 应答前进线，d4 重跑实锤），停流冲净才 AUDIO ON。
//! - seq 连续性记账（wrapping u32）：断档即板侧背压丢片（`INFO` 的 `drop` 互为
//!   对账）；大步进（≥ 0x8000_0000）= 板侧重启记 `restarts`，不按 wrapping 补
//!   天文 gap。收流端只对账不补救（fail-honest：wav 是真实收到的流）。
//! - 三级防护（0.0.5 批D④ d4 600s 实锤；机制与策略分离）：
//!   ① 读超时——机制面 rr-ipc `rr3_read_timeout`（poll 等就绪，`Ok(0)`=此刻无
//!   数据非 EOF），本件帧读/应答读全走超时档；阻塞读缺省语义保持为 rr-ipc
//!   其余用户（桥/哨兵）不动。
//!   ② 失步重同步——帧头 magic 线上字节 = "1ERA"（全 b64 字母表字符），失步后
//!   在载荷里撞上即"合法幻影帧头"+ 天文 payload_len。连续 [`ERR_STREAK_RESYNC`]
//!   次收帧失败即 magic 扫描对齐续收；连续 [`RESYNC_ZERO_BAIL`] 次零读（1.6s
//!   静默）= 线上断流，扫描救不了，转 ③。
//!   ③ 断流重连——重开口 settle + AUDIO ON 续收（开口翻转复位板子 → seq 重走
//!   → `restarts` 记账）。
//!   看门狗（逐片增量落盘 + 武装后 [`STALL`] 无新分片字节 → 补 wav 恢复头
//!   exit(3)）是最外层兜底；重连周期盖得住 [`STALL`] 不误触发。
//!
//! 用法：`rr3-audio-rec <device> [--out <path>] [--secs <n>] [--baud <n>] [--doa]`
//! 缺省 out=`rr3-audio-<unix秒>.wav`、secs=30、baud=921600（板面批D② 落板档；
//! 波特率仍过 rr-ipc 档位表白名单，不预设动物园）。
//! 退出码：0=收兵；1=开口/握手/0 分片；2=参数错；3=看门狗停滞恢复；4=重连失败
//! （3/4 都先补恢复头保住已落盘数据再退）。

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Seek, SeekFrom, Write as _};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
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
/// 停滞死线（看门狗）：武装后无新分片字节即判停滞。分片节拍 128ms/片，
/// 10s ≈ 78 片空窗——正常流不可达；重连周期（settle 1.2s + 握手 ≤4.2s）与
/// 重同步窗口（1.6s）都盖得住不误触发，触发即实锤重连后仍无流。
const STALL: Duration = Duration::from_secs(10);
/// 帧头 magic 小端线上字节（contracts/README.md 歧义裁定：字段值 0x41524531
/// 小端存储 = 31 45 52 41 = "1ERA"——与 rr-ipc protocol::header 同值锚定，
/// 板不依赖 rr-core、收流端常量自证同纪律）。b64 载荷里也会出现该子串，
/// 失步后撞上即"合法幻影帧头"，重同步扫描以它为锚。
const RR3_MAGIC_LE: [u8; 4] = 0x41524531u32.to_le_bytes();
/// 连续收帧失败达此数即判失步，触发 magic 扫描重同步（1-2 次可能是单帧毛刺，
/// 3 次 = 帧界已丢，盲目重试只会撞幻影帧头）。
const ERR_STREAK_RESYNC: u64 = 3;
/// 帧读超时（毫秒，机制面 `rr3_read_timeout`）：单次等就绪时长。正常流分片
/// 背靠背（921600 线速 ~89ms/片），200ms 静默 = 此读无数据（EOF 形状上交帧层
/// → `Closed`/`Torn*` 报错计 err_streak）；连续 3 次 → 重同步。缺省阻塞读
/// 语义留给其余用户，本件全部走超时档。
const READ_TIMEOUT_MS: i32 = 200;
/// 应答行逐字节超时（毫秒）：板面应答整行一次写出（write_all + wait_tx_done），
/// 3s 无字节 = 板面哑了（握手路径点名失败，不再无限等）。
const REPLY_TIMEOUT_MS: i32 = 3000;
/// 重同步零读宽限：连续 8 次 200ms 零读 = 1.6s 连续静默 → 扫 magic 也救不了
/// （线上断流），resync 返回 None 交主循环重连。数据在流时 256B 块 ~10ms 即满，
/// 零读必然是真静默（连续计数，数据一恢复即清零续扫）。
const RESYNC_ZERO_BAIL: u32 = 8;

// ---------------------------------------------------------------------------

struct Args {
    device: String,
    out: PathBuf,
    secs: u64,
    baud: u32,
    /// 批D⑤ DoA 冒烟面：逐 1s 窗 ILD 出角打印（不影响 wav 落盘）。
    doa: bool,
}

/// 参数解析（device 必给；波特率过 rr-ipc 档位表白名单——真值单点）。
fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut device: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut secs = 30u64;
    let mut baud = 921600u32;
    let mut doa = false;
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
            "--doa" => {
                doa = true;
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
        doa,
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
/// 首片记账不断档；大步进（≥ 0x8000_0000）= 板侧重启（重连开口翻转复位，seq
/// 归零重走）记 `restarts`——合法事件，不按 wrapping 补天文 gap。
struct SeqTracker {
    prev: Option<u32>,
    gaps: u64,
    restarts: u64,
}

/// DoA 冒烟面（`--doa`，批D⑤ 判据③）：逐 1s 窗双通道 ILD 出角。
/// **未标定冒烟口径**：az = 20·log10(rms_l/rms_r) × [`AZ_PER_DB`]（±90° ↔ ±12dB），
/// 判据只取"左/右发声方位角**符号分离**"，不冒领精度（标定留待真机装距实测）。
/// 正 az = 左通道占优。静默窗（peak ≤ [`DOA_ACTIVE_PEAK`]）只记账不出角。
struct DoaTracker {
    l_sq: f64,
    r_sq: f64,
    n: usize,
    peak: i32,
    win: usize,
    t0: Instant,
    /// active 窗出角账：(窗号, az°)——收兵汇总用。
    azs: Vec<(usize, f64)>,
}

/// ILD→方位角换算系数（°/dB，未标定冒烟值：±12dB 铺满 ±90°）。
const AZ_PER_DB: f64 = 7.5;
/// 1s 窗帧数（标称 16kHz；实测率偏差只挪窗边界，不影响 ILD 口径）。
const DOA_WIN_FRAMES: usize = 16000;
/// active 判据：窗内任意样本 |幅| 超此值才算"有事件"，静默窗不出角。
const DOA_ACTIVE_PEAK: i32 = 1000;

impl DoaTracker {
    fn new() -> Self {
        Self {
            l_sq: 0.0,
            r_sq: 0.0,
            n: 0,
            peak: 0,
            win: 0,
            t0: Instant::now(),
            azs: Vec::new(),
        }
    }

    /// 喂一片 s16le 交错 PCM；攒满 1s 窗回一行打印（None = 窗未满）。
    fn feed(&mut self, samples: &[u8]) -> Option<String> {
        let mut i = 0;
        while i + 3 < samples.len() {
            let l = i16::from_le_bytes([samples[i], samples[i + 1]]) as i32;
            let r = i16::from_le_bytes([samples[i + 2], samples[i + 3]]) as i32;
            self.l_sq += (l as f64) * (l as f64);
            self.r_sq += (r as f64) * (r as f64);
            self.peak = self.peak.max(l.abs()).max(r.abs());
            self.n += 1;
            i += 4;
        }
        if self.n < DOA_WIN_FRAMES {
            return None;
        }
        self.win += 1;
        let t = self.t0.elapsed().as_secs_f64();
        let l_rms = (self.l_sq / self.n as f64).sqrt();
        let r_rms = (self.r_sq / self.n as f64).sqrt();
        let out = if self.peak > DOA_ACTIVE_PEAK && l_rms > 0.0 && r_rms > 0.0 {
            let ild = 20.0 * (l_rms / r_rms).log10();
            let az = (ild * AZ_PER_DB).clamp(-90.0, 90.0);
            self.azs.push((self.win, az));
            format!(
                "[doa] t={t:5.1}s win={} peak={} l_rms={l_rms:.0} r_rms={r_rms:.0} ild={ild:+.1}dB az={az:+.0}° *",
                self.win, self.peak
            )
        } else {
            format!(
                "[doa] t={t:5.1}s win={} peak={} l_rms={l_rms:.0} r_rms={r_rms:.0} （静默）",
                self.win, self.peak
            )
        };
        self.l_sq = 0.0;
        self.r_sq = 0.0;
        self.n = 0;
        self.peak = 0;
        Some(out)
    }

    /// 收兵汇总：active 窗方位角列表 + 符号分离判定（正负都有 = 左右发声方向可分）。
    fn summary(&self) -> String {
        if self.azs.is_empty() {
            return "[doa] 无 active 窗（无出角）——事件音量或位置不足".into();
        }
        let list = self
            .azs
            .iter()
            .map(|(w, az)| format!("win{w}={az:+.0}°"))
            .collect::<Vec<_>>()
            .join(" ");
        let has_pos = self.azs.iter().any(|(_, az)| *az > 5.0);
        let has_neg = self.azs.iter().any(|(_, az)| *az < -5.0);
        let verdict = match (has_pos, has_neg) {
            (true, true) => "左右符号分离 ✓（冒烟过）",
            (true, false) => "仅正向角（对照位缺或方向映射相反）",
            (false, true) => "仅负向角（对照位缺或方向映射相反）",
            (false, false) => "全部近 0°（ILD 不显著，查音量/距离）",
        };
        format!("[doa] 汇总 active={}：{list} —— {verdict}", self.azs.len())
    }
}

impl SeqTracker {
    fn feed(&mut self, seq: u32) -> u64 {
        if let Some(prev) = self.prev {
            let step = seq.wrapping_sub(prev);
            if step >= 0x8000_0000 {
                // 板侧重启：seq 归零重走是合法事件，不是天文丢片
                self.restarts += 1;
            } else if step > 1 {
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
    /// 帧读超时（毫秒，机制面 `rr3_read_timeout`）：`Ok(0)` = 此刻无数据（非
    /// EOF），帧层以 EOF 形状（`Closed`/`Torn*`）上交——主循环按 err_streak
    /// 走重同步/重连，楔死不再可能。
    timeout: i32,
}

impl<'a> FrameStream<'a> {
    fn new(conn: &'a mut Rr3UartConn) -> Self {
        Self {
            conn,
            pre: Vec::new(),
            pos: 0,
            timeout: READ_TIMEOUT_MS,
        }
    }
}

impl Rr3ByteStream for FrameStream<'_> {
    fn rr3_read_into(&mut self, buf: &mut [u8]) -> usize {
        if self.pos < self.pre.len() {
            let n = (self.pre.len() - self.pos).min(buf.len());
            buf[..n].copy_from_slice(&self.pre[self.pos..self.pos + n]);
            self.pos += n;
            return n;
        }
        self.conn.rr3_read_timeout(buf, self.timeout).unwrap_or(0)
    }

    fn rr3_write_all_bytes(&mut self, buf: &[u8]) -> usize {
        match self.conn.write_all(buf) {
            Ok(()) => buf.len(),
            Err(_) => 0,
        }
    }
}

impl FrameStream<'_> {
    /// 失步重同步（收流端策略面，三级防护第 ② 级）：逐字节滑动窗口扫
    /// [`RR3_MAGIC_LE`]，对齐即把 4B magic 塞进 `pre` 回放进帧层（既有接缝，零
    /// 第二份帧语义）——后续 124B 头照常读。返回弃扫的字节数（诊断显示）。扫中
    /// b64 里的幻影 "1ERA" 也会返回，但那读出来的头必 BadMagic → 连续失败再次
    /// 触发重同步，收敛到真帧头。连续 [`RESYNC_ZERO_BAIL`] 次零读（1.6s 静默）
    /// = 线上断流，扫描救不了，返回 None 交主循环重连（第 ③ 级）。
    fn resync(&mut self) -> Option<u64> {
        let mut win = [0u8; 3]; // magic 前 3 字节滑动历史（第 4 字节逐个比）
        let mut n = 0u64;
        let mut zeros = 0u32;
        let mut buf = [0u8; 256];
        loop {
            let got = self.rr3_read_into(&mut buf);
            if got == 0 {
                zeros += 1;
                if zeros >= RESYNC_ZERO_BAIL {
                    return None; // 1.6s 连续静默 = 断流
                }
                continue;
            }
            zeros = 0; // 数据一恢复即清零续扫（连续静默才判死）
            for &b in &buf[..got] {
                if win == RR3_MAGIC_LE[..3] && b == RR3_MAGIC_LE[3] {
                    self.pre = RR3_MAGIC_LE.to_vec();
                    self.pos = 0;
                    return Some(n + 1);
                }
                win[0] = win[1];
                win[1] = win[2];
                win[2] = b;
                n += 1;
            }
        }
    }
}

/// 收一行（逐字节超时读 [`REPLY_TIMEOUT_MS`]；'\r' 剥离；换行即返回）。
/// 超时无字节即 Err（板面哑了——握手路径据此点名，不再无限等）。
fn read_reply_line(conn: &mut Rr3UartConn) -> Result<String, std::io::Error> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = conn.rr3_read_timeout(&mut byte, REPLY_TIMEOUT_MS)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("应答 {REPLY_TIMEOUT_MS}ms 无字节"),
            ));
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

/// 开口握手（首连/重连同款，批C settle 纪律）：停 1.2s 让开口翻转的板复位走完
/// → 冲残留 → **防御停流** → 再冲一次 → `AUDIO ON` → 核验应答。
///
/// 防御停流实锤（d4 重跑）：上一会话看门狗 exit(3) 不发 AUDIO OFF（conn 被帧层
/// 借用），板面 audio_on 残留开态续流——残流字节抢在 OK 应答之前进线，读到的
/// "应答行"是 b64 音频载荷，握手必挂。先发 OFF（幂等）并吞掉应答行（残流混行
/// 不解析），流停后冲净，AUDIO ON 的应答才是干净行首。板本来就没流时 OFF 照
/// 常应答，路径不变。
fn handshake(conn: &mut Rr3UartConn) -> Result<String, String> {
    std::thread::sleep(SETTLE);
    conn.rr3_flush_input()
        .map_err(|e| format!("flush 失败: {e}"))?;
    // 防御停流：残流开着也停、没流也幂等；应答行（可能混残流）读了即弃
    if let Err(e) = conn.write_all(b"AUDIO OFF\n") {
        return Err(format!("发防御 AUDIO OFF 失败: {e}"));
    }
    let _ = read_reply_line(conn);
    conn.rr3_flush_input()
        .map_err(|e| format!("二次 flush 失败: {e}"))?;
    conn.write_all(b"AUDIO ON\n")
        .map_err(|e| format!("发 AUDIO ON 失败: {e}"))?;
    let reply = read_reply_line(conn).map_err(|e| format!("读应答失败: {e}"))?;
    println!("[rec] board reply: {reply:?}");
    if !reply.starts_with("OK") {
        return Err(format!("板面拒收（{reply:?}）"));
    }
    Ok(reply)
}

/// fail-honest 退出（重连失败路径）：已落盘数据补恢复头保住，exit(4) 点名。
/// （看门狗 exit(3) 同款恢复头逻辑——两路独立小段，不抽象共享。）
fn bail_with_recovery(out_path: &std::path::Path, total_bytes: u64, watch: &WatchState) -> ! {
    let data_len = total_bytes as u32;
    let rate = watch.rate.load(Ordering::Acquire);
    if let Err(e) = OpenOptions::new().write(true).open(out_path).and_then(|mut f| {
        f.seek(SeekFrom::Start(0))?;
        f.write_all(&wav_header(data_len, 2, rate, 16))
    }) {
        eprintln!("[rec] 恢复头补写失败 {}: {e}", out_path.display());
    }
    std::process::exit(4);
}

/// 看门狗共享账（主线程逐片推进，看门狗线程只读）：停滞判据 = 已落盘字节不再
/// 增长（原始字节到达≠有分片——失步垃圾流也算死流，只认可落盘的分片）。
struct WatchState {
    /// 已落盘 PCM 字节（逐片 flush 后如实推进）。
    bytes: AtomicU64,
    /// 实测采样率（逐片刷新：frames/elapsed；不足 0.5s 测时标称）。
    rate: AtomicU32,
    /// AUDIO ON 应答后武装（武装前不计时——开口 settle/复位 banner 阶段静默合法）。
    armed: AtomicBool,
}

/// 长流楔死看门狗（收流端策略面，d4 600s 实锤的修复）。
///
/// 机理：rr-ipc uart 读 VMIN=1/VTIME=0 纯阻塞，宿主/CP2102 驱动层丢字节后
/// 帧层等幻影帧尾永不返回（第一轮 600s 稳态跑：板面 seq=9182 sent=9182
/// drop=0 全程健康，宿主 `rr3_recv_buf` 楔死 8 分钟无退出）。本线程每 500ms
/// 查推进：武装后 [`STALL`] 无新分片字节 → 补 wav 恢复头（data_len=已落盘
/// 字节）→ exit(3)。fail-honest：已收数据保得住，停滞事实打得响。
fn watchdog_run(w: Arc<WatchState>, out: String) {
    let mut last_seen = 0u64;
    let mut last_move = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let nb = w.bytes.load(Ordering::Acquire);
        if nb != last_seen {
            last_seen = nb;
            last_move = Instant::now();
            continue;
        }
        if !w.armed.load(Ordering::Acquire) {
            last_move = Instant::now(); // 未武装期不计时
            continue;
        }
        if last_move.elapsed() < STALL {
            continue;
        }
        eprintln!(
            "[rec] ✗ 收流停滞 ≥{STALL:?} 无新分片字节（楔死防护触发）：已落盘 {last_seen}B，补恢复头后退出"
        );
        let data_len = last_seen as u32;
        let rate = w.rate.load(Ordering::Acquire);
        if let Err(e) = OpenOptions::new().write(true).open(&out).and_then(|mut f| {
            f.seek(SeekFrom::Start(0))?;
            f.write_all(&wav_header(data_len, 2, rate, 16))
        }) {
            eprintln!("[rec] 恢复头补写失败 {out}: {e}");
        }
        std::process::exit(3);
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("rr3-audio-rec: {e}\n\n用法: rr3-audio-rec <device> [--out <path>] [--secs <n>] [--baud <n>] [--doa]\n缺省 out=rr3-audio-<unix秒>.wav secs=30 baud=921600（板面批D② 落板档）；--doa 逐 1s 窗 ILD 出角（批D⑤ 冒烟）");
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
    // 首连握手（重连同款路径，见 handshake；开口翻转复位板子 → settle 后冲残留）
    if let Err(e) = handshake(&mut conn) {
        eprintln!("rr3-audio-rec: 握手失败: {e}");
        std::process::exit(1);
    }

    // 帧收流循环：dst/Event/topic/b64 四道门 → 逐片增量落盘 + seq 对账。
    // 增量落盘是看门狗保数据的前提：停滞即整进程退，内存里攒的数据带不走。
    let out_path = args.out.clone();
    let mut wav = match File::create(&out_path).map(BufWriter::new) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("rr3-audio-rec: wav 建档失败 {}: {e}", out_path.display());
            std::process::exit(1);
        }
    };
    // 占位头（data_len=0）：正常收兵与看门狗停滞两路都回头 seek(0) 补真头。
    if let Err(e) = wav.write_all(&wav_header(0, 2, 16000, 16)) {
        eprintln!("rr3-audio-rec: wav 占位头写入失败: {e}");
        std::process::exit(1);
    }
    let watch = Arc::new(WatchState {
        bytes: AtomicU64::new(0),
        rate: AtomicU32::new(16000),
        armed: AtomicBool::new(false),
    });
    {
        let w = Arc::clone(&watch);
        let path = out_path.clone();
        std::thread::spawn(move || watchdog_run(w, path.to_string_lossy().into_owned()));
    }
    watch.armed.store(true, Ordering::Release);

    let mut tracker = SeqTracker {
        prev: None,
        gaps: 0,
        restarts: 0,
    };
    let mut doa = DoaTracker::new();
    let mut chunks = 0u64;
    let mut errors = 0u64;
    let mut err_streak = 0u64; // 连续收帧失败（好片即清零；达阈值触发重同步）
    let mut resyncs = 0u64;
    let mut reconnects = 0u64;
    let mut total_bytes = 0u64;
    let mut last_frame: Option<Instant> = None;
    // 连续段实测率锚 (锚时刻, 锚时帧数)：断档/重启/重同步/重连/首片即重新锚定
    // ——断流空洞不摊进实测率分母（旧版首末墙钟摊全程，断流后实测率被稀释）。
    let mut rate_anchor: Option<(Instant, u64)> = None;
    let mut need_reanchor = false; // resync/reconnect 后下一好片重新锚定
    let deadline = Instant::now() + Duration::from_secs(args.secs);
    let mut fs = FrameStream::new(&mut conn);
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
                        last_frame = Some(now);
                        let account_before = (tracker.gaps, tracker.restarts);
                        tracker.feed(chunk.seq);
                        let broke = (tracker.gaps, tracker.restarts) != account_before;
                        chunks += 1;
                        err_streak = 0;
                        // 逐片增量落盘（看门狗保数据的前提）+ 看门狗账推进
                        if let Err(e) = wav.write_all(&chunk.samples).and_then(|()| wav.flush()) {
                            eprintln!("rr3-audio-rec: wav 增量写失败: {e}");
                            std::process::exit(1);
                        }
                        total_bytes += chunk.samples.len() as u64;
                        watch.bytes.store(total_bytes, Ordering::Release);
                        let frames = total_bytes / 4;
                        // 连续段实测率：断档/重启即重新锚定（空洞不摊进分母）
                        if broke || need_reanchor || rate_anchor.is_none() {
                            rate_anchor = Some((now, frames));
                            need_reanchor = false;
                        }
                        if let Some((t0, f0)) = rate_anchor {
                            let d = now.duration_since(t0);
                            if d.as_secs_f64() > 0.5 {
                                let r =
                                    (((frames - f0) as f64) / d.as_secs_f64()).round() as u32;
                                if r > 0 {
                                    watch.rate.store(r, Ordering::Release);
                                }
                            }
                        }
                        if chunks % 8 == 0 {
                            println!(
                                "[rec] chunk={chunks} seq={} ms={} bytes={} gaps={} restarts={} err={errors} src_task={}",
                                chunk.seq,
                                chunk.ms,
                                total_bytes,
                                tracker.gaps,
                                tracker.restarts,
                                frame.header.src_task
                            );
                        }
                        if args.doa {
                            if let Some(line) = doa.feed(&chunk.samples) {
                                println!("{line}");
                            }
                        }
                    }
                    Err(e) => {
                        errors += 1;
                        err_streak += 1;
                        eprintln!("[rec] 事件门拒收: {e}");
                    }
                },
                Err(e) => {
                    errors += 1;
                    err_streak += 1;
                    eprintln!("[rec] 帧还原失败: {e:?}");
                }
            },
            Err(e) => {
                errors += 1;
                err_streak += 1;
                eprintln!("[rec] 收帧失败: {e:?}");
            }
        }
        // 失步重同步（三级防护第 ② 级）：连续失败 = 帧界已丢，盲目重试只会在
        // b64 载荷里撞幻影 "1ERA" 头等天文 payload。magic 扫描对齐后继续收；
        // 扫描期连续静默（None）= 线上断流，升级第 ③ 级重连。断档帧由 seq
        // 对账如实记账（不冒领数据，只求不死）。
        if err_streak >= ERR_STREAK_RESYNC {
            match fs.resync() {
                Some(scanned) => {
                    resyncs += 1;
                    err_streak = 0;
                    need_reanchor = true;
                    eprintln!("[rec] 失步重同步 #{resyncs}：弃扫 {scanned}B 后 magic 对齐，续收");
                }
                None => {
                    // 三级防护第 ③ 级：1.6s 连续静默 = 线上断流，重开口续收
                    reconnects += 1;
                    drop(fs); // 还 conn 可变借用
                    eprintln!("[rec] 重同步 1.6s 连续静默 → 断流重连 #{reconnects}");
                    match Rr3UartConn::open_with_baud(
                        std::path::Path::new(&args.device),
                        args.baud,
                    ) {
                        Ok(c) => conn = c,
                        Err(e) => {
                            eprintln!("[rec] 重连开口失败: {e}");
                            bail_with_recovery(&out_path, total_bytes, &watch);
                        }
                    }
                    if let Err(e) = handshake(&mut conn) {
                        eprintln!("[rec] 重连握手失败: {e}");
                        // 尽力停板面流（板死则无效；INFO 探针前仍须先 OFF 兜底）
                        let _ = conn.write_all(b"AUDIO OFF\n");
                        bail_with_recovery(&out_path, total_bytes, &watch);
                    }
                    fs = FrameStream::new(&mut conn);
                    err_streak = 0;
                    need_reanchor = true;
                    eprintln!("[rec] 重连 #{reconnects} 完成，续收（板侧已复位，seq 重走记 restarts）");
                }
            }
        }
    }
    // 收兵：先还 conn 可变借用（fs 持有 &mut conn），再发 OFF——板面队列余片
    // 照发，本端不再读，conn 关闭即弃。OFF 后解武装（本地补头不走看门狗）。
    drop(fs);
    let _ = conn.write_all(b"AUDIO OFF\n");
    watch.armed.store(false, Ordering::Release);

    if total_bytes == 0 {
        eprintln!("[rec] 0 分片（查板面 AUDIO ON 应答/I2S 面波特率档）");
        drop(wav);
        let _ = std::fs::remove_file(&out_path); // 占位空壳不留垃圾
        std::process::exit(1);
    }
    // 实测采样率改按**连续段**（锚后至最后一片的墙钟 × 段内帧数）：断流空洞
    // 不摊进分母。段 <0.5s 或退化率（<1kHz，锚后无实流的边缘段）退标称 rate。
    let (frames_total, nominal_rate) = (total_bytes / 4, 16000u32);
    let measured = match rate_anchor {
        Some((t0, f0)) => {
            let end = last_frame.unwrap_or(t0);
            let d = end.duration_since(t0).as_secs_f64();
            let r = if d > 0.5 {
                (((frames_total - f0) as f64) / d).round() as u32
            } else {
                0
            };
            if r >= 1000 {
                r
            } else {
                nominal_rate
            }
        }
        None => nominal_rate,
    };
    let data_len = total_bytes as u32;
    match wav
        .seek(SeekFrom::Start(0))
        .and_then(|_| wav.write_all(&wav_header(data_len, 2, measured, 16)))
        .and_then(|()| wav.flush())
    {
        Ok(()) => println!(
            "[rec] ✓ {}：chunks={chunks} gaps={} restarts={} err={errors} resyncs={resyncs} reconnects={reconnects} frames={frames_total} rate={}Hz（实测连续段）dur≈{:.1}s size={data_len}B",
            out_path.display(),
            tracker.gaps,
            tracker.restarts,
            measured,
            frames_total as f64 / measured as f64,
        ),
        Err(e) => {
            eprintln!("rr3-audio-rec: wav 补头失败 {}: {e}", out_path.display());
            std::process::exit(1);
        }
    }
    if args.doa {
        println!("{}", doa.summary());
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
        assert!(d2.feed(&synth_stereo(8000, 800)[..DOA_WIN_FRAMES * 2]).is_none());
        assert!(d2.feed(&synth_stereo(8000, 800)).is_some(), "补满即出");
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
