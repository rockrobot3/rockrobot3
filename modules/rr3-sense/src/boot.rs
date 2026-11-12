//! rr3-sense 接入层（0.0.8 W-A 批3 拆分，ADR-0017 五层表接入层）。
//!
//! config schema 解析（自原 composite.rs 迁入：缺键回落默认、类型错显式报错、
//! frame_ms/window_ms 收敛留痕——文案是装配观测面，逐字不变）＋ 音频源选择与
//! 开机探活编排（打开失败 Err 上抛：fail-closed 显式回错，不静默带病常驻也不
//! 给"没有音频源还常驻"的假活）。本层零业务 match——策略量经 config 数据化
//! 注入（阈值/档界/门控窗/反射环开关，机制件零策略载荷）。config 路径解析
//! （RR3_SENSE_CONFIG_ENV 覆盖 + manifest 同目录缺省）已随批0 并入 rr3mod
//! `load_config_text` 单点，本层不再持有路径段（0.0.5 批I S-1 与 power/connect
//! 同口径，批N28 单点收口）。

use rr_hal::backend::rr3_audio_source_cmd;
use rr_hal::backend::Rr3AudioCmdProducer;
use rr_hal_api::Rr3AudioCfg;
use rr_hal_api::Rr3AudioSource;

use crate::capture::Rr3AudioConfig;
use crate::vad::Rr3VadConfig;

/// 启动自述：四面 SURFACE 链式打印（装配段原序逐字沿用）。
pub(crate) fn rr3_sense_surface() {
    eprintln!("rr3-sense 0.0.1 (M3 实装)");
    for line in crate::capture::SURFACE
        .iter()
        .copied()
        .chain(crate::vad::SURFACE.iter().copied())
        .chain(crate::env::SURFACE.iter().copied())
        .chain(crate::imu::SURFACE.iter().copied())
    {
        eprintln!("  {line}");
    }
}

/// 音频源探活返回：源 + 命令旁路 + 后端名（错误文案复用）+ 开源形状（采集拍复用）
pub(crate) type Rr3SenseProbe = (
    Box<dyn Rr3AudioSource>,
    Option<Rr3AudioCmdProducer>,
    &'static str,
    Rr3AudioCfg,
);

/// 音频源选择 + 开机探活编排：[`rr3_audio_source_cmd`] 后端选择
/// （反射环命令旁路随源给出，None = 后端无旁路）＋ open 探活（打开失败 Err
/// 上抛）。「音频源就绪」回显归本编排（方案 §4.3 映射表字面）。返回四元——
/// 源、命令旁路、后端名（"读帧失败({backend})"文案复用）、开源形状（采集拍
/// 构造 `Rr3PcmStream` 复用）。
pub(crate) fn rr3_sense_probe(cfg: &Rr3SenseConfig) -> Result<Rr3SenseProbe, String> {
    let (mut source, cmd_tx, backend) = rr3_audio_source_cmd();
    rr3_reflex_warn(cfg, cmd_tx.is_some(), backend);
    // 采集配置按 config 真值开（R14：channels>1 即双麦期；默认 1 声道与旧硬编码逐点等价）
    let open_cfg = Rr3AudioCfg {
        sample_rate_hz: cfg.audio.sample_rate,
        channels: cfg.audio.channels,
        bit_depth: 16,
    };
    source
        .open(open_cfg)
        .map_err(|e| format!("打开音频源({backend})失败: {e}"))?;
    eprintln!(
        "音频源就绪: backend={backend} rate={} ch={} frame_ms={}",
        cfg.audio.sample_rate, cfg.audio.channels, cfg.audio.frame_ms
    );
    Ok((source, cmd_tx, backend, open_cfg))
}

/// 反射环启用的后端门：无命令旁路即点名（仅 esp32 档承载）。
fn rr3_reflex_warn(cfg: &Rr3SenseConfig, has_cmd: bool, backend: &str) {
    if cfg.reflex.enable && !has_cmd {
        eprintln!(
            "rr3-sense: 反射环已启用但后端 {backend} 无命令旁路，转头指令无处可去（仅 esp32 档承载）"
        );
    }
}

/// 模块全量运行期配置（自原 composite.rs 迁入，逐字）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3SenseConfig {
    pub audio: Rr3AudioConfig,
    pub vad: Rr3VadConfig,
    pub doa: Rr3DoaConfig,
    pub reflex: Rr3ReflexConfig,
}

