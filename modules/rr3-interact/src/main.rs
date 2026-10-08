//! rr3-interact 入口（组合件五件套②，0.0.5 批C③ 原子重铺）。
//!
//! 入口只做装配与启动（≤80 行，ADR-0019 裁定 8）：机制主体在 [`composite`]
//! （face ⊗ player，见 composite.toml），原子件在 `atoms/`（每原子一目录，
//! 五件套：atom.toml + 入口 + mech + assemble.toml + private.rs）；panel 状态面
//! 留组合件（0.0.5 方案 §12.1——面板是 HTTP 线程 + 动作通道的接入编排面）。
//! module.toml / config.toml / Cargo.toml 功能守恒零改动；SURFACE 启动自述
//! 逐字不变（打印序列见 composite::rr3_interact_run 开头）。

mod composite;

fn main() {
    if let Err(e) = composite::rr3_interact_run() {
        eprintln!("rr3-interact 退出: {e}");
        std::process::exit(1);
    }
}
