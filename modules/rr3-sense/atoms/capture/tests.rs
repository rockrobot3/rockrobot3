//! capture 机制核单测（文件头：本文件只放测试，生产面在 `mech.rs`）。
//!
//! 拆分动因：`mech.rs` 536 行、内联测试自 331 行起，先按仓内同形搬出。

use super::*;

#[test]
fn frame_conversion_covers_registered_and_odd_rates() {
    assert_eq!(rr3_frames_per_tick(16000, 20), 320, "登记配置的黄金值");
    assert_eq!(rr3_frames_per_tick(16000, 10), 160);
    assert_eq!(rr3_frames_per_tick(8000, 20), 160);
    assert_eq!(rr3_frames_per_tick(22050, 20), 441, "22050×20/1000 整除");
    assert_eq!(
        rr3_frames_per_tick(16000, 0),
        0,
        "0ms 节拍读 0 帧（不 panic）"
    );
}

/// 多通道单声道化（2026-10-31 实机订正）：选能量最高通道（不平均）、
/// 单声道原样、点数收缩、并列取首通道、空输入安全。
#[test]
fn best_channel_picks_louder_and_mono_passthrough() {
    // 单声道原样（含负值），零改动
    assert_eq!(rr3_pcm_best_channel(&[1, -2, 3], 1), vec![1, -2, 3]);
    assert_eq!(rr3_pcm_best_channel(&[1, -2, 3], 0), vec![1, -2, 3]);
    // 双声道：通道 0 能量更高 → 取通道 0
    assert_eq!(
        rr3_pcm_best_channel(&[1000, 100, 2000, 200, -500, -50], 2),
        vec![1000, 2000, -500]
    );
    // 通道 1 能量更高 → 取通道 1
    assert_eq!(
        rr3_pcm_best_channel(&[100, 900, 50, 1500, -30, 700], 2),
        vec![900, 1500, 700]
    );
    // 并列（全 0）→ 取首通道（确定性）
    assert_eq!(rr3_pcm_best_channel(&[0, 0, 0, 0], 2), vec![0, 0]);
    // 四声道：2 帧 × 4 → 2 点；通道 3 能量最高（16+64）
    assert_eq!(
        rr3_pcm_best_channel(&[0, 0, 0, 4, 8, 8, 8, 8], 4),
        vec![4, 8]
    );
    // 空输入安全
    assert!(rr3_pcm_best_channel(&[], 2).is_empty());
}

// ---------- TDOA 判向用例（B批 b3b，golden 镜像 /tmp/rr3-b3-smoke/golden_doa.py） ----------

/// 确定性宽带源：xorshift32(13,17,5) 高 16 位带符号截取 × amp 定标——
/// golden 向量任何机器重跑同值（S-8 整型化纪律的测试侧落点）。
struct Xorshift(u32);
impl Xorshift {
    fn next_i16_scaled(&mut self, amp: i32) -> i16 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        let v = (x as i32) >> 16; // 高 16 位带符号 → [−32768, 32767]
        ((v * amp) >> 15) as i16
    }
}

/// 延迟立体声窗：τ>0 声源在右（frame[0]=物理右麦先到，R 无延迟、L 延迟 |τ|），
/// τ<0 声源在左（frame[1] 先到）——与机制取样语义 1:1（b3 回归双侧交叉定案：
/// esp32 档 frame[0]=物理右、frame[1]=物理左，lag>0 = frame[0] 先到 = 右）。
fn delayed_stereo(tau: i32, frames: usize, amp: i32) -> Vec<i16> {
    let mut xs = Xorshift(0x1234_5678);
    let u: Vec<i16> = (0..frames + tau.unsigned_abs() as usize)
        .map(|_| xs.next_i16_scaled(amp))
        .collect();
    let (r, l): (Vec<i16>, Vec<i16>) = if tau >= 0 {
        (u[tau as usize..].to_vec(), u[..frames].to_vec())
    } else {
        (u[..frames].to_vec(), u[(-tau) as usize..].to_vec())
    };
    (0..frames).flat_map(|t| [r[t], l[t]]).collect()
}

fn doa_cfg() -> Rr3DoaCfg {
    Rr3DoaCfg {
        sample_rate_hz: 16000,
        frame_ms: 20,
        mic_baseline_mm: 150,
        window_rms_min: 800,
        bin_threshold_deg: 30,
    }
}

/// golden 向量（生成器 golden_doa.py 与本件 1:1 镜像）：延迟反演逐位钉死——
/// 符号约定、物理 lag 上限、Q7 抛物线、K_Q15 定标、asin 查表任何一步漂移
/// 当场红。0.2s 窗（onset 量级）+ 全幅度 3000（rms ~1732 > 门 800）。
#[test]
fn tdoa_inverts_delay_and_bins_pm90() {
    let cfg = doa_cfg();
    for (tau, want_deg, want_bin) in [
        (-7, -90, Rr3DoaBin::Left),
        (-4, -35, Rr3DoaBin::Left),
        (-2, -17, Rr3DoaBin::Center),
        (0, 0, Rr3DoaBin::Center),
        (2, 17, Rr3DoaBin::Center),
        (4, 35, Rr3DoaBin::Right),
        (7, 90, Rr3DoaBin::Right),
    ] {
        let samples = delayed_stereo(tau, 3200, 3000);
        let fix = rr3_doa_fix(&samples, 2, &cfg).unwrap_or_else(|| panic!("tau={tau} 应判出"));
        assert_eq!(fix.deg, want_deg, "tau={tau} 反演角");
        assert_eq!(fix.bin, want_bin, "tau={tau} 档位");
    }
}

