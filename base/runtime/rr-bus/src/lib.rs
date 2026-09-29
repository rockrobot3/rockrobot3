//! # rr-bus
//!
//! 总线客户端骨架：主题/广播常量层。M2 在此之上实现 UDS 客户端与订阅分发
//! （architecture.md §6）。v0.0.1 故意不放半成品的 `BusClient`，避免 M2 推翻自己。

use rr_ipc::RR3_CONTRACT_SCHEMA_VERSION;

/// 广播目标（A-IPC 标准：dst_task = 0 表示广播）
pub const RR3_BROADCAST_DST_TASK: u64 = 0;

/// EVENT 优先级下界（对齐调度类 0-139）
pub const RR3_PRIORITY_MIN: u32 = 0;

/// EVENT 优先级上界
pub const RR3_PRIORITY_MAX: u32 = 139;

/// 主题分隔符
pub const RR3_TOPIC_SEP: char = '/';

/// 契约 schema 版本透传（rrd 握手时回显）
pub const RR3_BUS_CONTRACT_VERSION: u32 = RR3_CONTRACT_SCHEMA_VERSION;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_dst_task_matches_standard() {
        assert_eq!(RR3_BROADCAST_DST_TASK, 0);
    }

    #[test]
    fn priority_window_matches_scheduling_classes() {
        assert_eq!((RR3_PRIORITY_MIN, RR3_PRIORITY_MAX), (0, 139));
    }

    #[test]
    fn bus_reexports_contract_version() {
        assert_eq!(RR3_BUS_CONTRACT_VERSION, 1);
    }
}
