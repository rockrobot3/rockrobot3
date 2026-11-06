//! 5V 轨电压采样换算与欠压判定机制（P0-A A2 自板档适配层迁入）——纯机制：
//! 分压换算 + 双阈值电平分类 + 迟滞边沿，**不含任何策略量**（阈值/分压比全由
//! 适配层按参传入）。
//!
//! 职责二分：**边沿管事件**（每低电段首拍报一次 → 打站/计数），**电平管钳制**
//! （每拍给档 → 驱动面拒动/降级）。二者共用同一对阈值，故"向下档变"与"边沿
//! 事件"逐拍等价：`Ok→Mild`（轻度线报）、`Ok→Severe`/`Mild→Severe`（重度线
//! 报）——回阈（向上档变）不报，正是"越阈"语义。入参电压由采样面注入（不读
//! 墙上时钟、不睡眠）。

use rr_mech::control::Rr3LowEdge;

/// 5V 轨欠压档位：正常 / 轻度（串行降级）/ 重度（拒动）。**电平分类**——每拍
/// 由 [`rr3_rail_tier_of`] 从轨压算出，驱动面按档钳制。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailTier {
    Ok,
    Mild,
    Severe,
}

impl RailTier {
    /// 档名（观测行与越阈打站文案同串）。
    pub fn name(self) -> &'static str {
        match self {
            RailTier::Ok => "ok",
            RailTier::Mild => "mild",
            RailTier::Severe => "severe",
        }
    }
}

/// 分压换算纯函数：轨压 mV = 引脚 mV × NUM / DEN（只算不碰硬件）。占位 1:1 下
/// 即恒等；标定后由策略面填入非平凡比值。
pub fn rr3_rail_mv_from_pin(pin_mv: u16, num: u32, den: u32) -> u16 {
    ((pin_mv as u32) * num / den) as u16
}

/// 直接 ADC 档 raw 有效性（classic GPIO34 / S3 GPIO1）：`0` 与 `4090..=4095`
/// 是悬空/饱和态，不得当轨压读数。有效 5V 轨经 2:1 分压落在约 2700–3300；
/// 本函数只做机制面有效性判定，分压比/阈值仍由策略面给。
pub fn rr3_rail_raw_valid(raw: u16) -> bool {
    raw > 0 && raw < 4090
}

/// 电平分类（**纯函数**，机制面）：轨压 mV → 档位。阈值按参传入；本函数只做
/// 比较、不含迟滞/事件语义（越阈事件走 [`RailJudge`] 的迟滞边沿）。
pub fn rr3_rail_tier_of(mv: u16, mild_mv: u16, severe_mv: u16) -> RailTier {
    if mv <= severe_mv {
        RailTier::Severe
    } else if mv <= mild_mv {
        RailTier::Mild
    } else {
        RailTier::Ok
    }
}

/// 欠压判定核（**机制面**）：**一核两条线**——轻度/重度两条迟滞边沿各由
/// rr-mech 安全核的 [`Rr3LowEdge`] 一实例承载，**不另写第二个边沿点**（"越阈
/// 一次一段、严格回阈才重新武装"的语义钉死在安全核内，调用方只能喂电压不能改
/// 语义）。两线阈值由策略面构造时注入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RailJudge {
    mild: Rr3LowEdge,
    severe: Rr3LowEdge,
    mild_mv: u16,
    severe_mv: u16,
}

impl RailJudge {
    /// 装配：重度阈值须 < 轻度阈值（策略面职责；本核只按两线各建一边沿）。
    pub fn new(mild_mv: u16, severe_mv: u16) -> Self {
        Self {
            mild: Rr3LowEdge::rr3_low_edge_new(mild_mv),
            severe: Rr3LowEdge::rr3_low_edge_new(severe_mv),
            mild_mv,
            severe_mv,
        }
    }

    /// 喂一拍轨压 mV：返回 `(本拍电平档位, 是否越阈首发事件)`。事件 = 两线任一
    /// "低电段首拍"，逐拍等价于**向下档变**（详见模块文档）。
    pub fn feed(&mut self, mv: u16) -> (RailTier, bool) {
        let mild_hit = self.mild.rr3_low_edge_cross(mv);
        let severe_hit = self.severe.rr3_low_edge_cross(mv);
        (
            rr3_rail_tier_of(mv, self.mild_mv, self.severe_mv),
            mild_hit || severe_hit,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MILD: u16 = 4_600;
    const SEVERE: u16 = 4_400;

    #[test]
    fn rail_mv_from_pin_scales_by_divider() {
        assert_eq!(rr3_rail_mv_from_pin(2_605, 2, 1), 5_210);
        assert_eq!(rr3_rail_mv_from_pin(2_605, 1, 1), 2_605);
    }

    #[test]
    fn raw_validity_rejects_zero_and_saturation() {
        assert!(!rr3_rail_raw_valid(0));
        assert!(rr3_rail_raw_valid(1));
        assert!(rr3_rail_raw_valid(3_100));
        assert!(rr3_rail_raw_valid(4_089));
        assert!(!rr3_rail_raw_valid(4_090));
        assert!(!rr3_rail_raw_valid(4_095));
    }

    #[test]
    fn tier_boundaries_are_inclusive() {
        assert_eq!(rr3_rail_tier_of(4_401, MILD, SEVERE), RailTier::Mild);
        assert_eq!(rr3_rail_tier_of(4_400, MILD, SEVERE), RailTier::Severe);
        assert_eq!(rr3_rail_tier_of(4_600, MILD, SEVERE), RailTier::Mild);
        assert_eq!(rr3_rail_tier_of(4_601, MILD, SEVERE), RailTier::Ok);
        assert_eq!(rr3_rail_tier_of(5_000, MILD, SEVERE), RailTier::Ok);
        assert_eq!(RailTier::Ok.name(), "ok");
        assert_eq!(RailTier::Mild.name(), "mild");
        assert_eq!(RailTier::Severe.name(), "severe");
    }

    #[test]
    fn judge_reports_each_low_segment_once_and_rearms_on_recovery() {
        let mut j = RailJudge::new(MILD, SEVERE);
        // 轻度段：首拍报、段内不重报
        assert_eq!(j.feed(4_500), (RailTier::Mild, true));
        assert_eq!(j.feed(4_500), (RailTier::Mild, false));
        // 回阈（严格大于）不报；再跌破重新武装
        assert_eq!(j.feed(4_700), (RailTier::Ok, false));
        assert_eq!(j.feed(4_600), (RailTier::Mild, true));
        // 恰等阈值仍算低电段内（回阈口径同安全核）
        assert_eq!(j.feed(4_600), (RailTier::Mild, false));
        // 恢复后深入重度段：两线同时首拍，事件仍只报一次
        assert_eq!(j.feed(4_700), (RailTier::Ok, false));
        assert_eq!(j.feed(4_200), (RailTier::Severe, true));
        assert_eq!(j.feed(4_300), (RailTier::Severe, false));
    }

    #[test]
    fn judge_starts_armed_and_fires_on_first_low_sample() {
        let mut j = RailJudge::new(MILD, SEVERE);
        assert_eq!(j.feed(4_300), (RailTier::Severe, true));
        assert_eq!(j.feed(4_300), (RailTier::Severe, false));
    }
}
