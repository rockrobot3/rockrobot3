//! face 原子件机制主体（五件套③，自原 src/face.rs 逐字迁入，逻辑零改动）。
//!
//! 每态配 8x8 点阵帧序列（blink/talk 双帧、其余单帧）；[`Rr3Face::rr3_face_tick`]
//! 一拍报当前帧并推进（到尾回卷），切换表情 / talk 挂起恢复都归零指针从首帧
//! 重播。最小表情机：持有当前表情；`talk` 是唯一的瞬态——播放开始挂起（记住
//! 被打断的表情），播放结束恢复。单线程主循环内使用，无并发面。

use serde_json::{json, Value};

use super::{Rr3FaceFrame, RR3_FACE_EXPRESSIONS, RR3_FACE_TALK, RR3_FACE_TOPIC};

/// idle：睁眼 + 平嘴（单帧）
const RR3_FRAMES_IDLE: [Rr3FaceFrame; 1] = [[
    0b0000_0000,
    0b0110_0110,
    0b0110_0110,
    0b0000_0000, //
    0b0000_0000,
    0b0011_1100,
    0b0000_0000,
    0b0000_0000,
]];

/// blink：睁眼（同 idle 首帧）→ 闭眼横线，双帧循环
const RR3_FRAMES_BLINK: [Rr3FaceFrame; 2] = [
    [
        0b0000_0000,
        0b0110_0110,
        0b0110_0110,
        0b0000_0000, //
        0b0000_0000,
        0b0011_1100,
        0b0000_0000,
        0b0000_0000,
    ],
    [
        0b0000_0000,
        0b0000_0000,
        0b0110_0110,
        0b0000_0000, //
        0b0000_0000,
        0b0011_1100,
        0b0000_0000,
        0b0000_0000,
    ],
];

/// talk：张口 / 闭口交替（口型），双帧循环
const RR3_FRAMES_TALK: [Rr3FaceFrame; 2] = [
    [
        0b0000_0000,
        0b0110_0110,
        0b0110_0110,
        0b0000_0000, //
        0b0011_1100,
        0b0011_1100,
        0b0000_0000,
        0b0000_0000,
    ],
    [
        0b0000_0000,
        0b0110_0110,
        0b0110_0110,
        0b0000_0000, //
        0b0000_0000,
        0b0001_1000,
        0b0000_0000,
        0b0000_0000,
    ],
];

/// think：眼上挑 + 小嘴（单帧）
const RR3_FRAMES_THINK: [Rr3FaceFrame; 1] = [[
    0b0110_0110,
    0b0110_0110,
    0b0000_0000,
    0b0000_0000, //
    0b0000_0000,
    0b0001_1000,
    0b0000_0000,
    0b0000_0000,
]];

/// happy：笑眼 + 大笑嘴（单帧）
const RR3_FRAMES_HAPPY: [Rr3FaceFrame; 1] = [[
    0b0000_0000,
    0b0110_0110,
    0b0110_0110,
    0b0000_0000, //
    0b0100_0010,
    0b0011_1100,
    0b0000_0000,
    0b0000_0000,
]];

/// listen：大睁眼（三行高）+ 小圆嘴（单帧）——"瞪眼听"
const RR3_FRAMES_LISTEN: [Rr3FaceFrame; 1] = [[
    0b0110_0110,
    0b0110_0110,
    0b0110_0110,
    0b0000_0000, //
    0b0000_0000,
    0b0001_1000,
    0b0000_0000,
    0b0000_0000,
]];

/// turn：双眼右移一格（看向转向侧）+ 平嘴（单帧）
const RR3_FRAMES_TURN: [Rr3FaceFrame; 1] = [[
    0b0000_0000,
    0b0011_0011,
    0b0011_0011,
    0b0000_0000, //
    0b0000_0000,
    0b0011_1100,
    0b0000_0000,
    0b0000_0000,
]];

