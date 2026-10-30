//! ESP32-UART 板面音频源后端（A 批 c3）：板面 INMP441 双麦流 → 宿主环形缓冲 →
//! [`Rr3AudioSource`]（`rr3_hal_audio_source()` esp32 档）。
//!
//! 链路：板面 mic.rs 分片组帧（16kHz 双声道 s16le，128ms/8192B 一片，A-IPC
//! Event 帧，topic `rr3-sense/audio/pcm`）→ UART2（[`crate::esp32_uart::
//! RR3_HAL_PERIPH_UART_ENV`] 指定设备，[`crate::esp32_uart::
//! RR3_HAL_PERIPH_BAUD_ENV`] 波特率——板固件已钉 `UART2_BAUD_HZ`（2026-10-30 起
//! 2M），须显式同档）→ 本件
//! 收流：开口 → `rr_ipc::audio_session::handshake`（settle/防御 OFF/ON 应答
//! 扫描，与 rr3-audio-rec 同一机制单点）→ 读线程大块裸读（判据② 节奏）→
//! [`FrameAssembler`](rr_ipc::FrameAssembler) 拆帧 → 四道门解析 → 环形缓冲
//! （~1s，溢出丢最旧整帧 + 计数）→ `read()` 整声道帧出样。
//!
//! 纪律：
//! - flock 单消费者（`Rr3UartConn` 开口即跨进程排他）：**宿主侧唯一持 conn 者
//!   = 本件**，板外设命令下行唯一通路 = 本件命令旁路（B 批 b1：语义命令入
//!   有界队列 → 读线程单写者逐帧写出，帧构造走
//!   [`crate::esp32_uart::rr3_bridge_request_frame`] 桥同形单点）；rr3-audio-rec
//!   等第二 opener 仍 fail-closed 点名，不是静默截流；A 链回放恒宿主扬声器
//!   （backend.rs sink 侧 esp32 档显式落占位），互斥面天然不成立。
//! - 旁路 fire-and-forget：板侧执行**前**回成功（status.md 对表实录），回执由
//!   板帧分类面放行记账（不污音频四道门的 err_streak）；队列满丢最旧 + 计数
//!   （板侧 audio_q 同款背压口径）。
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
//! cfg 宿主档门（批N8 件三，与 [`crate::esp32_uart`] 同口径 any(linux, macos)）：
//! 本模块是 Pi 侧桥消费件（rr-ipc 的 UART/会话机制面同在宿主档门内），espidf
//! 档不载——其余平台编译即错，显式优于隐式。

use std::collections::VecDeque;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rr_hal_api::{Rr3AudioCfg, Rr3AudioSource, Rr3ErrText, Rr3HalError};
use rr_ipc::audio_session::{
    handshake, HANDSHAKE_RETRY_PAUSE, HANDSHAKE_TRIES, READ_BLOCK, READ_TIMEOUT_MS,
    RESYNC_ZERO_BAIL,
};
use rr_ipc::{
    rr3_base64_decode, rr3_body_split, FrameAssembler, Rr3FrameBuf, Rr3Header, Rr3PayloadType,
    Rr3TransportConn, Rr3UartConn,
};
use serde_json::Value;

// ---------------------------------------------------------------------------
// 两端帧头约定（板不依赖 rr-core，板/宿主常量自证——与 tools/rr3-audio-rec
// 同纪律；与 crate::esp32_uart 的同值锚定测试同一防静默分叉口径）
// ---------------------------------------------------------------------------

/// 宿主 task 位形（帧头 dst 门）：真值单点在 rr-periph-proto（ADR-0024），本地
/// 别名保既有消费面与测试路径。
pub(crate) use rr_periph_proto::consts::RR3_TASK_HOST as RR3_HOST_TASK;
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

/// 命令旁路队列深度（策略量）：反射环 onset 驱动天然低频（VAD min_speech +
/// silence_tail 下限节流），2 深吸收偶发连发；满丢最旧保最新意图（板侧
/// audio_q 同款背压口径）。
const RR3_AUDIO_CMD_CAP: usize = 2;