/// DoA 判向配置（B批 b3b 起 TDOA 判向窗口径）：
/// - `bin_threshold_deg`：三档分档边界（S-5，原件内常量迁此）；
/// - `window_ms`：判向窗长（onset 时 ring 回看窗，0 收敛 1000 留痕）；
/// - `window_rms_min`：窗能量门（任一采集帧 rms ≥ 此值才算有声窗，0 = 门全开）；
/// - `mic_baseline_mm`：双麦间距（物理 lag 上限与 sinθ 反演的几何量，0 报错）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3DoaConfig {
    pub bin_threshold_deg: i32,
    pub window_ms: u64,
    pub window_rms_min: i64,
    pub mic_baseline_mm: u32,
}

/// 反射环配置（B批 b2）：策略量全部数据化。代码缺省 `enable = false`——不显式
/// 开（config-esp32.toml 档开）行为与旧版逐点等价：无门控无转头，纯采集照旧。
/// - `deadband_deg`：判向死区（|deg| < 死区不转头，正前方说话免抖动）；
/// - `gate_tail_ms`：TTS 帧流停后门保持开的尾窗（扬声器回声拖尾）；
/// - `gate_gap_ms`：tts_pcm 帧间隔超过此值判新一轮播放（门控累计重置）；
/// - `nod_deg`：点头示意行程（**相对 tilt 中位**的上仰角，0 = 不做点头手势；
///   0.0.8 批"转完点头"策略量，板侧行程 fail-closed 裁决，宿主不猜行程）；
/// - `nod_delay_ms`：pan 下发后等舵机到位的间隔（点头起点，须 ≥ pan 动作时长）；
/// - `nod_hold_ms`：回中位（点头蓄势位）到上仰的驻留时长（手势可见性）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ReflexConfig {
    pub enable: bool,
    pub deadband_deg: i32,
    pub gate_tail_ms: u64,
    pub gate_gap_ms: u64,
    pub nod_deg: i32,
    pub nod_delay_ms: u64,
    pub nod_hold_ms: u64,
}

impl Default for Rr3SenseConfig {
    fn default() -> Self {
        Self {
            audio: Rr3AudioConfig {
                sample_rate: 16000,
                channels: 1,
                frame_ms: 20,
            },
            vad: Rr3VadConfig::default(),
            doa: Rr3DoaConfig {
                bin_threshold_deg: 30,
                window_ms: 1000,
                window_rms_min: 800,
                mic_baseline_mm: 150,
            },
            reflex: Rr3ReflexConfig {
                enable: false,
                deadband_deg: 10,
                gate_tail_ms: 1000,
                gate_gap_ms: 500,
                nod_deg: 0,
                nod_delay_ms: 500,
                nod_hold_ms: 200,
            },
        }
    }
}

/// 采集面不变量上界（本轮审计）：采样率/声道/单拍时长属工程域，超界即配置错，
/// 不是扩展点——0 采样率/0 声道会让采集面永远静默却看起来活着，巨型
/// frame_ms/window_ms 会在 `frames_per_tick` 与判向 ring 分配处溢出或吃光内存。
const RR3_MAX_SAMPLE_RATE: u32 = 384_000;
const RR3_MAX_CHANNELS: u8 = 8;
const RR3_MAX_FRAME_MS: u64 = 1_000;
/// 判向窗 ring 的采样点总上界（8Mi 点 ≈ 16MiB）
const RR3_MAX_RING_SAMPLES: u128 = 8 * 1024 * 1024;
/// 反射环时间策略量上界（本轮静态审计）：门控尾窗/播放间隔/点头两段时序都是
/// 与 `Instant` 或 `Duration` 相加的毫秒量，畸形巨值在 debug 档触发加法溢出
/// panic、release 档回绕成"门立关/手势错时"——长于 10min 的门控与蓄势在真机上
/// 没有语义，超界即配置错，装载期点名拒（与 frame_ms/window_ms 同一 fail-closed 口径）。
const RR3_MAX_STRATEGY_MS: u64 = 600_000;

/// 解析配置文本：缺键回落默认，类型错显式报错（不静默吞）。
pub(crate) fn rr3_sense_cfg_parse(text: &str) -> Result<Rr3SenseConfig, String> {
    let root: toml::Value = text
        .parse()
        .map_err(|e| format!("配置不是合法 TOML: {e}"))?;
    let mut cfg = Rr3SenseConfig::default();
    let section = root.get("rr3-sense");
    rr3_cfg_audio(section, &mut cfg)?;
    rr3_cfg_vad(section, &mut cfg)?;
    rr3_cfg_doa(section, &mut cfg)?;
    rr3_cfg_reflex(section, &mut cfg)?;
    rr3_sense_cfg_check(&cfg)?;
    Ok(cfg)
}