/// 白名单序 → 帧序列表（与 RR3_FACE_EXPRESSIONS 下标对齐；blink/talk 双帧其余单帧）
const RR3_FACE_FRAME_TABLE: [&[Rr3FaceFrame]; 7] = [
    &RR3_FRAMES_IDLE,
    &RR3_FRAMES_BLINK,
    &RR3_FRAMES_TALK,
    &RR3_FRAMES_THINK,
    &RR3_FRAMES_HAPPY,
    &RR3_FRAMES_LISTEN,
    &RR3_FRAMES_TURN,
];

/// 表情名 → 帧序列。未知表情回退 idle 帧（调用方已过白名单，此为兜底）。
pub fn rr3_face_frames(expr: &str) -> &'static [Rr3FaceFrame] {
    let ix = RR3_FACE_EXPRESSIONS
        .iter()
        .position(|&e| e == expr)
        .unwrap_or(0);
    RR3_FACE_FRAME_TABLE[ix]
}

/// 最小表情机：持有当前表情；`talk` 是唯一的瞬态——播放开始挂起（记住被打断
/// 的表情），播放结束恢复。单线程主循环内使用，无并发面。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3Face {
    current: &'static str,
    /// talk 挂起前的表情（end_talk 恢复的去处）
    suspended: Option<&'static str>,
    /// 当前表情帧序列内的指针（rr3_face_tick 推进；入态归零从首帧重播）
    frame_ix: usize,
}

impl Default for Rr3Face {
    fn default() -> Self {
        Self {
            current: "idle",
            suspended: None,
            frame_ix: 0,
        }
    }
}

impl Rr3Face {
    /// 点名切换表情：名字必须在白名单内。返回 `(切换后, 切换前)`。
    /// talk 不可被点名（它是播放面的自动态），点名切换会打断挂起态——
    /// 显式指令优先于自动恢复，这是表情机的最小仲裁规则。
    pub fn rr3_face_set(&mut self, expr: &str) -> Result<(&'static str, &'static str), String> {
        let expr = RR3_FACE_EXPRESSIONS
            .iter()
            .find(|&&e| e == expr)
            .copied()
            .ok_or_else(|| {
                format!(
                    "{RR3_FACE_TOPIC} 不认表情 {expr:?}（允许 {expr_list}）",
                    expr_list = RR3_FACE_EXPRESSIONS.join("/")
                )
            })?;
        if expr == RR3_FACE_TALK {
            return Err(format!("{RR3_FACE_TALK} 由 TTS 播放自动驱动，不接受点名"));
        }
        let prev = std::mem::replace(&mut self.current, expr);
        self.suspended = None; // 显式指令打断 talk 挂起态
        self.frame_ix = 0; // 切换即从首帧重播
        Ok((expr, prev))
    }

    /// TTS 播放开始：切到 talk，返回被挂起的表情。已在 talk（嵌套播放）则幂等。
    pub fn rr3_face_begin_talk(&mut self) -> &'static str {
        if self.current == RR3_FACE_TALK {
            return self.suspended.unwrap_or("idle");
        }
        let prev = std::mem::replace(&mut self.current, RR3_FACE_TALK);
        self.suspended = Some(prev);
        self.frame_ix = 0; // 进 talk 从首帧（张口帧）播起
        prev
    }

    /// TTS 播放结束：恢复挂起前的表情。没有挂起态（如点名后直接调）则回 idle。
    pub fn rr3_face_end_talk(&mut self) -> &'static str {
        match self.suspended.take() {
            Some(prev) => {
                self.current = prev;
                self.frame_ix = 0;
                prev
            }
            None => {
                self.current = "idle";
                self.frame_ix = 0;
                "idle"
            }
        }
    }

    /// 帧动画一拍：报 `(当前表情, 本拍帧序号)` 并把指针推向下一帧（到尾回卷）。
    /// 先报后推——入态后的首拍报帧 0，调用方按返回值取帧渲染（真件随批F5a
    /// 显示 trait 接线；sim 档主循环按 ~250ms 节拍空推）。单帧态恒报 0。
    pub fn rr3_face_tick(&mut self) -> (&'static str, usize) {
        let ix = self.frame_ix;
        self.frame_ix = (self.frame_ix + 1) % rr3_face_frames(self.current).len();
        (self.current, ix)
    }
}

