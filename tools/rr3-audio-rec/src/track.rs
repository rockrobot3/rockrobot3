//! 记账面：seq 连续性/重启、DoA 冒烟出角、板侧声明率与连续段实测率。

use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::watch::WatchState;
use crate::NOMINAL_RATE_FALLBACK_HZ;

/// seq 连续性记账（wrapping u32）：断档数 = (seq - prev) - 1（wrapping 域），
/// 首片记账不断档；大步进（≥ 0x8000_0000）= 板侧重启（重连开口翻转复位，seq
/// 归零重走）记 `restarts`——合法事件，不按 wrapping 补天文 gap。
pub(crate) struct SeqTracker {
    pub(crate) prev: Option<u32>,
    pub(crate) gaps: u64,
    pub(crate) restarts: u64,
}

/// DoA 冒烟面（`--doa`，批D⑤ 判据③）：逐 1s 窗双通道 ILD 出角。
/// **未标定冒烟口径**：az = 20·log10(rms_l/rms_r) × [`AZ_PER_DB`]（±90° ↔ ±12dB），
/// 判据只取"左/右发声方位角**符号分离**"，不冒领精度（标定留待真机装距实测）。
/// 正 az = 左通道占优。静默窗（peak ≤ [`DOA_ACTIVE_PEAK`]）只记账不出角。
pub(crate) struct DoaTracker {
    l_sq: f64,
    r_sq: f64,
    n: usize,
    peak: i32,
    win: usize,
    t0: Instant,
    /// active 窗出角账：(窗号, az°)——收兵汇总用。
    pub(crate) azs: Vec<(usize, f64)>,
}

/// ILD→方位角换算系数（°/dB，未标定冒烟值：±12dB 铺满 ±90°）。
const AZ_PER_DB: f64 = 7.5;
/// 1s 窗帧数（标称 16kHz；实测率偏差只挪窗边界，不影响 ILD 口径）。
pub(crate) const DOA_WIN_FRAMES: usize = 16000;
/// active 判据：窗内任意样本 |幅| 超此值才算"有事件"，静默窗不出角。
const DOA_ACTIVE_PEAK: i32 = 1000;

impl DoaTracker {
    pub(crate) fn new() -> Self {
        Self {
            l_sq: 0.0,
            r_sq: 0.0,
            n: 0,
            peak: 0,
            win: 0,
            t0: Instant::now(),
            azs: Vec::new(),
        }
    }

    /// 喂一片 s16le 交错 PCM；攒满 1s 窗回一行打印（None = 窗未满）。
    pub(crate) fn feed(&mut self, samples: &[u8]) -> Option<String> {
        self.rr3_doa_accum(samples);
        if self.n < DOA_WIN_FRAMES {
            return None;
        }
        self.win += 1;
        let t = self.t0.elapsed().as_secs_f64();
        let out = self.rr3_doa_line(t);
        self.l_sq = 0.0;
        self.r_sq = 0.0;
        self.n = 0;
        self.peak = 0;
        Some(out)
    }

    /// 一片 s16le 交错 PCM 的平方和/峰值/帧数累加。
    fn rr3_doa_accum(&mut self, samples: &[u8]) {
        let mut i = 0;
        while i + 3 < samples.len() {
            let l = i16::from_le_bytes([samples[i], samples[i + 1]]) as i32;
            let r = i16::from_le_bytes([samples[i + 2], samples[i + 3]]) as i32;
            self.l_sq += (l as f64) * (l as f64);
            self.r_sq += (r as f64) * (r as f64);
            self.peak = self.peak.max(l.abs()).max(r.abs());
            self.n += 1;
            i += 4;
        }
    }

    /// 满窗一行打印（active 窗的方位角同时入袋）。
    fn rr3_doa_line(&mut self, t: f64) -> String {
        let l_rms = (self.l_sq / self.n as f64).sqrt();
        let r_rms = (self.r_sq / self.n as f64).sqrt();
        if let Some((ild, az)) = self.rr3_doa_az(l_rms, r_rms) {
            format!(
                "[doa] t={t:5.1}s win={} peak={} l_rms={l_rms:.0} r_rms={r_rms:.0} ild={ild:+.1}dB az={az:+.0}° *",
                self.win, self.peak
            )
        } else {
            format!(
                "[doa] t={t:5.1}s win={} peak={} l_rms={l_rms:.0} r_rms={r_rms:.0} （静默）",
                self.win, self.peak
            )
        }
    }

    /// active 窗的 ILD 与方位角（静默窗 None，不出角也不进账）。
    fn rr3_doa_az(&mut self, l_rms: f64, r_rms: f64) -> Option<(f64, f64)> {
        if self.peak > DOA_ACTIVE_PEAK && l_rms > 0.0 && r_rms > 0.0 {
            let ild = 20.0 * (l_rms / r_rms).log10();
            let az = (ild * AZ_PER_DB).clamp(-90.0, 90.0);
            self.azs.push((self.win, az));
            Some((ild, az))
        } else {
            None
        }
    }

    /// 收兵汇总：active 窗方位角列表 + 符号分离判定（正负都有 = 左右发声方向可分）。
    pub(crate) fn summary(&self) -> String {
        if self.azs.is_empty() {
            return "[doa] 无 active 窗（无出角）——事件音量或位置不足".into();
        }
        let list = self
            .azs
            .iter()
            .map(|(w, az)| format!("win{w}={az:+.0}°"))
            .collect::<Vec<_>>()
            .join(" ");
        let has_pos = self.azs.iter().any(|(_, az)| *az > 5.0);
        let has_neg = self.azs.iter().any(|(_, az)| *az < -5.0);
        let verdict = rr3_doa_verdict(has_pos, has_neg);
        format!("[doa] 汇总 active={}：{list} —— {verdict}", self.azs.len())
    }
}

