//! rr-hal-api —— 能力面契约（no_std，ADR-0018 修订 1 ③，0.0.5 批I F1）。
//!
//! 件 ↔ 硬件的唯一契约面：8 trait **同步签名**、去 `Send + Sync`、值类型去
//! `String`/`Vec`（core-only）。此前件经 `rr3_block_on`（yield_now 自旋）驱动
//! `#[async_trait]` 的 `rr-hal` 七 trait——件把 std 的调度形状当成了机制。本 crate
//! 把契约层从实现层（`rr-hal` 的 Linux/sim 后端）切出来：async 留模块侧（与
//! ADR-0008 裁定 1 同线），契约名与路径由 `rr-hal` re-export 保持不变。
//!
//! # 定形要点
//!
//! - **同步签名**：方法即调用即返回，无 future。写方法 `&mut self`（单所有者——
//!   bare 档无 Arc 可言，host 档组合件主循环独占持有）；读方法 `&self`。
//! - **值类型去 String**：`Rr3LinkState.ssid` 从 `Option<String>` 改 [`Rr3SsidText`]
//!   （定长 + 长度，SSID 上限 32 octets）；错型 [`Rr3HalError::InvalidParam`] 从
//!   `String` 改 [`Rr3ErrText`]（定长缓冲，实现层 format! 后截断入缓冲，文案语义
//!   不变——lib 测试对文案子串的断言继续成立）。
//! - **`Vec<i16>` 换借用形状**：[`Rr3AudioSource::read`] 改为写入调用方缓冲、返回
//!   有效样本数；[`Rr3AudioSink::write`] 改为借用切片（采样配置在 `open` 时固定，
//!   不随每次读写重复传递）。`Rr3PcmStream`（cfg + `Vec<i16>`）降为承载层数据，
//!   留在 `rr-hal`（std 侧），不再出现在契约签名里。
//!
//! 占位后端 [`Rr3UnavailableSource`] 一并迁入：它承载"能力缺失必须显式回错，不许
//! 静默退化"的设计裁定，no_std 可达面（rtos/bare 档）直接复用。

#![cfg_attr(not(test), no_std)]
// 契约错型 Rr3HalError 携带定长 128 octets 文案（Rr3ErrText）——Result 的体积是
// F1"值类型去 String"裁定的直接结果，不是失误：result_large_err 的默认阈值恰被
// 文案缓冲顶到。契约形状不让步，lint 在 crate 级显式豁免并留注（不留无解释 allow）。
#![allow(clippy::result_large_err)]

/// HAL 采样配置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3AudioCfg {
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub bit_depth: u8,
}

/// 与 legacy ESP32 链路一致的默认拾音格式（16k/单声道/16bit）
pub const RR3_AUDIO_CFG_16K_MONO16: Rr3AudioCfg = Rr3AudioCfg {
    sample_rate_hz: 16000,
    channels: 1,
    bit_depth: 16,
};

/// 定长错误文案（[`Rr3HalError::InvalidParam`] 载荷）。no_std 面没有 `String`，
/// 但实现层（Linux 后端）的错误文案是动态的（io error、路径、数值插值）——
/// 实现层 `format!` 后经 [`Rr3ErrText::rr3_err_text_new`] 截断入缓冲，按 char
/// 边界截（保证 `as_str` 恒合法），文案语义不变。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ErrText {
    bytes: [u8; 128],
    len: u8,
}

impl Rr3ErrText {
    /// 缓冲容量（字节数）
    pub const fn rr3_err_text_capacity() -> usize {
        128
    }

    /// 从字符串构造，按 char 边界截断到容量（截断不报错：错误文案是给人看的，
    /// 截断的尾巴不带语义——判据面靠变体与静态前缀，不靠尾巴）
    pub fn rr3_err_text_new(s: &str) -> Self {
        let mut bytes = [0u8; 128];
        let mut len = 0usize;
        for (i, b) in s.as_bytes().iter().enumerate() {
            if i >= bytes.len() {
                break;
            }
            bytes[i] = *b;
            len = i + 1;
        }
        // 退到 char 边界（UTF-8 多字节序列不劈半）
        while len > 0 && s.get(..len).is_none() {
            len -= 1;
        }
        Self {
            bytes,
            len: len as u8,
        }
    }

    /// 文本视图（构造时已按 char 边界截断，恒合法）
    pub fn rr3_err_text_as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

/// SSID 定长文本（[`Rr3LinkState`] 载荷）。IEEE 802.11 SSID 上限 32 octets，
/// 定长 + 长度承载，`⊙` 跨片不再过 `String`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3SsidText {
    bytes: [u8; 32],
    len: u8,
}