// ---------------------------------------------------------------------------
// 板外设命令旁路（B 批 b1）：flock 单消费者纪律下的命令下行唯一通路——
// 音频源持有 conn，舵机命令不再另开连接（跨进程第二 opener 必被拒），改为
// 语义命令入有界队列、读线程单写者逐帧写出。帧构造单点在
// [`crate::esp32_uart::rr3_bridge_request_frame`]（与舵机/显示桥同形零重复）。
// ---------------------------------------------------------------------------

// 板外设语义命令 + 帧构造：真值单点在 rr-periph-proto（ADR-0024，宿主与固件
// 双向复用），本地 re-export 保既有 `rr_hal::esp32audio::Rr3PeriphCmd` 路径。
pub use rr_periph_proto::cmd::Rr3PeriphCmd;
pub(crate) use rr_periph_proto::cmd::rr3_esp32_cmd_frame;
use rr_periph_proto::cmd::Rr3BoardFrame;

/// 命令旁路盒（Mutex 内纯数据）：有界、满丢最旧 + 溢丢计数（板侧 audio_q
/// 同款背压口径）。产面归构造方（rr3-sense 反射环），耗面归读线程独占。
struct Rr3AudioCmdShared {
    deque: Mutex<VecDeque<Rr3PeriphCmd>>,
    cap: usize,
    dropped: AtomicU64,
}

/// 命令旁路产面（Arc 克隆廉价；push 永不阻塞）。
#[derive(Clone)]
pub struct Rr3AudioCmdProducer {
    shared: Arc<Rr3AudioCmdShared>,
}

impl Rr3AudioCmdProducer {
    /// 推一条命令（满 = 丢最旧 + 计数，永不阻塞——反射环节拍不容背压等待）。
    pub fn rr3_cmd_push(&self, cmd: Rr3PeriphCmd) {
        let mut q = self.shared.deque.lock().unwrap_or_else(|p| p.into_inner());
        if q.len() >= self.shared.cap {
            q.pop_front();
            self.shared.dropped.fetch_add(1, Ordering::Release);
        }
        q.push_back(cmd);
    }

    /// 溢丢计数（记账对账面；>0 = 反射环产速超过排水能力，点名可见）。
    pub fn rr3_cmd_dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Acquire)
    }
}

/// 命令旁路耗面（读线程独占——单写者纪律：conn 的全部写收拢读线程）。
#[derive(Clone)]
struct Rr3AudioCmdConsumer {
    shared: Arc<Rr3AudioCmdShared>,
}

impl Rr3AudioCmdConsumer {
    /// 弹一条（空回 None）——逐条弹出制：写失败可回插队首不丢命令。
    fn rr3_cmd_pop(&self) -> Option<Rr3PeriphCmd> {
        let mut q = self.shared.deque.lock().unwrap_or_else(|p| p.into_inner());
        q.pop_front()
    }

    /// 回插队首（io 失败保命：命令不丢，重连后补写；不计溢丢账）。
    fn rr3_cmd_push_front(&self, cmd: Rr3PeriphCmd) {
        let mut q = self.shared.deque.lock().unwrap_or_else(|p| p.into_inner());
        q.push_front(cmd);
    }
}

/// 产/耗面对（cap 见 [`RR3_AUDIO_CMD_CAP`]）。
fn rr3_audio_cmd_pair() -> (Rr3AudioCmdProducer, Rr3AudioCmdConsumer) {
    let shared = Arc::new(Rr3AudioCmdShared {
        deque: Mutex::new(VecDeque::with_capacity(RR3_AUDIO_CMD_CAP)),
        cap: RR3_AUDIO_CMD_CAP,
        dropped: AtomicU64::new(0),
    });
    (
        Rr3AudioCmdProducer {
            shared: Arc::clone(&shared),
        },
        Rr3AudioCmdConsumer { shared },
    )
}

