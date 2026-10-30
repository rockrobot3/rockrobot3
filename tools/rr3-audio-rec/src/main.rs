//! rr3-audio-rec —— 批D③ 音频上行收流端（宿主；0.0.5 增补轮五 全案 §4 批D 判据①）。
//!
//! 链路：板面 INMP441 双麦（mic.rs 分片组帧，`rr3-sense/audio/pcm` Event @
//! trace_id=0 / src=2 / dst=1）→ UART2 1.5M（板固件 `board::UART2_BAUD_HZ` 同档；
//! 2026-10-30 自 921600 提档定档，音频 85KB/s 占线从 ~95% 降到 ~46%）→
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
//! - 三级防护（0.0.5 批D④ d4 600s 实锤；机制与策略分离；判据② 收流线程化——
//!   A/B 决定性实验定案：病灶是分相读节奏（128B 头→plen 载荷两段间让出），
//!   85KB/s 连续流下间歇 RX 溢出丢字节；python read(65536) 同款大块裸读 600s
//!   满速零污染。读线程只做大块裸读推裸字节通道，解码线程纯内存拆帧
//!   （[`FrameAssembler`]，头校验全数 rr-ipc [`Rr3Header::from_bytes`] 单点，
//!   零第二份帧语义））：
//!   ① 读超时——机制面 rr-ipc `rr3_read_timeout`（poll 等就绪，`Ok(0)`=此刻无
//!   数据非 EOF），读线程裸读/应答读全走超时档；阻塞读缺省语义保持为 rr-ipc
//!   其余用户（桥/哨兵）不动。
//!   ② 失步重同步——帧头 magic 线上字节 = "1ERA"（全 b64 字母表字符），失步后
//!   在载荷里撞上即"合法幻影帧头"+ 天文 payload_len。坏帧起点弃 4B magic 续扫
//!   （字节已在缓冲，弃扫零新 I/O——滑移不放大为风暴的关键）；连续
//!   [`ERR_STREAK_RESYNC`] 次拆帧失败记一次重同步账（日志对账用）。
//!   ③ 断流重连——读线程见过数据后连续 [`RESYNC_ZERO_BAIL`] 次零读（1.6s
//!   静默）= 线上断流，就地重开口 settle + AUDIO ON 续收（seq 重走/续走都如实
//!   记账：板复位 `restarts`，续走空洞入 `gaps`）；握手 [`HANDSHAKE_TRIES`] 次
//!   重试、每轮重开 fd（宿主侧 USB RX 冻结数分钟内自愈、板面四轮洗冤，重开 fd
//!   是宿主侧唯一捅法，骑行期看门狗解武装）。
//!   落盘走写线程（解码循环零磁盘 I/O，/dev/null 实验定案：逐片同步写刷是冻结
//!   放大器）；看门狗（武装后 [`STALL`] 无新分片字节 → 补 wav 恢复头 exit(3)）
//!   是最外层兜底；只管武装后的收流期，重连骑行期解武装不误触发。
//!
//! 用法：`rr3-audio-rec <device> [--out <path>] [--secs <n>] [--baud <n>] [--doa]`
//! 缺省 out=`rr3-audio-<unix秒>.wav`、secs=30、baud=1500000（板面 2026-10-30 定档
//! 实测档，与固件 `board::UART2_BAUD_HZ` 同值；
//! 波特率仍过 rr-ipc 档位表白名单，不预设动物园）。
//! 退出码：0=收兵；1=开口/握手/0 分片；2=参数错；3=看门狗停滞恢复；4=重连失败/
//! 收流线程亡（3/4 都先补恢复头保住已落盘数据再退）。

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write as _};
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use rr_ipc::audio_session::{
    handshake, HANDSHAKE_RETRY_PAUSE, HANDSHAKE_TRIES, READ_BLOCK, READ_TIMEOUT_MS,
    RESYNC_ZERO_BAIL,
};
use rr_ipc::{
    rr3_base64_decode, rr3_body_split, rr3_uart_baud_speed, FrameAssembler, Rr3Frame,
    Rr3PayloadType, Rr3UartConn,
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
/// 停滞死线（看门狗）：**武装后的收流期**内无新分片字节即判停滞。分片节拍
/// 128ms/片，10s ≈ 78 片空窗——正常流不可达；重连握手重试期（≈90s 骑行窗）
/// 与重同步窗口（1.6s）看门狗解武装不计停滞，否则长骑行必被误杀；触发即实锤
/// "武装收流中，分片字节 10s 未确认入队"。
const STALL: Duration = Duration::from_secs(10);
/// 开口 settle/应答超时/断流判据/大块裸读窗/重连预算七常量与握手三函数已上提
/// rr-ipc 单点（`rr_ipc::audio_session`，A 批 c2 随 rr-hal esp32 音频源后端共用；
/// 本件 import 接线，不再持第二份会话机制）。

/// 连续拆帧失败达此数记一次失步重同步账（日志对账；坏帧弃扫本身由
/// [`FrameAssembler`] 在内存里即时完成，无独立扫描动作——1-2 次可能是单帧毛刺，
/// 3 次 = 帧界成片损坏，值得一条日志点名）。
const ERR_STREAK_RESYNC: u64 = 3;
/// 解码线程等裸字节的醒来节拍（查收兵死线；断流/重连由读线程自治，解码面
/// 只管出帧与记账）。
const DECODE_TICK: Duration = Duration::from_millis(200);

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
    let mut baud = 1_500_000u32;
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

// 拆帧器已上提 rr-ipc 单点（`rr_ipc::framing`，2026-10-18 A 批随 rr-hal esp32
// 音频源后端共用而搬迁；机制本体与用例随迁彼处，本件只接线）。
// 收一行/OK 判据/开口握手三函数同批上提 `rr_ipc::audio_session`（机制单点）——
// 诊断日志前缀由 "[rec]" 改 "[audio]"（机制面属主中立，线上字节零变化）。

/// fail-honest 退出（重连失败路径）：已落盘数据补恢复头保住，exit(4) 点名。
/// data_len 用盘侧真相 [`WatchState::disk_bytes`]（写线程只在实际写透后推进，
/// 进程即退时 BufWriter 未刷尾不虚计）；`FileExt::write_at` 位置写不动游标、
/// 与写线程 payload 写无交叠（44B 头 @0 vs ≥44 偏移的数据），活线程下也安全。
/// （看门狗 exit(3) 同款恢复头逻辑——两路独立小段，不抽象共享。）
fn bail_with_recovery(out_path: &std::path::Path, watch: &WatchState) -> ! {
    let data_len = watch.disk_bytes.load(Ordering::Acquire) as u32;
    let rate = watch.rate.load(Ordering::Acquire);
    let head = wav_header(data_len, 2, rate, 16);
    if let Err(e) = OpenOptions::new()
        .write(true)
        .open(out_path)
        .and_then(|f| f.write_all_at(&head, 0))
    {
        eprintln!("[rec] 恢复头补写失败 {}: {e}", out_path.display());
    }
    std::process::exit(4);
}

/// 读线程（收流端机制面，判据② A/B 定案改造）：**只做大块裸读**——单次
/// [`Rr3UartConn::rr3_read_timeout`]（poll 等就绪 + read 一次 [`READ_BLOCK`] 窗）
/// 整块推进裸字节通道，分相读的 128B 头/plen 载荷两段间不再让出节奏（A/B
/// 决定性实验：python read(65536) 同款节奏 600s 满速零污染；分相读同等链路
/// 2-16s 起逐帧损坏风暴——病灶是读路径节奏，不是链路也不是板）。
///
/// 静默检测与重连同线程闭环（证据面在读侧）：见过数据后连续
/// [`RESYNC_ZERO_BAIL`] 次零读（1.6s 连续静默 = 宿主侧 USB RX 冻结实锤形状）
/// → 就地重开口 + 握手（[`reader_reconnect`]，[HANDSHAKE_TRIES] 次），武装旗
/// 骑行期解武装（看门狗不计停滞）。停流旗置位即发 AUDIO OFF 收兵（收兵语义
/// 与旧版同序：先停板面流，写线程排空由主线程收口）。
fn reader_run(
    mut conn: Rr3UartConn,
    tx: mpsc::Sender<Vec<u8>>,
    stop: Arc<AtomicBool>,
    watch: Arc<WatchState>,
    device: String,
    baud: u32,
    out_path: PathBuf,
) {
    let mut buf = vec![0u8; READ_BLOCK];
    let mut seen_data = false; // 本段（首连/重连后）见过数据才算真静默
    let mut zeros = 0u32;
    loop {
        if stop.load(Ordering::Acquire) {
            let _ = conn.write_all(b"AUDIO OFF\n");
            return;
        }
        match conn.rr3_read_timeout(&mut buf, READ_TIMEOUT_MS) {
            Ok(n) if n > 0 => {
                seen_data = true;
                zeros = 0;
                if tx.send(buf[..n].to_vec()).is_err() {
                    return; // 解码面已亡（主线程先退），主线程/看门狗收口
                }
            }
            _ => {
                if !seen_data {
                    continue; // 武装后首片前的静默合法（复位 banner/settle 窗）
                }
                zeros += 1;
                if zeros < RESYNC_ZERO_BAIL {
                    continue;
                }
                eprintln!("[rec] 1.6s 连续静默 → 线上断流，读线程重连");
                watch.armed.store(false, Ordering::Release);
                if !reader_reconnect(&mut conn, &device, baud) {
                    eprintln!("[rec] 读线程重连全败");
                    bail_with_recovery(&out_path, &watch); // exit(4)，盘侧真相保数据
                }
                watch.reconnects.fetch_add(1, Ordering::Relaxed);
                watch.armed.store(true, Ordering::Release);
                seen_data = false;
                zeros = 0;
            }
        }
    }
}

/// 读线程重连（三级防护第 ③ 级，判据② 改造后搬进读线程）：[HANDSHAKE_TRIES]
/// 次、每轮重开 fd（宿主侧 USB RX 冻结唯一捅法）+ 防御停流握手。全败 false
/// 交调用方 fail-honest 收场。
fn reader_reconnect(conn: &mut Rr3UartConn, device: &str, baud: u32) -> bool {
    let mut hs_err: Option<String> = None;
    for attempt in 1..=HANDSHAKE_TRIES {
        if attempt > 1 {
            eprintln!(
                "[rec] 重连握手重试 {attempt}/{HANDSHAKE_TRIES}（歇 {}s 后重开口…）",
                HANDSHAKE_RETRY_PAUSE.as_secs()
            );
            std::thread::sleep(HANDSHAKE_RETRY_PAUSE);
        }
        match Rr3UartConn::open_with_baud(std::path::Path::new(device), baud) {
            Ok(c) => *conn = c, // 赋值即 drop 旧 conn（close 旧 fd）
            Err(e) => {
                eprintln!("[rec] 重连开口失败（第 {attempt} 次）: {e}");
                hs_err = Some(format!("开口失败: {e}"));
                continue;
            }
        }
        match handshake(conn) {
            Ok(_) => return true,
            Err(e) => {
                eprintln!("[rec] 重连握手失败（第 {attempt} 次）: {e}");
                hs_err = Some(e);
                // 尽力停板面流（下一轮重开还会再发防御 OFF；板死则无效）
                let _ = conn.write_all(b"AUDIO OFF\n");
            }
        }
    }
    if let Some(e) = hs_err {
        eprintln!("[rec] 重连握手 {HANDSHAKE_TRIES} 次全败: {e}");
    }
    false
}

/// 看门狗共享账（解码线程推进 `ingested`/`rate`，写线程推进 `disk_bytes`，读
/// 线程推进 `reconnects`/`armed`，看门狗线程只读其余）：停滞判据 = 已确认入队
/// 字节不再增长（原始字节到达≠有分片——失步垃圾流也算死流，只认过了事件门的
/// 分片）。落盘线程化（d4 /dev/null 实验定案：解码循环零磁盘 I/O）后账拆**流侧/
/// 盘侧**双真相——`ingested` 给看门狗判停滞，`disk_bytes` 给恢复头报实际写透
/// 字节数（写线程 flush 后推进，未刷尾不虚计）。
struct WatchState {
    /// 解码线程已确认交给写线程的 PCM 字节（流侧真相；看门狗停滞判据）。
    ingested: AtomicU64,
    /// 写线程已写透落盘的 PCM 字节（盘侧真相；恢复头 data_len 事实源）。
    disk_bytes: AtomicU64,
    /// 读线程重连记账（骑行一次 +1；收兵汇总显示用）。
    reconnects: AtomicU64,
    /// 实测采样率（逐片刷新：frames/elapsed；不足 0.5s 测时标称）。
    rate: AtomicU32,
    /// AUDIO ON 应答后武装（武装前不计时——开口 settle/复位 banner 阶段静默合法）。
    armed: AtomicBool,
}

/// 长流楔死看门狗（收流端策略面，d4 600s 实锤的修复）。
///
/// 机理：宿主/CP2102 驱动层丢字节后帧层等幻影帧尾永不返回（第一轮 600s 稳态
/// 跑：板面 seq=9182 sent=9182 drop=0 全程健康，宿主 `rr3_recv_buf` 楔死 8
/// 分钟无退出）。本线程每 500ms 查推进：武装后 [`STALL`] 无新分片字节 → 补
/// wav 恢复头（data_len=盘侧写透字节）→ exit(3)。fail-honest：已收数据保得
/// 住，停滞事实打得响。
fn watchdog_run(w: Arc<WatchState>, out: String) {
    let mut last_seen = 0u64;
    let mut last_move = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let nb = w.ingested.load(Ordering::Acquire);
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
        let disk = w.disk_bytes.load(Ordering::Acquire);
        eprintln!(
            "[rec] ✗ 收流停滞 ≥{STALL:?} 无新分片字节（楔死防护触发）：入队 {last_seen}B/盘侧 {disk}B，补恢复头后退出"
        );
        let data_len = disk as u32;
        let rate = w.rate.load(Ordering::Acquire);
        let head = wav_header(data_len, 2, rate, 16);
        if let Err(e) = OpenOptions::new()
            .write(true)
            .open(&out)
            .and_then(|f| f.write_all_at(&head, 0))
        {
            eprintln!("[rec] 恢复头补写失败 {out}: {e}");
        }
        std::process::exit(3);
    }
}

