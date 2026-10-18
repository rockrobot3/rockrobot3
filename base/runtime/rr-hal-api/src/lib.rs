//! rr-hal-api —— 能力面契约（no_std，ADR-0018 修订 1 ③，0.0.5 批I F1）。
//!
//! 件 ↔ 硬件的唯一契约面：10 trait **同步签名**、去 `Send + Sync`、值类型去
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

/// ⑥-c2 判据实体（批N2 件三，全案 §6 T-6 运行期读数可信度）：按龄期折算有效档位。
///
/// 判据一条写完——`age_ms > max_age_ms` **或** 时基取不到基准（`age_ms` 观测端
/// 为 `None`）⇒ 折 [`Rr3RailTier::Unknown`]；否则读数原样。`Option` 表达"取不到
/// 基准"零新类型：产出侧（后端/RailMonitor）取不到 epoch 时本就不该编造龄期，
/// `None` 就是如实申报的形状（Y-4：折 0 的假龄期不许冒充新鲜）。
///
/// 单真值点：产出侧与消费侧（执行面闸门、展示面面板、brain）共用本函数，不许
/// 各自手抄 `>` 判据。边界 `age == max_age_ms` 不折（新鲜度上界是闭区间内）。
///
/// 消费纪律（[`Rr3RailTier`] 类型注记）：折出的 `Unknown` 在执行面按 `Severe`
/// 那一侧处置（闸门保持置位 / 不许解禁），在展示面原样显示——本函数只折档，
/// 不判处置策略，执法归校验⑥-c2 消费侧（板侧守护 / 件五 RailMonitor 归属搬家）。
pub fn rr3_rail_tier_effective(
    tier: Rr3RailTier,
    age_ms: Option<u32>,
    max_age_ms: u32,
) -> Rr3RailTier {
    match age_ms {
        None => Rr3RailTier::Unknown,
        Some(age) if age > max_age_ms => Rr3RailTier::Unknown,
        Some(_) => tier,
    }
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

/// 位置读数来源（批N3，全案 §1 坑 11：开环舵机的"位置"不是测量——任何把
/// `angle()` 回读当位置的说法都是该坑的重演，来源必须在接口上可区分）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3PositionSource {
    /// 指令回显：无位置反馈的开环面，"位置"= 最后一次已受理的指令值（记账位，
    /// 非测量——断电/堵转后它与真实轴角可任意偏离，消费方不得当反馈用）
    Commanded,
    /// 闭环反馈（`act.axis.enc` 预留格位，路线 D 换闭环执行器才进场——预留
    /// 不 = 立格，全案 §6 路线表；本版无实现）
    Encoder,
}

/// 位置读数（批N3）：角度与来源同帧——来源缺席的裸角度就是坑 11 的形状
/// （读数可信度与档位同帧给出的 T-6 同一族：拿不到"敢不敢信"的角度不许冒充测量）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3Position {
    /// 角度（度，本轴行程域内；越域值由后端拒收，不进读数）
    pub deg: u16,
    /// 读数来源（[`Rr3PositionSource`]——Commanded 是回显不是测量）
    pub source: Rr3PositionSource,
}

/// 执行件挡块事实（批N3，档案 `hard_stop` 键的契约投影，全案 §4.4/§6）。
/// `unknown` 不是"没有挡块"，是"还没标定过这个事实"——护栏判据按最保守一侧处置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ActHardStop {
    /// 挡块事实未标定：路线 B 禁用、路线 C 自动段禁用（档案注释原文）
    Unknown,
    /// 存在硬挡块（标定实测过）
    Yes,
    /// 无硬挡块（标定实测过——路线 B 无挡块可找，同样禁用）
    No,
}

impl Rr3ActHardStop {
    /// 档名（档案取值串同源：`unknown` / `yes` / `no`）
    pub fn rr3_act_hard_stop_name(self) -> &'static str {
        match self {
            Rr3ActHardStop::Unknown => "unknown",
            Rr3ActHardStop::Yes => "yes",
            Rr3ActHardStop::No => "no",
        }
    }

    /// 从档案词解析（词表外回 `None`，装载侧点名红，不猜——与 name 同源成对，
    /// 词形改动只许落这一处）
    pub fn rr3_act_hard_stop_parse(s: &str) -> Option<Self> {
        match s {
            "unknown" => Some(Rr3ActHardStop::Unknown),
            "yes" => Some(Rr3ActHardStop::Yes),
            "no" => Some(Rr3ActHardStop::No),
            _ => None,
        }
    }
}