/// 板面入帧分类（b1）：Response（命令旁路的板侧应答）放行记账，其余走音频
/// 四道门——旁路回执不再污 err_streak（帧界损坏记账语义保住）。协议级判别
/// （dst 门 + payload 前缀门 + Response 解析）在 rr-periph-proto；音频载荷门序
/// 与解码留在本件消费面。
#[derive(Debug)]
enum Rr3BoardFrameIn {
    /// 音频 Event（四道门全过）
    Audio(Rr3Esp32AudioChunk),
    /// 板侧请求应答（执行**前**回成功，status.md 对表实录）：id 回显 + 业务
    /// 结果——ok 形状即 pre-execution ack。
    Reply { id: u64, err: Option<String> },
}

fn rr3_esp32_classify_board_frame(
    hdr: &Rr3Header,
    payload: &[u8],
) -> Result<Rr3BoardFrameIn, String> {
    match rr_periph_proto::cmd::rr3_board_frame_classify(hdr, payload)? {
        Rr3BoardFrame::Reply { id, err } => Ok(Rr3BoardFrameIn::Reply { id, err }),
        Rr3BoardFrame::Event => {
            rr3_esp32_parse_audio_event(hdr, payload).map(Rr3BoardFrameIn::Audio)
        }
    }
}

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
#[derive(Debug)]
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
fn rr3_esp32_audio_open(path: &Path, baud: u32) -> Result<Rr3UartConn, String> {
    let mut conn =
        Rr3UartConn::open_with_baud(path, baud).map_err(|e| format!("open 失败: {e}"))?;
    handshake(&mut conn).map_err(|e| format!("握手失败: {e}"))?;
    Ok(conn)
}

// ---------------------------------------------------------------------------
// 运行期件（读线程 + 环形缓冲 + trait 面）
// ---------------------------------------------------------------------------

/// 读线程共享核：连接参数 + 环形缓冲 + 停旗 + 命令旁路耗面（None = 无旁路
/// 构造形态；open 时自源克隆注入）。
struct Rr3Esp32AudioCore {
    path: PathBuf,
    baud: u32,
    channels: u8,
    ring: Mutex<Rr3AudioRing>,
    stop: AtomicBool,
    cmd: Option<Rr3AudioCmdConsumer>,
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

/// 取证 tee 开关 env（批N12 件二，批N10 件二定案"原始流 tee 双录板面/宿主读面"
/// 的**宿主读面**半边）：值＝落盘文件路径，设了即开——读线程把宿主从串口读到的
/// **原始字节流**（拆帧前镜像，含帧间残渣/线噪——批N10 件二病灶取证层）逐字节
/// 追加写入。默认关（未设/空＝无 fd 无文件）。板面半边由取证会话并行走板面
/// USB 口外部录，不进本件（flock 单消费者，宿主 fd 唯一，第二 opener fail-closed）。
pub(crate) const RR3_HAL_ESP32_TEE_ENV: &str = "RR3_HAL_ESP32_TEE";

/// tee 开口（读线程起点一次；断流重连**续写同一文件**——一次取证会话内的重连
/// 是同一原始流的中断续传，换文件会撕账）。env 未设/空 = `None`：关闭态零落盘
/// 由**构造面**保证（不 open 即无 fd 无文件，非运行期判断）。打开失败 = tee
/// 缺席（打一条不挡链路——tee 是观测面，不是链路面）。
fn rr3_tee_open() -> Option<std::fs::File> {
    let path = std::env::var(RR3_HAL_ESP32_TEE_ENV).ok()?;
    if path.is_empty() {
        return None;
    }
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(f) => {
            eprintln!("rr-hal-esp32-audio: 取证 tee 开（原始流追加 → {path}）");
            Some(f)
        }
        Err(e) => {
            eprintln!("rr-hal-esp32-audio: 取证 tee 打开失败（tee 缺席，链路不受影响）: {e}");
            None
        }
    }
}

