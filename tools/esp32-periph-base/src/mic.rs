//! 批D 采集冒烟 + 音频上行（INMP441 双麦 @ I2S0；0.0.5 增补轮五 全案 §4 批D）。
//!
//! 接线（README runbook 批D 节；两麦 L/R 时隙复用共一条总线）：
//! - BCLK → GPIO4（两麦并接）、WS → GPIO26（两麦并接）、SD → GPIO27（两麦并接）
//!   （**2026-10-20 迁移**：原 GPIO25 装配期全部音频为图案假阳性；根因为 rig 接触
//!   面而非 pad 损坏——「BCLK 桥测」法对 BCLK 自身失明（自采样语义），撤回误判
//!   过程见 main.rs 装配注）
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
//! → 有界 audio_q → 控制面 `drain_audio` 整帧写 UART2（1.5M）；OFF 只出统计。

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
    AUDIO_CHUNK_FRAMES, AUDIO_HOST_TIMEOUT_MS, AUDIO_Q_CAP, MIC_GAIN, MIC_SR_HZ, RR3_AUDIO_TOPIC,
    RR3_BIND_I2S_BCLK, RR3_BIND_I2S_SD, RR3_BIND_I2S_WS, RR3_HOST_TASK, RR3_PERIPH_TASK,
};
use crate::capture::{chunk_to_pcm, frame32_to_pair, push_bounded, Rr3DcBlocker};
use crate::link::rr3_frame_now_ns;
use crate::state::{lock_bus, now_ms, Bus};

/// 采集线程栈（8KB→16KB 运行下限钉死的教训在案，勿再调低；buf 4KB 在栈上）。
const MIC_THREAD_STACK: usize = 16384;
/// I2S 事务阻塞读超时（u32::MAX = portMAX_DELAY 同义，批B I2C_TIMEOUT 同款）。
const MIC_I2C_TIMEOUT: TickType_t = u32::MAX;
/// 单次 read 缓冲：4096B = 512 帧 × 2ch × 4B（≈32ms @16kHz；栈上安全）。
const MIC_BUF: usize = 4096;
/// 统计打印窗：64 次 read ≈ 2s 一行（16 次≈512ms 时日志在 1.5M 上与音频上行抢
/// 带宽、且人眼来不及看；2s 一行足够判"有没有数据/速率对不对"）。
const MIC_WINDOW_READS: usize = 64;
/// 诊断位图窗口：每 200 次 read（≈6.4s）看一次前 16B 原始字节。
const MIC_DUMP_READS: u32 = 200;

