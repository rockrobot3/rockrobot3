// SPDX-License-Identifier: Apache-2.0
//! sim 方位面：注入角值域、ILD/ITD 反演自洽与立体声门控的时间语义。

use crate::sim::*;
use crate::{Rr3AudioCfg, Rr3AudioSource, RR3_AUDIO_CFG_16K_MONO16};

/// 双麦 DoA 回归共用：16k/立体声/16bit（本域各测 open 同口径）
const CFG_16K_STEREO: Rr3AudioCfg = Rr3AudioCfg {
    sample_rate_hz: 16000,
    channels: 2,
    bit_depth: 16,
};

/// 方位角构造的值域门：±90 闭区间收，越界与 NaN 不认。
#[test]
fn sim_doa_ctor_ok_pm90() {
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
fn sim_ild_direction() {
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
fn sim_lag_direction() {
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
fn burst_so_windows() {
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
fn sim_det_invariant() {
    let mk = || {
        Rr3SimAudioSource::rr3_sim_source_doa(Rr3SimWave::Sine, Rr3SimDoa::rr3_sim_doa_new(-60.0))
    };
    let (mut a, mut b) = (mk(), mk());
    a.open(CFG_16K_STEREO).unwrap();
    b.open(CFG_16K_STEREO).unwrap();
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
