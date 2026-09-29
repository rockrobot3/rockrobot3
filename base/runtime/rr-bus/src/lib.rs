//! # rr-bus
//!
//! 总线客户端骨架：主题/广播常量层。M2 在此之上实现 UDS 客户端与订阅分发
//! （architecture.md §6）。v0.0.1 故意不放半成品的 `BusClient`，避免 M2 推翻自己。

use rr_ipc::RR3_CONTRACT_SCHEMA_VERSION;

/// 广播目标（A-IPC 标准：dst_task = 0 表示广播）
pub const RR3_BROADCAST_DST_TASK: u64 = 0;

/// EVENT 优先级下界。
///
/// **占位值**：v0.0.1 没有任何契约/ADR/文档定义过优先级区间，这两个数只是给
/// `rr-ota`/`rrd` 留出的常量位；调度类与优先级语义随 M2 的调度 ADR 定稿后替换。
pub const RR3_PRIORITY_MIN: u32 = 0;

/// EVENT 优先级上界（同上：M2 调度 ADR 前的占位值，非对齐 Airy IPC 或 Linux 调度类）
pub const RR3_PRIORITY_MAX: u32 = 139;

/// 主题分隔符
pub const RR3_TOPIC_SEP: char = '/';

/// 契约 schema 版本透传（rr-bus 侧的再导出；rrd 握手回显属 M2 待落地）
pub const RR3_BUS_CONTRACT_VERSION: u32 = RR3_CONTRACT_SCHEMA_VERSION;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_dst_task_matches_standard() {
        assert_eq!(RR3_BROADCAST_DST_TASK, 0);
    }

    /// 占位窗口要有内容可言：区间非空（MIN < MAX），且两个值都落在常量文档自己声明的
    /// 0..=139 占位带内。**不再**把两个字面量互相比一遍冒充"对齐调度类"的验证——
    /// v0.0.1 没有任何契约/ADR 定义过 0-139，真正的优先级语义随 M2 的调度 ADR 落地后替换本测试。
    #[test]
    fn priority_window_is_nonempty_placeholder_band() {
        const BAND: std::ops::RangeInclusive<u32> = 0..=139; // 常量文档声明的占位带
        assert!(
            RR3_PRIORITY_MIN < RR3_PRIORITY_MAX,
            "占位窗口为空：MIN={} MAX={}",
            RR3_PRIORITY_MIN,
            RR3_PRIORITY_MAX
        );
        assert!(
            BAND.contains(&RR3_PRIORITY_MIN) && BAND.contains(&RR3_PRIORITY_MAX),
            "占位值越出常量文档声明的 {BAND:?}：MIN={} MAX={}",
            RR3_PRIORITY_MIN,
            RR3_PRIORITY_MAX
        );
    }

    #[test]
    fn bus_reexports_contract_version() {
        assert_eq!(RR3_BUS_CONTRACT_VERSION, 1);
    }
}