/// 写线程（落盘独占面）：占位头 → 逐片写透（write_all + flush，崩溃面数据风险
/// 与旧逐片同步落盘等价）→ 通道关闭即收兵退出。错误即整进程退（fail-honest，
/// 不带病续跑冒领数据）；`disk_bytes` 只在实际写透后推进——恢复头 data_len 的
/// 事实源。最终头（write_at(0)）由主线程 join 后补，无并发窗口。无界通道：写
/// 线程被磁盘反压时分片在内存排队（87.5KB/s 流速下秒级尖峰至多 MB 级），收流面
/// 不被磁盘 I/O 拖住（冻结放大器解耦的落点）。
fn writer_run(file: File, rx: mpsc::Receiver<Vec<u8>>, watch: Arc<WatchState>) {
    let mut w = BufWriter::new(file);
    // 占位头（data_len=0）：正常收兵与看门狗/重连失败两路都回头 write_at(0) 补
    // 真头。**立即 flush**：磁盘恒有合法头，恢复路径不依赖 BufWriter 析构刷出。
    if let Err(e) = w
        .write_all(&wav_header(0, 2, 16000, 16))
        .and_then(|()| w.flush())
    {
        eprintln!("rr3-audio-rec: wav 占位头写入失败: {e}");
        std::process::exit(1);
    }
    let mut written = 0u64;
    for buf in rx {
        if let Err(e) = w.write_all(&buf).and_then(|()| w.flush()) {
            eprintln!("rr3-audio-rec: wav 写线程写失败: {e}");
            std::process::exit(1);
        }
        written += buf.len() as u64;
        watch.disk_bytes.store(written, Ordering::Release);
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("rr3-audio-rec: {e}\n\n用法: rr3-audio-rec <device> [--out <path>] [--secs <n>] [--baud <n>] [--doa]\n缺省 out=rr3-audio-<unix秒>.wav secs=30 baud=1500000（板面 2026-10-30 定档）；--doa 逐 1s 窗 ILD 出角（批D⑤ 冒烟）");
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

    // 收流面线程化（判据② A/B 定案）：读线程（conn 所有权整体移交）只做大块
    // 裸读推裸字节通道；解码主循环纯内存拆帧 → dst/Event/topic/b64 四道门 →
    // 逐片交写线程落盘 + seq 对账。落盘线程化（d4 /dev/null 决定性实验定案）：
    // 解码循环零磁盘 I/O——每片同步写刷的延迟尖峰把冻结概率放大 ~7 倍（38/51s
    // vs 369s），通道解耦后尖峰不再进收流面；看门狗保数据前提不变（停滞即整
    // 进程退，盘侧逐片写透）。
    let out_path = args.out.clone();
    let wav_file = match File::create(&out_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("rr3-audio-rec: wav 建档失败 {}: {e}", out_path.display());
            std::process::exit(1);
        }
    };
    let watch = Arc::new(WatchState {
        ingested: AtomicU64::new(0),
        disk_bytes: AtomicU64::new(0),
        reconnects: AtomicU64::new(0),
        rate: AtomicU32::new(16000),
        armed: AtomicBool::new(false),
    });
    // 写线程独占 wav 文件（占位头/逐片写透全在此线程）；mpsc 解耦见 writer_run。
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let writer_thread = {
        let w = Arc::clone(&watch);
        std::thread::spawn(move || writer_run(wav_file, rx, w))
    };
    {
        let w = Arc::clone(&watch);
        let path = out_path.clone();
        std::thread::spawn(move || watchdog_run(w, path.to_string_lossy().into_owned()));
    }
    // 读线程：conn/裸字节通道/停流旗移交（静默重连与收兵 OFF 全在读侧闭环，
    // 见 reader_run）。
    let (byte_tx, byte_rx) = mpsc::channel::<Vec<u8>>();
    let stop = Arc::new(AtomicBool::new(false));
    let reader_thread = {
        let w = Arc::clone(&watch);
        let stop = Arc::clone(&stop);
        let dev = args.device.clone();
        let out = out_path.clone();
        let baud = args.baud;
        std::thread::spawn(move || reader_run(conn, byte_tx, stop, w, dev, baud, out))
    };
    watch.armed.store(true, Ordering::Release);

    let mut tracker = SeqTracker {
        prev: None,
        gaps: 0,
        restarts: 0,
    };
    let mut doa = DoaTracker::new();
    let mut chunks = 0u64;
    let mut errors = 0u64;
    let mut err_streak = 0u64; // 连续拆帧失败（好片即清零；达阈值记重同步账）
    let mut resyncs = 0u64;
    let mut total_bytes = 0u64;
    let mut last_frame: Option<Instant> = None;
    // 连续段实测率锚 (锚时刻, 锚时帧数)：断档/重启/首片即重新锚定——断流空洞
    // 不摊进实测率分母（旧版首末墙钟摊全程，断流后实测率被稀释）。重连/失步
    // 必然丢片（seq 断档或重启）→ `broke` 覆盖全部重锚场景。
    let mut rate_anchor: Option<(Instant, u64)> = None;
    let deadline = Instant::now() + Duration::from_secs(args.secs);
    let mut asm = FrameAssembler::new();
    println!(
        "[rec] streaming {}s @ {} ({})…",
        args.secs, args.baud, args.device
    );
    while Instant::now() < deadline {
        match byte_rx.recv_timeout(DECODE_TICK) {
            Ok(bytes) => asm.feed(&bytes),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("[rec] 裸字节通道断开（读线程已亡）");
                bail_with_recovery(&out_path, &watch);
            }
        }
        // 内存拆帧：出尽当前缓冲。坏帧弃扫即时完成（Err 起点已弃，续扫由下次
        // pull 自然接上），丢片由 seq 对账如实记账。
        loop {
            let assembled = match asm.pull() {
                Ok(Some(a)) => Some(a),
                Ok(None) => break,
                Err((dropped, why)) => {
                    errors += 1;
                    err_streak += 1;
                    eprintln!("[rec] 帧头拒收（弃 {dropped}B 续扫）: {why}");
                    None
                }
            };
            if let Some((header, payload)) = assembled {
                let frame = Rr3Frame { header, payload };
                match parse_audio_event(&frame) {
                    Ok(chunk) => {
                        let now = Instant::now();
                        last_frame = Some(now);
                        let account_before = (tracker.gaps, tracker.restarts);
                        tracker.feed(chunk.seq);
                        let broke = (tracker.gaps, tracker.restarts) != account_before;
                        chunks += 1;
                        err_streak = 0;
                        // DoA 在解码面算（纯计算）；分片移交写线程（零磁盘 I/O）
                        if args.doa {
                            if let Some(line) = doa.feed(&chunk.samples) {
                                println!("{line}");
                            }
                        }
                        let nsamp = chunk.samples.len();
                        if tx.send(chunk.samples).is_err() {
                            eprintln!("rr3-audio-rec: 写线程通道断开（写线程已亡）");
                            bail_with_recovery(&out_path, &watch);
                        }
                        total_bytes += nsamp as u64;
                        watch.ingested.store(total_bytes, Ordering::Release);
                        let frames = total_bytes / 4;
                        // 连续段实测率：断档/重启即重新锚定（空洞不摊进分母）
                        if broke || rate_anchor.is_none() {
                            rate_anchor = Some((now, frames));
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
                    }
                    Err(e) => {
                        errors += 1;
                        err_streak += 1;
                        eprintln!("[rec] 事件门拒收: {e}");
                    }
                }
            }
            // 失步重同步账（三级防护第 ② 级的记账面）：坏帧弃扫已在内存即时
            // 完成，这里只把成片损坏点名为日志（丢片由 seq 对账如实记账）。
            if err_streak >= ERR_STREAK_RESYNC {
                resyncs += 1;
                err_streak = 0;
                eprintln!(
                    "[rec] 失步重同步 #{resyncs}：内存弃扫 {}B 后对齐续收",
                    asm.discarded()
                );
            }
        }
    }
    // 收兵：停流旗 → 读线程发 AUDIO OFF 退出（板面队列余片照发，本端不再读，
    // conn 随线程收口关闭）。OFF 后解武装（本地补头不走看门狗）。
    stop.store(true, Ordering::Release);
    if reader_thread.join().is_err() {
        eprintln!("rr3-audio-rec: 读线程异常退出");
        std::process::exit(1);
    }
    watch.armed.store(false, Ordering::Release);
    // 关通道让写线程排空收兵（逐片已 flush，无尾数据）；写线程异常即整进程退，
    // 不冒领成功。
    drop(tx);
    if let Err(e) = writer_thread.join() {
        eprintln!("rr3-audio-rec: 写线程异常退出: {e:?}");
        std::process::exit(1);
    }

    if total_bytes == 0 {
        eprintln!("[rec] 0 分片（查板面 AUDIO ON 应答/I2S 面波特率档）");
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
    let data_len = watch.disk_bytes.load(Ordering::Acquire) as u32;
    match OpenOptions::new()
        .write(true)
        .open(&out_path)
        .and_then(|f| f.write_all_at(&wav_header(data_len, 2, measured, 16), 0))
    {
        Ok(()) => println!(
            "[rec] ✓ {}：chunks={chunks} gaps={} restarts={} err={errors} resyncs={resyncs} reconnects={} frames={frames_total} rate={}Hz（实测连续段）dur≈{:.1}s size={data_len}B",
            out_path.display(),
            tracker.gaps,
            tracker.restarts,
            watch.reconnects.load(Ordering::Acquire),
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
        assert!(d2.feed(&synth_stereo(8000, 800)[..DOA_WIN_FRAMES * 2]).is_none());
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
}
