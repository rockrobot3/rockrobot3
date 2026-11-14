//! sim 波形模式与 `RR3_HAL_SIM_WAVE` 取值解析。

/// sim 波形模式（环境变量 `RR3_HAL_SIM_WAVE` 的取值集，见 `backend.rs`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3SimWave {
    /// 440Hz 正弦、幅度 12000：能量远超 config.toml 的 `rms_start = 1500`，
    /// 专供 VAD 起音链路联调
    Sine,
    /// 确定性噪声（xorshift64* 固定种子）、幅度 ±8000
    Noise,
    /// 全零静音
    Silence,
    /// 1s 正弦 + 1s 静音循环：VAD 周期性发 onset/offset（≈2s 一轮），
    /// 多轮对话闭环（tools/sim --brain）的驱动源。门控按**绝对帧序号**
    /// （`idx/声道数/rate` 的偶秒有声——帧才是音频时间，交错样点序号会在
    /// 多通道下把窗口折半，批F4 真板实测教训），与 Sine 同一确定性纪律
    Burst,
}

impl Rr3SimWave {
    /// 波形词表（取值串与波形的单点真值）
    const WORDS: [(Rr3SimWave, &'static str); 4] = [
        (Rr3SimWave::Sine, "sine"),
        (Rr3SimWave::Noise, "noise"),
        (Rr3SimWave::Silence, "silence"),
        (Rr3SimWave::Burst, "burst"),
    ];

    /// 环境变量取值 → 波形（`sine` / `noise` / `silence` / `burst`，大小写不敏感）
    pub fn rr3_sim_wave_parse(s: &str) -> Option<Self> {
        let lower = s.to_ascii_lowercase();
        Self::WORDS
            .iter()
            .find(|(_, w)| *w == lower.as_str())
            .map(|(w, _)| *w)
    }

    pub fn rr3_sim_wave_as_str(&self) -> &'static str {
        Self::WORDS
            .iter()
            .find(|(w, _)| w == self)
            .map_or("", |(_, s)| *s)
    }
}
