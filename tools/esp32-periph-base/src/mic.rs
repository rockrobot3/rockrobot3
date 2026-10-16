//! 批D 采集冒烟 + 音频上行（INMP441 双麦 @ I2S0；0.0.5 增补轮五 全案 §4 批D）。
//!
//! 接线（README runbook 批D 节；两麦 L/R 时隙复用共一条总线）：
//! - BCLK → GPIO25（两麦并接）、WS → GPIO26（两麦并接）、SD → GPIO27（两麦并接）
//! - 麦甲 L/R → GND（左时隙）、麦乙 L/R → 3V3（右时隙）——接错则双通道同相/单声道假象
//! - VDD → 3V3、GND → GND；INMP441 无 MCLK 需求（mclk 传 None）
//!
//! API 对表实录（esp-idf-hal 0.44.1 vendored 源，批B 纪律）：
//! - `I2sDriver::new_std_rx(i2s, config, bclk, din, mclk, ws)`（i2s/std.rs L825；
//!   引脚序 bclk/din/mclk/ws，mclk: Option，INMP441 传 None::<AnyIOPin>）
//! - `I2sDriver<I2sRx>::read(&mut self, buffer: &mut [u8], timeout: TickType_t)`
//!   （i2s.rs L894）；`I2C_TIMEOUT = u32::MAX`（portMAX_DELAY 同款）复用主面常量
//! - `StdConfig::philips(sample_rate, DataBitWidth)`（std.rs L66，Stereo 缺省）
//!   Bits32 档 slot 默认 msb_right=false（L574：<=16bit 才置右对齐）——标准
//!   Philips 32bit 帧，INMP441 24bit 有效数据**左对齐高位**，MSB first 大端
//!   线序 → i32::from_be_bytes 后算术右移 8 位即 24bit 符号扩展幅度。
//!
//! 冒烟判据（判据①前哨）：双通道均有幅度（非全零/非饱和）、拍手可见峰值抬升、
//! 左右通道非全等（全等 = L/R 脚接错或单麦虚接）。
//!
//! 批D③ 音频上行：`AUDIO ON` 后攒满 [`AUDIO_CHUNK_FRAMES`] 帧即组 Event 帧
//! （逐通道减本分片 DC → 24bit 截 16bit → s16le → b64 → JSON 体，topic 并入体）
//! → 有界 audio_q → 控制面 `drain_audio` 整帧写 UART2（921600）；OFF 只出统计。

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use esp_idf_hal::gpio::{AnyIOPin, InputPin, OutputPin};
// API 对表订正（专用门首跑实爆）：DataBitWidth 在 i2s::config 子模块（i2s.rs L267
// 嵌套于 config mod），I2S0 外设类型在 i2s 模块（peripherals.rs L41 `i2s0: i2s::I2S0`）。
use esp_idf_hal::i2s::config::{DataBitWidth, StdConfig};
use esp_idf_hal::i2s::{I2sDriver, I2sRx, I2S0};
use esp_idf_hal::peripheral::Peripheral;
use esp_idf_sys::TickType_t;
use rr_ipc::{
    opcode, rr3_base64_encode, rr3_body_encode, rr3_frame_to_bytes, Rr3Frame, Rr3Header,
    Rr3PayloadType,
};
use serde_json::json;

use crate::board::{
    AUDIO_CHUNK_FRAMES, AUDIO_Q_CAP, RR3_AUDIO_TOPIC, RR3_HOST_TASK, RR3_PERIPH_TASK,
};
use crate::link::rr3_frame_now_ns;
use crate::state::{lock_bus, Bus};

/// 采集线程栈（8KB→16KB 运行下限钉死的教训在案，勿再调低；buf 4KB 在栈上）。
const MIC_THREAD_STACK: usize = 16384;
/// I2S 事务阻塞读超时（u32::MAX = portMAX_DELAY 同义，批B I2C_TIMEOUT 同款）。
const MIC_I2C_TIMEOUT: TickType_t = u32::MAX;
/// 单次 read 缓冲：4096B = 512 帧 × 2ch × 4B（≈32ms @16kHz；栈上安全）。
const MIC_BUF: usize = 4096;
/// 统计打印窗：16 次 read ≈ 512ms 一行。
const MIC_WINDOW_READS: usize = 16;

/// 语音档采样率（判据① wav 口径；全案批D §4）。
pub const MIC_SR_HZ: u32 = 16000;

