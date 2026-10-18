//! ESP32-UART 板面音频源后端（A 批 c3）：板面 INMP441 双麦流 → 宿主环形缓冲 →
//! [`Rr3AudioSource`]（`rr3_hal_audio_source()` esp32 档）。
//!
//! 链路：板面 mic.rs 分片组帧（16kHz 双声道 s16le，128ms/8192B 一片，A-IPC
//! Event 帧，topic `rr3-sense/audio/pcm`）→ UART2（[`crate::esp32_uart::
//! RR3_HAL_PERIPH_UART_ENV`] 指定设备，[`crate::esp32_uart::
//! RR3_HAL_PERIPH_BAUD_ENV`] 波特率——板固件已钉 921600，须显式同档）→ 本件
//! 收流：开口 → `rr_ipc::audio_session::handshake`（settle/防御 OFF/ON 应答
//! 扫描，与 rr3-audio-rec 同一机制单点）→ 读线程大块裸读（判据② 节奏）→
//! [`FrameAssembler`](rr_ipc::FrameAssembler) 拆帧 → 四道门解析 → 环形缓冲
//! （~1s，溢出丢最旧整帧 + 计数）→ `read()` 整声道帧出样。
//!
//! 纪律：
//! - flock 单消费者（`Rr3UartConn` 开口即跨进程排他）：流期间舵机/显示/
//!   rr3-audio-rec 不可并存——fail-closed 点名，不是静默截流；A 链回放恒宿主
//!   扬声器（backend.rs sink 侧 esp32 档显式落占位），互斥面天然不成立。
//! - 欠载语义：`read()` 非阻塞，无新数据回 0——消费面（rr3-sense）走既有
//!   `pcm_empty_frames` 记账（饥饿可见，不阻塞节拍）。
//! - 断流重连**永不放弃**：见过数据后连续 [`RESYNC_ZERO_BAIL`] 次零读（1.6s
//!   静默）= 线上断流，就地重开口重握手（[`HANDSHAKE_TRIES`] 轮 + 歇口，
//!   每轮重开 fd——宿主侧 USB RX 冻结唯一捅法）；全败后外层循环续轮。与
//!   工具侧 fail-honest exit(4) 的差异是刻意的：源是常驻件，不新增
//!   `Rr3HalError` 变体（重连失败不是一次性调用的错误，是持续饥饿）。
//! - 板-宿主时钟 ~2% 滑移（run4 588s 实测"有损可用"）：环形缓冲溢丢是稳态
//!   损耗，VAD/ASR 可容忍；首溢点名一条日志，此后静默计数。
//! - 四道门解析（dst/payload 类型/topic/字段+b64）在本件内实现，与
//!   tools/rr3-audio-rec 的 `parse_audio_event` **同值锚定**（rr-ipc 零第三方
//!   依赖约束，serde_json 面不上提；门序与判据逐字同源，双侧测试兜底防分叉）。
//!
//! cfg 无平台门（与 [`crate::esp32_uart`] 同口径）：rr-ipc 的 UART/会话机制
//! 面 cfg 门（linux/macos）兜底，其余平台编译即错——显式优于隐式。

use std::collections::VecDeque;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rr_hal_api::{Rr3AudioCfg, Rr3AudioSource, Rr3ErrText, Rr3HalError};
use rr_ipc::audio_session::{
    handshake, HANDSHAKE_RETRY_PAUSE, HANDSHAKE_TRIES, READ_BLOCK, READ_TIMEOUT_MS,
    RESYNC_ZERO_BAIL,
};
use rr_ipc::{
    rr3_base64_decode, rr3_body_split, FrameAssembler, Rr3Header, Rr3PayloadType, Rr3UartConn,
};
use serde_json::Value;

// ---------------------------------------------------------------------------
// 两端帧头约定（板不依赖 rr-core，板/宿主常量自证——与 tools/rr3-audio-rec
// 同纪律；与 crate::esp32_uart 的同值锚定测试同一防静默分叉口径）
// ---------------------------------------------------------------------------

/// 宿主 task 位形（帧头 dst 门：只收发给宿主的 Event）。与
/// `crate::esp32_uart::RR3_TASK_HOST` 同值 1（彼处私有，本地自证 + 注释锚定）。
const RR3_HOST_TASK: u64 = 1;
/// 音频 PCM 上行主题（在册 stream L1：contracts/topics.toml；事实源
/// modules/rr3-sense composite::`RR3_PCM_TOPIC`，本 crate 不加依赖边，同值锚定）。
const RR3_AUDIO_TOPIC: &str = "rr3-sense/audio/pcm";
/// 连续门失败/拆帧失败达此数记一条日志（1-2 次可能是单帧毛刺，3 次 = 帧界
/// 成片损坏，值得点名；坏帧弃扫本身由 [`FrameAssembler`] 即时完成）。
const RR3_ERR_STREAK_LOG: u64 = 3;