/// `[rr3-sense.audio]` 段（缺段回落默认；frame_ms=0 收敛 20ms 留痕）。
fn rr3_cfg_audio(section: Option<&toml::Value>, cfg: &mut Rr3SenseConfig) -> Result<(), String> {
    let Some(audio) = section.and_then(|s| s.get("audio")) else {
        return Ok(());
    };
    let (rate, ch, fms) = rr3_audio_raw(audio, cfg)?;
    // 0.0.5 批I S-1：frame_ms < 1（u64 域即 0）无节拍意义（0 帧 → read 空
    // → 全速刷总线），收敛到登记默认 20ms 并留痕——半有效的配置比报错更隐蔽
    let fms = rr3_frame_ms(fms);
    (cfg.audio.sample_rate, cfg.audio.channels) = rr3_audio_take(rate, ch)?;
    cfg.audio.frame_ms = fms;
    Ok(())
}

/// frame_ms 收敛：0 无节拍意义，折登记默认 20ms 并留痕。
fn rr3_frame_ms(fms: u64) -> u64 {
    if fms < 1 {
        eprintln!("config [rr3-sense.audio] frame_ms={fms} 无节拍意义，收敛 20ms");
        return 20;
    }
    fms
}

/// `[rr3-sense.audio]` 三键读取（缺键保留现值，u64 域中转）。
fn rr3_audio_raw(audio: &toml::Value, cfg: &Rr3SenseConfig) -> Result<(u64, u64, u64), String> {
    let mut rate = cfg.audio.sample_rate as u64;
    let mut ch = cfg.audio.channels as u64;
    let mut fms = cfg.audio.frame_ms;
    rr3_cfg_u64(audio, "sample_rate", &mut rate)?;
    rr3_cfg_u64(audio, "channels", &mut ch)?;
    rr3_cfg_u64(audio, "frame_ms", &mut fms)?;
    Ok((rate, ch, fms))
}

/// 采样率/声道收窄到装配域（越界逐键点名）。
fn rr3_audio_take(rate: u64, ch: u64) -> Result<(u32, u8), String> {
    let sample_rate =
        u32::try_from(rate).map_err(|_| format!("[rr3-sense.audio] sample_rate 越界: {rate}"))?;
    let channels =
        u8::try_from(ch).map_err(|_| format!("[rr3-sense.audio] channels 越界: {ch}"))?;
    Ok((sample_rate, channels))
}

/// `[rr3-sense.vad]` 段（缺段回落默认）。
fn rr3_cfg_vad(section: Option<&toml::Value>, cfg: &mut Rr3SenseConfig) -> Result<(), String> {
    let Some(vad) = section.and_then(|s| s.get("vad")) else {
        return Ok(());
    };
    rr3_cfg_i64(vad, "rms_start", &mut cfg.vad.rms_start)?;
    rr3_cfg_u64(vad, "silence_tail_ms", &mut cfg.vad.silence_tail_ms)?;
    rr3_cfg_u64(vad, "min_speech_ms", &mut cfg.vad.min_speech_ms)?;
    Ok(())
}

/// `[rr3-sense.doa]` 段（S-5 + B批 b3b：缺段回落默认；异常值显式报错，
/// window_ms=0 无窗意义收敛默认留痕——与 audio.frame_ms 同一口径）。
fn rr3_cfg_doa(section: Option<&toml::Value>, cfg: &mut Rr3SenseConfig) -> Result<(), String> {
    let Some(doa) = section.and_then(|s| s.get("doa")) else {
        return Ok(());
    };
    cfg.doa.bin_threshold_deg = rr3_doa_deg(doa, cfg.doa.bin_threshold_deg)?;
    cfg.doa.window_ms = rr3_doa_window(doa, cfg.doa.window_ms)?;
    cfg.doa.window_rms_min = rr3_doa_rms(doa, cfg.doa.window_rms_min)?;
    cfg.doa.mic_baseline_mm = rr3_doa_base(doa, cfg.doa.mic_baseline_mm)?;
    Ok(())
}