/// 一分片 → 整帧字节（批D③ Event 帧面，rr-bus `rr3_publish` 板侧等价形）：
/// 采样变换算术（逐通道减**本分片** DC → 24bit 截 16bit → 增益钳位 → s16le 双
/// 通道交错）在 capture 机制件 [`chunk_to_pcm`]（批N23 件三收编，策略量按参）；
/// 本函数只做组帧面——PCM → b64 → JSON 体（seq/format/rate/channels/ms/
/// pcm_b64，形状对齐 modules/rr3-sense `rr3_pcm_event_body`；topic 键并入体
/// ——rr3_publish 的 `body["topic"]` 同形）→ Event payload → 帧头（SEND /
/// trace_id=0 direct / src=板上 task 2 / dst=宿主 1）→ 整帧字节。
/// 帧构造失败回 None（调用方跳过不 panic 不冒领）。
fn audio_chunk_to_frame(chunk: &[i32], seq: u32, dc: &mut Rr3DcBlocker) -> Option<Vec<u8>> {
    let frames = chunk.len() / 2;
    if frames == 0 {
        return None;
    }
    let pcm = chunk_to_pcm(chunk, MIC_GAIN, dc);
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

/// 冒烟统计窗（[`MIC_WINDOW_READS`] 读一窗）：RMS/peak/声道等值率/组帧耗时累计，
/// 窗满打印并清零。实测采样率分母用本窗时长——窗重置连 `t0` 一起重置（首版
/// 分母用累计时长导致 rate 逐窗走低，真板实录）。
struct MicStats {
    reads: usize,
    frames: u64,
    l_sum: i64,
    r_sum: i64,
    l_peak: i32,
    r_peak: i32,
    equal: u64,
    build_us: u64,
    build_n: u32,
    t0: Instant,
}

impl MicStats {
    fn new() -> Self {
        Self {
            reads: 0,
            frames: 0,
            l_sum: 0,
            r_sum: 0,
            l_peak: 0,
            r_peak: 0,
            equal: 0,
            build_us: 0,
            build_n: 0,
            t0: Instant::now(),
        }
    }

    /// 一帧进窗（RMS/peak/等值率记账；分片/组帧计时由调用方走 [`MicStats::add_build`]）。
    fn accum(&mut self, l: i32, r: i32) {
        self.l_sum += (l as i64) * (l as i64);
        self.r_sum += (r as i64) * (r as i64);
        self.l_peak = self.l_peak.max(l.abs());
        self.r_peak = self.r_peak.max(r.abs());
        if l == r {
            self.equal += 1;
        }
    }

    fn add_build(&mut self, us: u64) {
        self.build_us += us;
        self.build_n += 1;
    }

    /// 窗满统计行 + 清零重开窗（rate 分母 = 本窗时长）。
    fn window_line(&mut self) {
        if self.reads < MIC_WINDOW_READS {
            return;
        }
        let dur = self.t0.elapsed().as_secs_f64();
        let rate = if dur > 0.1 {
            (self.frames as f64 / dur) as u32
        } else {
            0
        };
        let n_total = (self.frames as f64).max(1.0);
        let l_rms = ((self.l_sum as f64) / n_total).sqrt() as i32;
        let r_rms = ((self.r_sum as f64) / n_total).sqrt() as i32;
        let eq_pct = (self.equal * 100 / self.frames.max(1)) as u64;
        let build_ms = if self.build_n > 0 {
            self.build_us as f64 / self.build_n as f64 / 1000.0
        } else {
            0.0
        };
        let (l_peak, r_peak, build_n) = (self.l_peak, self.r_peak, self.build_n);
        println!(
            "[mic] {rate}Hz L rms={l_rms} peak={l_peak} | R rms={r_rms} peak={r_peak} | L==R {eq_pct}% | build={build_ms:.1}ms×{build_n}"
        );
        *self = Self::new();
    }
}

/// 诊断位图对账：每 ~[`MIC_DUMP_READS`] 读比一次前 16B 原始位图——**只有与上
/// 一窗完全相同才打印**（正常采集字节窗逐窗都在变 → 静默）。逐窗恒等（0x00E0FF00
/// 一类固定模式）= SD 三态残留（硬件断/虚接）或驱动停摆，此时每 6.4s 一行告警即够。
fn dump_watch(buf: &[u8], n: usize, last_dump: &mut [u8; 16], dump_count: u32) {
    if dump_count % MIC_DUMP_READS != 1 {
        return;
    }
    let w = 16.min(n);
    let now = &buf[..w];
    if now == &last_dump[..w] {
        let hex: String = now
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!("[mic] dump 逐窗不变（疑似 SD 虚接/停摆） {hex}");
    }
    last_dump[..w].copy_from_slice(now);
}

/// A9.3 宿主失联看门狗：见过心跳（host_ms != 0）才启用（旧宿主无 PING，混版
/// 部署保持常流语义），超窗即自动停流并清空待发队列——宿主进程被杀/整机断电
/// 后，板侧不再对着无人 UART 无穷推流。宿主回来重连即重新握手起流。
fn host_watchdog(bus: &Arc<Mutex<Bus>>, audio_on: bool, host_ms: u64) -> bool {
    if audio_on && host_ms != 0 && now_ms().saturating_sub(host_ms) > AUDIO_HOST_TIMEOUT_MS {
        let mut b = lock_bus(bus);
        b.st.audio_on = false;
        b.audio_q.clear();
        println!("[mic] 宿主心跳停发超 {AUDIO_HOST_TIMEOUT_MS}ms → 自动停流（空行心跳看门狗）");
        false
    } else {
        audio_on
    }
}

/// 分片就绪段：取 seq（wrapping 自增）→ 组帧（脱锁，b64+JSON ~几 ms）→ 有界
/// 入队（满丢**最旧** + 计 audio_dropped——背压观测面，与 recorder 端 seq 断档
/// 互为对账）。锁只护取号/入队两小段。
fn chunk_flush(
    chunk: &mut Vec<i32>,
    dc: &mut Rr3DcBlocker,
    bus: &Arc<Mutex<Bus>>,
    stats: &mut MicStats,
) {
    let seq = {
        let mut b = lock_bus(bus);
        let s = b.st.audio_seq;
        b.st.audio_seq = b.st.audio_seq.wrapping_add(1);
        s
    };
    let t_build = Instant::now();
    let frame = audio_chunk_to_frame(chunk, seq, dc);
    stats.add_build(t_build.elapsed().as_micros() as u64);
    if let Some(bytes) = frame {
        let mut b = lock_bus(bus);
        // 背压入队（capture 机制件）：满丢**最旧**，true = 发生丢弃 → 计观测计数
        if push_bounded(&mut b.audio_q, bytes, AUDIO_Q_CAP) {
            b.st.audio_dropped += 1;
        }
    }
    chunk.clear();
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
        .spawn(move || mic_run(bus, cfg, i2s0, bclk, din, ws))
        .expect("起 mic 冒烟线程失败");
}

/// I2S0 驱动装配 + 显式 `rx_enable`；任一步失败点名后回 `None`（线程退出，不 panic）。
fn mic_open(
    cfg: &StdConfig,
    i2s0: impl Peripheral<P = I2S0> + 'static,
    bclk: impl Peripheral<P = impl InputPin + OutputPin> + 'static,
    din: impl Peripheral<P = impl InputPin> + 'static,
    ws: impl Peripheral<P = impl InputPin + OutputPin> + 'static,
) -> Option<I2sDriver<'static, I2sRx>> {
    let mut d = match I2sDriver::new_std_rx(i2s0, cfg, bclk, din, None::<AnyIOPin>, ws) {
        Ok(d) => d,
        Err(e) => {
            println!("[mic] I2S0 驱动安装失败: {e:?}（查 4/26/27 占用/接线）");
            return None;
        }
    };
    // **enable 必须显式调**（esp-idf-hal 0.44.1 i2s.rs L803/L804 注释在案：enable
    // 后 BCLK/WS 才开始输出，INMP441 才出数据——真板首烧 read() 永挂无 Err 无统计行，
    // 根因即缺 enable）
    if let Err(e) = d.rx_enable() {
        println!("[mic] I2S0 rx_enable 失败: {e:?}");
        return None;
    }
    Some(d)
}

