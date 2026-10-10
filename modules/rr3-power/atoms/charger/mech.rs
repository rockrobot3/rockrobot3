//! 充电状态（`rr3-power/charger/state`）。
//! 原子件机制主体（五件套③，自原 src/charger.rs 逐字迁入，逻辑零改动）。
//!
//! 本流由最近一次 battery 读数的 `charging` 位**派生**（HAL 的 Rr3Power 只给
//! 一个三元组读数，不设独立 charger trait——充电器状态是电池读数的一部分）；
//! 拍点独立于 battery/state（config 的 state_period_ms），seq 也是本流自己的。
//!
//! 载荷组装（`rr3_charger_state_body` 的 json!）已上移承载层（0.0.5 批I P-7，
//! ADR-0019 修订 4）：本件无独立机制面（派生读数 + 组装），组装归
//! `src/composite.rs`，线上字节逐字不变。
