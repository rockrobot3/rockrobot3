//! I2S1 TX 真实 PCM 播放面（W4-2a → W7-b2 流式环）：源 PCM 边收边整形成立体声块，
//! 写入 16 KB 有界环，播放线程按 4 KB 块写 I2S——不再聚合整段 PCM Vec，也不再有
//! "2 条全量 PCM"的播放队列。
//!
//! 与 `speaker.rs`（W4-1 验收提示音）互斥：两者都独占 I2S1，同开在编译期报错。
//! 只在 `--features tts` 编译；默认外设底座固件零行为变化。
//!
//! 半双工：流开始即置 `st.tts_gate`，播完/中止清位（播放线程单点管理，含错误路径）。
//! 退出语义：生产者按环空位阻塞（TCP 背压），播放线程死亡时 `alive=false` 让生产者
//! 立刻失败，不无限等待。
//!
//! 心跳：播放线程空转每 1 s、播放中每写完一块续 `audio-out`；卡在 I2S 写里＝DMA
//! 不排空，由自治监督壳判死（W3-3 语义）。
//!
//! 采样率固定 16 kHz（与采集链、云 TTS `sample_rate=16000` 同源）；回包率不符
//! 显式失败，不在板侧做重采样。WAV/裸 PCM 的容器识别与全局淡入淡出走
//! [`rr_audio_mech::Rr3PcmStream`]（与整段整形逐位同源）。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use esp_idf_hal::gpio::{AnyIOPin, InputPin, OutputPin};
use esp_idf_hal::i2s::config::{DataBitWidth, StdConfig};
use esp_idf_hal::i2s::{I2sDriver, I2sTx, I2S1};
use esp_idf_hal::peripheral::Peripheral;
use esp_idf_sys::TickType_t;
use rr_audio_mech::{rr3_pcm_ms, rr3_wav_pcm, Rr3PcmStream};
use rr_autonomy::Rr3Task;

use crate::board::PLAY_IDLE_MS;
use crate::state::{lock_bus, Bus};

#[cfg(feature = "spk-tone")]
compile_error!("tts 与 spk-tone 互斥：两者都独占 I2S1（分开烧录/验收）");

/// 播放采样率（云 TTS 请求值与采集链同源）。
const PLAY_RATE_HZ: u32 = 16_000;
/// 首尾淡化帧数（10 ms @16k，消功放上/下电爆音）。
pub(crate) const PLAY_FADE_FRAMES: usize = 160;
/// 环容量（立体声 s16le 字节；16 KB = 128 ms @16k 双声道——ADR-0031 的 8–16 KB 档）。
const PLAY_RING_BYTES: usize = 16_384;
/// 播放线程单次 I2S 写块（4 KB = 64 ms @16k 双声道；栈上）。
const PLAY_CHUNK: usize = 4_096;
/// 生产者源分块（喂 `Rr3PcmStream` 的粒度；4 KB 栈上）。
const PLAY_SRC_CHUNK: usize = 4_096;
/// 等待上一条流播完的上限（`/say` 连发时串行化，不丢也不叠播）。
const PLAY_BUSY_MS: u64 = 30_000;
/// 播放线程栈（与 mic/控制面同口径的 16KB 运行下限）。
const PLAY_STACK: usize = 16_384;
/// I2S 写阻塞上限（portMAX_DELAY 同义）。
const PLAY_TIMEOUT: TickType_t = u32::MAX;

/// 播放环状态（生产者与播放线程共享；锁只护短临界区，不跨 I2S 写）。
struct Rr3PlayState {
    /// 立体声 s16le 待播字节（≤ [`PLAY_RING_BYTES`]）。
    bytes: VecDeque<u8>,
    /// 生产者已声明流结束。
    end: bool,
    /// 生产者中止（不记 `tts_played`，记 `tts_fail`）。
    abort: bool,
    /// 本条流应播时长（ms；对账用）。
    want_ms: u64,
    /// 本条流源字节数（点名用，不参与整形成本）。
    src_bytes: usize,
    /// 是否有一条流在收/播。
    running: bool,
    /// 播放线程是否存活（I2S 装配失败 = false，生产者立即失败）。
    alive: bool,
}