/// 板面音频流事实（板固件 mic.rs 钉定）：16kHz 采样率。open 校验配置同款，
/// 不匹配回 `InvalidParam`（fail-closed，指路 config-esp32.toml）。
pub const RR3_ESP32_AUDIO_RATE: u32 = 16000;
/// 板面音频流事实：双声道（INMP441 ×2，DoA 判向的物理基础）。
pub const RR3_ESP32_AUDIO_CHANNELS: u8 = 2;

/// 环形缓冲容量（采样点）：恰 ~1s @16k 双声道。溢出丢最旧**整帧**（保声道
/// 交织结构）+ 计数；~2% 板-宿主钟滑移下溢丢是稳态损耗（见模块文档）。
const RR3_ESP32_RING_CAP_SAMPLES: usize =
    RR3_ESP32_AUDIO_RATE as usize * RR3_ESP32_AUDIO_CHANNELS as usize;

// ---------------------------------------------------------------------------
// 纯逻辑件（不依赖串口/线程，单测直测）
// ---------------------------------------------------------------------------

/// s16le 字节流 → i16 采样（LE 对；奇数尾字节弃——8192B 片恒偶，防御不改变
/// 现实形状）。
fn rr3_s16le_decode(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|p| i16::from_le_bytes([p[0], p[1]]))
        .collect()
}

/// 环形采样缓冲：Mutex 内持有的纯数据件。溢出丢最旧**整声道帧**（保交织
/// 结构），丢弃计数对账（稳态滑移损耗可见）。
struct Rr3AudioRing {
    deque: VecDeque<i16>,
    cap: usize,
    dropped: u64,
}

impl Rr3AudioRing {
    fn rr3_ring_new(cap: usize) -> Self {
        Self {
            deque: VecDeque::with_capacity(cap),
            cap,
            dropped: 0,
        }
    }

    /// 推入一段采样（板面片解码产物）。防御性整帧对齐：输入按 ch 取整
    /// （8192B 片恒 4096 样本，现实输入恒对齐）；溢出丢最旧整帧 + 计数。
    fn rr3_ring_push(&mut self, samples: &[i16], ch: usize) {
        let ch = ch.max(1);
        let aligned = samples.len() - samples.len() % ch;
        let samples = &samples[..aligned];
        // 溢出量按整帧对齐后从最旧端丢弃
        let overflow = (self.deque.len() + samples.len()).saturating_sub(self.cap);
        let drop_n = overflow - overflow % ch;
        for _ in 0..drop_n {
            self.deque.pop_front();
        }
        self.dropped += drop_n as u64;
        self.deque.extend(samples.iter().copied());
        // 入样本身超容量（理论形状，片 4096 << cap 32000 不可达）：再削最旧整帧
        let over = self.deque.len().saturating_sub(self.cap);
        let over_n = over - over % ch;
        for _ in 0..over_n {
            self.deque.pop_front();
        }
        self.dropped += over_n as u64;
    }

    /// 弹出整声道帧填充 `out`：`floor(avail/ch)` 与 `floor(out.len()/ch)` 取小
    /// ×ch，返回实填采样点数；欠载（含空环）回 0。非阻塞。
    fn rr3_ring_take(&mut self, out: &mut [i16], ch: usize) -> usize {
        let ch = ch.max(1);
        let take = (self.deque.len() / ch).min(out.len() / ch) * ch;
        for slot in out.iter_mut().take(take) {
            *slot = self.deque.pop_front().unwrap_or(0);
        }
        take
    }

    fn rr3_ring_dropped(&self) -> u64 {
        self.dropped
    }
}

/// 一条音频事件（四道门全过后的解码前载荷）。
struct Rr3Esp32AudioChunk {
    /// 板面实测采样率（流就绪日志用；open 已按板事实校验配置）
    rate: u32,
    /// s16le 交错原始字节（b64 解码后）
    samples: Vec<u8>,
}