/// 单声道/空窗/全静音/欠能量窗恒 None（无方向信息不硬判）；能量门按帧评，
/// 窗内只要有一帧过阈即判（onset 窗"头部静音尾部语音"形状照样判出）。
#[test]
fn tdoa_returns_none_for_mono_silence_and_quiet() {
    let cfg = doa_cfg();
    assert_eq!(
        rr3_doa_fix(&[10, -10, 20, -20], 1, &cfg),
        None,
        "mono 无方向"
    );
    assert_eq!(rr3_doa_fix(&[], 2, &cfg), None, "空窗");
    assert_eq!(rr3_doa_fix(&[0; 640], 2, &cfg), None, "全静音");
    // amp 375 峰值 → 帧 rms ~216 < 800：整窗欠能量（golden：fmax=224 → None）
    let quiet = delayed_stereo(4, 3200, 375);
    assert_eq!(rr3_doa_fix(&quiet, 2, &cfg), None, "欠能量窗不判");
    // 0.2s 窗里只有尾 0.02s 有声：单帧过阈即过门，照样判出右向
    let mut burst = vec![0i16; 3200 * 2];
    for (i, v) in delayed_stereo(4, 320, 3000).into_iter().enumerate() {
        burst[(3200 - 320) * 2 + i] = v;
    }
    assert_eq!(
        rr3_doa_fix(&burst, 2, &cfg).map(|f| f.bin),
        Some(Rr3DoaBin::Right),
        "尾部语音帧过门判向"
    );
}

/// 分档阈值注入面（S-5 延续）：同一窗换 `bin_threshold_deg` 即换档，
/// 反演角与阈值无关。
#[test]
fn injected_threshold_moves_the_bins() {
    let samples = delayed_stereo(2, 3200, 3000); // 17°
    let wide = rr3_doa_fix(&samples, 2, &doa_cfg()).unwrap();
    assert_eq!(wide.bin, Rr3DoaBin::Center, "30° 边界：17° 落中档");
    let mut narrow = doa_cfg();
    narrow.bin_threshold_deg = 10;
    let narrowed = rr3_doa_fix(&samples, 2, &narrow).unwrap();
    assert_eq!(narrowed.bin, Rr3DoaBin::Right, "10° 边界越界落右档");
    assert_eq!(wide.deg, narrowed.deg, "反演角与阈值无关");
}

/// 多通道（>2）取前两路 L/R 对：第三路起的内容不影响判向结果。
#[test]
fn tdoa_uses_first_two_channels_only() {
    let stereo = delayed_stereo(-4, 1600, 3000);
    let three: Vec<i16> = stereo
        .as_chunks::<2>()
        .0
        .iter()
        .flat_map(|f| [f[0], f[1], -777])
        .collect();
    let cfg = doa_cfg();
    assert_eq!(
        rr3_doa_fix(&stereo, 2, &cfg),
        rr3_doa_fix(&three, 3, &cfg),
        "第三路不得影响 L/R 对的判向"
    );
}

// ---------- 两腿对拍用例（0.0.5 批K H1，architecture §4.0.3 判据②） ----------

#[test]
fn parity_leg1_capture_output_train_is_deterministic() {
    // 腿一·确定性：同一延迟立体声序列两跑，(下混流, 判向序列) 逐帧相等
    fn run() -> (Vec<i16>, Vec<Option<Rr3DoaFix>>) {
        let cfg = doa_cfg();
        let seq: Vec<Vec<i16>> = [(-4i32, 800usize), (0, 800), (5, 800)]
            .iter()
            .map(|&(tau, n)| delayed_stereo(tau, n, 3000))
            .collect();
        let mixed: Vec<i16> = seq
            .iter()
            .flat_map(|s| rr3_pcm_best_channel(s, 2))
            .collect();
        let fixes: Vec<Option<Rr3DoaFix>> = seq.iter().map(|s| rr3_doa_fix(s, 2, &cfg)).collect();
        (mixed, fixes)
    }
    assert_eq!(run(), run(), "同输入序列两跑逐帧相等（腿一·确定性）");
}

#[test]
fn parity_leg2_capture_matches_ruling_semantics() {
    // 腿二·正确性：分档等号边界独立重写（|deg| ≤ 30° 含等号落中档）；
    // 零值输入不产帧循环（0 帧换算、空样本安全返回）
    for (tau, want_bin) in [(-4, Rr3DoaBin::Left), (4, Rr3DoaBin::Right)] {
        let fix = rr3_doa_fix(&delayed_stereo(tau, 3200, 3000), 2, &doa_cfg()).unwrap();
        assert_eq!(fix.bin, want_bin, "tau={tau} 档位");
    }
    // 零值：0ms 节拍与 0Hz 采样都换算出 0 帧（调用方不得拿 0 帧当 buf 死循环）
    assert_eq!(rr3_frames_per_tick(16000, 0), 0, "(16000, 0) → 0 帧");
    assert_eq!(rr3_frames_per_tick(0, 20), 0, "(0, 20) → 0 帧");
    // 零样本单声道化/判向安全返回（空进空出，不 panic 不产帧）
    assert!(rr3_pcm_best_channel(&[], 2).is_empty());
    assert_eq!(rr3_doa_fix(&[], 2, &doa_cfg()), None, "空窗无方向可判");
}
