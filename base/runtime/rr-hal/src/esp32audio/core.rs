//! 连接/重连、周期统计与原始流 tee。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rr_hal_api::{Rr3AudioCfg, Rr3AudioSource, Rr3ErrText, Rr3HalError};
use rr_ipc::audio_session::{handshake, HANDSHAKE_RETRY_PAUSE, HANDSHAKE_TRIES};
use rr_ipc::Rr3UartConn;

use super::cmd::{rr3_audio_cmd_pair, Rr3AudioCmdConsumer, Rr3AudioCmdProducer};
use super::reader::rr3_esp32_reader;
use super::ring::{Rr3AudioRing, RR3_ESP32_RING_CAP_SAMPLES};

/// 板面音频流事实（板固件 mic.rs 钉定）：16kHz 采样率。open 校验配置同款，
/// 不匹配回 `InvalidParam`（fail-closed，指路 config-esp32.toml）。
pub const RR3_ESP32_AUDIO_RATE: u32 = 16000;
/// 板面音频流事实：双声道（INMP441 ×2，DoA 判向的物理基础）。
pub const RR3_ESP32_AUDIO_CHANNELS: u8 = 2;

/// 桥链周期统计窗（10min）：帧数/坏帧/重连累计，1.5M 长跑的误码口径。
/// 误码率 = 坏帧 / 音频帧；一次会话内即可算，跨重连也连续（计数在核上）。
pub(super) const RR3_BRIDGE_STAT_PERIOD: Duration = Duration::from_secs(600);
/// 宿主音频心跳周期（A9.3）：每 5s 向板侧发一个**空行**（`\n`）——空行不是
/// 协议命令，新旧固件都静默忽略（旧固件零应答污染，新固件在音频会话中把它
/// 计为宿主存活）。板侧 15s 收不到即自动停流并清 audio_q——宿主进程被
/// SIGKILL/整机断电后，板侧不再对着无人 UART 空转推流。
pub(super) const RR3_PING_PERIOD: Duration = Duration::from_secs(5);

/// open 配置校验（纯函数）：板面流事实（16k/双声道/s16le）钉定，配置须同款。
/// 不匹配回 `InvalidParam` 并指路 config-esp32.toml——单声道缺省配置误吃
/// esp32 档时在探活期就点名，不静默出交织垃圾。
pub(super) fn rr3_esp32_cfg_check(cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
    if [
        cfg.sample_rate_hz == RR3_ESP32_AUDIO_RATE,
        cfg.channels == RR3_ESP32_AUDIO_CHANNELS,
        cfg.bit_depth == 16,
    ]
    .iter()
    .all(|ok| *ok)
    {
        return Ok(());
    }
    Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
        "esp32 板面流钉 16k/双声道/s16le，配置须同款（见 modules/rr3-sense/config-esp32.toml）",
    )))
}

/// 开口 + 握手（首连/重连同款路径）：open 自带 flock 排他（跨进程第二 opener
/// 当场被拒）→ `audio_session::handshake`（settle/防御 OFF/ON 应答扫描）。
fn rr3_esp32_audio_open(path: &Path, baud: u32) -> Result<Rr3UartConn, String> {
    let mut conn =
        Rr3UartConn::open_with_baud(path, baud).map_err(|e| format!("open 失败: {e}"))?;
    handshake(&mut conn).map_err(|e| format!("握手失败: {e}"))?;
    Ok(conn)
}

/// 开口失败收敛为 `Busy`（点名实际路径与原因）。
fn rr3_esp32_open_conn(path: &Path, baud: u32) -> Result<Rr3UartConn, Rr3HalError> {
    match rr3_esp32_audio_open(path, baud) {
        Ok(c) => Ok(c),
        Err(e) => {
            eprintln!("rr-hal-esp32-audio: 开口失败 {}: {e}", path.display());
            Err(Rr3HalError::Busy)
        }
    }
}