/// 判向角阈值键（i64 中转后收 i32 域，越界点名）。
fn rr3_doa_deg(doa: &toml::Value, cur: i32) -> Result<i32, String> {
    let mut t = cur as i64;
    rr3_cfg_i64(doa, "bin_threshold_deg", &mut t)?;
    i32::try_from(t).map_err(|_| format!("[rr3-sense.doa] bin_threshold_deg 越界: {t}"))
}

/// 判向窗长键（0 无窗意义收敛 1000ms 留痕）。
fn rr3_doa_window(doa: &toml::Value, cur: u64) -> Result<u64, String> {
    let mut ms = cur;
    rr3_cfg_u64(doa, "window_ms", &mut ms)?;
    Ok(rr3_doa_window_fold(ms))
}

/// 窗长 0 无窗意义：折 1000ms 并留痕。
fn rr3_doa_window_fold(ms: u64) -> u64 {
    if ms < 1 {
        eprintln!(
            "config [rr3-sense.doa] window_ms={} 无窗意义，收敛 1000ms",
            ms
        );
        return 1000;
    }
    ms
}

/// 判向窗 RMS 门限键（i16 采样域）。
fn rr3_doa_rms(doa: &toml::Value, cur: i64) -> Result<i64, String> {
    let mut wrm = cur;
    rr3_cfg_i64(doa, "window_rms_min", &mut wrm)?;
    rr3_rms_cap(wrm)?;
    Ok(wrm)
}

/// RMS 门限值域（i16 采样域 0..=32767）。
fn rr3_rms_cap(wrm: i64) -> Result<(), String> {
    if !(0..=32767).contains(&wrm) {
        return Err(format!(
            "[rr3-sense.doa] window_rms_min 须在 0..=32767，收到 {wrm}"
        ));
    }
    Ok(())
}

/// 麦间距键（0 无几何可判向；u64 中转后收 u32 域）。
fn rr3_doa_base(doa: &toml::Value, cur: u32) -> Result<u32, String> {
    let mut mm = cur as u64;
    rr3_cfg_u64(doa, "mic_baseline_mm", &mut mm)?;
    if mm == 0 {
        return Err("[rr3-sense.doa] mic_baseline_mm=0 无几何可判向".to_string());
    }
    rr3_base_u32(mm)
}

/// 麦间距收窄到 u32 域（越界点名）。
fn rr3_base_u32(mm: u64) -> Result<u32, String> {
    u32::try_from(mm).map_err(|_| format!("[rr3-sense.doa] mic_baseline_mm 越界: {mm}"))
}

/// `[rr3-sense.reflex]` 段（B批 b2：缺键回落默认，enable 缺省 false）。
fn rr3_cfg_reflex(section: Option<&toml::Value>, cfg: &mut Rr3SenseConfig) -> Result<(), String> {
    let Some(reflex) = section.and_then(|s| s.get("reflex")) else {
        return Ok(());
    };
    rr3_cfg_bool(reflex, "enable", &mut cfg.reflex.enable)?;
    cfg.reflex.deadband_deg = rr3_reflex_deg(reflex, "deadband_deg", cfg.reflex.deadband_deg)?;
    rr3_reflex_gate_ms(reflex, cfg)?;
    // 0.0.8 批"转完点头"：策略三键（缺键回落默认；nod_deg 缺省 0 = 手势关，
    // 与旧版逐点等价）。行程不在此预钳——板侧 fail-closed 是唯一裁决面。
    rr3_cfg_nod(reflex, cfg)
}

/// 点头策略三键（nod_deg 缺省 0 = 手势关）。
fn rr3_cfg_nod(reflex: &toml::Value, cfg: &mut Rr3SenseConfig) -> Result<(), String> {
    cfg.reflex.nod_deg = rr3_reflex_deg(reflex, "nod_deg", cfg.reflex.nod_deg)?;
    rr3_reflex_nod_ms(reflex, cfg)
}

/// deadband_deg/nod_deg 同形角度键（缺键保留现值，越 i32 点名）。
fn rr3_reflex_deg(reflex: &toml::Value, key: &str, cur: i32) -> Result<i32, String> {
    let mut raw = cur as i64;
    rr3_cfg_i64(reflex, key, &mut raw)?;
    i32::try_from(raw).map_err(|_| format!("[rr3-sense.reflex] {key} 越界: {raw}"))
}