impl Rr3SsidText {
    /// 缓冲容量（octets，IEEE 802.11 SSID 上限）
    pub const fn rr3_ssid_text_capacity() -> usize {
        32
    }

    /// 从字符串构造，按 char 边界截断到 32 octets（SSID 是观测读数不是指令，
    /// 超长截断进摘要；真 SSID 本就不该超限）
    pub fn rr3_ssid_text_new(s: &str) -> Self {
        let mut bytes = [0u8; 32];
        let mut len = 0usize;
        for (i, b) in s.as_bytes().iter().enumerate() {
            if i >= bytes.len() {
                break;
            }
            bytes[i] = *b;
            len = i + 1;
        }
        while len > 0 && s.get(..len).is_none() {
            len -= 1;
        }
        Self {
            bytes,
            len: len as u8,
        }
    }

    /// 文本视图
    pub fn rr3_ssid_text_as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

/// 链路状态摘要
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3LinkState {
    pub up: bool,
    pub ssid: Option<Rr3SsidText>,
    pub rssi_dbm: Option<i8>,
}

/// 电池状态摘要（关断决策留在 rr3-power 本地，不经总线）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3BatteryState {
    pub voltage_mv: u16,
    pub percent: u8,
    pub charging: bool,
}

/// 轨压/域压档位（批N2 P-15，全案 §3.1 ⑥）：`Ok`/`Mild`/`Severe` 是电平三档
/// （板侧 `RailTier` 同名同序的契约投影），[`Rr3RailTier::Unknown`] 是**可信度**第四态——
/// T-6：`age_ms` 超上界或时基取不到时读数不可信，在接口上必须与"新鲜的 Severe"可区分
/// （没有它，陈旧的 `Severe` 与从未取到的 `Ok` 不可区分，面板 / brain 拿着不敢信的数据
/// 照样判定）。
///
/// 消费纪律（全案 §6 运行期读数可信度）：`Unknown` 在**执行面**一律按 `Severe` 那一侧
/// 处置（闸门保持置位 / 不许解禁），在**展示面**必须原样显示、**不许折算成 `Ok`**。
/// 该规则的装载/运行期执法归校验⑥-c2（批N2 件三），契约层只供给取值，不判策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3RailTier {
    Ok,
    Mild,
    Severe,
    Unknown,
}

impl Rr3RailTier {
    /// 档名（守护输出行 `INFO rail_tier=` 同串；板侧 `RailTier::name` 投影到此，
    /// 第四态补 `"unknown"`——两条线的文案自此同源一个词表）。
    pub fn rr3_rail_tier_name(self) -> &'static str {
        match self {
            Rr3RailTier::Ok => "ok",
            Rr3RailTier::Mild => "mild",
            Rr3RailTier::Severe => "severe",
            Rr3RailTier::Unknown => "unknown",
        }
    }
}

/// 轨压/域压读数（批N2 P-15）：mv 与档位同帧，另带采样龄期。
///
/// `age_ms` 的新鲜度上界住在 profile 的域声明（`[[supplies.domain]].max_age_ms`，
/// 装载期校验⑥-c1 管它必须 > 0），契约层只带观测值——消费者拿 `age_ms` 对自己的
/// `max_age_ms` 判龄（判据一条写完：`age_ms > max_age_ms ⇒ tier = Unknown`，
/// 全案 §6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3RailState {
    /// 轨压/域压 mV（分压换算后的轨侧值，同板侧 `RailMonitor::sample` 口径）
    pub mv: u16,
    /// 电平档位或可信度第四态（类型注记见 [`Rr3RailTier`]）
    pub tier: Rr3RailTier,
    /// 采样龄期 ms（距最近一次成功采样；时基取不到时由后端如实给保守值并置
    /// [`Rr3RailTier::Unknown`]——Y-4：拿不到 epoch 折 0 的假龄期不许冒充新鲜）
    pub age_ms: u32,
}

/// HAL 错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3HalError {
    /// 后端未实现该能力
    Unsupported(&'static str),
    /// 设备忙 / 已被其他模块持有
    Busy,
    /// 参数越界（定长文案，见 [`Rr3ErrText`]）
    InvalidParam(Rr3ErrText),
}

