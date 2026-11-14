//! 反射环宿主线程 spawn（进程内 1kHz 闭环，永不进总线）。

use rr3mod::BusHandle;

use crate::reflex::{rr3_reflex_loop, Rr3ReflexCfg};

/// 反射环宿主线程 spawn：进程内 1kHz 闭环的位子，永不进总线（ADR-0007 裁定 1）。
/// `bus.clone()` 进 move 闭包——该 BusHandle clone 只归反射环（0.0.8 方案 §4.6），
/// 反射环与 Dispatcher 零共享状态不变量保持；Builder 名与失败文案逐字随迁。
pub(crate) fn rr3_act_spawn_reflex(cfg: Rr3ReflexCfg, bus: &BusHandle) -> Result<(), String> {
    let reflex_bus = bus.clone();
    std::thread::Builder::new()
        .name("rr3-act-reflex".into())
        .spawn(move || rr3_reflex_loop(cfg, reflex_bus))
        .map_err(|e| format!("起反射环宿主线程失败: {e}"))?;
    Ok(())
}
