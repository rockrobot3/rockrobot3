//! dialog 原子件入口（五件套②，批N32 件四 Rust 拆分收敛，ADR-0016）：只声明
//! 公开面，机制在 mech.rs。占格 `cognition.dialog`（atom.toml）。
//! brain 启动自述是组合件级逐字面（main.rs 自印 publish/subscribe/rpc——Python
//! composite.py 打印序列契约零改动），不走 atom SURFACE 链；装配 = src/main.rs
//! `#[path]` 原位挂载（rr3-sense 同形），参数槽 max_utterance_ms 见 assemble.toml。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 公开面：utterance 边界状态机与话轮载体（自 src/dialog.rs 原形状迁入）
pub use mech::{Rr3Dialog, Rr3DialogTurn};