impl core::fmt::Display for Rr3HalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unsupported(b) => write!(f, "HAL 后端不支持: {b}"),
            Self::Busy => write!(f, "HAL 设备忙"),
            Self::InvalidParam(s) => write!(f, "HAL 参数非法: {}", s.rr3_err_text_as_str()),
        }
    }
}

/// 数字输出（继电器、LED）
pub trait Rr3GpioOut {
    fn set(&mut self, level: bool) -> Result<(), Rr3HalError>;
    fn level(&self) -> Result<bool, Rr3HalError>;
}

/// PWM 舵机（角度级指令，闭环在 rr3-act 反射环内）
pub trait Rr3PwmServo {
    fn set_angle(&mut self, deg: u16) -> Result<(), Rr3HalError>;
    fn angle(&self) -> Result<u16, Rr3HalError>;
}

/// 音频源（上行拾音）。`open` 固定采样配置；`read` 写入调用方缓冲、返回有效
/// 样本数（0 = 无新数据）。缓冲形状由调用方定（固定帧长用栈数组，承载层可
/// 用堆缓冲），契约层不再拥有 `Vec`。
pub trait Rr3AudioSource {
    fn open(&mut self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError>;
    fn read(&mut self, buf: &mut [i16]) -> Result<usize, Rr3HalError>;
}

/// 音频播放面（下行出话）。与 [`Rr3AudioSource`] 对称：`open` 固定采样配置，
/// `write` 消费借用切片。TTS 文本到 PCM 的合成属模块侧（rr3-interact），不进
/// HAL；HAL 只搬运已合成的波形。
pub trait Rr3AudioSink {
    fn open(&mut self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError>;
    fn write(&mut self, samples: &[i16]) -> Result<(), Rr3HalError>;
}

/// 电源读数
pub trait Rr3Power {
    fn battery(&self) -> Result<Rr3BatteryState, Rr3HalError>;
}

/// 轨压/域压能力面（批N2，ADR-0021 裁定 2 第七位供给的读出面；全案 §3.1 ⑥）。
///
/// 新立 trait 而不给 [`Rr3Power`] 加方法——增长律"只增不改"：电池读数是单值单域
/// 的既有形状（既有实现与 round-trip 用例不返工），多域观测是另一对象。三方法在
/// 立 trait 时一次定形（§18 F-32：不许有一个未定义就进文档的方法名）：
///
/// - [`Rr3Rail::rr3_read`]：域压读数，[`Rr3RailState`] 的 mv/tier/age_ms 同帧给出；
/// - [`Rr3Rail::rr3_current_ma`]：逐域电流（预算调度的前提，§1 坑 9）——无
///   per-domain 采样的后端回 [`Rr3HalError::Unsupported`]，不许静默回 0
///   （0 mA 是"读数为零"，不是"没有采样"，两者的可信度语义不同）；
/// - [`Rr3Rail::rr3_switch`]：可切域通断（断电轨 T1/T2 的调度抓手）——`switchable
///   = false` 的域由后端回 `Unsupported`，接闸纪律（未标定不武装）归域声明
///   `armed` 位与运行期调度，不进签名。
///
/// `domain` 取值 = profile `[[supplies.domain]].name` 在册名（`rail5v` 这类；
/// 跨形态名单源 = 该形态域集，§18 F-5：文档示例值不进代码）。`&str` 借用入参、
/// 返回值类型拷贝出参——契约面 core-only，与 [`Rr3Power`] 同一线。
pub trait Rr3Rail {
    /// 读一次域压（mv/tier/age_ms 同帧）。无测点域（`sense = "none"`）回
    /// `Unsupported`——缺测点显式回错，不静默给陈旧快照。
    fn rr3_read(&self, domain: &str) -> Result<Rr3RailState, Rr3HalError>;
    /// 读一次逐域电流 mA。无电流采样域（`current_sense = "none"`）回 `Unsupported`。
    fn rr3_current_ma(&self, domain: &str) -> Result<u16, Rr3HalError>;
    /// 可切域通断（只收 `switchable = true` 的域）。
    fn rr3_switch(&mut self, domain: &str, on: bool) -> Result<(), Rr3HalError>;
}

/// 网络链路读数
pub trait Rr3Link {
    fn state(&self) -> Result<Rr3LinkState, Rr3HalError>;
}

/// 8x8 点阵单帧（R11③ 定案档）：行主序 8 字节，每字节 bit7 为该行最左点
/// （MAX7219 行寄存器直排）。类型在契约层定义是刻意的：它是 [`Rr3Display`]
/// trait 签名的事实源，模块面（rr3-interact 表情机）只引用不复制定义。
pub type Rr3FaceFrame = [u8; 8];

/// 显示面（8x8 点阵帧渲染，R11③——单 trait 帧面）。
///
/// "状态→帧"映射与表情白名单是模块面策略（同 led 灯效白名单先例，不进契约层）：
/// 表情机出帧、显示件收帧，HAL 只搬运已定的位图。`draw` 语义是整帧替换
/// （MAX7219 是 8 个行寄存器的一次全刷），没有局部改写。
pub trait Rr3Display {
    /// 渲染一帧（整帧替换，阻塞至行寄存器落定——事件级慢速面，无帧队列）
    fn draw(&mut self, frame: &Rr3FaceFrame) -> Result<(), Rr3HalError>;
    /// 读回当前帧（开机探活与对账：读回 == 上次 draw 的帧）
    fn frame(&self) -> Result<Rr3FaceFrame, Rr3HalError>;
}

/// 占位后端：所有能力返回 `Unsupported`。它承载的是"能力缺失必须显式回错，不许
/// 静默退化"的设计裁定，不是一时代替后端的临时件。音频两路（source/sink）由
/// `rr-hal` 的 `backend::sim` / `backend::linux` 承担；gpio/pwm 已有 linux 后端；
/// power/link/display 仍由本占位回错，直到对应 linux 后端实装（display 真件档
/// MAX7219 SPI，R11③）。no_std 可达，rtos/bare 档直接复用。
pub struct Rr3UnavailableSource;

impl Rr3GpioOut for Rr3UnavailableSource {
    fn set(&mut self, _level: bool) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/gpio"))
    }
    fn level(&self) -> Result<bool, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/gpio"))
    }
}

