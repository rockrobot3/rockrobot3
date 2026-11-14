//! 桥的链路机制：寿命管理（三态）+ 双向透传。形状依据 0.0.7 方案 §5.1（钉死直写），
//! 本模块之外零设计决策；实现细节（块大小 512B、退避 1s→10s、日志措辞）属方案
//! 明示留给实装的自由度。

mod conn;
mod route;
mod stat;
#[cfg(test)]
mod tests;

pub use conn::{rr3_signal_handlers, run, BridgeConfig, RR3_BRIDGE_BAUD_DEFAULT};