impl Rr3PlayState {
    fn new() -> Self {
        Self {
            bytes: VecDeque::with_capacity(PLAY_RING_BYTES),
            end: false,
            abort: false,
            want_ms: 0,
            src_bytes: 0,
            running: false,
            alive: false,
        }
    }

    /// 收尾/中止共用：清空并回到空闲态。
    fn finish(&mut self) {
        self.bytes.clear();
        self.end = false;
        self.abort = false;
        self.want_ms = 0;
        self.src_bytes = 0;
        self.running = false;
    }
}

/// 播放环槽（未装配 = None；克隆 Arc 后立即放锁）。
static PLAY_SLOT: Mutex<Option<Arc<Mutex<Rr3PlayState>>>> = Mutex::new(None);

/// 起常驻播放线程（I2S1 引脚由调用方按芯片档传入）。
pub(crate) fn spawn_player(
    i2s1: impl Peripheral<P = I2S1> + Send + 'static,
    bclk: impl Peripheral<P = impl InputPin + OutputPin> + Send + 'static,
    dout: impl Peripheral<P = impl OutputPin> + Send + 'static,
    ws: impl Peripheral<P = impl InputPin + OutputPin> + Send + 'static,
    bus: Arc<Mutex<Bus>>,
) {
    let state = Arc::new(Mutex::new(Rr3PlayState::new()));
    let _ = PLAY_SLOT.lock().map(|mut slot| *slot = Some(state.clone()));
    let _ = thread::Builder::new()
        .name("spk-play".into())
        .stack_size(PLAY_STACK)
        .spawn(move || play_run(i2s1, bclk, dout, ws, state, bus))
        .map_err(|e| println!("[spk] 起播放线程失败: {e:?}"));
}

/// 取环句柄（未装配/锁中毒都如实回错）。
fn play_state() -> Result<Arc<Mutex<Rr3PlayState>>, &'static str> {
    let slot = PLAY_SLOT.lock().map_err(|_| "播放面锁中毒")?;
    slot.clone().ok_or("播放面未装配")
}

/// 开一条流：等上一条播完（有界），复位环并置 running。生产者随后 feed/end。
pub(crate) fn rr3_play_begin() -> Result<(), &'static str> {
    let state = play_state()?;
    let t0 = Instant::now();
    loop {
        {
            let mut st = state.lock().map_err(|_| "播放面锁中毒")?;
            // 起播早于 I2S 装配完成是合法竞态；给 1 s 装配窗，逾时才判死。
            if !st.alive && t0.elapsed() < Duration::from_millis(1_000) {
                drop(st);
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            if !st.alive {
                return Err("播放线程未存活");
            }
            if !st.running {
                st.finish();
                st.running = true;
                return Ok(());
            }
        }
        if t0.elapsed().as_millis() as u64 > PLAY_BUSY_MS {
            return Err("播放面忙（上一条流未播完）");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// 生产者塞一块**已成形的立体声**字节：环满时阻塞（播放线程按 32 KB/s 消费）。
pub(crate) fn rr3_play_feed(data: &[u8]) -> Result<(), &'static str> {
    let state = play_state()?;
    let mut data = data;
    while !data.is_empty() {
        let n = {
            let mut st = state.lock().map_err(|_| "播放面锁中毒")?;
            if !st.alive {
                return Err("播放线程已退出");
            }
            if st.abort {
                return Err("播放已中止");
            }
            let room = PLAY_RING_BYTES.saturating_sub(st.bytes.len());
            if room == 0 {
                0
            } else {
                let n = room.min(data.len());
                st.bytes.extend(data[..n].iter().copied());
                n
            }
        };
        if n == 0 {
            thread::sleep(Duration::from_millis(2));
            continue;
        }
        data = &data[n..];
    }
    Ok(())
}

/// 声明流结束：播放线程排空环后计 `tts_played` 并清门。
pub(crate) fn rr3_play_end(want_ms: u64, src_bytes: usize) -> Result<(), &'static str> {
    let state = play_state()?;
    let mut st = state.lock().map_err(|_| "播放面锁中毒")?;
    st.end = true;
    st.want_ms = want_ms;
    st.src_bytes = src_bytes;
    Ok(())
}

/// 中止当前流：清环、置 abort；播放线程计 `tts_fail`，不冒充播完。
pub(crate) fn rr3_play_abort() {
    let Ok(state) = play_state() else {
        return;
    };
    let Ok(mut st) = state.lock() else {
        return;
    };
    st.abort = true;
    st.end = true;
    st.bytes.clear();
}

/// 整段 PCM（WAV/裸 16k）→ 流式环：`/api/board/play` 与云端 `Play` 作业共用。
///
/// 源 Vec 由云 worker 持有（HTTP 已收全的短消息路径）；播放本身走 16 KB 环，
/// 不再把整段拷进播放队列。失败路径中止并点名，不残留半条流。
pub(crate) fn rr3_play_stream(src: &[u8]) -> Result<(), &'static str> {
    crate::mem::rr3_heap_mark("playout", "start");
    let rc = rr3_play_feed_all(src);
    if rc.is_err() {
        rr3_play_abort();
        crate::mem::rr3_heap_mark("playout", "fail");
    }
    rc
}

