//! sim 后端波形、方位模型与模拟件的单元测试。

use super::source::RR3_SIM_NOISE_AMP;
use super::*;

use crate::Rr3Display;
use crate::{
    Rr3AudioCfg, Rr3AudioSink, Rr3AudioSource, Rr3GpioOut, Rr3HalError, Rr3Link, Rr3Power,
    Rr3PwmServo, RR3_AUDIO_CFG_16K_MONO16,
};

/// 占位 TTS 波形（I-10 迁入）：与独立双精度公式 12000·sin(2π·440·i/16000)
/// 的全程偏差 ≤100（幅度 0.8%，听感等价预算）；逐块推进（20ms 块）验证
/// 相位跨块连续
#[test]
fn tts_block_tracks_sine_formula_within_budget() {
    let mut buf = [0i16; 320];
    for chunk in (0..16_000).step_by(320) {
        rr3_sim_tts_block(&mut buf, chunk);
        for (k, &v) in buf.iter().enumerate() {
            let i = (chunk + k) as f64;
            let want =
                (12_000.0f64 * (2.0 * std::f64::consts::PI * 440.0 * i / 16_000.0).sin()) as i64;
            assert!(
                (v as i64 - want).abs() <= 100,
                "sample #{}: {v} vs {want}",
                chunk + k
            );
        }
    }
}

/// 扫描列表（N-9 迁入）：首行 SSID 与 sim 链路缺省 SSID 同一真值
#[test]
fn scan_list_first_ssid_ties_to_link_default() {
    let list = rr3_sim_scan_list();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].0, crate::backend::RR3_HAL_SIM_LINK_SSID_DEFAULT);
    assert_eq!(list[0].1, -42);
    assert!(list[0].2);
}

fn source(wave: Rr3SimWave) -> Rr3SimAudioSource {
    Rr3SimAudioSource::rr3_sim_source_new(wave)
}

/// 双麦 DoA 回归共用：16k/立体声/16bit（三处 open 同口径）
const CFG_16K_STEREO: Rr3AudioCfg = Rr3AudioCfg {
    sample_rate_hz: 16000,
    channels: 2,
    bit_depth: 16,
};

#[test]
fn wave_parse_covers_three_modes_and_rejects_garbage() {
    for (s, want) in [
        ("sine", Rr3SimWave::Sine),
        ("SINE", Rr3SimWave::Sine),
        ("noise", Rr3SimWave::Noise),
        ("silence", Rr3SimWave::Silence),
        ("burst", Rr3SimWave::Burst),
    ] {
        assert_eq!(Rr3SimWave::rr3_sim_wave_parse(s), Some(want), "{s}");
        assert_eq!(want.rr3_sim_wave_as_str(), s.to_ascii_lowercase());
    }
    for bad in ["", "square", "sine ", "0"] {
        assert_eq!(Rr3SimWave::rr3_sim_wave_parse(bad), None, "{bad}");
    }
}

/// golden 向量：正弦前 8 点逐点相等（16k 采样率下 440Hz 的确定性开头）。
/// 这批字面量同时钉住频率与幅度——改波形参数，这里当场红。
#[test]
fn sine_wave_matches_golden_prefix() {
    let mut s = source(Rr3SimWave::Sine);
    s.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut buf = [0i16; 8];
    let n = s.read(&mut buf).unwrap();
    assert_eq!(n, 8);
    // golden 前缀：12000·sin(2π·440·i/16000) 的 i64 截断值（独立双精度实现算出，
    // 不是被测代码自己的输出回抄）——改频率/幅度/取整方式，这里当场红。
    let golden: [i16; 8] = [0, 2063, 4064, 5945, 7649, 9124, 10328, 11225];
    assert_eq!(&buf[..], &golden, "正弦开头必须与 golden 逐点一致");
}

/// 相位跨 read 连续：两次 read 拼起来等于一次读双份
#[test]
fn sine_phase_is_continuous_across_reads() {
    let mut a = source(Rr3SimWave::Sine);
    a.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut b1 = [0i16; 160];
    let n1 = a.read(&mut b1).unwrap();
    let mut b2 = [0i16; 160];
    let n2 = a.read(&mut b2).unwrap();

    let mut b = source(Rr3SimWave::Sine);
    b.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut whole = [0i16; 320];
    let nw = b.read(&mut whole).unwrap();
    let mut joined = Vec::new();
    joined.extend_from_slice(&b1[..n1]);
    joined.extend_from_slice(&b2[..n2]);
    assert_eq!(joined, &whole[..nw]);
}

