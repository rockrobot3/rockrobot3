//! I2S1 TX 真实 PCM 播放面（W4-2a）：WAV/裸 PCM → 有界队列 → 立体声分块写。
//!
//! 与 `speaker.rs`（W4-1 验收提示音）互斥：两者都独占 I2S1，同开在编译期报错。
//! 只在 `--features tts` 编译；默认外设底座固件零行为变化。
//!
//! 半双工：播放期置 `st.tts_gate`，mic 线据此停攒上行分片（本板扬声器回授不
//! 进云端 VAD）；门由 [`PlayGuard`] 的 Drop 收尾，错误路径也不会漏清。
//!
//! 心跳：播放线程空转每 1 s、播放中每写完一块 PCM 续 `audio-out`；卡在 I2S
//! 写里＝DMA 不排空，由自治监督壳判死（W3-3 语义）。
//!
//! 采样率固定 16 kHz（与采集链、云 TTS `sample_rate=16000` 同源）；回包率不符
//! 显式失败，不在板侧做重采样。

use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use esp_idf_hal::gpio::{AnyIOPin, InputPin, OutputPin};
use esp_idf_hal::i2s::config::{DataBitWidth, StdConfig};
use esp_idf_hal::i2s::{I2sDriver, I2sTx, I2S1};
use esp_idf_hal::peripheral::Peripheral;
use esp_idf_sys::TickType_t;
use rr_audio_mech::{rr3_pcm_ms, rr3_pcm_stereo, rr3_wav_pcm};
use rr_autonomy::Rr3Task;

use crate::state::{lock_bus, Bus};

#[cfg(feature = "spk-tone")]
compile_error!("tts 与 spk-tone 互斥：两者都独占 I2S1（分开烧录/验收）");

/// 播放采样率（云 TTS 请求值与采集链同源）。
const PLAY_RATE_HZ: u32 = 16_000;
/// 首尾淡化帧数（10 ms @16k，消功放上/下电爆音）。
const PLAY_FADE_FRAMES: usize = 160;
/// 分块帧数（1024 帧 = 64 ms @16k，立体声 4 KB——内存与心跳粒度折中）。
const PLAY_CHUNK_FRAMES: usize = 1024;
/// 播放队列容量（条；满即拒，不阻塞 HTTP/云线程）。
const PLAY_Q_CAP: usize = 2;
/// 播放线程栈（与 mic/控制面同口径的 16KB 运行下限）。
const PLAY_STACK: usize = 16_384;
/// I2S 写阻塞上限（portMAX_DELAY 同义）。
const PLAY_TIMEOUT: TickType_t = u32::MAX;
/// 空转心跳周期。
const PLAY_IDLE_MS: u64 = 1_000;

/// 播放队列发送端（未装配 = None；锁只护取端，`try_send` 不阻塞）。
static PLAY_TX: Mutex<Option<SyncSender<Vec<u8>>>> = Mutex::new(None);

/// 起常驻播放线程（I2S1 引脚由调用方按芯片档传入）。
pub(crate) fn spawn_player(
    i2s1: impl Peripheral<P = I2S1> + Send + 'static,
    bclk: impl Peripheral<P = impl InputPin + OutputPin> + Send + 'static,
    dout: impl Peripheral<P = impl OutputPin> + Send + 'static,
    ws: impl Peripheral<P = impl InputPin + OutputPin> + Send + 'static,
    bus: Arc<Mutex<Bus>>,
) {
    let (tx, rx) = sync_channel(PLAY_Q_CAP);
    let _ = PLAY_TX.lock().map(|mut slot| *slot = Some(tx));
    // 起线程失败＝接收端随闭包一起丢弃：`rr3_play` 的 `try_send` 会如实回
    // "播放线程已退出"；此处只点名。
    let _ = thread::Builder::new()
        .name("spk-play".into())
        .stack_size(PLAY_STACK)
        .spawn(move || play_run(i2s1, bclk, dout, ws, rx, bus))
        .map_err(|e| println!("[spk] 起播放线程失败: {e:?}"));
}

/// 入队一条待播音频（WAV 或裸 16k PCM）：满/未装配/线程退出都如实回错，
/// 绝不阻塞调用方（HTTP 与云线程）。
pub(crate) fn rr3_play(pcm: Vec<u8>) -> Result<(), &'static str> {
    let Ok(slot) = PLAY_TX.lock() else {
        return Err("播放面锁中毒");
    };
    let Some(tx) = slot.as_ref() else {
        return Err("播放面未装配");
    };
    tx.try_send(pcm).map_err(|e| {
        if matches!(e, TrySendError::Full(_)) {
            "播放队列满"
        } else {
            "播放线程已退出"
        }
    })
}

