//! rr3-act 入口（组合件五件套②，0.0.5 批C② 原子重铺）。
//!
//! 入口只做装配与启动（≤80 行，ADR-0019 裁定 8）：机制主体在 [`composite`]
//! （servo ⊗ led ⊗ relay，见 composite.toml），原子件在 `atoms/`（每原子
//! 一目录，五件套：atom.toml + 入口 + mech + assemble.toml + private.rs）；
//! intent 订阅执行面与 reflex 反射环宿主留组合件（0.0.5 方案 §12.1——
//! intent 是总线接入编排面，reflex 守护面归批D）。
//! module.toml / config.toml / Cargo.toml 功能守恒零改动；SURFACE 启动自述
//! 逐字不变（打印序列见 composite::rr3_act_run 开头）。

mod composite;

fn main() {
    if let Err(e) = composite::rr3_act_run() {
        eprintln!("rr3-act 退出: {e}");
        std::process::exit(1);
    }
}