/// 执行件动作形态（批N3，判据实体 [`rr3_act_motion_allowed`] 的判据对象）。
/// 四形态各有判据，不许在调用点手抄第二份。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ActMotion {
    /// 点动（带 deadline，§6：唯一永不拒的动作形态——未标定轴的合法动作面）
    Jog,
    /// 运行期自动动作（扫摆 / 表情联动 / goto 走位）——未标定一律拒（§6 硬约束）
    Auto,
    /// 标定路线 C 自动段（开环自恢复走向软挡块，D13 已裁默认路线）——
    /// `hard_stop = unknown` 拒（§4.4 档案注释原文）
    CalibOpenLoop,
    /// 标定路线 B（电流找挡块）——`hard_stop = unknown` 拒、`no` 拒（无挡块可找）；
    /// 逐域电流采样前提归采样侧判据（校验⑥/⑨），不进本判据
    CalibStallSearch,
}

impl Rr3ActMotion {
    /// 词名（命令面取值串同源：`jog` / `auto` / `calib-openloop` / `calib-stall-search`）
    pub fn rr3_act_motion_name(self) -> &'static str {
        match self {
            Rr3ActMotion::Jog => "jog",
            Rr3ActMotion::Auto => "auto",
            Rr3ActMotion::CalibOpenLoop => "calib-openloop",
            Rr3ActMotion::CalibStallSearch => "calib-stall-search",
        }
    }

    /// 从词解析（词表外回 `None`，调用侧点名红，不猜——与 name 同源成对，
    /// 词形改动只许落这一处）
    pub fn rr3_act_motion_parse(s: &str) -> Option<Self> {
        match s {
            "jog" => Some(Rr3ActMotion::Jog),
            "auto" => Some(Rr3ActMotion::Auto),
            "calib-openloop" => Some(Rr3ActMotion::CalibOpenLoop),
            "calib-stall-search" => Some(Rr3ActMotion::CalibStallSearch),
            _ => None,
        }
    }
}

/// 执行件动作合法性判据实体（批N3，全案 §6 硬约束——"未标定不得自动"与
/// "hard_stop=unknown 护栏"都是**代码拒动**，非文档建议）。
///
/// 判据两条写完：
/// 1. `calibrated = false` ⇒ [`Rr3ActMotion::Auto`] 一律拒（`rr3_goto` 只收点动）；
/// 2. `hard_stop = unknown` ⇒ [`Rr3ActMotion::CalibOpenLoop`] /
///    [`Rr3ActMotion::CalibStallSearch`] 拒；`hard_stop = no` ⇒
///    [`Rr3ActMotion::CalibStallSearch`] 拒（无硬挡块可找）。
/// [`Rr3ActMotion::Jog`] 永远放行。
///
/// 单真值点：产出侧（rr3-act 组合件执行面）与判据用例共用本函数，不许各自
/// 手抄 `if` 链。拒收回 [`Rr3HalError::InvalidParam`]（文案带动作形态与判据名，
/// 与板侧 `axis_check` 越域回错同形——越界显式回错不静默 clamp）。
pub fn rr3_act_motion_allowed(
    hard_stop: Rr3ActHardStop,
    calibrated: bool,
    motion: Rr3ActMotion,
) -> Result<(), Rr3HalError> {
    let refuse = |why: &str| Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(why)));
    match motion {
        Rr3ActMotion::Jog => Ok(()),
        Rr3ActMotion::Auto if calibrated => Ok(()),
        Rr3ActMotion::Auto => refuse("未标定轴不得自动（只许点动，§6 硬约束）"),
        Rr3ActMotion::CalibOpenLoop if hard_stop == Rr3ActHardStop::Unknown => {
            refuse("hard_stop=unknown：路线 C 自动段禁用（§6 护栏）")
        }
        Rr3ActMotion::CalibOpenLoop => Ok(()),
        Rr3ActMotion::CalibStallSearch if hard_stop != Rr3ActHardStop::Yes => {
            refuse("hard_stop≠yes：路线 B 禁用（unknown 未标定挡块事实 / no 无挡块可找）")
        }
        Rr3ActMotion::CalibStallSearch => Ok(()),
    }
}

