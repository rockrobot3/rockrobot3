//! face 原子件入口（五件套②，0.0.5 批C③ 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `ui.face`（atom.toml）。SURFACE 是启动自述（组合件 rr3_interact_run 开头
//! 逐字打印，契约面零改动红线）。表情渲染：idle / blink / talk / think / happy /
//! listen / turn——七态白名单与 `talk` 自动挂起恢复是现在就要钉死的契约面；
//! **"不可点名"规则是策略**（0.0.5 批M MC-10 I-11）：值本身（表情名）是契约字面量
//! 不许改，本形态排除哪个子集由 config `[rr3-interact.face] non_pointable` 注入。
//! 显示面经 rr-hal 显示 trait（批F5a③）由组合件注入，原子只钉"状态→帧"映射
//! 与推进语义。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/face.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] = &["face: rpc rr3-interact/face/expression (request)"];

/// 本模块 exposes 的表情主题（module.toml [rpc].exposes 同源；原子层不重述 rpc 契约）
pub const RR3_FACE_TOPIC: &str = "rr3-interact/face/expression";

/// 合法表情白名单（contracts/topics.toml 的 note 同源；批F3 增 listen/turn——
/// 判据 9 点名的听取/转向两态）
pub const RR3_FACE_EXPRESSIONS: [&str; 7] =
    ["idle", "blink", "talk", "think", "happy", "listen", "turn"];

/// TTS 播放期间的临时表情（模块内自动切换；"不可点名"规则由 config 注入，见下）
pub const RR3_FACE_TALK: &str = "talk";

/// 表情名 → 白名单里的 `&'static str`（不认返回 `None`）。config 注入值与
/// [`RR3_FACE_EXPRESSIONS`] 的对账点（值本身是契约字面量，扩值走契约评审）。
pub fn rr3_face_expression_lookup(name: &str) -> Option<&'static str> {
    RR3_FACE_EXPRESSIONS.iter().find(|&&e| e == name).copied()
}

/// 不可点名集的展示串（`talk`；错误文案的唯一渲染点）。空集显示为 `(空)`——
/// "本形态不排除任何表情点名"是合法数据，不是错。
pub fn rr3_face_non_pointable_join(non_pointable: &[&str]) -> String {
    if non_pointable.is_empty() {
        return "(空)".to_string();
    }
    non_pointable.join("/")
}

/// 8x8 点阵单帧：类型事实源在 rr-hal 的 `Rr3Display` trait 签名（批F5a③），
/// 此处 re-export 保模块内名。
pub use rr_hal::Rr3FaceFrame;

pub use mech::{rr3_face_frames, Rr3Face};

#[cfg(test)]
mod tests {
    use super::*;

    /// 白名单全集逐个查得回；非在册名返回 None（I-11：config 注入值的对账点）
    #[test]
    fn expression_lookup_covers_whitelist() {
        for e in RR3_FACE_EXPRESSIONS {
            assert_eq!(rr3_face_expression_lookup(e), Some(e), "在册名必须查得回: {e}");
        }
        assert_eq!(rr3_face_expression_lookup("wink"), None, "非在册名不认");
    }

    /// 不可点名集展示串：常规集 / 单元素 / 空集（空集是合法数据选择，非错）
    #[test]
    fn non_pointable_join_renders_set_and_empty() {
        assert_eq!(rr3_face_non_pointable_join(&["talk"]), "talk");
        assert_eq!(rr3_face_non_pointable_join(&["talk", "blink"]), "talk/blink");
        assert_eq!(rr3_face_non_pointable_join(&[]), "(空)");
    }
}
