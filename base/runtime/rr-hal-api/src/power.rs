//! 电源契约：电池读数、轨压/域压档位与读数、折档判据。

use crate::err::Rr3HalError;

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
    /// 档位词表（档名与档位的单点真值）
    const NAMES: [(Rr3RailTier, &'static str); 4] = [
        (Rr3RailTier::Ok, "ok"),
        (Rr3RailTier::Mild, "mild"),
        (Rr3RailTier::Severe, "severe"),
        (Rr3RailTier::Unknown, "unknown"),
    ];

    /// 档名（守护输出行 `INFO rail_tier=` 同串；板侧 `RailTier::name` 投影到此，
    /// 第四态补 `"unknown"`——两条线的文案自此同源一个词表）。
    pub fn rr3_rail_tier_name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(t, _)| *t == self)
            .map_or("", |(_, n)| *n)
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
pub fn rr3_rail_tier_fold(tier: Rr3RailTier, age_ms: Option<u32>, max_age_ms: u32) -> Rr3RailTier {
    if age_ms.is_none_or(|age| age > max_age_ms) {
        return Rr3RailTier::Unknown;
    }
    tier
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