/// 执行件动作面（批N3，全案 §9 N3——"关节成为对象"的契约承载）。
///
/// 实现归属：rr3-act 组合件在 `dyn Rr3PwmServo` 句柄之上包行程 / 标定 / 拒动
/// 语义（与批N2 件五 RailMonitor 归属搬家同构——机制住组合件，契约住本层）。
/// 拒动判据不进签名：调用方先经 [`rr3_act_motion_allowed`] 判，再调本面——
/// 契约层只供给动作形态与判据实体，不持策略（[`Rr3Rail`] 同线）。
///
/// 四方法在立 trait 时一次定形（§18 F-32：不许有一个未定义就进文档的方法名）：
/// - [`Rr3Actuator::rr3_jog`]：点动一步（带 deadline），未标定轴唯一合法动作；
/// - [`Rr3Actuator::rr3_goto`]：自动走位（判据拒绝时本方法不受理）；
/// - [`Rr3Actuator::rr3_position`]：位置读数，角度与来源同帧（[`Rr3Position`]）；
/// - [`Rr3Actuator::rr3_hold`]：激磁保持语义（`false` = 释放激磁，`failsafe=release`
///   既有裁定，§6 复位语义③）。
pub trait Rr3Actuator {
    /// 点动一步到 `deg`（超 `deadline_ms` 回错——受控堵转护栏的时间维，§6 路线 B）
    fn rr3_jog(&mut self, deg: u16, deadline_ms: u32) -> Result<(), Rr3HalError>;
    /// 自动走位到 `deg`（判据 [`rr3_act_motion_allowed`] 拒收的动作形态不受理）
    fn rr3_goto(&mut self, deg: u16, deadline_ms: u32) -> Result<(), Rr3HalError>;
    /// 位置读数（角度 + 来源同帧；无反馈面回 `Commanded` 回显，不许冒充测量）
    fn rr3_position(&self) -> Result<Rr3Position, Rr3HalError>;
    /// 激磁保持（`true` = 保持激磁；`false` = 释放——欠压/复位后让轴自由，
    /// 避免复位循环里反复堵转，§6 复位语义③）
    fn rr3_hold(&mut self, hold: bool) -> Result<(), Rr3HalError>;
}

/// 执行件档案字段的读者三态（ADR-0022 修订 1 工单 F-25，门 ⑨-m 的**标注面**——
/// "标注与实际读者集合不一致即红"的唯一数据源）。三态含义：
/// [`Rr3ActFieldReader::Adjudicate`] = 参与裁决（判据输入，拒动 / 对账一侧）、
/// [`Rr3ActFieldReader::Act`] = 有动作侧读者（驱动执行）、
/// [`Rr3ActFieldReader::Echo`] = 仅回显（写了保护语义却没有动作侧——如实标注，
/// "登记为只回显不丢人，写了保护语义却没有动作侧才丢人"，§4.4）。
///
/// 已知"只回显"如实登记（§4.4 点名 `settle_ms`（批N3 前无串行读者）与
/// `warmup_ms`（激磁预热无动作侧）两处候选；`source` 标定来源是追溯回显面，
/// 同属回显——标注与实际读者集合一致，门 ⑨-m 建成后按本表对账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ActFieldReader {
    Adjudicate,
    Act,
    Echo,
}

/// 执行件档案字段词表（批N3，全案 §4.4 档案形状的**契约面**：键名 / 值类型 /
/// 读者三态一张表）。装载侧（rrd 档案装载器）的白名单、类型派发与门 ⑨-m 的
/// 标注对账都只认本表——键名第二份真值点不存在。
///
/// `schema` 与 `pin` 两键是装载域键（版本对账 / 外键对账），读者同属裁决侧。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ActProfileField {
    pub key: &'static str,
    /// 值类型（int = 整数域，取值域是键自己的事实、由装载侧键级判据管——行程键
    /// 非负、`dir_sign` ∈ {±1}、ms 键上限 u32；text = 在册枚举词；bool = 显式布尔）
    pub kind: Rr3ActFieldKind,
    pub reader: Rr3ActFieldReader,
}

/// 档案字段值类型（装载派发用，与 [`Rr3ActProfileField`] 同表）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ActFieldKind {
    Int,
    Text,
    Bool,
}