/// 一分片 → 整帧字节（批D③ Event 帧面，rr-bus `rr3_publish` 板侧等价形）：
/// 逐通道减**本分片** DC（INMP441 近零偏置，分片内均值即干净基线，无跨片状态）→
/// 24bit 截 16bit（>>8 保高位）→ s16le 双通道交错 → b64 → JSON 体（seq/format/
/// rate/channels/ms/pcm_b64，形状对齐 modules/rr3-sense `rr3_pcm_event_body`；
/// topic 键并入体——rr3_publish 的 `body["topic"]` 同形）→ Event payload →
/// 帧头（SEND / trace_id=0 direct / src=板上 task 2 / dst=宿主 1）→ 整帧字节。
/// 帧构造失败回 None（调用方跳过不 panic 不冒领）。
fn audio_chunk_to_frame(chunk: &[i32], seq: u32) -> Option<Vec<u8>> {
    let frames = chunk.len() / 2;
    if frames == 0 {
        return None;
    }
    let mut l_dc = 0i64;
    let mut r_dc = 0i64;
    for f in 0..frames {
        l_dc += chunk[f * 2] as i64;
        r_dc += chunk[f * 2 + 1] as i64;
    }
    l_dc /= frames as i64;
    r_dc /= frames as i64;
    let mut pcm = Vec::with_capacity(frames * 4);
    for f in 0..frames {
        // 24bit（±2^23）>> 8 落 16bit 满幅；i64 域减 DC 后回 i16 无截断
        let l = ((chunk[f * 2] as i64 - l_dc) >> 8) as i16;
        let r = ((chunk[f * 2 + 1] as i64 - r_dc) >> 8) as i16;
        pcm.extend_from_slice(&l.to_le_bytes());
        pcm.extend_from_slice(&r.to_le_bytes());
    }
    let body = json!({
        "topic": RR3_AUDIO_TOPIC,
        "seq": seq,
        "format": "s16le",
        "rate": MIC_SR_HZ,
        "channels": 2,
        "ms": AUDIO_CHUNK_FRAMES * 1000 / MIC_SR_HZ as usize,
        "pcm_b64": rr3_base64_encode(&pcm),
    });
    let payload = rr3_body_encode(Rr3PayloadType::Event, body.to_string().as_bytes());
    let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
    hdr.trace_id = 0; // direct 上行（无请求回路）
    hdr.timestamp_ns = rr3_frame_now_ns();
    hdr.src_task = RR3_PERIPH_TASK;
    hdr.dst_task = RR3_HOST_TASK;
    rr3_frame_to_bytes(&Rr3Frame {
        header: hdr,
        payload,
    })
    .ok()
}

