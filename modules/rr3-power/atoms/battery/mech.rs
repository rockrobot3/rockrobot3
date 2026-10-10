//! 电池采样与低电边沿（`rr3-power/battery/state` 周期流 + `rr3-power/battery/low`）。
//! 原子件机制主体（五件套③，自原 src/battery.rs 逐字迁入，逻辑零改动）。
//!
//! 关断决策在本地完成，不等总线、不等 brain（architecture §7.3 安全兜底）；
//! `battery/state` 只是摘要上报。低电事件的**边沿语义**（schema 的同一句话）：
//! 同一次低电段只发一次，恢复到阈值之上后再次越过才再发——`Rr3LowEdge` 是它的
//! 唯一真值（批D₃ 起真值在 rr-core `control`，ADR-0018 裁定 5：安全核纯函数，
//! 本文件 re-export 保既有路径）。cutoff 电压阈值本轮只解析不动作（0.0.2 方案
//! Q4：关机与充电策略归 rr3-power 演进与 M6），装配时回显进启动日志，不装"已实现"。
//!
//! 载荷组装（`*_body` 的 json!）已上移承载层（0.0.5 批I P-7，ADR-0019 修订 4）：
//! 件面只交 `Rr3LowEdge` 判定，组装归 `src/composite.rs`，线上字节逐字不变。

pub use rr_core::control::Rr3LowEdge;

#[cfg(test)]
mod tests {
    use super::*;

    /// 低电边沿真值批D₃ 迁 rr-core `control`（越阈/段内/再武装用例随迁）——
    /// 本地只钉"re-export 可用"：同一构造 + 一次注入调用走通 rr-core 面。
    #[test]
    fn low_edge_is_rr_core_reexport() {
        let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
        assert!(e.rr3_low_edge_cross(3290), "越阈触发（rr-core 判定）");
        assert!(!e.rr3_low_edge_cross(3295), "段内不重复报（rr-core 判定）");
    }
}