/// 档案十四键（§4.4 示例档逐键对齐；加键 = 本表 append + 装载侧同批，契约级评审）
pub const RR3_ACT_PROFILE_FIELDS: &[Rr3ActProfileField] = &[
    Rr3ActProfileField {
        key: "schema",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "pin",
        kind: Rr3ActFieldKind::Text,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "center_deg",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Act,
    },
    Rr3ActProfileField {
        key: "lo_deg",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "hi_deg",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "dir_sign",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Act,
    },
    Rr3ActProfileField {
        key: "slew_deg_s",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Act,
    },
    Rr3ActProfileField {
        key: "settle_ms",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Echo,
    },
    Rr3ActProfileField {
        key: "hold",
        kind: Rr3ActFieldKind::Text,
        reader: Rr3ActFieldReader::Act,
    },
    Rr3ActProfileField {
        key: "current_ceil_ma",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "warmup_ms",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Echo,
    },
    Rr3ActProfileField {
        key: "hard_stop",
        kind: Rr3ActFieldKind::Text,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "calibrated",
        kind: Rr3ActFieldKind::Bool,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "source",
        kind: Rr3ActFieldKind::Text,
        reader: Rr3ActFieldReader::Echo,
    },
];

/// 激磁保持策略（批N3，档案 `hold` 键的契约投影，全案 §4.4：`hold | release`
/// 对应"不保持激磁"的既有裁定）。词表两态显式给词，不给布尔缺省——缺省与
/// 漏填不可区分（T-5 同一族）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ActHold {
    /// 保持激磁
    Hold,
    /// 释放激磁（§4.4 示例档取值——欠压/复位让轴自由，§6 复位语义③）
    Release,
}

impl Rr3ActHold {
    /// 档名（档案取值串同源：`hold` / `release`）
    pub fn rr3_act_hold_name(self) -> &'static str {
        match self {
            Rr3ActHold::Hold => "hold",
            Rr3ActHold::Release => "release",
        }
    }

    /// 从档案词解析（词表外回 `None`，装载侧点名红，不猜——与 name 同源成对）
    pub fn rr3_act_hold_parse(s: &str) -> Option<Self> {
        match s {
            "hold" => Some(Rr3ActHold::Hold),
            "release" => Some(Rr3ActHold::Release),
            _ => None,
        }
    }
}

/// 标定来源（批N3，档案 `source` 键的契约投影，全案 §4.4：标定动作的来源，
/// 可追溯）。词表四态与 §6 标定路线表同源——`openloop-recover` = 路线 C、
/// `stall-search` = 路线 B；`manual` / `fixture` 是人工两侧（手持 / 夹具）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ActSource {
    /// 手工标定（手持限位/目测，出厂或现场）
    Manual,
    /// 夹具标定（定位工装，产线口径）
    Fixture,
    /// 开环自恢复（路线 C 自动段）
    OpenLoopRecover,
    /// 电流找挡块（路线 B）
    StallSearch,
}

impl Rr3ActSource {
    /// 档名（档案取值串同源：`manual` / `fixture` / `openloop-recover` / `stall-search`）
    pub fn rr3_act_source_name(self) -> &'static str {
        match self {
            Rr3ActSource::Manual => "manual",
            Rr3ActSource::Fixture => "fixture",
            Rr3ActSource::OpenLoopRecover => "openloop-recover",
            Rr3ActSource::StallSearch => "stall-search",
        }
    }

    /// 从档案词解析（词表外回 `None`，装载侧点名红，不猜——与 name 同源成对）
    pub fn rr3_act_source_parse(s: &str) -> Option<Self> {
        match s {
            "manual" => Some(Rr3ActSource::Manual),
            "fixture" => Some(Rr3ActSource::Fixture),
            "openloop-recover" => Some(Rr3ActSource::OpenLoopRecover),
            "stall-search" => Some(Rr3ActSource::StallSearch),
            _ => None,
        }
    }
}