/// 播放线程主体：I2S1 TX 装配 → 收一条播一条；空转续 `audio-out` 心跳。
fn play_run(
    i2s1: impl Peripheral<P = I2S1> + 'static,
    bclk: impl Peripheral<P = impl InputPin + OutputPin> + 'static,
    dout: impl Peripheral<P = impl OutputPin> + 'static,
    ws: impl Peripheral<P = impl InputPin + OutputPin> + 'static,
    rx: Receiver<Vec<u8>>,
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
    println!("[spk] 播放面起：I2S1 TX {PLAY_RATE_HZ}Hz 16bit stereo（W4-2；WAV/裸 PCM）");
    loop {
        // 发送端是进程级静态（`PLAY_TX`），断开只发生在进程收尾；空转/断连统一
        // 走 1 s 心跳，不做线程退出分支（避免把"队列暂时空"当停机）。
        let Ok(body) = rx.recv_timeout(Duration::from_millis(PLAY_IDLE_MS)) else {
            crate::autonomy::rr3_beat(Rr3Task::AudioOut);
            continue;
        };
        play_one(&mut tx, &bus, body);
    }
}

/// 播一条：容器识别 → 分块整形 → 写 I2S（逐块心跳）→ 时长对账。
fn play_one(tx: &mut I2sDriver<'static, I2sTx>, bus: &Arc<Mutex<Bus>>, body: Vec<u8>) {
    let Ok(wav) = rr3_wav_pcm(&body) else {
        return play_fail(bus, "WAV 容器不合法（缺 fmt/data 或非 16bit PCM）");
    };
    let (rate, channels, data) = wav.map_or((PLAY_RATE_HZ, 1u16, body.as_slice()), |w| {
        (w.sample_rate, w.channels, w.data)
    });
    if rate != PLAY_RATE_HZ {
        return play_fail(bus, "采样率非 16 kHz（板侧不重采样，请云侧按 16000 请求）");
    }
    let want_ms = rr3_pcm_ms(data.len(), channels, rate);
    let t0 = Instant::now();
    if let Err(e) = play_write(tx, bus, data, channels) {
        return play_fail(bus, &e);
    }
    let elapsed = t0.elapsed().as_millis() as u64;
    {
        let mut b = lock_bus(bus);
        b.st.tts_played = b.st.tts_played.saturating_add(1);
    }
    println!(
        "[spk] 播放完成 pcm={}B 应播={want_ms}ms 实耗={elapsed}ms 误差={}%",
        data.len(),
        err_pct(want_ms, elapsed)
    );
}

/// 分块写：首块淡入、末块淡出（中间块不淡化，避免块边界台阶）；每块一次心跳。
fn play_write(
    tx: &mut I2sDriver<'static, I2sTx>,
    bus: &Arc<Mutex<Bus>>,
    data: &[u8],
    channels: u16,
) -> Result<(), String> {
    let frame_bytes = channels as usize * 2;
    let frames = data.len() / frame_bytes;
    let mut out = Vec::with_capacity(PLAY_CHUNK_FRAMES * 4);
    lock_bus(bus).st.tts_gate = true;
    let _gate = PlayGuard { bus };
    let mut done = 0usize;
    while done < frames {
        let n = (frames - done).min(PLAY_CHUNK_FRAMES);
        let first = done == 0;
        let last = done + n == frames;
        let src = &data[done * frame_bytes..(done + n) * frame_bytes];
        let Ok(()) = rr3_pcm_stereo(
            src,
            channels,
            (first as usize) * PLAY_FADE_FRAMES,
            (last as usize) * PLAY_FADE_FRAMES,
            &mut out,
        ) else {
            return Err("PCM 整形失败（声道/帧长）".into());
        };
        let Ok(()) = tx.write_all(&out, PLAY_TIMEOUT) else {
            return Err("I2S 写失败（DMA 不排空？查功放供电/时钟）".into());
        };
        crate::autonomy::rr3_beat(Rr3Task::AudioOut);
        done += n;
    }
    Ok(())
}

/// 半双工门：播放期置位，Drop 清零（含 `?` 提前返回的所有错误路径）。
struct PlayGuard<'a> {
    bus: &'a Arc<Mutex<Bus>>,
}

impl Drop for PlayGuard<'_> {
    fn drop(&mut self) {
        lock_bus(self.bus).st.tts_gate = false;
    }
}

/// 播放失败：计数 + 点名（不 panic；板其余功能照常）。
fn play_fail(bus: &Arc<Mutex<Bus>>, msg: &str) {
    {
        let mut b = lock_bus(bus);
        b.st.tts_fail = b.st.tts_fail.saturating_add(1);
    }
    println!("[spk] 播放失败: {msg}");
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