/// 读线程共享核：连接参数 + 环形缓冲 + 停旗 + 命令旁路耗面（None = 无旁路
/// 构造形态；open 时自源克隆注入）。
pub(super) struct Rr3Esp32AudioCore {
    path: PathBuf,
    baud: u32,
    pub(super) channels: u8,
    pub(super) ring: Mutex<Rr3AudioRing>,
    pub(super) stop: AtomicBool,
    pub(super) cmd: Option<Rr3AudioCmdConsumer>,
    /// 桥链累计统计（本进程生命周期）：音频帧、坏帧、重连次数——周期统计行与
    /// 长跑误码取证的读面。原子量只为跨重连累计，不参与控制流。
    pub(super) chunks: AtomicU64,
    pub(super) errs: AtomicU64,
    pub(super) reconnects: AtomicU64,
}

/// 断连重连（永不放弃）：歇口 → [`HANDSHAKE_TRIES`] 轮逐次**重开 fd** + 握手
/// （宿主侧 USB RX 冻结唯一捅法）；全败外层循环续轮。stop 置位即收兵回 None。
///
/// 打点节流（2026-11-10 B26 静态审计）：板长期不在位时每轮 10 次失败会按 3s
/// 周期刷屏（rrd 日志面持续增长、现场排障被噪声淹没）。首败必打、其后每
/// [`RR3_RECON_LOG_EVERY`] 次失败一条；轮摘要每 [`RR3_RECON_ROUND_LOG_EVERY`]
/// 轮一条；重连成功必打（恢复事实不能漏）。状态机与重试次数零改动。
pub(super) fn rr3_esp32_reconnect(core: &Rr3Esp32AudioCore) -> Option<Rr3UartConn> {
    let mut fails: u64 = 0;
    let mut round: u64 = 0;
    loop {
        if core.stop.load(Ordering::Acquire) {
            return None;
        }
        std::thread::sleep(HANDSHAKE_RETRY_PAUSE);
        round += 1;
        match rr3_recon_round(core, &mut fails) {
            Rr3ReconStep::Conn(c) => return Some(c),
            Rr3ReconStep::Stop => return None,
            Rr3ReconStep::Spent => rr3_recon_summary(round),
        }
    }
}

/// 一轮重连的结局。
enum Rr3ReconStep {
    Stop,
    Conn(Rr3UartConn),
    Spent,
}

/// 一轮 [`HANDSHAKE_TRIES`] 次逐次重开 fd + 握手。
fn rr3_recon_round(core: &Rr3Esp32AudioCore, fails: &mut u64) -> Rr3ReconStep {
    for attempt in 1..=HANDSHAKE_TRIES {
        if core.stop.load(Ordering::Acquire) {
            return Rr3ReconStep::Stop;
        }
        if let Some(step) = rr3_recon_try(core, attempt, fails) {
            return step;
        }
        std::thread::sleep(HANDSHAKE_RETRY_PAUSE);
    }
    Rr3ReconStep::Spent
}

/// 单轮重开：成功交回连接，失败累计计数后继续（`Spent` 由循环终局给出）。
fn rr3_recon_try(core: &Rr3Esp32AudioCore, attempt: u32, fails: &mut u64) -> Option<Rr3ReconStep> {
    match rr3_esp32_audio_open(&core.path, core.baud) {
        Ok(c) => Some(rr3_recon_ok(core, attempt, c)),
        Err(e) => {
            rr3_recon_fail(attempt, fails, &e);
            None
        }
    }
}

/// 单轮重开成功：计数 + 点名并交回连接。
fn rr3_recon_ok(core: &Rr3Esp32AudioCore, attempt: u32, c: Rr3UartConn) -> Rr3ReconStep {
    core.reconnects.fetch_add(1, Ordering::Relaxed);
    eprintln!("rr-hal-esp32-audio: 重连成功（第 {attempt}/{HANDSHAKE_TRIES} 轮）");
    Rr3ReconStep::Conn(c)
}

/// 单轮重开失败：累计计数，按打点判据点名。
fn rr3_recon_fail(attempt: u32, fails: &mut u64, e: &str) {
    *fails += 1;
    if rr3_recon_log_now(*fails) {
        eprintln!(
            "rr-hal-esp32-audio: 重连第 {attempt}/{HANDSHAKE_TRIES} 轮失败（累计 {fails} 次）: {e}"
        );
    }
}

