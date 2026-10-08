//! rr3-connect 入口（组合件五件套②，0.0.5 批C④ 原子重铺）。
//!
//! 入口只做装配与启动（≤80 行，ADR-0019 裁定 8）：机制主体在 [`composite`]
//! （wifi ⊗ bt，见 composite.toml），原子件在 `atoms/`（每原子一目录，五件套：
//! atom.toml + 入口 + mech + assemble.toml + private.rs）；session 云会话事务面
//! 留组合件（0.0.5 方案 §12.2——request 事务接入/运行面成分）。module.toml /
//! config.toml / Cargo.toml 功能守恒零改动；SURFACE 启动自述逐字不变（打印
//! 序列见 composite::rr3_connect_run 开头）。

mod composite;

fn main() {
    if let Err(e) = composite::rr3_connect_run() {
        eprintln!("rr3-connect 退出: {e}");
        std::process::exit(1);
    }
}