/// 门控两时间键（tail/gap，缺键保留现值）。
fn rr3_reflex_gate_ms(reflex: &toml::Value, cfg: &mut Rr3SenseConfig) -> Result<(), String> {
    rr3_cfg_u64(reflex, "gate_tail_ms", &mut cfg.reflex.gate_tail_ms)?;
    rr3_cfg_u64(reflex, "gate_gap_ms", &mut cfg.reflex.gate_gap_ms)?;
    Ok(())
}

/// 点头两时间键（delay/hold，缺键保留现值）。
fn rr3_reflex_nod_ms(reflex: &toml::Value, cfg: &mut Rr3SenseConfig) -> Result<(), String> {
    rr3_cfg_u64(reflex, "nod_delay_ms", &mut cfg.reflex.nod_delay_ms)?;
    rr3_cfg_u64(reflex, "nod_hold_ms", &mut cfg.reflex.nod_hold_ms)?;
    Ok(())
}

/// 采集/判据面不变量（解析后统一校验，默认值同过一遍）：全部 fail-closed，
/// 错误文案点名键、收到值与合法域。
fn rr3_sense_cfg_check(cfg: &Rr3SenseConfig) -> Result<(), String> {
    rr3_media_cap_check(&cfg.audio, &cfg.vad)?;
    rr3_deg_cap_pair(cfg)?;
    rr3_ring_cap_check(cfg)?;
    rr3_span_cap_check(&cfg.reflex)
}

/// 采集面与 VAD 面域（采样/声道/单拍 + 阈值）。
fn rr3_media_cap_check(audio: &Rr3AudioConfig, vad: &Rr3VadConfig) -> Result<(), String> {
    rr3_audio_cap_check(audio)?;
    rr3_vad_cap_check(vad)
}

/// 两处角度键域（doa 档界 + reflex 死区）。
fn rr3_deg_cap_pair(cfg: &Rr3SenseConfig) -> Result<(), String> {
    rr3_deg_cap_check(
        "rr3-sense.doa",
        "bin_threshold_deg",
        cfg.doa.bin_threshold_deg,
    )?;
    rr3_deg_cap_check("rr3-sense.reflex", "deadband_deg", cfg.reflex.deadband_deg)
}

/// 采集面三键域（采样率/声道/单拍时长）。
fn rr3_audio_cap_check(audio: &Rr3AudioConfig) -> Result<(), String> {
    rr3_cap_rate(audio)?;
    rr3_cap_chan(audio)?;
    rr3_cap_frame(audio)
}

/// 单拍时长域（1..=上限）。
fn rr3_cap_frame(audio: &Rr3AudioConfig) -> Result<(), String> {
    if audio.frame_ms > RR3_MAX_FRAME_MS {
        return Err(format!(
            "[rr3-sense.audio] frame_ms 须在 1..={RR3_MAX_FRAME_MS}，收到 {}",
            audio.frame_ms
        ));
    }
    Ok(())
}

/// 采样率域（1..=上限）。
fn rr3_cap_rate(audio: &Rr3AudioConfig) -> Result<(), String> {
    if audio.sample_rate == 0 || audio.sample_rate > RR3_MAX_SAMPLE_RATE {
        return Err(format!(
            "[rr3-sense.audio] sample_rate 须在 1..={RR3_MAX_SAMPLE_RATE}，收到 {}",
            audio.sample_rate
        ));
    }
    Ok(())
}

/// 声道数域（1..=上限）。
fn rr3_cap_chan(audio: &Rr3AudioConfig) -> Result<(), String> {
    if audio.channels == 0 || audio.channels > RR3_MAX_CHANNELS {
        return Err(format!(
            "[rr3-sense.audio] channels 须在 1..={RR3_MAX_CHANNELS}，收到 {}",
            audio.channels
        ));
    }
    Ok(())
}

/// VAD 阈值域（i16 采样域）。
fn rr3_vad_cap_check(vad: &Rr3VadConfig) -> Result<(), String> {
    if !(0..=32767).contains(&vad.rms_start) {
        return Err(format!(
            "[rr3-sense.vad] rms_start 须在 0..=32767（i16 采样域），收到 {}",
            vad.rms_start
        ));
    }
    Ok(())
}

/// 角度键域（0..=180；段名/键名由调用方点名，便于定位配置错）。
fn rr3_deg_cap_check(tag: &str, key: &str, deg: i32) -> Result<(), String> {
    if !(0..=180).contains(&deg) {
        return Err(format!("[{tag}] {key} 须在 0..=180，收到 {deg}"));
    }
    Ok(())
}