/// Event 帧解析（四道门全过才 Ok，任一失败 Err 点名——与板侧 frame_recv 同款
/// fail-closed 口径；与 tools/rr3-audio-rec `parse_audio_event` 同值锚定）：
/// dst=宿主门 → payload Event 类型门 → topic 门 → 字段/b64 门。
fn rr3_esp32_parse_audio_event(
    hdr: &Rr3Header,
    payload: &[u8],
) -> Result<Rr3Esp32AudioChunk, String> {
    if hdr.dst_task != RR3_HOST_TASK {
        return Err(format!(
            "dst_task={} 非宿主位形 {RR3_HOST_TASK}",
            hdr.dst_task
        ));
    }
    let (ty, body) = rr3_body_split(payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
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
    let rate = v.get("rate").and_then(Value::as_u64).ok_or("缺 rate")? as u32;
    if v.get("seq").and_then(Value::as_u64).is_none() {
        return Err("缺 seq".into());
    }
    if v.get("ms").and_then(Value::as_u64).is_none() {
        return Err("缺 ms".into());
    }
    let b64 = v
        .get("pcm_b64")
        .and_then(Value::as_str)
        .ok_or("缺 pcm_b64")?;
    let samples = rr3_base64_decode(b64).map_err(|e| format!("pcm_b64 解码失败: {e:?}"))?;
    Ok(Rr3Esp32AudioChunk { rate, samples })
}

/// open 配置校验（纯函数）：板面流事实（16k/双声道/s16le）钉定，配置须同款。
/// 不匹配回 `InvalidParam` 并指路 config-esp32.toml——单声道缺省配置误吃
/// esp32 档时在探活期就点名，不静默出交织垃圾。
fn rr3_esp32_audio_cfg_check(cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
    if cfg.sample_rate_hz != RR3_ESP32_AUDIO_RATE
        || cfg.channels != RR3_ESP32_AUDIO_CHANNELS
        || cfg.bit_depth != 16
    {
        return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
            "esp32 板面流钉 16k/双声道/s16le，配置须同款（见 modules/rr3-sense/config-esp32.toml）",
        )));
    }
    Ok(())
}

/// 开口 + 握手（首连/重连同款路径）：open 自带 flock 排他（跨进程第二 opener
/// 当场被拒）→ `audio_session::handshake`（settle/防御 OFF/ON 应答扫描）。
fn rr3_esp32_audio_open(path: &PathBuf, baud: u32) -> Result<Rr3UartConn, String> {
    let mut conn =
        Rr3UartConn::open_with_baud(path, baud).map_err(|e| format!("open 失败: {e}"))?;
    handshake(&mut conn).map_err(|e| format!("握手失败: {e}"))?;
    Ok(conn)
}

// ---------------------------------------------------------------------------
// 运行期件（读线程 + 环形缓冲 + trait 面）
// ---------------------------------------------------------------------------

/// 读线程共享核：连接参数 + 环形缓冲 + 停旗。
struct Rr3Esp32AudioCore {
    path: PathBuf,
    baud: u32,
    channels: u8,
    ring: Mutex<Rr3AudioRing>,
    stop: AtomicBool,
}

/// 断流重连（永不放弃）：歇口 → [`HANDSHAKE_TRIES`] 轮逐次**重开 fd** + 握手
/// （宿主侧 USB RX 冻结唯一捅法）；全败外层循环续轮。stop 置位即收兵回 None。
fn rr3_esp32_reconnect(core: &Rr3Esp32AudioCore) -> Option<Rr3UartConn> {
    loop {
        if core.stop.load(Ordering::Acquire) {
            return None;
        }
        std::thread::sleep(HANDSHAKE_RETRY_PAUSE);
        for attempt in 1..=HANDSHAKE_TRIES {
            if core.stop.load(Ordering::Acquire) {
                return None;
            }
            match rr3_esp32_audio_open(&core.path, core.baud) {
                Ok(c) => {
                    eprintln!("rr-hal-esp32-audio: 重连成功（第 {attempt}/{HANDSHAKE_TRIES} 轮）");
                    return Some(c);
                }
                Err(e) => {
                    eprintln!("rr-hal-esp32-audio: 重连第 {attempt}/{HANDSHAKE_TRIES} 轮失败: {e}")
                }
            }
            std::thread::sleep(HANDSHAKE_RETRY_PAUSE);
        }
        eprintln!(
            "rr-hal-esp32-audio: 重连 {HANDSHAKE_TRIES} 轮全败，继续循环（常驻源不放弃；饥饿期消费面走 pcm_empty_frames 记账）"
        );
    }
}