/// 采集线程主体：装配 → 逐 read 批处理（统计 / 诊断位图 / 音频分片）。
fn mic_run(
    bus: Arc<Mutex<Bus>>,
    cfg: StdConfig,
    i2s0: impl Peripheral<P = I2S0> + 'static,
    bclk: impl Peripheral<P = impl InputPin + OutputPin> + 'static,
    din: impl Peripheral<P = impl InputPin> + 'static,
    ws: impl Peripheral<P = impl InputPin + OutputPin> + 'static,
) {
    let Some(mut rx) = mic_open(&cfg, i2s0, bclk, din, ws) else {
        return;
    };
    println!(
        "[mic] I2S0 ✓ philips {MIC_SR_HZ}Hz 32bit stereo BCLK={RR3_BIND_I2S_BCLK} SD={RR3_BIND_I2S_SD} WS={RR3_BIND_I2S_WS}（INMP441 双麦 L/R 时隙；对账锚，board.rs）"
    );
    let mut buf = [0u8; MIC_BUF];
    let mut run = MicRun::new();
    loop {
        match rx.read(&mut buf, MIC_I2C_TIMEOUT) {
            Ok(n) if n >= 8 => run.batch(&buf, n, &bus),
            Ok(_) => {}
            Err(e) => {
                println!("[mic] I2S read 失败: {e:?}");
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// 跨 read 保持的采集运行态（统计窗 / 诊断位图 / 分片累积 / DC 阻断）。
struct MicRun {
    stats: MicStats,
    dump_count: u32,
    /// 上一次采样的前 16B（诊断位图对账面）：正常有数据的会话里字节窗一次一个样，
    /// 只有**卡死**（SD 三态残留/虚接/驱动停摆）才会逐窗完全相同——故只在"与上次
    /// 相同"时打印一次，正常采集全程静默。
    last_dump: [u8; 16],
    /// 批D③ 分片累积（交错 L,R 原始 24bit；堆上，16KB——栈 16KB 只放 buf 4KB）。
    chunk: Vec<i32>,
    /// 跨分片连续的 DC 阻断器（2026-10-31 S3 订正：逐片减均值会在 128ms 边界造
    /// 台阶，频谱 31/47/62/78Hz 拉链噪声压过语音——见 capture/mech.rs）。
    dc: Rr3DcBlocker,
}

impl MicRun {
    fn new() -> Self {
        Self {
            stats: MicStats::new(),
            dump_count: 0,
            last_dump: [0u8; 16],
            chunk: Vec::with_capacity(AUDIO_CHUNK_FRAMES * 2),
            dc: Rr3DcBlocker::new(),
        }
    }

    /// 一次有效 read：统计 + 诊断 + 音频开关（每 read 批读一次，不逐帧拿锁）。
    fn batch(&mut self, buf: &[u8], n: usize, bus: &Arc<Mutex<Bus>>) {
        self.stats.reads += 1;
        self.dump_count += 1;
        dump_watch(buf, n, &mut self.last_dump, self.dump_count);
        let (audio_on, host_ms) = {
            let b = lock_bus(bus);
            (b.st.audio_on, b.st.audio_host_ms)
        };
        let audio_on = host_watchdog(bus, audio_on, host_ms);
        if !audio_on {
            self.chunk.clear();
            self.dc.reset();
        }
        let nframes = n / 8;
        self.stats.frames += nframes as u64;
        self.frames(buf, nframes, audio_on, bus);
        self.stats.window_line();
    }

    /// 逐帧 32bit 大端 → 24bit 符号幅度（capture 机制件），统计累加；ON 时累积到
    /// 分片，满片即发。
    fn frames(&mut self, buf: &[u8], nframes: usize, audio_on: bool, bus: &Arc<Mutex<Bus>>) {
        for f in 0..nframes {
            let o = f * 8;
            let (l, r) = frame32_to_pair(&buf[o..o + 8]);
            self.stats.accum(l, r);
            if audio_on {
                self.chunk.push(l);
                self.chunk.push(r);
                if self.chunk.len() >= AUDIO_CHUNK_FRAMES * 2 {
                    chunk_flush(&mut self.chunk, &mut self.dc, bus, &mut self.stats);
                }
            }
        }
    }
}