/// tee 写入面（单测锚点）：`sink = None`（tee 关）即 no-op 零副作用；打开时逐
/// 字节镜像追加。写失败 = 观测面降级（打一条后停 tee），**绝不反噬链路面**——
/// 音频链 fail-closed 纪律管的是链路判定，观测面降级不构成链路故障。
fn rr3_tee_write(sink: &mut Option<std::fs::File>, bytes: &[u8]) {
    let Some(f) = sink.as_mut() else { return };
    use std::io::Write;
    if let Err(e) = f.write_all(bytes).and_then(|()| f.flush()) {
        eprintln!("rr-hal-esp32-audio: tee 写失败（tee 停用，链路不受影响）: {e}");
        *sink = None;
    }
}

/// 读线程主体：会话内大块裸读 → 拆帧 → 四道门 → 入环；断流/错会话就地转
/// 重连。收兵（stop 旗）时板面 `AUDIO OFF`（防残流，conn 随作用域释放 →
/// flock 退账）。
fn rr3_esp32_reader_main(core: Arc<Rr3Esp32AudioCore>, first: Rr3UartConn) {
    // 取证 tee（批N12 件二）：线程级开口一次，重连续写同一文件（见 [`rr3_tee_open`]）
    let mut tee = rr3_tee_open();
    let mut conn: Option<Rr3UartConn> = Some(first);
    let mut logged_ready = false;
    let mut logged_overflow = false;
    // 命令旁路请求 id 计数（读线程局部；fire-and-forget 面只服务回执对账日志，
    // 无跨会话唯一性要求）
    let mut cmd_id: u64 = 1;
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
            // —— 命令旁路排水（每拍先排水再读）：逐条弹出 fire-and-forget 写出
            // （板侧执行前回成功，回执由分类面记账）；io 失败 = 连接坏，命令回插
            // 队首不丢，按读失败同路重连后补写（有界丢最旧兜底）。b3 冒烟修正：
            // 原实现把排水放在建连拍——会话中途入队的命令永远等不到写出口
            // （队容 2 满丢最旧），板面收不到 servo 点名——b1 计划原文即
            // 「读循环每拍先排水再读」，此为落位笔误。
            if let Some(rx) = core.cmd.as_ref() {
                let mut cmd_io_err = false;
                while let Some(cmd) = rx.rr3_cmd_pop() {
                    let frame = rr3_esp32_cmd_frame(cmd_id, &cmd);
                    if let Err(e) = c.rr3_send_buf(Rr3FrameBuf::from(&frame)) {
                        rx.rr3_cmd_push_front(cmd);
                        eprintln!(
                            "rr-hal-esp32-audio: 命令写出失败（回插队首，连接报废转重连）: {e:?}"
                        );
                        cmd_io_err = true;
                        break;
                    }
                    cmd_id += 1;
                }
                if cmd_io_err {
                    conn = None;
                    break;
                }
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
                    // 批N12 件二：宿主读面原始字节 tee（拆帧前镜像——批N10 件二
                    // 病灶"帧内尾随残渣"的取证层，tee 流与消费流逐字节一致）
                    rr3_tee_write(&mut tee, &raw[..n]);
                    asm.feed(&raw[..n]);
                    loop {
                        match asm.pull() {
                            Ok(Some((hdr, body))) => {
                                match rr3_esp32_classify_board_frame(&hdr, &body) {
                                    Ok(Rr3BoardFrameIn::Audio(chunk)) => {
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
                                    Ok(Rr3BoardFrameIn::Reply { id, err }) => match err {
                                        Some(e) => eprintln!(
                                            "rr-hal-esp32-audio: 板面应答 id={id} 业务失败: {e}"
                                        ),
                                        None => eprintln!(
                                            "rr-hal-esp32-audio: 板面应答 id={id} ok（pre-execution ack）"
                                        ),
                                    },
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

/// ESP32-UART 板面音频源（A 批 c3；B 批 b1 增命令旁路面）。构造不触设备
/// （选择函数口径）；open 时真开 UART + 握手 + 起读线程。
pub struct Rr3Esp32UartAudioSource {
    path: PathBuf,
    baud: u32,
    inner: Mutex<Option<Rr3Esp32AudioStream>>,
    /// 命令旁路盒（构造即建）：产面是否外发由构造子裁定（[`Self::
    /// rr3_esp32_audio_source_new_with_cmd`]），耗面 open 时注入读线程核。
    cmd: (Rr3AudioCmdProducer, Rr3AudioCmdConsumer),
}

struct Rr3Esp32AudioStream {
    core: Arc<Rr3Esp32AudioCore>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Rr3Esp32UartAudioSource {
    /// 构造（设备路径/波特率来自应急 env 的裁决结果；open 时才真开）。无旁路
    /// 产面形态（产面随 self 内部弃置——push 无人调，队列恒空，行为同旧版）。
    pub fn rr3_esp32_audio_source_new(path: PathBuf, baud: u32) -> Self {
        Self {
            path,
            baud,
            inner: Mutex::new(None),
            cmd: rr3_audio_cmd_pair(),
        }
    }

    /// 带旁路构造（B 批 b1）：返 `(源, 产面)`——产面交宿主消费方（rr3-sense
    /// 反射环），耗面 open 时注入读线程（单写者纪律）。
    pub fn rr3_esp32_audio_source_new_with_cmd(
        path: PathBuf,
        baud: u32,
    ) -> (Self, Rr3AudioCmdProducer) {
        let s = Self::rr3_esp32_audio_source_new(path, baud);
        let producer = s.cmd.0.clone();
        (s, producer)
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
            cmd: Some(self.cmd.1.clone()),
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
        let tilt_bridge = crate::esp32_uart::rr3_bridge_call_frame(
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
        let bridge = crate::esp32_uart::rr3_bridge_call_frame(
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
        match rr3_esp32_classify_board_frame(&audio.header, &audio.payload) {
            Ok(Rr3BoardFrameIn::Audio(c)) => assert_eq!(c.samples, pcm),
            other => panic!("音频帧须落 Audio 臂: {other:?}"),
        }
        // 板侧应答 ok → Reply{id, err=None}
        let ok = board_reply_frame(7, None);
        match rr3_esp32_classify_board_frame(&ok.header, &ok.payload) {
            Ok(Rr3BoardFrameIn::Reply { id, err }) => {
                assert_eq!(id, 7);
                assert_eq!(err, None);
            }
            other => panic!("应答帧须落 Reply 臂: {other:?}"),
        }
        // 板侧业务失败（越程拒收）→ Reply{err=Some}（fail-visible，不静默）
        let bad = board_reply_frame(8, Some("越程拒收"));
        match rr3_esp32_classify_board_frame(&bad.header, &bad.payload) {
            Ok(Rr3BoardFrameIn::Reply { id, err }) => {
                assert_eq!(id, 8);
                assert_eq!(err.as_deref(), Some("越程拒收"));
            }
            other => panic!("失败应答须落 Reply 臂带 err: {other:?}"),
        }
        // dst 门：非宿主位形拒收（与音频门同口径）
        let mut other_dst = board_reply_frame(9, None);
        other_dst.header.dst_task = 9;
        assert!(rr3_esp32_classify_board_frame(&other_dst.header, &other_dst.payload).is_err());
    }

    /// 批N12 件二（敞口 16 对表 tee 工具面）：tee 关（None）＝no-op 零副作用；
    /// tee 开＝多笔写入**逐字节一致**且按序追加（tee 流 == 消费流判据的单测
    /// 锚点）。关闭态"零落盘"由构造面保证（env 未设不 open，见 [`rr3_tee_open`]），
    /// 无路径可断言，不在此冒领。
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
}