/// 执行件档案值面（批N3，全案 §4.4 十四键的**值投影**）。装载域两键不进本面：
/// `schema` 是版本对账数（装载侧只认 1，升版走契约级评审）、`pin` 是外键
/// （F-4，由装载产物 `Rr3AxisFile` 持有）——十二键值域逐键注记给到。
/// no_std 值类型：无 String / 无 Vec，与契约面 F1 同线。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ActProfile {
    /// 中位角（度）——点动与走位的角域锚（自洽判据 lo < center < hi）
    pub center_deg: u16,
    /// 行程下限（度）——**行程真值收敛点**（§1 坑 8：档案之外的行程读者数 = 0）
    pub lo_deg: u16,
    /// 行程上限（度）——同上
    pub hi_deg: u16,
    /// 方向符号（±1；0 = 物理矛盾，装载红）
    pub dir_sign: i8,
    /// 速度上限（度/秒，>0）
    pub slew_deg_s: u16,
    /// 到位稳定窗口 ms（当前仅回显——F-25 如实标注，串行读者进场同批改标）
    pub settle_ms: u32,
    /// 激磁保持策略（[`Rr3ActHold`]）
    pub hold: Rr3ActHold,
    /// 电流上限 mA（>0，校验⑨ 预算一侧的执行件事实）
    pub current_ceil_ma: u16,
    /// 激磁预热窗口 ms（当前仅回显——F-25 如实标注，激磁预热无动作侧）
    pub warmup_ms: u32,
    /// 挡块事实（[`Rr3ActHardStop`]——unknown 按最保守一侧处置，判据
    /// [`rr3_act_motion_allowed`]）
    pub hard_stop: Rr3ActHardStop,
    /// 标定完成位（false ⇒ Auto 拒，§6 硬约束——rtos 档只许运行期内存态，
    /// §4.4 载体语义②/S-14）
    pub calibrated: bool,
    /// 标定来源（[`Rr3ActSource`]，当前仅回显——追溯面）
    pub source: Rr3ActSource,
}

/// 载体窄缝（批N3，全案 §4.4 Y-2/Y-7 + 工单 sense S-14）：执行件档案的掉电载体
/// 抽象。host 档档案是文件（rrd 装载链直读直写，复用 counters 原子序，不经本缝）；
/// rtos 档的 NVS 实现走本缝。**S-14 三项载体性质实测前（`set_blob` 掉电原子性 /
/// 写放大上限 / 单键容量上限），实现侧 fail-closed 回 `Unsupported`**——
/// `calibrated` 只许运行期内存态（§6 复位语义④），rtos 可达终点表明写
/// "标定状态重启即失"（§17），不许拿一个没测过掉电原子性的写路径冒充持久化。
pub trait Rr3Store {
    /// 读一个键到调用方缓冲，返回有效字节数（缓冲不足回 `InvalidParam`——
    /// 容量语义显式，不静默截断：档案字节被劈半比失败更糟）
    fn rr3_store_read(&self, key: &str, buf: &mut [u8]) -> Result<usize, Rr3HalError>;
    /// 写一个键（掉电原子性未实测前实现侧必须回 `Unsupported`，见类型注记）
    fn rr3_store_write(&mut self, key: &str, data: &[u8]) -> Result<(), Rr3HalError>;
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

impl Rr3Actuator for Rr3UnavailableSource {
    fn rr3_jog(&mut self, _deg: u16, _deadline_ms: u32) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/actuator"))
    }
    fn rr3_goto(&mut self, _deg: u16, _deadline_ms: u32) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/actuator"))
    }
    fn rr3_position(&self) -> Result<Rr3Position, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/actuator"))
    }
    fn rr3_hold(&mut self, _hold: bool) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/actuator"))
    }
}

