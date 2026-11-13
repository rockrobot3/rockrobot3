//! boot 的单测（自同名单测模块原样抽出，判据不变）。

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
    let text = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/config.toml"));
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
