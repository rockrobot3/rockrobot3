//! face 原子件入口（五件套②，0.0.5 批C③ 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `ui.face`（atom.toml）。SURFACE 是启动自述（组合件 rr3_interact_run 开头
//! 逐字打印，契约面零改动红线）。表情渲染：七核心态（idle/blink/talk/think/
//! happy/listen/turn）+ char 单字符档 36 态（charA..charZ/char0..char9，
//! 2026-10-15 板上批）——白名单与 `talk` 自动挂起恢复是现在就要钉死的契约面；
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

/// 合法表情白名单（contracts/events/interact-face-expression.schema.json 的 enum
/// 与 contracts/topics.toml 的 note 同源；批F3 增 listen/turn——判据 9 点名的
/// 听取/转向两态；2026-10-15 板上批增 char 单字符档 36 态——charA..charZ +
/// char0..char9，面板真链路单字 8x8 显示；2026-10-25 追加 4 情绪态——sleepy/
/// surprised/angry/sad；扩态只追加末尾，char 档帧表下标不前移）
pub const RR3_FACE_EXPRESSIONS: [&str; 47] = [
    "idle", "blink", "talk", "think", "happy", "listen", "turn", "charA", "charB", "charC",
    "charD", "charE", "charF", "charG", "charH", "charI", "charJ", "charK", "charL", "charM",
    "charN", "charO", "charP", "charQ", "charR", "charS", "charT", "charU", "charV", "charW",
    "charX", "charY", "charZ", "char0", "char1", "char2", "char3", "char4", "char5", "char6",
    "char7", "char8", "char9", "sleepy", "surprised", "angry", "sad",
];

/// TTS 播放期间的临时表情（模块内自动切换；"不可点名"规则由 config 注入，见下）
pub const RR3_FACE_TALK: &str = "talk";

/// [`RR3_FACE_EXPRESSIONS`] 的 `/` 连接串常量投影（批N24 件四零堆化）：错误
/// 文案面的 `join("/")` 在运行时落堆，改用本常量（编译期字面，零分配）；
/// 下方对账单测钉死与 `join` 输出逐字一致——白名单扩员时单测红，提示同步
/// 扩本投影（一个事实两个面的对账锚）。
pub const RR3_FACE_EXPRESSIONS_JOINED: &str = "idle/blink/talk/think/happy/listen/turn/charA/charB/charC/charD/charE/charF/charG/charH/charI/charJ/charK/charL/charM/charN/charO/charP/charQ/charR/charS/charT/charU/charV/charW/charX/charY/charZ/char0/char1/char2/char3/char4/char5/char6/char7/char8/char9/sleepy/surprised/angry/sad";

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
pub use rr_hal_api::Rr3FaceFrame;

pub use mech::{rr3_face_frames, Rr3Face, Rr3FaceSetError};

/// 带兜底表情的构造器（批N4 件五 I-17/M-3）：mech 状态机各回落点的表情
/// 数据化注入面（config `default_expression` → 组合件装配 → 此处转出）。
pub fn rr3_face_with_default(default_expr: &'static str) -> Rr3Face {
    Rr3Face::rr3_face_with_default(default_expr)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 白名单全集逐个查得回；非在册名返回 None（I-11：config 注入值的对账点）
    #[test]
    fn expression_lookup_covers_whitelist() {
        for e in RR3_FACE_EXPRESSIONS {
            assert_eq!(
                rr3_face_expression_lookup(e),
                Some(e),
                "在册名必须查得回: {e}"
            );
        }
        assert_eq!(rr3_face_expression_lookup("wink"), None, "非在册名不认");
    }

    /// 不可点名集展示串：常规集 / 单元素 / 空集（空集是合法数据选择，非错）
    #[test]
    fn non_pointable_join_renders_set_and_empty() {
        assert_eq!(rr3_face_non_pointable_join(&["talk"]), "talk");
        assert_eq!(
            rr3_face_non_pointable_join(&["talk", "blink"]),
            "talk/blink"
        );
        assert_eq!(rr3_face_non_pointable_join(&[]), "(空)");
    }

    /// 连接串常量投影对账（批N24 件四）：投影与 `join("/")` 输出逐字一致——
    /// 白名单扩员时本测红，提示同步扩投影（错误文案面不再运行时 join 落堆）。
    #[test]
    fn joined_projection_matches_whitelist_join() {
        assert_eq!(
            RR3_FACE_EXPRESSIONS_JOINED,
            RR3_FACE_EXPRESSIONS.join("/"),
            "常量投影 = 白名单 join（对账锚，扩员两处同步）"
        );
    }

    /// 本常量是契约真值点（contracts/face-expressions.toml）的 Rust 投影——
    /// 全案 0.0.5 增补轮六 §4.5：白名单唯一来源落 contracts/，代码常量是对账
    /// 副本。本测试把真值点文本逐名比对钉死：扩态须契约文件与本常量同批，
    /// 单改一侧即红（板侧副本由 rr3-codegen 生成，门 f 拟 ⑨-d 重跑 diff 为空）。
    #[test]
    fn expressions_match_contract_truth_point() {
        let contract = include_str!("../../../../contracts/face-expressions.toml");
        // 提取 TOML 数组（跨行）内的字符串项——契约文件形状由门 f 与真值点
        // 自身钉死，此处只做最小文本提取，不引 toml 解析依赖。
        let arr_of = |key: &str| -> Vec<String> {
            let start = contract
                .find(&format!("{key} = ["))
                .unwrap_or_else(|| panic!("契约文件缺 {key}"));
            let rest = &contract[start + key.len() + 3..];
            let end = rest.find(']').expect("数组未闭合");
            rest[..end]
                .split('"')
                .enumerate()
                .filter(|(i, _)| i % 2 == 1)
                .map(|(_, s)| s.to_string())
                .collect()
        };
        let all = arr_of("expressions");
        let board_subset = arr_of("board_subset");
        assert_eq!(
            all, RR3_FACE_EXPRESSIONS,
            "契约真值点与宿主投影漂移：扩态走契约级评审，contracts/face-expressions.toml 与本常量同批"
        );
        // 板侧可渲染子集：前 7 位与基态同序（下标对齐板侧预设表），追加项按
        // "check 挡板"补齐 emotions——两侧同序、全集内、不重复。
        assert_eq!(
            board_subset[..7],
            RR3_FACE_EXPRESSIONS[..7],
            "板侧可渲染子集前 7 位须与基态同序（板侧生成物 rr3-codegen 用它）"
        );
        for e in &board_subset {
            assert!(
                RR3_FACE_EXPRESSIONS.contains(&e.as_str()),
                "board_subset 项 \"{e}\" 须在宿主全集在册"
            );
        }
    }
}