/// burst：偶秒正弦、奇秒静音；2s 周期恰为 440Hz 整周期数，第 3 秒与第 1 秒逐点相等
#[test]
fn burst_gates_sine_by_even_seconds_and_stays_deterministic() {
    let mut s = source(Rr3SimWave::Burst);
    s.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut pcm = [0i16; 3 * 16000];
    let n = s.read(&mut pcm).unwrap();
    assert_eq!(n, 3 * 16000);
    // 第 1 秒有声（远超 VAD 起音阈值），第 2 秒全零，第 3 秒与第 1 秒逐点相等
    assert!(
        pcm[..16000].iter().any(|&v| v.abs() > 1500),
        "第 1 秒必过起音阈值"
    );
    assert!(
        pcm[16000..32000].iter().all(|&v| v == 0),
        "第 2 秒必须全静音"
    );
    assert_eq!(&pcm[32000..], &pcm[..16000], "第 3 秒复现第 1 秒");
    // 门控与正弦同源：第 1 秒头部与 Sine 波形的同序号采样逐点相等
    let mut sine = source(Rr3SimWave::Sine);
    sine.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut head = [0i16; 16000];
    sine.read(&mut head).unwrap();
    assert_eq!(&pcm[..16000], &head[..], "burst 有声段 = Sine 同序号采样");
}

/// 噪声确定性：同波两源读出逐点相等，且幅度不越 ±8000
#[test]
fn noise_is_deterministic_and_bounded() {
    let mut a = source(Rr3SimWave::Noise);
    a.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut ba = [0i16; 512];
    a.read(&mut ba).unwrap();
    let mut b = source(Rr3SimWave::Noise);
    b.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut bb = [0i16; 512];
    b.read(&mut bb).unwrap();
    assert_eq!(ba, bb);
    assert!(ba.iter().all(|&v| v.abs() <= RR3_SIM_NOISE_AMP));
    // 不是正弦：存在相邻样本跳变超过 2000（440Hz 正弦 16k 下的最大步进约 1900）
    assert!(ba
        .windows(2)
        .any(|w| (w[0] as i32 - w[1] as i32).abs() > 2000));
}

#[test]
fn silence_is_all_zero_and_read_before_open_is_invalid() {
    let mut s = source(Rr3SimWave::Silence);
    let mut probe = [0i16; 10];
    let e = s.read(&mut probe).unwrap_err();
    assert!(matches!(e, Rr3HalError::InvalidParam(_)));
    s.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    let mut pcm = [0i16; 64];
    s.read(&mut pcm).unwrap();
    assert!(pcm.iter().all(|&v| v == 0));
}

#[test]
fn open_rejects_zero_rate_and_non_16bit() {
    for bad in [
        Rr3AudioCfg {
            sample_rate_hz: 0,
            channels: 1,
            bit_depth: 16,
        },
        Rr3AudioCfg {
            sample_rate_hz: 16000,
            channels: 0,
            bit_depth: 16,
        },
        Rr3AudioCfg {
            sample_rate_hz: 16000,
            channels: 1,
            bit_depth: 24,
        },
    ] {
        let mut s = source(Rr3SimWave::Sine);
        let e = s.open(bad).unwrap_err();
        assert!(matches!(e, Rr3HalError::InvalidParam(_)), "{bad:?}");
    }
}

#[test]
fn sink_requires_open_and_caps_tail_window() {
    let mut sink = Rr3SimAudioSink::rr3_sim_sink_new(100);
    let e = sink.write(&[1]).unwrap_err();
    assert!(
        matches!(e, Rr3HalError::InvalidParam(_)),
        "write 前必须 open"
    );
    sink.open(RR3_AUDIO_CFG_16K_MONO16).unwrap();
    for chunk in 0..7u8 {
        sink.write(&[chunk as i16; 40]).unwrap();
    }
    let played = sink.rr3_sim_sink_played();
    assert_eq!(played.len(), 100, "尾窗封顶");
    // 7×40=280 采样进 cap=100 的尾窗：最旧的 180 个被挤出，
    // 窗内 = chunk4 的后 20 个 + chunk5 整块 + chunk6 整块
    assert_eq!(&played[..20], &[4i16; 20], "最旧部分被挤出");
    assert_eq!(&played[20..60], &[5i16; 40]);
    assert_eq!(&played[60..], &[6i16; 40], "最新一块保留");
    assert_eq!(sink.rr3_sim_sink_count(), 100);
}