/// 判向 ring 分配量 = rate × window_ms / 1000 × channels（采样点）：用 u128
/// 计算再比上界，避免 u64 乘法在畸形配置下溢出。
fn rr3_ring_cap_check(cfg: &Rr3SenseConfig) -> Result<(), String> {
    let ring_samples = cfg.audio.sample_rate as u128 * cfg.doa.window_ms as u128 / 1000
        * cfg.audio.channels as u128;
    if ring_samples > RR3_MAX_RING_SAMPLES {
        return Err(format!(
            "[rr3-sense.doa] window_ms={} 的判向 ring 需 {ring_samples} 采样点，超上界 {RR3_MAX_RING_SAMPLES}（约 16MiB）",
            cfg.doa.window_ms
        ));
    }
    Ok(())
}

/// 反射环四时间键的上界判定（判据单点；键名逐条点名便于定位配置错）。
fn rr3_span_cap_check(r: &Rr3ReflexConfig) -> Result<(), String> {
    for (key, v) in [
        ("gate_tail_ms", r.gate_tail_ms),
        ("gate_gap_ms", r.gate_gap_ms),
        ("nod_delay_ms", r.nod_delay_ms),
        ("nod_hold_ms", r.nod_hold_ms),
    ] {
        rr3_span_one(key, v)?;
    }
    Ok(())
}

/// 单键上界判定（键名逐条点名便于定位配置错）。
fn rr3_span_one(key: &str, v: u64) -> Result<(), String> {
    if v > RR3_MAX_STRATEGY_MS {
        return Err(format!(
            "[rr3-sense.reflex] {key} 须在 0..={RR3_MAX_STRATEGY_MS}，收到 {v}（门控/手势毫秒量与 Instant 相加，超界会溢出 panic 或回绕）"
        ));
    }
    Ok(())
}

fn rr3_cfg_u64(table: &toml::Value, key: &str, slot: &mut u64) -> Result<(), String> {
    rr3_cfg_slot(
        table,
        key,
        slot,
        |v| v.as_integer().and_then(|i| u64::try_from(i).ok()),
        "非负整数",
    )
}

fn rr3_cfg_i64(table: &toml::Value, key: &str, slot: &mut i64) -> Result<(), String> {
    rr3_cfg_slot(table, key, slot, toml::Value::as_integer, "整数")
}

fn rr3_cfg_bool(table: &toml::Value, key: &str, slot: &mut bool) -> Result<(), String> {
    rr3_cfg_slot(table, key, slot, toml::Value::as_bool, "布尔值")
}