/// 符号分离判定文案（正负都有 = 左右发声方向可分）。索引 = (正 << 0) | (负 << 1)。
fn rr3_doa_verdict(has_pos: bool, has_neg: bool) -> &'static str {
    const TEXT: [&str; 4] = [
        "全部近 0°（ILD 不显著，查音量/距离）",
        "仅正向角（对照位缺或方向映射相反）",
        "仅负向角（对照位缺或方向映射相反）",
        "左右符号分离 ✓（冒烟过）",
    ];
    TEXT[has_pos as usize | (has_neg as usize) << 1]
}

impl SeqTracker {
    pub(crate) fn feed(&mut self, seq: u32) -> u64 {
        if let Some(prev) = self.prev {
            let step = seq.wrapping_sub(prev);
            if step >= 0x8000_0000 {
                // 板侧重启：seq 归零重走是合法事件，不是天文丢片
                self.restarts += 1;
            } else if step > 1 {
                self.gaps += (step - 1) as u64;
            }
        }
        self.prev = Some(seq);
        self.gaps
    }
}

/// 分片记账：seq 连续性对账（返回 `broke`=断档/重启，供实测率重锚）＋板侧声明
/// 率首见入账/变更告警（变更=板侧重配置，须重新对账；wav 头以末值/实测为准）。
pub(crate) fn account_chunk(
    seq: u32,
    rate: u32,
    tracker: &mut SeqTracker,
    declared: &mut Option<u32>,
    watch: &WatchState,
) -> bool {
    let before = (tracker.gaps, tracker.restarts);
    tracker.feed(seq);
    let broke = (tracker.gaps, tracker.restarts) != before;
    rr3_declare_rate(rate, declared, watch);
    broke
}

/// 板侧声明率首见入账/变更告警（首片同步写成 wav 头初值）。
fn rr3_declare_rate(rate: u32, declared: &mut Option<u32>, watch: &WatchState) {
    match *declared {
        None => rr3_declare_first(rate, declared, watch),
        Some(r) => rr3_declare_change(r, rate, declared),
    }
}

/// 首片声明率入账（实测率 0.5s 后才可信，先写成 wav 头初值）。
fn rr3_declare_first(rate: u32, declared: &mut Option<u32>, watch: &WatchState) {
    *declared = Some(rate);
    watch.rate.store(rate, Ordering::Release);
}

/// 声明率变更告警（板侧重配置，wav 头以末值/实测为准）。
fn rr3_declare_change(r: u32, rate: u32, declared: &mut Option<u32>) {
    if r != rate {
        eprintln!("rr3-audio-rec: 板侧声明率变更 {r}Hz → {rate}Hz（wav 头以末值/实测为准）");
        *declared = Some(rate);
    }
}

/// 连续段实测率推进：断档/重启/首片重锚（空洞不摊进分母）；段 >0.5s 才把
/// 实测率刷进 `watch.rate`（不足 0.5s 测时标称）。
pub(crate) fn rate_progress(
    anchor: &mut Option<(Instant, u64)>,
    broke: bool,
    now: Instant,
    frames: u64,
    watch: &WatchState,
) {
    rr3_rate_anchor(anchor, broke, now, frames);
    if let Some(r) = rr3_rate_sample(*anchor, now, frames) {
        watch.rate.store(r, Ordering::Release);
    }
}

/// 断档/重启/首片重锚（空洞不摊进分母）。
fn rr3_rate_anchor(anchor: &mut Option<(Instant, u64)>, broke: bool, now: Instant, frames: u64) {
    if broke || anchor.is_none() {
        *anchor = Some((now, frames));
    }
}

/// 连续段实测率：段 >0.5s 且率非零才给值（不足 0.5s 测时标称）。
fn rr3_rate_sample(anchor: Option<(Instant, u64)>, now: Instant, frames: u64) -> Option<u32> {
    let (t0, f0) = anchor?;
    let d = now.duration_since(t0).as_secs_f64();
    (d > 0.5)
        .then(|| (((frames - f0) as f64) / d).round() as u32)
        .filter(|r| *r > 0)
}

/// 收兵期实测率终值：连续段（锚后至最后一片的墙钟 × 段内帧数），断流空洞不摊
/// 进分母。段 <0.5s 或退化率（<1kHz，锚后无实流的边缘段）退板侧声明率；未声明
/// 才退 [`NOMINAL_RATE_FALLBACK_HZ`]。
pub(crate) fn measured_rate(
    anchor: Option<(Instant, u64)>,
    last_frame: Option<Instant>,
    frames_total: u64,
    declared: Option<u32>,
) -> u32 {
    let nominal = declared.unwrap_or(NOMINAL_RATE_FALLBACK_HZ);
    anchor.map_or(nominal, |(t0, f0)| {
        rr3_measure_pick(t0, f0, last_frame.unwrap_or(t0), frames_total, nominal)
    })
}

/// 锚后实测率与 1kHz 下限：退化段（<1kHz）回板侧声明率。
fn rr3_measure_pick(t0: Instant, f0: u64, end: Instant, frames_total: u64, nominal: u32) -> u32 {
    let r = rr3_measure_span(t0, f0, end, frames_total);
    if r >= 1000 {
        r
    } else {
        nominal
    }
}

/// 连续段实测率：段 >0.5s 才给值（不足 0.5s 测时回 0）。
fn rr3_measure_span(t0: Instant, f0: u64, end: Instant, frames_total: u64) -> u32 {
    let d = end.duration_since(t0).as_secs_f64();
    if d > 0.5 {
        (((frames_total - f0) as f64) / d).round() as u32
    } else {
        0
    }
}