/// sim 舵机：初始角即构造值，set 后读回最后一次的值——设备面只记录不裁决，
/// 越界裁决属模块侧（angle_limit 在模块 config），这条用例钉住分工边界。
#[test]
fn sim_servo_records_last_set_angle_without_judging() {
    let mut s = Rr3SimServo::rr3_sim_servo_new(90);
    assert_eq!(s.rr3_sim_servo_angle(), 90, "初始角 = 中位 90°");
    assert_eq!(
        s.set_angle(135).unwrap(),
        (),
        "set_angle 如实接受任何 u16——裁决不在这里"
    );
    assert_eq!(s.rr3_sim_servo_angle(), 135);
    s.set_angle(0).unwrap();
    s.set_angle(200).unwrap();
    assert_eq!(s.angle().unwrap(), 200, "读回最后一次 set 的值");
}

/// 方位角构造的值域门：±90 闭区间收，越界与 NaN 不认。
#[test]
fn sim_doa_ctor_accepts_only_pm90() {
    for deg in [-90.0, -60.0, 0.0, 45.0, 90.0] {
        let d = Rr3SimDoa::rr3_sim_doa_new(deg).unwrap_or_else(|| panic!("{deg} 应收"));
        assert_eq!(d.rr3_sim_doa_deg(), deg);
    }
    for bad in [-90.5, 90.1, 180.0, f64::NAN] {
        assert!(
            Rr3SimDoa::rr3_sim_doa_new(bad).is_none(),
            "{bad} 越域必须不认"
        );
    }
}

/// 双麦 ILD 模型的可逆性：按注入角读双通道帧，逐通道 RMS 反演出 sinθ，
/// 必须与注入角自洽（sense 侧 DoA 的正门前提——注入与反演同一模型）。
/// 通道序按板端口径（ch0=物理右）；注入模型 `g_r = 1 + sinθ`、`g_l = 1 - sinθ`。
#[test]
fn sim_doa_stereo_ild_matches_injected_direction() {
    for deg in [-90.0, -60.0, -30.0, 0.0, 30.0, 60.0, 90.0] {
        let mut s = Rr3SimAudioSource::rr3_sim_source_doa(
            Rr3SimWave::Sine,
            Rr3SimDoa::rr3_sim_doa_new(deg),
        );
        s.open(CFG_16K_STEREO).unwrap();
        // 读 16000 帧（1 秒）：ITD 相移对 1s 整周期数平均后不扰 RMS 比值
        let mut pcm = [0i16; 32000];
        let n = s.read(&mut pcm).unwrap();
        assert_eq!(n, 32000);
        let (mut e0, mut e1) = (0i64, 0i64);
        for f in pcm.chunks_exact(2) {
            e0 += f[0] as i64 * f[0] as i64;
            e1 += f[1] as i64 * f[1] as i64;
        }
        // sinθ 的 RMS 反演 = (r_r - r_l)/(r_r + r_l)；帧序 ch0=物理右（g_r = 1+sinθ）
        let (r0, r1) = ((e0 as f64).sqrt(), (e1 as f64).sqrt());
        let k_est = (r0 - r1) / (r0 + r1);
        let k_want = deg.to_radians().sin();
        assert!(
            (k_est - k_want).abs() < 0.01,
            "deg={deg}: RMS 反演 k={k_est:.4} 必须贴合注入 sinθ={k_want:.4}"
        );
    }
}