/// 整段源字节的流式整形（WAV 头剥离、声道/采样率门、全局淡入淡出）。
fn rr3_play_feed_all(src: &[u8]) -> Result<(), &'static str> {
    let wav = rr3_wav_pcm(src).map_err(|_| "WAV 容器不合法（缺 fmt/data 或非 16bit PCM）")?;
    let (rate, channels, data) = wav.map_or((PLAY_RATE_HZ, 1u16, src), |w| {
        (w.sample_rate, w.channels, w.data)
    });
    if rate != PLAY_RATE_HZ {
        return Err("采样率非 16 kHz（板侧不重采样，请云侧按 16000 请求）");
    }
    if data.is_empty() {
        return Err("PCM 为空");
    }
    let want_ms = rr3_pcm_ms(data.len(), channels, PLAY_RATE_HZ);
    let mut stream = Rr3PcmStream::rr3_new(channels, Some(data.len()), PLAY_FADE_FRAMES)?;
    let mut out = Vec::with_capacity(PLAY_SRC_CHUNK * 2);
    rr3_play_begin()?;
    for chunk in data.chunks(PLAY_SRC_CHUNK) {
        out.clear();
        stream.rr3_feed(chunk, &mut out)?;
        rr3_play_feed(&out)?;
    }
    stream.rr3_finish()?;
    rr3_play_end(want_ms, data.len())
}

/// 播放线程主体：I2S1 TX 装配 → 消费环；空转续 `audio-out` 心跳。
fn play_run(
    i2s1: impl Peripheral<P = I2S1> + 'static,
    bclk: impl Peripheral<P = impl InputPin + OutputPin> + 'static,
    dout: impl Peripheral<P = impl OutputPin> + 'static,
    ws: impl Peripheral<P = impl InputPin + OutputPin> + 'static,
    state: Arc<Mutex<Rr3PlayState>>,
    bus: Arc<Mutex<Bus>>,
) {
    let cfg = StdConfig::philips(PLAY_RATE_HZ, DataBitWidth::Bits16);
    let Ok(mut tx) =
        I2sDriver::new_std_tx(i2s1, &cfg, bclk, dout, None::<AnyIOPin>, ws).map_err(|e| {
            println!("[spk] I2S1 TX 装配失败: {e:?}（播放面缺席，查功放三线与供电/共地）")
        })
    else {
        return;
    };
    if tx.tx_enable().is_err() {
        println!("[spk] I2S1 tx_enable 失败（播放面缺席）");
        return;
    }
    {
        let Ok(mut st) = state.lock() else { return };
        st.alive = true;
    }
    println!(
        "[spk] 播放面起：I2S1 TX {PLAY_RATE_HZ}Hz 16bit stereo ring={PLAY_RING_BYTES}B（W7-b2 流式）"
    );
    let mut buf = [0u8; PLAY_CHUNK];
    let mut t0: Option<Instant> = None;
    loop {
        let Some(action) = play_take(&state, &mut buf) else {
            return;
        };
        match action {
            Action::Idle { running } => {
                crate::autonomy::rr3_beat(Rr3Task::AudioOut);
                let ms = if running { 5 } else { PLAY_IDLE_MS };
                thread::sleep(Duration::from_millis(ms));
            }
            Action::Data(n) => {
                if play_write(&mut tx, &bus, &state, &buf[..n]) {
                    t0.get_or_insert_with(Instant::now);
                } else {
                    t0 = None;
                }
            }
            Action::Done {
                abort,
                want_ms,
                src_bytes,
            } => play_done(&bus, abort, want_ms, src_bytes, &mut t0),
        }
    }
}