impl Rr3Store for Rr3UnavailableSource {
    fn rr3_store_read(&self, _key: &str, _buf: &mut [u8]) -> Result<usize, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/store"))
    }
    fn rr3_store_write(&mut self, _key: &str, _data: &[u8]) -> Result<(), Rr3HalError> {
        // S-14 载体性质实测前写侧恒拒（fail-closed）——rtos 档 calibrated 只许
        // 内存态，冒充持久化即违 §6 复位语义④
        Err(Rr3HalError::Unsupported("placeholder/store"))
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
        assert_ne!(
            Rr3RailState {
                mv: 4400,
                tier: Rr3RailTier::Severe,
                age_ms: 0
            },
            s,
            "陈旧的 Severe 与新鲜的 Severe 在接口上必须可区分（age_ms 入等值面）"
        );
    }

    /// ⑥-c2 判据红例组（批N2 件三，全案 §6）：超龄与无基准折 Unknown、新鲜原样、
    /// 边界不折、陈旧 Severe 不许冒充新鲜（T-6 立项理由——没有 Unknown 第四态，
    /// 面板/brain 拿着不敢信的数据照样判定）。
    #[test]
    fn rail_tier_effective_folds_stale_or_unclocked_readings_into_unknown() {
        // 新鲜读数原样（三电平档都不折）
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Ok, Some(100), 1_000),
            Rr3RailTier::Ok
        );
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Severe, Some(1_000), 1_000),
            Rr3RailTier::Severe,
            "边界 age == max_age_ms 不折（判据是严格 >，新鲜度上界闭区间内）"
        );
        // 超龄 Ok ⇒ Unknown（陈旧的 Ok 不许冒充新鲜）
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Ok, Some(1_001), 1_000),
            Rr3RailTier::Unknown
        );
        // 超龄 Severe ⇒ Unknown（陈旧的 Severe 与"新鲜的 Severe"在执行面等处置，
        // 但在可信度上必须可区分——冒充新鲜即 T-6 红例）
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Severe, Some(u32::MAX), 1_000),
            Rr3RailTier::Unknown
        );
        // 时基取不到基准（None）⇒ Unknown（Y-4：编造龄期不许冒充新鲜）
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Ok, None, 1_000),
            Rr3RailTier::Unknown
        );
    }

    /// 拒动判据红例组（批N3，全案 §6 硬约束——"未标定不得自动"与 hard_stop
    /// 护栏都是代码拒动）：四形态 × 三挡块事实 × 标定位的全组合关键臂。
    #[test]
    fn act_motion_allowed_refuses_uncalibrated_auto_and_unknown_hardstop() {
        use Rr3ActHardStop as Hs;
        use Rr3ActMotion as Mo;
        // 点动永远放行（未标定轴的唯一合法动作面）
        for hs in [Hs::Unknown, Hs::Yes, Hs::No] {
            for cal in [false, true] {
                assert!(rr3_act_motion_allowed(hs, cal, Mo::Jog).is_ok());
            }
        }
        // 硬约束一：calibrated=false ⇒ Auto 一律拒（判据文案点名判据名）
        let e = rr3_act_motion_allowed(Hs::Yes, false, Mo::Auto).unwrap_err();
        assert!(e.to_string().contains("未标定轴不得自动"), "{e}");
        // 标定过 ⇒ Auto 放行
        assert!(rr3_act_motion_allowed(Hs::Yes, true, Mo::Auto).is_ok());
        // 硬约束二：hard_stop=unknown ⇒ C 自动段 / B 找挡块都拒
        let e = rr3_act_motion_allowed(Hs::Unknown, true, Mo::CalibOpenLoop).unwrap_err();
        assert!(e.to_string().contains("hard_stop=unknown"), "{e}");
        assert!(rr3_act_motion_allowed(Hs::Unknown, false, Mo::CalibStallSearch).is_err());
        // hard_stop=yes ⇒ C 自动段 / B 找挡块放行（标定动作，与 calibrated 无关）
        assert!(rr3_act_motion_allowed(Hs::Yes, false, Mo::CalibOpenLoop).is_ok());
        assert!(rr3_act_motion_allowed(Hs::Yes, false, Mo::CalibStallSearch).is_ok());
        // hard_stop=no ⇒ B 拒（无硬挡块可找）、C 自动段放行（软挡块路线不依赖该事实）
        assert!(rr3_act_motion_allowed(Hs::No, true, Mo::CalibStallSearch).is_err());
        assert!(rr3_act_motion_allowed(Hs::No, true, Mo::CalibOpenLoop).is_ok());
    }

    /// 档案字段词表形状 pin（批N3，§4.4 档案十四键）：键名唯一、两处"只回显"
    /// 如实标注（F-25 读者三态——settle_ms/warmup_ms 是 §4.4 点名的已知候选）。
    #[test]
    fn act_profile_fields_pinned_shape_and_reader_annotations() {
        assert_eq!(RR3_ACT_PROFILE_FIELDS.len(), 14);
        for i in 0..RR3_ACT_PROFILE_FIELDS.len() {
            for j in (i + 1)..RR3_ACT_PROFILE_FIELDS.len() {
                assert_ne!(
                    RR3_ACT_PROFILE_FIELDS[i].key, RR3_ACT_PROFILE_FIELDS[j].key,
                    "档案键名重复：{}",
                    RR3_ACT_PROFILE_FIELDS[i].key
                );
            }
        }
        let echo_keys: Vec<_> = RR3_ACT_PROFILE_FIELDS
            .iter()
            .filter(|f| f.reader == Rr3ActFieldReader::Echo)
            .map(|f| f.key)
            .collect();
        assert_eq!(echo_keys, ["settle_ms", "warmup_ms", "source"]);
        // 装载域键与判据输入键都属裁决侧（读者三态标注与实际读者集合一致）
        for k in [
            "schema",
            "pin",
            "lo_deg",
            "hi_deg",
            "hard_stop",
            "calibrated",
        ] {
            let f = RR3_ACT_PROFILE_FIELDS.iter().find(|f| f.key == k).unwrap();
            assert_eq!(f.reader, Rr3ActFieldReader::Adjudicate, "{k}");
        }
        // 挡块事实档名与档案取值串同源
        assert_eq!(Rr3ActHardStop::Unknown.rr3_act_hard_stop_name(), "unknown");
        assert_eq!(Rr3ActHardStop::Yes.rr3_act_hard_stop_name(), "yes");
        assert_eq!(Rr3ActHardStop::No.rr3_act_hard_stop_name(), "no");
    }

    /// 三档案词枚举（hold / hard_stop / source）name ↔ parse 同源 round-trip：
    /// 词形改动只许落各 enum 的 impl 一处，档案文本串漂移即红。
    #[test]
    fn act_word_enums_name_parse_round_trip() {
        for (h, w) in [(Rr3ActHold::Hold, "hold"), (Rr3ActHold::Release, "release")] {
            assert_eq!(h.rr3_act_hold_name(), w);
            assert_eq!(Rr3ActHold::rr3_act_hold_parse(w), Some(h));
        }
        assert_eq!(Rr3ActHold::rr3_act_hold_parse("auto"), None);
        for (hs, w) in [
            (Rr3ActHardStop::Unknown, "unknown"),
            (Rr3ActHardStop::Yes, "yes"),
            (Rr3ActHardStop::No, "no"),
        ] {
            assert_eq!(Rr3ActHardStop::rr3_act_hard_stop_parse(w), Some(hs));
        }
        assert_eq!(Rr3ActHardStop::rr3_act_hard_stop_parse("maybe"), None);
        for (s, w) in [
            (Rr3ActSource::Manual, "manual"),
            (Rr3ActSource::Fixture, "fixture"),
            (Rr3ActSource::OpenLoopRecover, "openloop-recover"),
            (Rr3ActSource::StallSearch, "stall-search"),
        ] {
            assert_eq!(s.rr3_act_source_name(), w);
            assert_eq!(Rr3ActSource::rr3_act_source_parse(w), Some(s));
        }
        assert_eq!(Rr3ActSource::rr3_act_source_parse("unknown"), None);
        for (m, w) in [
            (Rr3ActMotion::Jog, "jog"),
            (Rr3ActMotion::Auto, "auto"),
            (Rr3ActMotion::CalibOpenLoop, "calib-openloop"),
            (Rr3ActMotion::CalibStallSearch, "calib-stall-search"),
        ] {
            assert_eq!(m.rr3_act_motion_name(), w);
            assert_eq!(Rr3ActMotion::rr3_act_motion_parse(w), Some(m));
        }
        assert_eq!(Rr3ActMotion::rr3_act_motion_parse("walk"), None);
    }

    /// 位置读数来源在册（坑 11：开环回显与闭环测量在接口上可区分——来源缺席的
    /// 裸角度不许冒充测量）；占位后端对执行件/载体缝回错（能力缺失显式回错）。
    #[test]
    fn position_and_placeholder_actuator_store_pinned() {
        let p = Rr3Position {
            deg: 90,
            source: Rr3PositionSource::Commanded,
        };
        assert_eq!(p.source, Rr3PositionSource::Commanded);
        let _ = Rr3PositionSource::Encoder; // 预留格位（路线 D）在册不撤名
        let mut boxed: Box<dyn Rr3Actuator> = Box::new(Rr3UnavailableSource);
        assert!(matches!(
            boxed.rr3_jog(90, 800),
            Err(Rr3HalError::Unsupported("placeholder/actuator"))
        ));
        assert!(matches!(
            boxed.rr3_goto(90, 800),
            Err(Rr3HalError::Unsupported("placeholder/actuator"))
        ));
        let mut store: Box<dyn Rr3Store> = Box::new(Rr3UnavailableSource);
        assert!(matches!(
            store.rr3_store_write("calibrated", b"true"),
            Err(Rr3HalError::Unsupported("placeholder/store"))
        ));
    }
}
