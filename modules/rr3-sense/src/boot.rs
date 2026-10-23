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

use rr_hal::backend::rr3_hal_audio_source_with_cmd;
use rr_hal::backend::Rr3AudioCmdProducer;
use rr_hal_api::Rr3AudioCfg;
use rr_hal_api::Rr3AudioSource;

use crate::capture::Rr3AudioConfig;
use crate::vad::Rr3VadConfig;

/// 音频源选择 + 开机探活编排：[`rr3_hal_audio_source_with_cmd`] 后端选择
/// （反射环命令旁路随源给出，None = 后端无旁路）＋ open 探活（打开失败 Err
/// 上抛）。「音频源就绪」回显归本编排（方案 §4.3 映射表字面）。返回四元——
/// 源、命令旁路、后端名（"读帧失败({backend})"文案复用）、开源形状（采集拍
/// 构造 `Rr3PcmStream` 复用）。
pub(crate) fn rr3_sense_probe(
    cfg: &Rr3SenseConfig,
) -> Result<
    (
        Box<dyn Rr3AudioSource>,
        Option<Rr3AudioCmdProducer>,
        &'static str,
        Rr3AudioCfg,
    ),
    String,
> {
    let (mut source, cmd_tx, backend) = rr3_hal_audio_source_with_cmd();
    if cfg.reflex.enable && cmd_tx.is_none() {
        eprintln!(
            "rr3-sense: 反射环已启用但后端 {backend} 无命令旁路，转头指令无处可去（仅 esp32 档承载）"
        );
    }
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
/// - `gate_gap_ms`：tts_pcm 帧间隔超过此值判新一轮播放（门控累计重置）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ReflexConfig {
    pub enable: bool,
    pub deadband_deg: i32,
    pub gate_tail_ms: u64,
    pub gate_gap_ms: u64,
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
            },
        }
    }
}

/// 解析配置文本：缺键回落默认，类型错显式报错（不静默吞）。
pub(crate) fn rr3_sense_config_parse(text: &str) -> Result<Rr3SenseConfig, String> {
    let root: toml::Value = text
        .parse()
        .map_err(|e| format!("配置不是合法 TOML: {e}"))?;
    let mut cfg = Rr3SenseConfig::default();
    let section = root.get("rr3-sense");
    if let Some(audio) = section.and_then(|s| s.get("audio")) {
        let mut rate = cfg.audio.sample_rate as u64;
        let mut ch = cfg.audio.channels as u64;
        let mut fms = cfg.audio.frame_ms;
        rr3_cfg_u64(audio, "sample_rate", &mut rate)?;
        rr3_cfg_u64(audio, "channels", &mut ch)?;
        rr3_cfg_u64(audio, "frame_ms", &mut fms)?;
        // 0.0.5 批I S-1：frame_ms < 1（u64 域即 0）无节拍意义（0 帧 → read 空
        // → 全速刷总线），收敛到登记默认 20ms 并留痕——半有效的配置比报错更隐蔽
        if fms < 1 {
            eprintln!("config [rr3-sense.audio] frame_ms={fms} 无节拍意义，收敛 20ms");
            fms = 20;
        }
        cfg.audio.sample_rate = u32::try_from(rate)
            .map_err(|_| format!("[rr3-sense.audio] sample_rate 越界: {rate}"))?;
        cfg.audio.channels =
            u8::try_from(ch).map_err(|_| format!("[rr3-sense.audio] channels 越界: {ch}"))?;
        cfg.audio.frame_ms = fms;
    }
    if let Some(vad) = section.and_then(|s| s.get("vad")) {
        rr3_cfg_i64(vad, "rms_start", &mut cfg.vad.rms_start)?;
        rr3_cfg_u64(vad, "silence_tail_ms", &mut cfg.vad.silence_tail_ms)?;
        rr3_cfg_u64(vad, "min_speech_ms", &mut cfg.vad.min_speech_ms)?;
    }
    // S-5 + B批 b3b：DoA 档界与判向窗策略量（缺键回落默认；异常值显式报错，
    // window_ms=0 无窗意义收敛默认留痕——与 audio.frame_ms 同一口径）
    if let Some(doa) = section.and_then(|s| s.get("doa")) {
        let mut t = cfg.doa.bin_threshold_deg as i64;
        rr3_cfg_i64(doa, "bin_threshold_deg", &mut t)?;
        cfg.doa.bin_threshold_deg =
            i32::try_from(t).map_err(|_| format!("[rr3-sense.doa] bin_threshold_deg 越界: {t}"))?;
        rr3_cfg_u64(doa, "window_ms", &mut cfg.doa.window_ms)?;
        if cfg.doa.window_ms < 1 {
            eprintln!(
                "config [rr3-sense.doa] window_ms={} 无窗意义，收敛 1000ms",
                cfg.doa.window_ms
            );
            cfg.doa.window_ms = 1000;
        }
        let mut wrm = cfg.doa.window_rms_min;
        rr3_cfg_i64(doa, "window_rms_min", &mut wrm)?;
        if !(0..=32767).contains(&wrm) {
            return Err(format!(
                "[rr3-sense.doa] window_rms_min 须在 0..=32767，收到 {wrm}"
            ));
        }
        cfg.doa.window_rms_min = wrm;
        let mut mm = cfg.doa.mic_baseline_mm as u64;
        rr3_cfg_u64(doa, "mic_baseline_mm", &mut mm)?;
        if mm == 0 {
            return Err("[rr3-sense.doa] mic_baseline_mm=0 无几何可判向".to_string());
        }
        cfg.doa.mic_baseline_mm =
            u32::try_from(mm).map_err(|_| format!("[rr3-sense.doa] mic_baseline_mm 越界: {mm}"))?;
    }
    // B批 b2：反射环与播放期门控策略量（缺键回落默认，enable 缺省 false）
    if let Some(reflex) = section.and_then(|s| s.get("reflex")) {
        rr3_cfg_bool(reflex, "enable", &mut cfg.reflex.enable)?;
        let mut db = cfg.reflex.deadband_deg as i64;
        rr3_cfg_i64(reflex, "deadband_deg", &mut db)?;
        cfg.reflex.deadband_deg =
            i32::try_from(db).map_err(|_| format!("[rr3-sense.reflex] deadband_deg 越界: {db}"))?;
        rr3_cfg_u64(reflex, "gate_tail_ms", &mut cfg.reflex.gate_tail_ms)?;
        rr3_cfg_u64(reflex, "gate_gap_ms", &mut cfg.reflex.gate_gap_ms)?;
    }
    Ok(cfg)
}