/// 单拍取数（锁内只做判态/搬块，I2S 写在锁外）；None = 播放线程退出。
fn play_take(state: &Arc<Mutex<Rr3PlayState>>, buf: &mut [u8; PLAY_CHUNK]) -> Option<Action> {
    let Ok(mut st) = state.lock() else {
        return None;
    };
    if !st.running {
        return Some(Action::Idle { running: false });
    }
    if !st.bytes.is_empty() {
        let n = st.bytes.len().min(buf.len());
        for (dst, src) in buf[..n].iter_mut().zip(st.bytes.drain(..n)) {
            *dst = src;
        }
        return Some(Action::Data(n));
    }
    if st.end {
        let action = Action::Done {
            abort: st.abort,
            want_ms: st.want_ms,
            src_bytes: st.src_bytes,
        };
        st.finish();
        return Some(action);
    }
    Some(Action::Idle { running: true })
}

/// 写一块到 I2S；失败计数并复位本条流，返回 false（不 panic）。
fn play_write(
    tx: &mut I2sDriver<'static, I2sTx>,
    bus: &Arc<Mutex<Bus>>,
    state: &Arc<Mutex<Rr3PlayState>>,
    chunk: &[u8],
) -> bool {
    lock_bus(bus).st.tts_gate = true;
    if tx.write_all(chunk, PLAY_TIMEOUT).is_ok() {
        crate::autonomy::rr3_beat(Rr3Task::AudioOut);
        return true;
    }
    crate::mem::rr3_heap_mark("playout", "fail");
    println!("[spk] I2S 写失败（DMA 不排空？查功放供电/时钟）——中止本条流");
    {
        let mut b = lock_bus(bus);
        b.st.tts_fail = b.st.tts_fail.saturating_add(1);
        b.st.tts_gate = false;
    }
    if let Ok(mut st) = state.lock() {
        st.finish();
    }
    false
}

/// 流结束收尾：清半双工门，按 abort/空流计 fail，否则计 played 并对账时长。
fn play_done(
    bus: &Arc<Mutex<Bus>>,
    abort: bool,
    want_ms: u64,
    src_bytes: usize,
    t0: &mut Option<Instant>,
) {
    lock_bus(bus).st.tts_gate = false;
    let elapsed = t0.take().map_or(0, |t| t.elapsed().as_millis() as u64);
    if abort || src_bytes == 0 {
        {
            let mut b = lock_bus(bus);
            b.st.tts_fail = b.st.tts_fail.saturating_add(1);
        }
        crate::mem::rr3_heap_mark("playout", "fail");
        println!(
            "[spk] 播放{}: src={src_bytes}B 应播={want_ms}ms 实耗={elapsed}ms",
            if abort { "中止" } else { "空流" }
        );
        return;
    }
    {
        let mut b = lock_bus(bus);
        b.st.tts_played = b.st.tts_played.saturating_add(1);
    }
    crate::mem::rr3_heap_mark("playout", "done");
    println!(
        "[spk] 播放完成 pcm={src_bytes}B 应播={want_ms}ms 实耗={elapsed}ms 误差={}%",
        err_pct(want_ms, elapsed)
    );
}

/// 播放线程单拍动作（锁内只做判态/取数，I2S 写在锁外）。
enum Action {
    Idle {
        running: bool,
    },
    Data(usize),
    Done {
        abort: bool,
        want_ms: u64,
        src_bytes: usize,
    },
}

/// 时长误差百分比（整数；`want=0` 回 0，不造伪精度）。
fn err_pct(want: u64, got: u64) -> u64 {
    want.abs_diff(got) * 100 / want.max(1) * (want > 0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn err_pct_is_zero_safe_and_rounds_down() {
        assert_eq!(err_pct(1000, 1000), 0);
        assert_eq!(err_pct(1000, 1050), 5);
        assert_eq!(err_pct(1000, 950), 5);
        assert_eq!(err_pct(0, 123), 0);
    }
}
