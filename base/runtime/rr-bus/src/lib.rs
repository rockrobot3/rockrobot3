//! # rr-bus
//!
//! 总线**语义常量 re-export 层**：真值在 `rr-core`（0.0.5 第 II 轮批D₁，ADR-0018
//! 裁定 1），本 crate re-export 保持既有 import 路径逐字不变（`rrd`/`tools` 的
//! `use rr_bus::…` 零波及）。
//!
//! **std 承载客户端已迁 `rr-bus-api`**（基-35 窄化②，批N9 件一，照 `rr-hal-api`
//! ＋`⑨-w` 先例）：五单元承载面直依契约 crate，不再绑本 crate；客户端（连接 →
//! 注册 → 心跳 → 发布 / `rr3.call` / 收帧）见 `rr-bus-api::client`。

pub use rr_core::{
    rr3_event_compose, rr3_frame_compose, rr3_task_kind, Rr3TaskKind, RR3_BROADCAST_DST_TASK,
    RR3_BUS_CONTRACT_VERSION, RR3_METHOD_CALL, RR3_PAYLOAD_EVENT, RR3_PAYLOAD_REQUEST,
    RR3_PAYLOAD_RESPONSE, RR3_PRIORITY_MAX, RR3_PRIORITY_MIN, RR3_TASK_FIRST_MODULE, RR3_TASK_RRD,
    RR3_TOPIC_SEP, RR3_TRACE_ID_DIRECT,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_dst_task_matches_standard() {
        assert_eq!(RR3_BROADCAST_DST_TASK, 0);
    }

    /// 任务号三段不得重叠：0 广播 / 1 rrd / ≥2 模块
    #[test]
    fn task_id_space_is_disjoint() {
        assert_eq!(RR3_BROADCAST_DST_TASK, 0);
        assert_eq!(RR3_TASK_RRD, 1);
        assert_eq!(RR3_TASK_FIRST_MODULE, 2);
        const {
            assert!(
                RR3_TASK_FIRST_MODULE > RR3_TASK_RRD,
                "模块池必须让开两个保留号"
            );
        }
    }

    /// 占位窗口要有内容可言：区间非空（MIN < MAX），且两个值都落在常量文档自己声明的
    /// 0..=139 占位带内。**不再**把两个字面量互相比一遍冒充"对齐调度类"的验证——
    /// v0.0.1 没有任何契约/ADR 定义过 0-139，真正的优先级语义随 M2 的调度 ADR 落地后替换本测试。
    #[test]
    fn priority_window_is_nonempty_placeholder_band() {
        const BAND: std::ops::RangeInclusive<u32> = 0..=139; // 常量文档声明的占位带
        const {
            // const 上下文不允许 format!：字面值见上方两个常量本体，此处只钉关系
            assert!(
                RR3_PRIORITY_MIN < RR3_PRIORITY_MAX,
                "占位窗口为空（MIN >= MAX）"
            );
        }
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

    /// re-export 与 rr-core 真值一致（下沉后防两册漂移）
    #[test]
    fn reexports_match_rr_core_truth() {
        assert_eq!(rr_core::RR3_TASK_RRD, RR3_TASK_RRD);
        assert_eq!(rr_core::RR3_METHOD_CALL, RR3_METHOD_CALL);
        assert_eq!(rr_core::RR3_TOPIC_SEP, RR3_TOPIC_SEP);
    }
}