impl Rr3PwmServo for Rr3UnavailableSource {
    fn set_angle(&mut self, _deg: u16) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/pwm"))
    }
    fn angle(&self) -> Result<u16, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/pwm"))
    }
}

impl Rr3AudioSource for Rr3UnavailableSource {
    fn open(&mut self, _cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/audio"))
    }
    fn read(&mut self, _buf: &mut [i16]) -> Result<usize, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/audio"))
    }
}

impl Rr3AudioSink for Rr3UnavailableSource {
    fn open(&mut self, _cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/audio"))
    }
    fn write(&mut self, _samples: &[i16]) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/audio"))
    }
}

impl Rr3Power for Rr3UnavailableSource {
    fn battery(&self) -> Result<Rr3BatteryState, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/power"))
    }
}

impl Rr3Rail for Rr3UnavailableSource {
    fn rr3_read(&self, _domain: &str) -> Result<Rr3RailState, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/rail"))
    }
    fn rr3_current_ma(&self, _domain: &str) -> Result<u16, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/rail"))
    }
    fn rr3_switch(&mut self, _domain: &str, _on: bool) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/rail"))
    }
}

impl Rr3Link for Rr3UnavailableSource {
    fn state(&self) -> Result<Rr3LinkState, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/link"))
    }
}

impl Rr3Display for Rr3UnavailableSource {
    fn draw(&mut self, _frame: &Rr3FaceFrame) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/display"))
    }
    fn frame(&self) -> Result<Rr3FaceFrame, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/display"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyGpio(bool);

    impl Rr3GpioOut for DummyGpio {
        fn set(&mut self, level: bool) -> Result<(), Rr3HalError> {
            self.0 = level;
            Ok(())
        }
        fn level(&self) -> Result<bool, Rr3HalError> {
            Ok(self.0)
        }
    }

    /// dyn 兼容校验：trait 对象必须能装箱/借用（承载层要持有异构设备集合）。
    /// 同步 trait 无 `Send + Sync` 约束——单所有者语义，承载层主循环独占。
    #[test]
    fn hal_traits_are_object_safe() {
        let mut boxed: Box<dyn Rr3GpioOut> = Box::new(DummyGpio(false));
        assert!(boxed.set(true).is_ok());
        assert!(boxed.level().unwrap());
        let _: Box<dyn Rr3AudioSource> = Box::new(Rr3UnavailableSource);
        let _: Box<dyn Rr3AudioSink> = Box::new(Rr3UnavailableSource);
        let _: Box<dyn Rr3Power> = Box::new(Rr3UnavailableSource);
        let _: Box<dyn Rr3Rail> = Box::new(Rr3UnavailableSource);
        let _: Box<dyn Rr3Link> = Box::new(Rr3UnavailableSource);
        let _: Box<dyn Rr3PwmServo> = Box::new(Rr3UnavailableSource);
        let _: Box<dyn Rr3Display> = Box::new(Rr3UnavailableSource);
    }

    #[test]
    fn audio_default_cfg_matches_legacy_pcm() {
        let cfg = RR3_AUDIO_CFG_16K_MONO16;
        assert_eq!(
            (cfg.sample_rate_hz, cfg.channels, cfg.bit_depth),
            (16000, 1, 16)
        );
    }

    #[test]
    fn hal_error_carries_backend_name() {
        let e = Rr3HalError::Unsupported("linux/iio");
        assert_eq!(e.to_string(), "HAL 后端不支持: linux/iio");
        let e = Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new("行程 [0, 180]"));
        assert_eq!(e.to_string(), "HAL 参数非法: 行程 [0, 180]");
    }

    /// 定长缓冲：容量内逐字保留；超长按 char 边界截断（多字节字符不劈半），
    /// `as_str` 恒合法。
    #[test]
    fn err_text_truncates_on_char_boundary() {
        let t = Rr3ErrText::rr3_err_text_new("行程 [0, 180] 越域");
        assert_eq!(t.rr3_err_text_as_str(), "行程 [0, 180] 越域");
        let long = "汉".repeat(200);
        let t = Rr3ErrText::rr3_err_text_new(&long);
        // char 边界截断后字节数不必顶满容量（42×3=126 ≤ 128），只须不超且不劈字符
        assert!(t.rr3_err_text_as_str().len() <= Rr3ErrText::rr3_err_text_capacity());
        assert_eq!(t.rr3_err_text_as_str().chars().count(), 42); // 42×3=126 ≤ 128
    }

    #[test]
    fn ssid_text_roundtrips_and_truncates() {
        let t = Rr3SsidText::rr3_ssid_text_new("spharx-5g");
        assert_eq!(t.rr3_ssid_text_as_str(), "spharx-5g");
        let long = "s".repeat(40);
        let t = Rr3SsidText::rr3_ssid_text_new(&long);
        assert_eq!(
            t.rr3_ssid_text_as_str().len(),
            Rr3SsidText::rr3_ssid_text_capacity()
        );
    }

    /// 占位后端回错（能力缺失显式回错，不静默退化）
    #[test]
    fn placeholder_returns_unsupported() {
        let mut p = Rr3UnavailableSource;
        assert!(matches!(
            p.set(true),
            Err(Rr3HalError::Unsupported("placeholder/gpio"))
        ));
        assert!(matches!(
            p.battery(),
            Err(Rr3HalError::Unsupported("placeholder/power"))
        ));
        assert!(matches!(
            p.rr3_read("rail5v"),
            Err(Rr3HalError::Unsupported("placeholder/rail"))
        ));
        assert!(matches!(
            p.rr3_current_ma("rail5v"),
            Err(Rr3HalError::Unsupported("placeholder/rail"))
        ));
        assert!(matches!(
            p.rr3_switch("rail5v", false),
            Err(Rr3HalError::Unsupported("placeholder/rail"))
        ));
    }

    /// P-15（T-6）：档位词表四态在册——`Unknown` 与三电平档在接口上可区分，
    /// 档名串与板侧守护行同源（三档同 `RailTier::name`，第四态补 `unknown`）。
    #[test]
    fn rail_tier_has_four_states_with_pinned_names() {
        assert_eq!(Rr3RailTier::Ok.rr3_rail_tier_name(), "ok");
        assert_eq!(Rr3RailTier::Mild.rr3_rail_tier_name(), "mild");
        assert_eq!(Rr3RailTier::Severe.rr3_rail_tier_name(), "severe");
        assert_eq!(Rr3RailTier::Unknown.rr3_rail_tier_name(), "unknown");
        // 读数形状：mv/tier/age_ms 同帧（T-6 断头清单的闭合形状——龄期与档位
        // 不可拆着给，消费者判龄判的是同一帧）
        let s = Rr3RailState {
            mv: 4400,
            tier: Rr3RailTier::Unknown,
            age_ms: 9_999,
        };
        assert_eq!(s.tier, Rr3RailTier::Unknown);
        assert_ne!(Rr3RailState {
            mv: 4400,
            tier: Rr3RailTier::Severe,
            age_ms: 0
        }, s, "陈旧的 Severe 与新鲜的 Severe 在接口上必须可区分（age_ms 入等值面）");
    }
}