/// 读线程主体：会话内大块裸读 → 拆帧 → 四道门 → 入环；断流/错会话就地转
/// 重连。收兵（stop 旗）时板面 `AUDIO OFF`（防残流，conn 随作用域释放 →
/// flock 退账）。
fn rr3_esp32_reader_main(core: Arc<Rr3Esp32AudioCore>, first: Rr3UartConn) {
    let mut conn: Option<Rr3UartConn> = Some(first);
    let mut logged_ready = false;
    let mut logged_overflow = false;
    while !core.stop.load(Ordering::Acquire) {
        let Some(c) = conn.as_mut() else {
            conn = rr3_esp32_reconnect(&core);
            continue;
        };
        // —— 会话内读循环（拆帧器/记账随会话重置：新会话是新流）——
        let mut asm = FrameAssembler::new();
        let mut raw = vec![0u8; READ_BLOCK];
        let mut zeros = 0u32;
        let mut seen = false;
        let mut err_streak = 0u64;
        loop {
            if core.stop.load(Ordering::Acquire) {
                break;
            }
            match c.rr3_read_timeout(&mut raw, READ_TIMEOUT_MS) {
                Err(e) => {
                    eprintln!("rr-hal-esp32-audio: 读失败: {e} → 重连");
                    conn = None;
                    break;
                }
                Ok(0) => {
                    // 此刻无数据（EOF 形状）；见过数据后连续零读 = 断流
                    if seen {
                        zeros += 1;
                        if zeros >= RESYNC_ZERO_BAIL {
                            eprintln!(
                                "rr-hal-esp32-audio: 断流（连续 {RESYNC_ZERO_BAIL}×{READ_TIMEOUT_MS}ms 静默）→ 重连"
                            );
                            conn = None;
                            break;
                        }
                    }
                }
                Ok(n) => {
                    zeros = 0;
                    seen = true;
                    asm.feed(&raw[..n]);
                    loop {
                        match asm.pull() {
                            Ok(Some((hdr, body))) => {
                                match rr3_esp32_parse_audio_event(&hdr, &body) {
                                    Ok(chunk) => {
                                        err_streak = 0;
                                        if !logged_ready {
                                            logged_ready = true;
                                            eprintln!(
                                                "rr-hal-esp32-audio: 板面流就绪（实测 rate={} ch=2 s16le）",
                                                chunk.rate
                                            );
                                        }
                                        let samples = rr3_s16le_decode(&chunk.samples);
                                        let mut ring =
                                            core.ring.lock().unwrap_or_else(|p| p.into_inner());
                                        ring.rr3_ring_push(&samples, core.channels as usize);
                                        if ring.rr3_ring_dropped() > 0 && !logged_overflow {
                                            logged_overflow = true;
                                            eprintln!(
                                                "rr-hal-esp32-audio: 环形缓冲开始溢丢最旧（板-宿主钟滑移稳态损耗，VAD/ASR 可容忍）"
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        err_streak += 1;
                                        if err_streak == RR3_ERR_STREAK_LOG {
                                            eprintln!(
                                                "rr-hal-esp32-audio: 连续 {RR3_ERR_STREAK_LOG} 帧未过门（帧界损坏？）: {e}"
                                            );
                                        }
                                    }
                                }
                            }
                            Ok(None) => break,
                            Err((discarded, why)) => {
                                err_streak += 1;
                                if err_streak == RR3_ERR_STREAK_LOG {
                                    eprintln!(
                                        "rr-hal-esp32-audio: 连续 {RR3_ERR_STREAK_LOG} 次拆帧失败（已弃 {discarded}B 重扫）: {why}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // 收兵：板面停流（防御残流），conn 释放 → flock 退账
    if let Some(mut c) = conn {
        let _ = c.write_all(b"AUDIO OFF\n");
    }
}

/// ESP32-UART 板面音频源（A 批 c3）。构造不触设备（选择函数口径）；open 时
/// 真开 UART + 握手 + 起读线程。
pub struct Rr3Esp32UartAudioSource {
    path: PathBuf,
    baud: u32,
    inner: Mutex<Option<Rr3Esp32AudioStream>>,
}

struct Rr3Esp32AudioStream {
    core: Arc<Rr3Esp32AudioCore>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Rr3Esp32UartAudioSource {
    /// 构造（设备路径/波特率来自应急 env 的裁决结果；open 时才真开）。
    pub fn rr3_esp32_audio_source_new(path: PathBuf, baud: u32) -> Self {
        Self {
            path,
            baud,
            inner: Mutex::new(None),
        }
    }

    /// 停流（open 重入/Drop 共用）：置停旗 → 有界等读线程退出（读循环最迟
    /// [`READ_TIMEOUT_MS`] 查一次旗，重连路径每次尝试前也查；2s 上界足够，
    /// 超时 detach——线程下一查旗点自灭，板面 AUDIO OFF 自会补发）。
    fn rr3_close_stream(stream: Rr3Esp32AudioStream) {
        stream.core.stop.store(true, Ordering::Release);
        if let Some(h) = stream.reader.as_ref() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !h.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

impl Rr3AudioSource for Rr3Esp32UartAudioSource {
    fn open(&mut self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        rr3_esp32_audio_cfg_check(cfg)?;
        // 重入：先停旧流（flock 排他下旧连接不退账新开口必被拒）
        if let Some(old) = self.inner.lock().unwrap_or_else(|p| p.into_inner()).take() {
            Self::rr3_close_stream(old);
        }
        let conn = match rr3_esp32_audio_open(&self.path, self.baud) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("rr-hal-esp32-audio: 开口失败 {}: {e}", self.path.display());
                return Err(Rr3HalError::Busy);
            }
        };
        let core = Arc::new(Rr3Esp32AudioCore {
            path: self.path.clone(),
            baud: self.baud,
            channels: cfg.channels,
            ring: Mutex::new(Rr3AudioRing::rr3_ring_new(RR3_ESP32_RING_CAP_SAMPLES)),
            stop: AtomicBool::new(false),
        });
        let reader_core = Arc::clone(&core);
        let reader = std::thread::Builder::new()
            .name("rr3-hal-esp32-audio".into())
            .spawn(move || rr3_esp32_reader_main(reader_core, conn))
            .map_err(|e| {
                eprintln!("rr-hal-esp32-audio: 读线程起建失败: {e}");
                Rr3HalError::Busy
            })?;
        *self.inner.lock().unwrap_or_else(|p| p.into_inner()) = Some(Rr3Esp32AudioStream {
            core,
            reader: Some(reader),
        });
        Ok(())
    }

    fn read(&mut self, buf: &mut [i16]) -> Result<usize, Rr3HalError> {
        let guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let Some(st) = guard.as_ref() else {
            return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
                "esp32 源 read 前必须 open",
            )));
        };
        let mut ring = st.core.ring.lock().unwrap_or_else(|p| p.into_inner());
        Ok(ring.rr3_ring_take(buf, st.core.channels as usize))
    }
}

impl Drop for Rr3Esp32UartAudioSource {
    fn drop(&mut self) {
        if let Some(stream) = self.inner.lock().unwrap_or_else(|p| p.into_inner()).take() {
            Self::rr3_close_stream(stream);
        }
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rr_ipc::{opcode, rr3_base64_encode, rr3_body_encode, Rr3Frame, Rr3Header};

    /// 板面音频流事实：16k/双声道/s16le。
    fn board_cfg() -> Rr3AudioCfg {
        Rr3AudioCfg {
            sample_rate_hz: RR3_ESP32_AUDIO_RATE,
            channels: RR3_ESP32_AUDIO_CHANNELS,
            bit_depth: 16,
        }
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
        assert!(rr3_esp32_audio_cfg_check(board_cfg()).is_ok());
        // 单声道缺省配置（modules/rr3-sense/config.toml 现值）必须被拒——
        // 误吃 esp32 档在探活期点名，不静默出交织垃圾
        let mono = Rr3AudioCfg {
            sample_rate_hz: 16000,
            channels: 1,
            bit_depth: 16,
        };
        let err = rr3_esp32_audio_cfg_check(mono).unwrap_err();
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
        assert!(rr3_esp32_audio_cfg_check(fast).is_err());
        let wide = Rr3AudioCfg {
            sample_rate_hz: 16000,
            channels: 2,
            bit_depth: 24,
        };
        assert!(rr3_esp32_audio_cfg_check(wide).is_err());
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
        let chunk =
            rr3_esp32_parse_audio_event(&frame.header, &frame.payload).expect("板形帧必须全过");
        assert_eq!(chunk.rate, 16000);
        assert_eq!(chunk.samples, pcm);

        // dst 门：非宿主位形拒收
        let other = board_like_frame(8, &pcm, 9);
        assert!(
            rr3_esp32_parse_audio_event(&other.header, &other.payload).is_err(),
            "dst=9 必须被拒"
        );
    }
}