/// 起 I2S0 双麦采集线程（引脚 move 进线程独占，无并发访问面；`bus` 供音频上行：
/// 读 `audio_on` 开关 + 组帧推 `audio_q`，统计面照旧走控制台 println）。
/// `+ Send`：外设 token 与引脚是 ZST，cross-thread move 安全（哨兵 Send 纪律同款，
/// 编译器实爆驱动补 bound，非 unsafe impl）。
pub fn spawn_mic_smoke(
    bus: Arc<Mutex<Bus>>,
    i2s0: impl Peripheral<P = I2S0> + Send + 'static,
    bclk: impl Peripheral<P = impl InputPin + OutputPin> + Send + 'static,
    din: impl Peripheral<P = impl InputPin> + Send + 'static,
    ws: impl Peripheral<P = impl InputPin + OutputPin> + Send + 'static,
) {
    let cfg = StdConfig::philips(MIC_SR_HZ, DataBitWidth::Bits32);
    thread::Builder::new()
        .name("mic-smoke".into())
        .stack_size(MIC_THREAD_STACK)
        .spawn(move || {
            let mut rx: I2sDriver<'static, I2sRx> =
                match I2sDriver::new_std_rx(i2s0, &cfg, bclk, din, None::<AnyIOPin>, ws) {
                    Ok(mut d) => {
                        // **enable 必须显式调**（esp-idf-hal 0.44.1 i2s.rs L803/L804 注释
                        // 在案：enable 后 BCLK/WS 才开始输出，INMP441 才出数据——真板
                        // 首烧 read() 永挂无 Err 无统计行，根因即缺 enable）
                        if let Err(e) = d.rx_enable() {
                            println!("[mic] I2S0 rx_enable 失败: {e:?}");
                            return;
                        }
                        d
                    }
                    Err(e) => {
                        println!("[mic] I2S0 驱动安装失败: {e:?}（查 25/26/27 占用/接线）");
                        return;
                    }
                };
            println!(
                "[mic] I2S0 ✓ philips {MIC_SR_HZ}Hz 32bit stereo BCLK=25 SD=27 WS=26（INMP441 双麦 L/R 时隙）"
            );
            let mut buf = [0u8; MIC_BUF];
            let mut reads = 0usize;
            let mut frames = 0u64;
            let mut l_sum = 0i64;
            let mut r_sum = 0i64;
            let mut l_peak = 0i32;
            let mut r_peak = 0i32;
            let mut equal = 0u64;
            let mut t0 = Instant::now();
            let mut dump_count = 0u32;
            // 组帧计时窗累计（排障定案面）：本窗各分片 audio_chunk_to_frame 耗时
            // 微秒和 + 次数，统计行 build= 均值——"组帧慢" vs "整帧写同步排水饿死"
            // 两论一测定案（b64/JSON 已被对表排除为 ms 级，此处真板实测收口）。
            let mut build_us = 0u64;
            let mut build_n = 0u32;
            // 批D③ 分片累积（交错 L,R 原始 24bit；堆上，16KB——栈 16KB 只放 buf 4KB）
            let mut chunk: Vec<i32> = Vec::with_capacity(AUDIO_CHUNK_FRAMES * 2);
            loop {
                match rx.read(&mut buf, MIC_I2C_TIMEOUT) {
                    Ok(n) if n >= 8 => {
                        reads += 1;
                        dump_count += 1;
                        // 诊断面：每 200 次 read（≈6.4s）打前 16B 原始位图——
                        // 全程重复 0x00E0FF00/0x1E0FF00 一类固定模式 = SD 三态残留
                        // （硬件断/虚接）；位图丰富随机 = 有数据，配置/时隙问题。
                        if dump_count % 200 == 1 {
                            let hex: String =
                                buf[..16].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
                            println!("[mic] dump {hex}");
                        }
                        // 音频开关每 read 批读一次（不逐帧拿锁）；关时清分片，
                        // ON 起始即整片（seq/DC 基线都从零起）
                        let audio_on = lock_bus(&bus).st.audio_on;
                        if !audio_on {
                            chunk.clear();
                        }
                        let nframes = n / 8;
                        frames += nframes as u64;
                        for f in 0..nframes {
                            let o = f * 8;
                            // 32bit 帧大端线序，24bit 有效左对齐：右移 8 位得符号幅度
                            let l =
                                i32::from_be_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]])
                                    >> 8;
                            let r = i32::from_be_bytes([
                                buf[o + 4],
                                buf[o + 5],
                                buf[o + 6],
                                buf[o + 7],
                            ]) >> 8;
                            l_sum += (l as i64) * (l as i64);
                            r_sum += (r as i64) * (r as i64);
                            l_peak = l_peak.max(l.abs());
                            r_peak = r_peak.max(r.abs());
                            if l == r {
                                equal += 1;
                            }
                            if audio_on {
                                chunk.push(l);
                                chunk.push(r);
                                if chunk.len() >= AUDIO_CHUNK_FRAMES * 2 {
                                    // 分片就绪：取 seq（wrapping 自增）→ 组帧 → 有界
                                    // 入队（满丢**最旧** + 计 audio_dropped——背压观测
                                    // 面，与 recorder 端 seq 断档互为对账）。锁只护
                                    // 取号/入队两小段，组帧（b64+JSON ~几 ms）脱锁。
                                    let seq = {
                                        let mut b = lock_bus(&bus);
                                        let s = b.st.audio_seq;
                                        b.st.audio_seq = b.st.audio_seq.wrapping_add(1);
                                        s
                                    };
                                    let t_build = Instant::now();
                                    let frame = audio_chunk_to_frame(&chunk, seq);
                                    build_us += t_build.elapsed().as_micros() as u64;
                                    build_n += 1;
                                    if let Some(bytes) = frame {
                                        let mut b = lock_bus(&bus);
                                        if b.audio_q.len() >= AUDIO_Q_CAP {
                                            b.audio_q.pop_front();
                                            b.st.audio_dropped += 1;
                                        }
                                        b.audio_q.push_back(bytes);
                                    }
                                    chunk.clear();
                                }
                            }
                        }
                        if reads >= MIC_WINDOW_READS {
                            // 实测采样率 = 本窗帧数 / 本窗时长（修复：窗重置后 t0 也要
                            // 重置——首版分母用累计时长导致 rate 逐窗走低，真板实录）
                            let dur = t0.elapsed().as_secs_f64();
                            let rate = if dur > 0.1 { (frames as f64 / dur) as u32 } else { 0 };
                            let n_total = (frames as f64).max(1.0);
                            let l_rms = ((l_sum as f64) / n_total).sqrt() as i32;
                            let r_rms = ((r_sum as f64) / n_total).sqrt() as i32;
                            let eq_pct = (equal * 100 / frames.max(1)) as u64;
                            let build_ms = if build_n > 0 {
                                build_us as f64 / build_n as f64 / 1000.0
                            } else {
                                0.0
                            };
                            println!(
                                "[mic] {rate}Hz L rms={l_rms} peak={l_peak} | R rms={r_rms} peak={r_peak} | L==R {eq_pct}% | build={build_ms:.1}ms×{build_n}"
                            );
                            reads = 0;
                            frames = 0;
                            l_sum = 0;
                            r_sum = 0;
                            l_peak = 0;
                            r_peak = 0;
                            equal = 0;
                            build_us = 0;
                            build_n = 0;
                            t0 = Instant::now();
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        println!("[mic] I2S read 失败: {e:?}");
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        })
        .expect("起 mic 冒烟线程失败");
}
