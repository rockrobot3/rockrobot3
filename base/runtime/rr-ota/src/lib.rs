//! rr-ota —— 升级通道骨架（architecture.md §8、ADR-0007 决策二）。
//!
//! v0.0.1 只冻结"双轨"的类型形状：整镜像 A/B（M6）与模块级 staging（M6+）。
//! 具体 slot 切换/断电安全实现随 Buildroot 镜像一起落地。

/// 整镜像 A/B 与出厂槽位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3Slot {
    SlotA,
    SlotB,
    Factory,
}

/// 槽位状态机
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3SlotState {
    /// 当前运行
    Active,
    /// 已下载并通过 manifest 兼容校验，等待切换
    Staged,
    /// 空
    Idle,
    /// 校验失败，自动回滚候选
    Corrupt,
}

/// A/B 槽位名称（与 genimage.cfg 分区标签一致，M2 建分区时核对）
pub const RR3_OTA_SLOT_NAMES: [&str; 2] = ["rr_a", "rr_b"];

/// 模块级更新的 staging 分区标签（ADR-0007 决策二：布局现在即预留）
pub const RR3_OTA_STAGING_PARTITION: &str = "rr_staging";

impl Rr3Slot {
    /// 对侧槽位（切换用）
    pub fn rr3_slot_peer(self) -> Self {
        match self {
            Self::SlotA => Self::SlotB,
            Self::SlotB => Self::SlotA,
            Self::Factory => Self::Factory,
        }
    }

    pub fn rr3_slot_label(self) -> &'static str {
        match self {
            Self::SlotA => "rr_a",
            Self::SlotB => "rr_b",
            Self::Factory => "rr_factory",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ota_slot_peer_roundtrip() {
        assert_eq!(Rr3Slot::SlotA.rr3_slot_peer(), Rr3Slot::SlotB);
        assert_eq!(Rr3Slot::SlotB.rr3_slot_peer(), Rr3Slot::SlotA);
        assert_eq!(Rr3Slot::Factory.rr3_slot_peer(), Rr3Slot::Factory);
    }

    #[test]
    fn ota_labels_match_partition_plan() {
        assert_eq!(
            RR3_OTA_SLOT_NAMES,
            [Rr3Slot::SlotA.rr3_slot_label(), Rr3Slot::SlotB.rr3_slot_label()]
        );
        assert_eq!(RR3_OTA_STAGING_PARTITION, "rr_staging");
    }

    #[test]
    fn ota_state_variants_are_distinct() {
        let all = [
            Rr3SlotState::Active,
            Rr3SlotState::Staged,
            Rr3SlotState::Idle,
            Rr3SlotState::Corrupt,
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                assert_eq!(a == b, i == j);
            }
        }
    }
}
