//! rr-ota —— 升级通道（architecture.md §8、ADR-0007 决策二）。
//!
//! v0.0.1 冻结"双轨"的类型形状：整镜像 A/B（M6）与模块级 staging（M6+）。
//! 0.0.3 批D 增加引导配置改写面（[`bootconf`]）：switch/factory 的判定与
//! 原子落盘逻辑主机可测；U-Boot 环境变量写侧与块设备探测属板上件，不 claim。

pub mod bootconf;

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

/// A/B 槽位名称（与 base/os/buildroot/board/radxa-zero3/genimage.cfg 的分区标签一致，
/// 由下方 include_str 守护用例逐字钉死）
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

    /// 分区标签反查槽位（bootconf/cmdline 解析入口；不在册标签返回 None）
    pub fn rr3_slot_from_label(label: &str) -> Option<Self> {
        match label {
            "rr_a" => Some(Self::SlotA),
            "rr_b" => Some(Self::SlotB),
            "rr_factory" => Some(Self::Factory),
            _ => None,
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
            [
                Rr3Slot::SlotA.rr3_slot_label(),
                Rr3Slot::SlotB.rr3_slot_label()
            ]
        );
        assert_eq!(RR3_OTA_STAGING_PARTITION, "rr_staging");
    }

    #[test]
    fn ota_slot_peer_is_involutive() {
        // peer() 必须是对合（切两次回到原位），且 Factory 无对侧自映射；
        // 若 A/B 映射被调换或 Factory 误指向 A/B，此测试即失败
        for s in [Rr3Slot::SlotA, Rr3Slot::SlotB, Rr3Slot::Factory] {
            assert_eq!(
                s.rr3_slot_peer().rr3_slot_peer(),
                s,
                "peer() 对合律被破坏: {s:?}"
            );
        }
        assert_eq!(Rr3Slot::Factory.rr3_slot_peer(), Rr3Slot::Factory);
    }

    #[test]
    fn ota_slot_from_label_roundtrip() {
        // slot → label → slot 全槽 roundtrip；杂牌与空串必须 None
        for s in [Rr3Slot::SlotA, Rr3Slot::SlotB, Rr3Slot::Factory] {
            assert_eq!(Rr3Slot::rr3_slot_from_label(s.rr3_slot_label()), Some(s));
        }
        assert_eq!(Rr3Slot::rr3_slot_from_label("rr_x"), None);
        assert_eq!(Rr3Slot::rr3_slot_from_label("rr_A"), None);
        assert_eq!(Rr3Slot::rr3_slot_from_label(""), None);
    }

    #[test]
    fn ota_slot_labels_are_injective() {
        // 三槽位标签两两不同且逐一钉死：表被调换或标签重复即失败
        let all = [Rr3Slot::SlotA, Rr3Slot::SlotB, Rr3Slot::Factory];
        let mut labels: Vec<&str> = all.map(|s| s.rr3_slot_label()).to_vec();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), all.len(), "槽位标签必须互不相同");
        assert_eq!(Rr3Slot::SlotA.rr3_slot_label(), "rr_a");
        assert_eq!(Rr3Slot::SlotB.rr3_slot_label(), "rr_b");
        assert_eq!(Rr3Slot::Factory.rr3_slot_label(), "rr_factory");
    }

    /// genimage.cfg 里的分区标签必须与本文件的常量逐字一致。
    ///
    /// include_str! 把布局文件拉进编译：文件改名/移动则**编译**失败（守存在），
    /// 标签改动则本用例**测试**失败（守一致）——"文档说 rr_a、代码用 rr_x"
    /// 的分家在结构上不可能发生（base/ota/README.md 的承诺由此兑现）。
    /// 容量是构建期占位符（post-image.sh 注入），不在本用例关心范围内。
    #[test]
    fn ota_labels_match_genimage_cfg() {
        const GENIMAGE: &str =
            include_str!("../../../../base/os/buildroot/board/radxa-zero3/genimage.cfg");

        // 从 cfg 文本里抓出全部 `label = "X"`（逐段扫描，不引正则依赖）
        let mut labels_in_cfg: Vec<&str> = Vec::new();
        let mut rest = GENIMAGE;
        while let Some(pos) = rest.find("label = \"") {
            rest = &rest[pos + "label = \"".len()..];
            let end = rest.find('"').expect("genimage.cfg 的 label 值缺右引号");
            labels_in_cfg.push(&rest[..end]);
            rest = &rest[end..];
        }
        assert!(
            !labels_in_cfg.is_empty(),
            "genimage.cfg 里没有任何 label 条目"
        );

        // 四个 OTA 槽位标签必须与常量逐字一致
        let ota_labels = [
            Rr3Slot::SlotA.rr3_slot_label(),
            Rr3Slot::SlotB.rr3_slot_label(),
            Rr3Slot::Factory.rr3_slot_label(),
            RR3_OTA_STAGING_PARTITION,
        ];
        for want in ota_labels {
            assert!(
                labels_in_cfg.contains(&want),
                "genimage.cfg 缺 OTA 槽位标签 {want:?}，实有 {labels_in_cfg:?}——\
                 布局与 rr-ota 常量分家，改布局须同批改常量（反之亦然）"
            );
        }

        // 也不得有常量认领不了的杂牌标签（防手误拼出 rr_A / rr_Staging 之类）：
        // 已知合法的非 OTA 标签只有 boot 与 rr_data。
        for got in &labels_in_cfg {
            assert!(
                ota_labels.contains(got) || matches!(*got, "boot" | "rr_data"),
                "genimage.cfg 出现未知分区标签 {got:?}——若为新增分区，\
                 须同批在 rr-ota 登记常量或在本用例登记合法清单"
            );
        }
    }
}