/// 双麦 ITD 模型的可逆性（B批 b3b TDOA 回归钉）：按注入角读双通道正弦帧，
/// 通道间互相关峰的时延 lag 反演出 sinθ，必须与注入角自洽——sense 侧
/// `rr3_doa_fix` 用同一物理量（lag→sinθ→deg）反演，sim 基线与反演基线同源
/// 由本测与 e2e_audio_chain 的 ±2° 容差双面钉住。
#[test]
fn sim_doa_stereo_itd_lag_matches_injected_direction() {
    // TDOA 判据域 ±60°：±90° 是 ILD 模型边界（单侧 2 倍另侧归零）——另侧静音
    // 无时延可测，TDOA 在该角退化（sense 侧互相关全零峰退 0°，物理同构），
    // 不属判据域。
    for deg in [-60.0, -30.0, 0.0, 30.0, 60.0] {
        let mut s = Rr3SimAudioSource::rr3_sim_source_doa(
            Rr3SimWave::Sine,
            Rr3SimDoa::rr3_sim_doa_new(deg),
        );
        s.open(CFG_16K_STEREO).unwrap();
        // 读 200ms 双通道（3200 帧——覆盖整周期多圈，互相关峰稳定）
        let n_frames = 3200;
        let mut pcm = vec![0i16; n_frames * 2];
        let n = s.read(&mut pcm).unwrap();
        assert_eq!(n, pcm.len());
        // 通道间互相关：cc[d] = Σ frame0[t]·frame1[t+d]，d ∈ ±12（覆盖 90° 全域
        // lag = round(16000·150/343000) = 7，留 ±1 供抛物线取邻与噪声余量）
        let (r0, r1): (Vec<f64>, Vec<f64>) = (
            pcm.chunks_exact(2).map(|f| f[0] as f64).collect(),
            pcm.chunks_exact(2).map(|f| f[1] as f64).collect(),
        );
        let lag_max = 12i64;
        let mut best = 0f64;
        let mut bv = -1f64;
        for d in -lag_max..=lag_max {
            let off = d.unsigned_abs() as usize;
            let acc: f64 = if d >= 0 {
                (0..n_frames - off).map(|t| r0[t] * r1[t + off]).sum()
            } else {
                (off..n_frames).map(|t| r0[t] * r1[t - off]).sum()
            };
            if acc.abs() > bv {
                bv = acc.abs();
                best = d as f64;
            }
        }
        // lag 反演 sinθ（与 sense rr3_doa_fix 同一物理式：lag·c/(sr·d)）。
        // 容差 0.15 ≈ 1 样本 lag 误差带（1·343/(16000·0.150) = 0.143）：窄带单频
        // 正弦是 TDOA 最劣信号（36 样本周期的余弦宽峰），亚样本抛物线插值在
        // 宽峰上有 ≤1 样本系统性偏差；真宽带语音（板端 b3 双侧交叉实测）无此
        // 退化。e2e_audio_chain 的 ±2° 容差（±60° 判据点）另钉端到端。
        let sin_est = best * 343.0 / (16000.0 * 0.150);
        let k_want = deg.to_radians().sin();
        assert!(
            (sin_est - k_want).abs() < 0.15,
            "deg={deg}: ITD lag={best} 反演 sinθ={sin_est:.4} 必须贴合注入 {k_want:.4}"
        );
    }
}

/// 批F4 真板实测回归：burst 门控按**帧**序号——双通道下仍保证"1s 有声 +
/// 1s 静音"的音频时间语义。交错样点序号口径会把窗口折半成 500ms，短于
/// sense VAD 的 silence_tail_ms=700ms，offset 永不触发（--chain 演练
/// 60s 只见一次 onset 的根因）。
#[test]
fn burst_gate_counts_frames_so_stereo_keeps_one_second_windows() {
    let mut s =
        Rr3SimAudioSource::rr3_sim_source_doa(Rr3SimWave::Burst, Rr3SimDoa::rr3_sim_doa_new(-60.0));
    s.open(CFG_16K_STEREO).unwrap();
    // 第 1 秒（16000 帧）有声
    let mut loud = [0i16; 32000];
    let n = s.read(&mut loud).unwrap();
    assert_eq!(n, 32000);
    assert!(loud.iter().any(|&v| v != 0), "第 1 秒必须有声");
    // 第 2 秒全零（交错序号口径下这里是第 2 个"有声 500ms 窗"，修前必红）
    let mut silent = [0i16; 32000];
    s.read(&mut silent).unwrap();
    assert!(silent.iter().all(|&v| v == 0), "第 2 秒必须全静音");
    // 第 3 秒与第 1 秒逐点相等（440Hz 整周期 + 门控按帧，确定性）
    let mut again = [0i16; 32000];
    s.read(&mut again).unwrap();
    assert_eq!(again, loud, "burst 第 3 秒须复刻第 1 秒");
}

/// 确定性：同角两源读出逐点相等；单声道链路不受方位注入影响（逐点等价）。
#[test]
fn sim_doa_is_deterministic_and_mono_is_invariant() {
    let mk = || {
        Rr3SimAudioSource::rr3_sim_source_doa(Rr3SimWave::Sine, Rr3SimDoa::rr3_sim_doa_new(-60.0))
    };
    let (mut a, mut b) = (mk(), mk());
    let cfg2 = Rr3AudioCfg {
        sample_rate_hz: 16000,
        channels: 2,
        bit_depth: 16,
    };
    a.open(cfg2).unwrap();
    b.open(cfg2).unwrap();
    let mut ba = [0i16; 160];
    let mut bb = [0i16; 160];
    a.read(&mut ba).unwrap();
    b.read(&mut bb).unwrap();
    assert_eq!(ba, bb, "同角同帧序逐点相等");
    // 单声道：带注入与不带注入逐点相等（mono 无方向语义）
    let (mut m1, mut m2) = (
        mk(),
        Rr3SimAudioSource::rr3_sim_source_new(Rr3SimWave::Sine),
    );
    let cfg1 = RR3_AUDIO_CFG_16K_MONO16;
    m1.open(cfg1).unwrap();
    m2.open(cfg1).unwrap();
    let mut b1 = [0i16; 160];
    let mut b2 = [0i16; 160];
    m1.read(&mut b1).unwrap();
    m2.read(&mut b2).unwrap();
    assert_eq!(b1, b2, "mono + DoA 注入必须逐点等价无注入");
}