/// 从 rr3.call 的 `params.args` 里取表情请求参数：`expression` 必填。
/// 合法性交给 [`Rr3Face::rr3_face_set`]（错误信息里带白名单，这里不做两遍）。
pub fn rr3_face_parse_args(args: &Value) -> Result<String, String> {
    args.get("expression")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{RR3_FACE_TOPIC} 缺 string 参数 expression"))
}

/// `rr3-interact/face/expression` 的应答体：切换后的表情与切换前的表情。
pub fn rr3_face_result(current: &str, previous: &str) -> Value {
    json!({ "expression": current, "previous": previous })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn whitelist_covers_seven_expressions() {
        assert_eq!(
            RR3_FACE_EXPRESSIONS,
            ["idle", "blink", "talk", "think", "happy", "listen", "turn"]
        );
        // 初始态 idle：首次切换的"切换前"就是它
        assert_eq!(Rr3Face::default().rr3_face_set("blink").unwrap().1, "idle");
    }

    #[test]
    fn new_states_are_point_namable() {
        let mut f = Rr3Face::default();
        assert_eq!(f.rr3_face_set("listen").unwrap(), ("listen", "idle"));
        assert_eq!(f.rr3_face_set("turn").unwrap(), ("turn", "listen"));
    }

    #[test]
    fn single_frame_state_ticks_in_place() {
        let mut f = Rr3Face::default();
        for _ in 0..3 {
            assert_eq!(f.rr3_face_tick(), ("idle", 0), "单帧态恒报帧 0");
        }
    }

    #[test]
    fn multi_frame_state_cycles() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("blink").unwrap();
        assert_eq!(f.rr3_face_tick(), ("blink", 0));
        assert_eq!(f.rr3_face_tick(), ("blink", 1));
        assert_eq!(f.rr3_face_tick(), ("blink", 0), "到尾回卷");
        assert_eq!(f.rr3_face_tick(), ("blink", 1));
    }

    #[test]
    fn switch_resets_frame_pointer() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("blink").unwrap();
        f.rr3_face_tick(); // 指针已推到 1
        f.rr3_face_set("think").unwrap();
        assert_eq!(f.rr3_face_tick(), ("think", 0), "切换后从首帧重播");
    }

    #[test]
    fn talk_window_ticks_talk_frames() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("happy").unwrap();
        f.rr3_face_begin_talk();
        assert_eq!(f.rr3_face_tick(), ("talk", 0));
        assert_eq!(f.rr3_face_tick(), ("talk", 1), "挂起期间走 talk 帧表");
        assert_eq!(f.rr3_face_end_talk(), "happy");
        assert_eq!(f.rr3_face_tick(), ("happy", 0), "恢复后指针归零");
    }

    #[test]
    fn frame_table_shape_is_sound() {
        assert_eq!(RR3_FACE_EXPRESSIONS.len(), RR3_FACE_FRAME_TABLE.len());
        // 双帧态帧间互异（有动画可言）；全表序列两两互异（状态→帧映射无歧义）
        assert_ne!(RR3_FRAMES_BLINK[0], RR3_FRAMES_BLINK[1]);
        assert_ne!(RR3_FRAMES_TALK[0], RR3_FRAMES_TALK[1]);
        for i in 0..RR3_FACE_FRAME_TABLE.len() {
            assert!(!rr3_face_frames(RR3_FACE_EXPRESSIONS[i]).is_empty());
            for j in 0..i {
                assert_ne!(
                    RR3_FACE_FRAME_TABLE[i], RR3_FACE_FRAME_TABLE[j],
                    "表情 {} 与 {} 的帧序列必须互异",
                    RR3_FACE_EXPRESSIONS[i], RR3_FACE_EXPRESSIONS[j]
                );
            }
        }
        // 白名单名逐个查得回对应表项；未知名兜底回 idle 帧
        assert_eq!(rr3_face_frames("idle"), RR3_FRAMES_IDLE.as_slice());
        assert_eq!(rr3_face_frames("blink"), RR3_FRAMES_BLINK.as_slice());
        assert_eq!(rr3_face_frames("talk"), RR3_FRAMES_TALK.as_slice());
        assert_eq!(rr3_face_frames("think"), RR3_FRAMES_THINK.as_slice());
        assert_eq!(rr3_face_frames("happy"), RR3_FRAMES_HAPPY.as_slice());
        assert_eq!(rr3_face_frames("listen"), RR3_FRAMES_LISTEN.as_slice());
        assert_eq!(rr3_face_frames("turn"), RR3_FRAMES_TURN.as_slice());
        assert_eq!(rr3_face_frames("nope"), rr3_face_frames("idle"));
    }

    #[test]
    fn set_accepts_whitelist_and_reports_transition() {
        let mut f = Rr3Face::default();
        assert_eq!(f.rr3_face_set("happy").unwrap(), ("happy", "idle"));
        assert_eq!(f.rr3_face_set("think").unwrap(), ("think", "happy"));
    }

    #[test]
    fn set_rejects_unknown_and_point_named_talk() {
        let mut f = Rr3Face::default();
        assert!(f.rr3_face_set("wink").is_err());
        assert!(f.rr3_face_set("").is_err());
        assert!(f.rr3_face_set("talk").is_err(), "talk 是播放面的自动态");
        // 失败切换不动状态：下次成功切换的"切换前"仍是 idle
        assert_eq!(f.rr3_face_set("think").unwrap(), ("think", "idle"));
    }

    #[test]
    fn talk_suspends_and_restores_previous() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("think").unwrap();
        assert_eq!(f.rr3_face_begin_talk(), "think", "挂起前的表情");
        assert_eq!(
            f.rr3_face_set("blink").unwrap(),
            ("blink", "talk"),
            "talk 期间 current 是 talk"
        );
        // talk 期间被点名打断了挂起态，end_talk 只能回 idle——见 explicit_set 测试；
        // 这里另起一台机器验证正常恢复路径
        let mut g = Rr3Face::default();
        g.rr3_face_set("think").unwrap();
        g.rr3_face_begin_talk();
        assert_eq!(g.rr3_face_end_talk(), "think", "播放结束恢复");
        assert_eq!(g.rr3_face_set("blink").unwrap(), ("blink", "think"));
    }

    #[test]
    fn talk_without_suspend_restores_idle_and_is_idempotent() {
        let mut f = Rr3Face::default();
        // 无挂起态的 end_talk 回 idle
        assert_eq!(f.rr3_face_end_talk(), "idle");
        // 嵌套 begin_talk 幂等：不覆盖 suspended
        f.rr3_face_set("happy").unwrap();
        f.rr3_face_begin_talk();
        assert_eq!(f.rr3_face_begin_talk(), "happy", "嵌套挂起不丢原表情");
        assert_eq!(f.rr3_face_end_talk(), "happy");
        assert_eq!(
            f.rr3_face_set("think").unwrap(),
            ("think", "happy"),
            "恢复到位"
        );
    }

    #[test]
    fn explicit_set_interrupts_talk_suspend() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("blink").unwrap();
        f.rr3_face_begin_talk();
        // 播放途中被点名 happy：挂起态作废，end_talk 不再回 blink
        let (cur, prev) = f.rr3_face_set("happy").unwrap();
        assert_eq!((cur, prev), ("happy", "talk"));
        assert_eq!(f.rr3_face_end_talk(), "idle", "挂起态已被显式指令打断");
        assert_eq!(f.rr3_face_set("blink").unwrap(), ("blink", "idle"));
    }

    #[test]
    fn args_and_result_shapes() {
        assert_eq!(
            rr3_face_parse_args(&json!({ "expression": "blink" })).unwrap(),
            "blink"
        );
        assert!(rr3_face_parse_args(&json!({})).is_err());
        assert!(rr3_face_parse_args(&json!({ "expression": 1 })).is_err());
        let r = rr3_face_result("talk", "idle");
        assert_eq!(r["expression"], "talk");
        assert_eq!(r["previous"], "idle");
        assert_eq!(r.get("topic"), None, "应答体不带 topic");
    }
}