fn rr3_cfg_u64(table: &toml::Value, key: &str, slot: &mut u64) -> Result<(), String> {
    match table.get(key) {
        None => Ok(()),
        Some(v) => {
            *slot = v
                .as_integer()
                .and_then(|i| u64::try_from(i).ok())
                .ok_or_else(|| format!("配置项 {key} 须为非负整数，收到 {v}"))?;
            Ok(())
        }
    }
}

fn rr3_cfg_i64(table: &toml::Value, key: &str, slot: &mut i64) -> Result<(), String> {
    match table.get(key) {
        None => Ok(()),
        Some(v) => {
            *slot = v
                .as_integer()
                .ok_or_else(|| format!("配置项 {key} 须为整数，收到 {v}"))?;
            Ok(())
        }
    }
}

fn rr3_cfg_bool(table: &toml::Value, key: &str, slot: &mut bool) -> Result<(), String> {
    match table.get(key) {
        None => Ok(()),
        Some(v) => {
            *slot = v
                .as_bool()
                .ok_or_else(|| format!("配置项 {key} 须为布尔值，收到 {v}"))?;
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
        assert_eq!(
            rr3_sense_config_parse("").unwrap(),
            Rr3SenseConfig::default()
        );
        // 只给部分键：给的生效，缺的回落默认
        let cfg = rr3_sense_config_parse("[rr3-sense.vad]\nrms_start = 900\n").unwrap();
        assert_eq!(cfg.vad.rms_start, 900);
        assert_eq!(cfg.vad.silence_tail_ms, 700);
        assert_eq!(cfg.audio.sample_rate, 16000);
        // 全量
        let full = rr3_sense_config_parse(
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
        assert!(rr3_sense_config_parse("[rr3-sense.vad]\nrms_start = \"loud\"\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.audio]\nframe_ms = -20\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.doa]\nbin_threshold_deg = \"wide\"\n").is_err());
        assert!(rr3_sense_config_parse("not toml at all [").is_err());
    }

    /// 0.0.5 批I S-1：frame_ms=0 无节拍意义，收敛 20 留痕（eprintln 进测试输出无害）；
    /// 负值仍被 rr3_cfg_u64 显式拒收，不参与收敛。
    #[test]
    fn config_parse_clamps_zero_frame_ms_to_registered_20() {
        let cfg = rr3_sense_config_parse("[rr3-sense.audio]\nframe_ms = 0\n").unwrap();
        assert_eq!(cfg.audio.frame_ms, 20);
        assert!(rr3_sense_config_parse("[rr3-sense.audio]\nframe_ms = -5\n").is_err());
    }

    /// reflex 段解析：缺段 = enable 缺省 false（不开与旧版逐点等价）；全量注入
    /// 生效；类型错显式拒收（bool 有专属 helper，字符串 "on" 不认）。
    #[test]
    fn config_parse_reflex_section_defaults_disabled() {
        let cfg = rr3_sense_config_parse("").unwrap();
        assert!(!cfg.reflex.enable);
        assert_eq!(cfg.reflex.deadband_deg, 10);
        assert_eq!(cfg.reflex.gate_tail_ms, 1000);
        assert_eq!(cfg.reflex.gate_gap_ms, 500);
        let cfg = rr3_sense_config_parse(
            "[rr3-sense.reflex]\nenable = true\ndeadband_deg = 15\ngate_tail_ms = 800\ngate_gap_ms = 300\n",
        )
        .unwrap();
        assert!(cfg.reflex.enable);
        assert_eq!(cfg.reflex.deadband_deg, 15);
        assert_eq!(cfg.reflex.gate_tail_ms, 800);
        assert_eq!(cfg.reflex.gate_gap_ms, 300);
        assert!(rr3_sense_config_parse("[rr3-sense.reflex]\nenable = \"on\"\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.reflex]\ndeadband_deg = \"wide\"\n").is_err());
    }

    /// doa 段解析：缺段 = 全默认；全量注入生效；window_ms=0 收敛 1000 留痕
    /// （与 audio.frame_ms 同口径）；异常值显式拒收（window_rms_min 越界 /
    /// mic_baseline_mm=0 无几何），类型错走 rr3_cfg_* 通用报错。
    #[test]
    fn config_parse_doa_section_defaults_full_and_rejects() {
        let cfg = rr3_sense_config_parse("").unwrap();
        assert_eq!(cfg.doa.bin_threshold_deg, 30);
        assert_eq!(cfg.doa.window_ms, 1000);
        assert_eq!(cfg.doa.window_rms_min, 800);
        assert_eq!(cfg.doa.mic_baseline_mm, 150);
        let cfg = rr3_sense_config_parse(
            "[rr3-sense.doa]\nbin_threshold_deg = 20\nwindow_ms = 500\nwindow_rms_min = 1200\nmic_baseline_mm = 180\n",
        )
        .unwrap();
        assert_eq!(cfg.doa.bin_threshold_deg, 20);
        assert_eq!(cfg.doa.window_ms, 500);
        assert_eq!(cfg.doa.window_rms_min, 1200);
        assert_eq!(cfg.doa.mic_baseline_mm, 180);
        let cfg = rr3_sense_config_parse("[rr3-sense.doa]\nwindow_ms = 0\n").unwrap();
        assert_eq!(cfg.doa.window_ms, 1000, "0 窗无意义收敛默认");
        assert!(rr3_sense_config_parse("[rr3-sense.doa]\nwindow_rms_min = -1\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.doa]\nwindow_rms_min = 32768\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.doa]\nmic_baseline_mm = 0\n").is_err());
        assert!(rr3_sense_config_parse("[rr3-sense.doa]\nwindow_ms = \"long\"\n").is_err());
    }

    /// 前置核对件漂移钉子（方案 §2.7 改形，批N28 册裁定 3）：config.toml 全量
    /// parse 后逐字段断言现值——此后 config.toml 被意外改动即红。window_rms_min
    /// 与 Default 800 的有意分歧如实钉死（S-1 后 config.toml 是缺省生效活值，
    /// 出厂策略优先——注释自证与 vad.rms_start 同口径 1500；方案「不一致先修
    /// config.toml」前提已消灭，登记不改）。
    #[test]
    fn config_toml_matches_registered_values() {
        let text = include_str!("../config.toml");
        let cfg = rr3_sense_config_parse(text).unwrap();
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
        assert!(!cfg.reflex.enable, "reflex 段不在 config.toml——出厂不开反射环");
    }
}