/// sim 数字输出：电平写读回环 + 覆写。LED/继电器各持一路，行为同一件。
#[test]
fn sim_gpio_level_roundtrip_and_overwrite() {
    let mut g = Rr3SimGpioOut::rr3_sim_gpio_new(false);
    assert!(!g.rr3_sim_gpio_level(), "初始断电态");
    assert!(!g.level().unwrap());
    g.set(true).unwrap();
    assert!(g.rr3_sim_gpio_level());
    g.set(false).unwrap();
    assert!(!g.level().unwrap(), "覆写生效");
}

/// sim 显示件：整帧替换 + 读回一致。draw 全帧替换无局部改写。
/// （批I F1 起 trait 去 `Send + Sync`、写面 `&mut self`——单所有者独占写，
/// 旧"跨线程写读"用例随线程面拆除，共享写归承载层的互斥面。）
#[test]
fn sim_display_draws_and_reads_back() {
    let mut d = Rr3SimDisplay::rr3_sim_display_new([0; 8]);
    assert_eq!(d.rr3_sim_frame(), [0; 8], "构造初始黑屏");
    d.draw(&[0b1111_1111, 0, 0, 0, 0, 0, 0, 0b1010_1010])
        .unwrap();
    assert_eq!(
        d.rr3_sim_frame(),
        [0b1111_1111, 0, 0, 0, 0, 0, 0, 0b1010_1010],
        "draw 即整帧替换"
    );
    d.draw(&[1; 8]).unwrap();
    assert_eq!(d.rr3_sim_frame(), [1; 8], "覆写读回一致");
}

/// sim 电池：trait battery 读回构造/覆写后的三元组，电压曲线按
/// 钉死的线性映射逐点核对——"设备只报数、策略不在这里"的分工同舵机那条。
#[test]
fn sim_battery_reads_percent_charging_and_derived_voltage() {
    let b = Rr3SimBattery::rr3_sim_battery_new(80, false);
    let st = b.battery().unwrap();
    assert_eq!(
        (st.percent, st.charging, st.voltage_mv),
        (80, false, 4000),
        "初始 80% 未充电，电压 = 3200 + 80×10"
    );
    b.rr3_sim_battery_set(5, true);
    let st = b.battery().unwrap();
    assert_eq!(
        (st.percent, st.charging, st.voltage_mv),
        (5, true, 3250),
        "覆写后读回同一真值"
    );
    // 特性曲线两端钉死：e2e 的低电断言（5% → 3250 ≤ low_voltage_mv=3300）靠它成立
    assert_eq!(Rr3SimBattery::rr3_sim_batt_mv(0), 3200);
    assert_eq!(Rr3SimBattery::rr3_sim_batt_mv(100), 4200);
}

/// sim 链路：trait state 读回构造/覆写后的三元组；断链时
/// ssid/rssi 强制 None——设备面如实表现，模块侧不许从死链里读出网络信息。
#[test]
fn sim_link_reads_up_ssid_rssi_and_blanks_them_when_down() {
    let l = Rr3SimLink::rr3_sim_link_new(true, "rock-sim", -50);
    let st = l.state().unwrap();
    let ssid = st.ssid.as_ref().map(|s| s.rr3_ssid_text_as_str());
    assert_eq!(
        (st.up, ssid, st.rssi_dbm),
        (true, Some("rock-sim"), Some(-50)),
        "初始在线，三元组原样读回"
    );
    l.rr3_sim_link_set(false, "rock-sim", -90);
    let st = l.state().unwrap();
    assert_eq!(
        (
            st.up,
            st.ssid.as_ref().map(|s| s.rr3_ssid_text_as_str()),
            st.rssi_dbm
        ),
        (false, None, None),
        "断链后 ssid/rssi 必须被设备面抹掉"
    );
    l.rr3_sim_link_set(true, "other-net", -40);
    let st = l.state().unwrap();
    assert_eq!(
        (
            st.up,
            st.ssid.as_ref().map(|s| s.rr3_ssid_text_as_str()),
            st.rssi_dbm
        ),
        (true, Some("other-net"), Some(-40)),
        "覆写恢复在线后读回新真值"
    );
}
