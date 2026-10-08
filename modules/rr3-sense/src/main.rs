//! rr3-sense 入口（组合件五件套②，0.0.5 批C① 原子重铺）。
//!
//! 入口只做装配与启动（≤80 行，ADR-0019 裁定 8）：机制主体在 [`composite`]
//! （capture ⊗ vad ⊗ env ⊗ imu，见 composite.toml），原子件在 `atoms/`（每原子
//! 一目录，五件套：atom.toml + 入口 + mech + assemble.toml + private.rs）。
//! module.toml / config.toml / Cargo.toml 功能守恒零改动；SURFACE 启动自述
//! 逐字不变（打印序列见 composite::rr3_sense_run 开头）。

mod composite;

fn main() {
    if let Err(e) = composite::rr3_sense_run() {
        eprintln!("rr3-sense 退出: {e}");
        std::process::exit(1);
    }
}