/// 轮摘要打点（每 [`RR3_RECON_ROUND_LOG_EVERY`] 轮一条）。
fn rr3_recon_summary(round: u64) {
    if round % RR3_RECON_ROUND_LOG_EVERY == 0 {
        eprintln!(
            "rr-hal-esp32-audio: 重连 {HANDSHAKE_TRIES} 轮全败（第 {round} 轮），继续循环（常驻源不放弃；饥饿期消费面走 pcm_empty_frames 记账）"
        );
    }
}

/// 重连失败打点判据：首败必打，其后每 [`RR3_RECON_LOG_EVERY`] 次一条。
const RR3_RECON_LOG_EVERY: u64 = 100;
/// 轮摘要打点周期（轮）。
const RR3_RECON_ROUND_LOG_EVERY: u64 = 5;

pub(super) fn rr3_recon_log_now(fails: u64) -> bool {
    fails == 1 || fails % RR3_RECON_LOG_EVERY == 0
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
pub(super) fn rr3_tee_open() -> Option<std::fs::File> {
    let path = rr3_tee_path()?;
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

/// tee 目标路径：env 未设或为空即无（关闭态零落盘）。
fn rr3_tee_path() -> Option<String> {
    let path = std::env::var(RR3_HAL_ESP32_TEE_ENV).ok()?;
    if path.is_empty() {
        return None;
    }
    Some(path)
}

/// tee 单文件上限（真板实录 2026-11-09）：取证开关被留在 systemd drop-in 里时，
/// 无界追加会填满宿主 tmpfs（实测 923MB 塞满 /tmp，直接阻断部署/OTA）。上限到点
/// 停 tee 并留一条日志——观测面降级，链路面不受影响。
pub(super) const RR3_TEE_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// tee 写入面（单测锚点）：`sink = None`（tee 关）即 no-op 零副作用；打开时逐
/// 字节镜像追加。写失败 = 观测面降级（打一条后停 tee），**绝不反噬链路面**——
/// 音频链 fail-closed 纪律管的是链路判定，观测面降级不构成链路故障。
pub(super) fn rr3_tee_write(sink: &mut Option<std::fs::File>, bytes: &[u8]) {
    let Some(f) = sink.as_mut() else { return };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len + bytes.len() as u64 > RR3_TEE_MAX_BYTES {
        eprintln!(
            "rr-hal-esp32-audio: 取证 tee 达上限 {RR3_TEE_MAX_BYTES}B（tee 停用，链路不受影响）"
        );
        *sink = None;
        return;
    }
    use std::io::Write;
    if let Err(e) = f.write_all(bytes).and_then(|()| f.flush()) {
        eprintln!("rr-hal-esp32-audio: tee 写失败（tee 停用，链路不受影响）: {e}");
        *sink = None;
    }
}

/// 本窗坏帧率告警判据：>0.1%（千分之一）即尾标 `[注意]`，异常窗一眼可见。
pub(super) fn rr3_bad_alert(frames: u64, bad: u64) -> bool {
    frames > 0 && bad.saturating_mul(1000) > frames
}

/// 桥链周期统计（10min 窗）：本窗增量 + 生命周期累计。误码率按**本窗**坏/帧
/// 计算（累计口径受重连/重启影响，只作参考）；增量是排障主读数。
pub(super) fn rr3_stat_tick(core: &Rr3Esp32AudioCore, mark: &mut Rr3StatMark) {
    let chunks = core.chunks.load(Ordering::Relaxed);
    let errs = core.errs.load(Ordering::Relaxed);
    let recs = core.reconnects.load(Ordering::Relaxed);
    let dframe = chunks.saturating_sub(mark.chunks);
    let dbad = errs.saturating_sub(mark.errs);
    let flag = if rr3_bad_alert(dframe, dbad) {
        " [注意:本窗坏帧率>0.1%]"
    } else {
        ""
    };
    eprintln!(
        "rr-hal-esp32-audio: 桥链统计（{}min 窗）: 本窗帧=+{} 坏=+{} 重连=+{} | 累计帧={} 坏={} 重连={}{flag}",
        RR3_BRIDGE_STAT_PERIOD.as_secs() / 60,
        dframe,
        dbad,
        recs.saturating_sub(mark.recs),
        chunks,
        errs,
        recs,
    );
    *mark = Rr3StatMark { chunks, errs, recs };
}

/// 统计水位（上一窗的累计值；只由读线程持有）。
#[derive(Default)]
pub(super) struct Rr3StatMark {
    chunks: u64,
    errs: u64,
    recs: u64,
}

/// ESP32-UART 板面音频源（A 批 c3；B 批 b1 增命令旁路面）。构造不触设备
/// （选择函数口径）；open 时真开 UART + 握手 + 起读线程。
pub struct Rr3Esp32UartAudioSource {
    path: PathBuf,
    baud: u32,
    inner: Mutex<Option<Rr3Esp32AudioStream>>,
    /// 命令旁路盒（构造即建）：产面是否外发由构造子裁定（[`Self::
    /// rr3_esp32_source_cmd`]），耗面 open 时注入读线程核。
    cmd: (Rr3AudioCmdProducer, Rr3AudioCmdConsumer),
}

struct Rr3Esp32AudioStream {
    core: Arc<Rr3Esp32AudioCore>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Rr3Esp32UartAudioSource {
    /// 构造（设备路径/波特率来自应急 env 的裁决结果；open 时才真开）。无旁路
    /// 产面形态（产面随 self 内部弃置——push 无人调，队列恒空，行为同旧版）。
    pub fn rr3_esp32_source_new(path: PathBuf, baud: u32) -> Self {
        Self {
            path,
            baud,
            inner: Mutex::new(None),
            cmd: rr3_audio_cmd_pair(),
        }
    }

    /// 带旁路构造（B 批 b1）：返 `(源, 产面)`——产面交宿主消费方（rr3-sense
    /// 反射环），耗面 open 时注入读线程（单写者纪律）。
    pub fn rr3_esp32_source_cmd(path: PathBuf, baud: u32) -> (Self, Rr3AudioCmdProducer) {
        let s = Self::rr3_esp32_source_new(path, baud);
        let producer = s.cmd.0.clone();
        (s, producer)
    }

    /// 停流（open 重入/Drop 共用）：置停旗 → 有界等读线程退出（读循环最迟
    /// [`READ_TIMEOUT_MS`](rr_ipc::audio_session::READ_TIMEOUT_MS) 查一次旗，重连路径每次尝试前也查；2s 上界足够，
    /// 超时 detach——线程下一查旗点自灭，板面 AUDIO OFF 自会补发）。
    fn rr3_close_stream(stream: Rr3Esp32AudioStream) {
        stream.core.stop.store(true, Ordering::Release);
        if let Some(h) = stream.reader.as_ref() {
            rr3_join_reader(h);
        }
    }
}

/// 有界等读线程退出（超时 detach——线程下一查旗点自灭）。
fn rr3_join_reader(h: &std::thread::JoinHandle<()>) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !h.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl Rr3AudioSource for Rr3Esp32UartAudioSource {
    fn open(&mut self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        rr3_esp32_cfg_check(cfg)?;
        // 重入：先停旧流（flock 排他下旧连接不退账新开口必被拒）
        if let Some(old) = self.inner.lock().unwrap_or_else(|p| p.into_inner()).take() {
            Self::rr3_close_stream(old);
        }
        let conn = rr3_esp32_open_conn(&self.path, self.baud)?;
        let core = Arc::new(Rr3Esp32AudioCore {
            path: self.path.clone(),
            baud: self.baud,
            channels: cfg.channels,
            ring: Mutex::new(Rr3AudioRing::rr3_ring_new(RR3_ESP32_RING_CAP_SAMPLES)),
            stop: AtomicBool::new(false),
            cmd: Some(self.cmd.1.clone()),
            chunks: AtomicU64::new(0),
            errs: AtomicU64::new(0),
            reconnects: AtomicU64::new(0),
        });
        let reader_core = Arc::clone(&core);
        let reader = std::thread::Builder::new()
            .name("rr3-hal-esp32-audio".into())
            .spawn(move || rr3_esp32_reader(reader_core, conn))
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
