//! 启动自述与意图开放集展示串。

/// 启动自述：模块名/版本行 + 四个原子件的 SURFACE 行（装配层调用，文案逐字不变）。
pub(crate) fn rr3_act_banner() {
    use crate::{led, reflex, relay, servo};
    eprintln!("rr3-act 0.0.1 (0.0.2 T4a 实装, 0.0.3 T3 意图执行)");
    for line in servo::SURFACE
        .iter()
        .copied()
        .chain(led::SURFACE.iter().copied())
        .chain(relay::SURFACE.iter().copied())
        .chain(reflex::SURFACE.iter().copied())
    {
        eprintln!("  {line}");
    }
}

/// intent 开放集展示串（空集 = `(空)`）。
pub(crate) fn rr3_intent_open(open: &[String]) -> String {
    if open.is_empty() {
        "(空)".into()
    } else {
        open.join("/")
    }
}