/// 可选配置项落槽：键缺省不动（回落调用方默认），类型错按 `what` 点名拒。
fn rr3_cfg_slot<T>(
    table: &toml::Value,
    key: &str,
    slot: &mut T,
    pick: impl Fn(&toml::Value) -> Option<T>,
    what: &str,
) -> Result<(), String> {
    match table.get(key) {
        None => Ok(()),
        Some(v) => {
            *slot = pick(v).ok_or_else(|| format!("配置项 {key} 须为{what}，收到 {v}"))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parse_defaults_on_empty_and_fills_partial() {
        // 空文本 = 全默认
        assert_eq!(rr3_sense_cfg_parse("").unwrap(), Rr3SenseConfig::default());
        // 只给部分键：给的生效，缺的回落默认
        let cfg = rr3_sense_cfg_parse("[rr3-sense.vad]\nrms_start = 900\n").unwrap();
        assert_eq!(cfg.vad.rms_start, 900);
        assert_eq!(cfg.vad.silence_tail_ms, 700);
        assert_eq!(cfg.audio.sample_rate, 16000);
        // 全量
        let full = rr3_sense_cfg_parse(
            "[rr3-sense.audio]\nsample_rate = 48000\nchannels = 2\nframe_ms = 10\n\n\
             [rr3-sense.vad]\nrms_start = 2000\nsilence_tail_ms = 500\nmin_speech_ms = 150\n\n\
             [rr3-sense.doa]\nbin_threshold_deg = 20\n",
        )
        .unwrap();
        assert_eq!(full.audio.sample_rate, 48000);
        assert_eq!(full.audio.channels, 2);
        assert_eq!(full.audio.frame_ms, 10);
        assert_eq!(full.vad.rms_start, 2000);
        assert_eq!(full.vad.silence_tail_ms, 500);
        assert_eq!(full.vad.min_speech_ms, 150);
        assert_eq!(full.doa.bin_threshold_deg, 20, "S-5 阈值注入生效");
    }

    #[test]
    fn config_parse_rejects_wrong_types_and_broken_toml() {
        // 类型错显式报错，不静默吞
        assert!(rr3_sense_cfg_parse("[rr3-sense.vad]\nrms_start = \"loud\"\n").is_err());
        assert!(rr3_sense_cfg_parse("[rr3-sense.audio]\nframe_ms = -20\n").is_err());
        assert!(rr3_sense_cfg_parse("[rr3-sense.doa]\nbin_threshold_deg = \"wide\"\n").is_err());
        assert!(rr3_sense_cfg_parse("not toml at all [").is_err());
    }

    /// 0.0.5 批I S-1：frame_ms=0 无节拍意义，收敛 20 留痕（eprintln 进测试输出无害）；
    /// 负值仍被 rr3_cfg_u64 显式拒收，不参与收敛。
    #[test]
    fn config_parse_clamps_zero_frame_ms_to_registered_20() {
        let cfg = rr3_sense_cfg_parse("[rr3-sense.audio]\nframe_ms = 0\n").unwrap();
        assert_eq!(cfg.audio.frame_ms, 20);
        assert!(rr3_sense_cfg_parse("[rr3-sense.audio]\nframe_ms = -5\n").is_err());
    }

    /// 采集/判据域（本轮审计）：0 采样率/声道=假活，巨型 frame/window=溢出或
    /// 吃内存，负阈值/越界档界=语义反转——一律 fail-closed。
    #[test]
    fn config_rejects_degenerate_audio_and_doa_domains() {
        for text in [
            "[rr3-sense.audio]\nsample_rate = 0\n",
            "[rr3-sense.audio]\nsample_rate = 384001\n",
            "[rr3-sense.audio]\nchannels = 0\n",
            "[rr3-sense.audio]\nchannels = 9\n",
            "[rr3-sense.audio]\nframe_ms = 1001\n",
            "[rr3-sense.vad]\nrms_start = -1\n",
            "[rr3-sense.vad]\nrms_start = 32768\n",
            "[rr3-sense.doa]\nbin_threshold_deg = -1\n",
            "[rr3-sense.doa]\nbin_threshold_deg = 181\n",
            "[rr3-sense.reflex]\ndeadband_deg = -1\n",
            "[rr3-sense.doa]\nwindow_ms = 3600000\n",
            // 本轮审计：时间策略量超上界会参与 Instant 加法（debug panic / release 回绕）
            "[rr3-sense.reflex]\ngate_tail_ms = 600001\n",
            "[rr3-sense.reflex]\ngate_gap_ms = 18446744073709551615\n",
            "[rr3-sense.reflex]\nnod_delay_ms = 600001\n",
            "[rr3-sense.reflex]\nnod_hold_ms = 600001\n",
        ] {
            assert!(rr3_sense_cfg_parse(text).is_err(), "必须拒: {text}");
        }
        // 上界本身合法（含门控尾窗取满）
        assert!(rr3_sense_cfg_parse("[rr3-sense.reflex]\ngate_tail_ms = 600000\n").is_ok());
        assert!(rr3_sense_cfg_parse(
            "[rr3-sense.audio]\nsample_rate = 48000\nchannels = 2\nframe_ms = 10\n"
        )
        .is_ok());
    }

    /// reflex 段解析：缺段 = enable 缺省 false（不开与旧版逐点等价）；全量注入
    /// 生效；类型错显式拒收（bool 有专属 helper，字符串 "on" 不认）。
    #[test]
    fn config_parse_reflex_section_defaults_disabled() {
        let cfg = rr3_sense_cfg_parse("").unwrap();
        assert!(!cfg.reflex.enable);
        assert_eq!(cfg.reflex.deadband_deg, 10);
        assert_eq!(cfg.reflex.gate_tail_ms, 1000);
        assert_eq!(cfg.reflex.gate_gap_ms, 500);
        assert_eq!(cfg.reflex.nod_deg, 0, "缺键 = 不做点头手势（旧版逐点等价）");
        assert_eq!(cfg.reflex.nod_delay_ms, 500);
        assert_eq!(cfg.reflex.nod_hold_ms, 200);
        let cfg = rr3_sense_cfg_parse(
            "[rr3-sense.reflex]\nenable = true\ndeadband_deg = 15\ngate_tail_ms = 800\ngate_gap_ms = 300\nnod_deg = 8\nnod_delay_ms = 400\nnod_hold_ms = 150\n",
        )
        .unwrap();
        assert!(cfg.reflex.enable);
        assert_eq!(cfg.reflex.deadband_deg, 15);
        assert_eq!(cfg.reflex.gate_tail_ms, 800);
        assert_eq!(cfg.reflex.gate_gap_ms, 300);
        assert_eq!(cfg.reflex.nod_deg, 8);
        assert_eq!(cfg.reflex.nod_delay_ms, 400);
        assert_eq!(cfg.reflex.nod_hold_ms, 150);
        assert!(rr3_sense_cfg_parse("[rr3-sense.reflex]\nenable = \"on\"\n").is_err());
        assert!(rr3_sense_cfg_parse("[rr3-sense.reflex]\ndeadband_deg = \"wide\"\n").is_err());
        assert!(rr3_sense_cfg_parse("[rr3-sense.reflex]\nnod_delay_ms = -1\n").is_err());
    }

    /// doa 段解析：缺段 = 全默认；全量注入生效；window_ms=0 收敛 1000 留痕
    /// （与 audio.frame_ms 同口径）；异常值显式拒收（window_rms_min 越界 /
    /// mic_baseline_mm=0 无几何），类型错走 rr3_cfg_* 通用报错。
    #[test]
    fn config_parse_doa_section_defaults_full_and_rejects() {
        let cfg = rr3_sense_cfg_parse("").unwrap();
        assert_eq!(cfg.doa.bin_threshold_deg, 30);
        assert_eq!(cfg.doa.window_ms, 1000);
        assert_eq!(cfg.doa.window_rms_min, 800);
        assert_eq!(cfg.doa.mic_baseline_mm, 150);
        let cfg = rr3_sense_cfg_parse(
            "[rr3-sense.doa]\nbin_threshold_deg = 20\nwindow_ms = 500\nwindow_rms_min = 1200\nmic_baseline_mm = 180\n",
        )
        .unwrap();
        assert_eq!(cfg.doa.bin_threshold_deg, 20);
        assert_eq!(cfg.doa.window_ms, 500);
        assert_eq!(cfg.doa.window_rms_min, 1200);
        assert_eq!(cfg.doa.mic_baseline_mm, 180);
        let cfg = rr3_sense_cfg_parse("[rr3-sense.doa]\nwindow_ms = 0\n").unwrap();
        assert_eq!(cfg.doa.window_ms, 1000, "0 窗无意义收敛默认");
        assert!(rr3_sense_cfg_parse("[rr3-sense.doa]\nwindow_rms_min = -1\n").is_err());
        assert!(rr3_sense_cfg_parse("[rr3-sense.doa]\nwindow_rms_min = 32768\n").is_err());
        assert!(rr3_sense_cfg_parse("[rr3-sense.doa]\nmic_baseline_mm = 0\n").is_err());
        assert!(rr3_sense_cfg_parse("[rr3-sense.doa]\nwindow_ms = \"long\"\n").is_err());
    }

    /// 前置核对件漂移钉子（方案 §2.7 改形，批N28 册裁定 3）：config.toml 全量
    /// parse 后逐字段断言现值——此后 config.toml 被意外改动即红。window_rms_min
    /// 与 Default 800 的有意分歧如实钉死（S-1 后 config.toml 是缺省生效活值，
    /// 出厂策略优先——注释自证与 vad.rms_start 同口径 1500；方案「不一致先修
    /// config.toml」前提已消灭，登记不改）。
    #[test]
    fn config_toml_matches_registered_values() {
        let text = include_str!("../config.toml");
        let cfg = rr3_sense_cfg_parse(text).unwrap();
        assert_eq!(cfg.audio.sample_rate, 16000);
        assert_eq!(cfg.audio.channels, 1);
        assert_eq!(cfg.audio.frame_ms, 20);
        assert_eq!(cfg.vad.rms_start, 1500);
        assert_eq!(cfg.vad.silence_tail_ms, 700);
        assert_eq!(cfg.vad.min_speech_ms, 200);
        assert_eq!(cfg.doa.bin_threshold_deg, 30);
        assert_eq!(cfg.doa.window_ms, 1000);
        assert_eq!(
            cfg.doa.window_rms_min, 1500,
            "出厂策略 1500（Default 回落 800 的有意分歧，S-1 后 config.toml 是活值）"
        );
        assert_eq!(cfg.doa.mic_baseline_mm, 150);
        assert!(
            !cfg.reflex.enable,
            "reflex 段不在 config.toml——出厂不开反射环"
        );
    }
}
