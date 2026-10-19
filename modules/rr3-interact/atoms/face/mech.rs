//! face 原子件机制主体（五件套③，自原 src/face.rs 逐字迁入，逻辑零改动）。
//!
//! 每态配 8x8 点阵帧序列（blink/talk 双帧、其余单帧）；[`Rr3Face::rr3_face_tick`]
//! 一拍报当前帧并推进（到尾回卷），切换表情 / talk 挂起恢复都归零指针从首帧
//! 重播。最小表情机：持有当前表情；`talk` 是唯一的瞬态——播放开始挂起（记住
//! 被打断的表情），播放结束恢复。单线程主循环内使用，无并发面。
//!
//! 载荷组装（`rr3_face_result` 的 json!）与点名下钻（`rr3_face_parse_args`）
//! 已上移承载层（0.0.5 批I I-8，ADR-0019 修订 4）：件面只交状态机
//! （结构体 + 判定），组装归 `src/composite.rs`，线上字节逐字不变。

use super::{
    rr3_face_expression_lookup, rr3_face_non_pointable_join, Rr3FaceFrame, RR3_FACE_EXPRESSIONS,
    RR3_FACE_TALK, RR3_FACE_TOPIC,
};

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

// ---------- char 单字符档（2026-10-15 板上批：36 字符静态单帧，机制零改动） ----------
// 字模 = 经典 5×7 列位图（与板上 marquee 实验同源），行 0-6 顶对齐、5 列落在
// bit6..bit2（glyph 左列 = bit6，bit7 最左空一位）；行 7 恒空。

/// charA：单字符 "A"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_A: [Rr3FaceFrame; 1] = [[
    0b0011_1000,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0111_1100,
    0b0100_0100,
    0b0100_0100,
    0b0000_0000,
]];

/// charB：单字符 "B"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_B: [Rr3FaceFrame; 1] = [[
    0b0111_1000,
    0b0100_0100,
    0b0100_0100,
    0b0111_1000,
    0b0100_0100,
    0b0100_0100,
    0b0111_1000,
    0b0000_0000,
]];

/// charC：单字符 "C"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_C: [Rr3FaceFrame; 1] = [[
    0b0011_1000,
    0b0100_0100,
    0b0100_0000,
    0b0100_0000,
    0b0100_0000,
    0b0100_0100,
    0b0011_1000,
    0b0000_0000,
]];

/// charD：单字符 "D"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_D: [Rr3FaceFrame; 1] = [[
    0b0111_0000,
    0b0100_1000,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0100_1000,
    0b0111_0000,
    0b0000_0000,
]];

/// charE：单字符 "E"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_E: [Rr3FaceFrame; 1] = [[
    0b0111_1100,
    0b0100_0000,
    0b0100_0000,
    0b0111_1000,
    0b0100_0000,
    0b0100_0000,
    0b0111_1100,
    0b0000_0000,
]];

/// charF：单字符 "F"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_F: [Rr3FaceFrame; 1] = [[
    0b0111_1100,
    0b0100_0000,
    0b0100_0000,
    0b0111_1000,
    0b0100_0000,
    0b0100_0000,
    0b0100_0000,
    0b0000_0000,
]];

/// charG：单字符 "G"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_G: [Rr3FaceFrame; 1] = [[
    0b0011_1000,
    0b0100_0100,
    0b0100_0000,
    0b0101_1100,
    0b0100_0100,
    0b0100_0100,
    0b0011_1000,
    0b0000_0000,
]];

/// charH：单字符 "H"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_H: [Rr3FaceFrame; 1] = [[
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0111_1100,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0000_0000,
]];

/// charI：单字符 "I"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_I: [Rr3FaceFrame; 1] = [[
    0b0011_1000,
    0b0001_0000,
    0b0001_0000,
    0b0001_0000,
    0b0001_0000,
    0b0001_0000,
    0b0011_1000,
    0b0000_0000,
]];

/// charJ：单字符 "J"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_J: [Rr3FaceFrame; 1] = [[
    0b0001_1100,
    0b0000_1000,
    0b0000_1000,
    0b0000_1000,
    0b0000_1000,
    0b0100_1000,
    0b0011_0000,
    0b0000_0000,
]];

/// charK：单字符 "K"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_K: [Rr3FaceFrame; 1] = [[
    0b0100_0100,
    0b0100_1000,
    0b0101_0000,
    0b0110_0000,
    0b0101_0000,
    0b0100_1000,
    0b0100_0100,
    0b0000_0000,
]];

/// charL：单字符 "L"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_L: [Rr3FaceFrame; 1] = [[
    0b0100_0000,
    0b0100_0000,
    0b0100_0000,
    0b0100_0000,
    0b0100_0000,
    0b0100_0000,
    0b0111_1100,
    0b0000_0000,
]];

/// charM：单字符 "M"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_M: [Rr3FaceFrame; 1] = [[
    0b0100_0100,
    0b0110_1100,
    0b0101_0100,
    0b0101_0100,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0000_0000,
]];

/// charN：单字符 "N"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_N: [Rr3FaceFrame; 1] = [[
    0b0100_0100,
    0b0100_0100,
    0b0110_0100,
    0b0101_0100,
    0b0100_1100,
    0b0100_0100,
    0b0100_0100,
    0b0000_0000,
]];

/// charO：单字符 "O"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_O: [Rr3FaceFrame; 1] = [[
    0b0011_1000,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0011_1000,
    0b0000_0000,
]];

/// charP：单字符 "P"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_P: [Rr3FaceFrame; 1] = [[
    0b0111_1000,
    0b0100_0100,
    0b0100_0100,
    0b0111_1000,
    0b0100_0000,
    0b0100_0000,
    0b0100_0000,
    0b0000_0000,
]];

/// charQ：单字符 "Q"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_Q: [Rr3FaceFrame; 1] = [[
    0b0011_1000,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0101_0100,
    0b0100_1000,
    0b0011_0100,
    0b0000_0000,
]];

/// charR：单字符 "R"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_R: [Rr3FaceFrame; 1] = [[
    0b0111_1000,
    0b0100_0100,
    0b0100_0100,
    0b0111_1000,
    0b0101_0000,
    0b0100_1000,
    0b0100_0100,
    0b0000_0000,
]];

/// charS：单字符 "S"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_S: [Rr3FaceFrame; 1] = [[
    0b0011_1100,
    0b0100_0000,
    0b0100_0000,
    0b0011_1000,
    0b0000_0100,
    0b0000_0100,
    0b0111_1000,
    0b0000_0000,
]];

/// charT：单字符 "T"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_T: [Rr3FaceFrame; 1] = [[
    0b0111_1100,
    0b0001_0000,
    0b0001_0000,
    0b0001_0000,
    0b0001_0000,
    0b0001_0000,
    0b0001_0000,
    0b0000_0000,
]];

/// charU：单字符 "U"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_U: [Rr3FaceFrame; 1] = [[
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0011_1000,
    0b0000_0000,
]];

/// charV：单字符 "V"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_V: [Rr3FaceFrame; 1] = [[
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0010_1000,
    0b0001_0000,
    0b0000_0000,
]];

/// charW：单字符 "W"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_W: [Rr3FaceFrame; 1] = [[
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0101_0100,
    0b0101_0100,
    0b0101_0100,
    0b0010_1000,
    0b0000_0000,
]];

/// charX：单字符 "X"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_X: [Rr3FaceFrame; 1] = [[
    0b0100_0100,
    0b0100_0100,
    0b0010_1000,
    0b0001_0000,
    0b0010_1000,
    0b0100_0100,
    0b0100_0100,
    0b0000_0000,
]];

/// charY：单字符 "Y"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_Y: [Rr3FaceFrame; 1] = [[
    0b0100_0100,
    0b0100_0100,
    0b0100_0100,
    0b0010_1000,
    0b0001_0000,
    0b0001_0000,
    0b0001_0000,
    0b0000_0000,
]];

/// charZ：单字符 "Z"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_Z: [Rr3FaceFrame; 1] = [[
    0b0111_1100,
    0b0000_0100,
    0b0000_1000,
    0b0001_0000,
    0b0010_0000,
    0b0100_0000,
    0b0111_1100,
    0b0000_0000,
]];

/// char0：单字符 "0"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_D0: [Rr3FaceFrame; 1] = [[
    0b0011_1000,
    0b0100_0100,
    0b0100_1100,
    0b0101_0100,
    0b0110_0100,
    0b0100_0100,
    0b0011_1000,
    0b0000_0000,
]];

/// char1：单字符 "1"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_D1: [Rr3FaceFrame; 1] = [[
    0b0001_0000,
    0b0011_0000,
    0b0001_0000,
    0b0001_0000,
    0b0001_0000,
    0b0001_0000,
    0b0011_1000,
    0b0000_0000,
]];

/// char2：单字符 "2"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_D2: [Rr3FaceFrame; 1] = [[
    0b0011_1000,
    0b0100_0100,
    0b0000_0100,
    0b0000_1000,
    0b0001_0000,
    0b0010_0100,
    0b0111_1100,
    0b0000_0000,
]];

/// char3：单字符 "3"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_D3: [Rr3FaceFrame; 1] = [[
    0b0111_1100,
    0b0000_1000,
    0b0001_0000,
    0b0000_1000,
    0b0000_0100,
    0b0100_0100,
    0b0011_1000,
    0b0000_0000,
]];

/// char4：单字符 "4"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_D4: [Rr3FaceFrame; 1] = [[
    0b0000_1000,
    0b0001_1000,
    0b0010_1000,
    0b0100_1000,
    0b0111_1100,
    0b0000_1000,
    0b0000_1000,
    0b0000_0000,
]];

/// char5：单字符 "5"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_D5: [Rr3FaceFrame; 1] = [[
    0b0111_1100,
    0b0100_0000,
    0b0111_1000,
    0b0000_0100,
    0b0000_0100,
    0b0100_0100,
    0b0011_1000,
    0b0000_0000,
]];

/// char6：单字符 "6"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_D6: [Rr3FaceFrame; 1] = [[
    0b0001_1000,
    0b0010_0000,
    0b0100_0000,
    0b0111_1000,
    0b0100_0100,
    0b0100_0100,
    0b0011_1000,
    0b0000_0000,
]];

/// char7：单字符 "7"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_D7: [Rr3FaceFrame; 1] = [[
    0b0111_1100,
    0b0000_0100,
    0b0000_1000,
    0b0001_0000,
    0b0010_0000,
    0b0010_0000,
    0b0010_0000,
    0b0000_0000,
]];

/// char8：单字符 "8"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_D8: [Rr3FaceFrame; 1] = [[
    0b0011_1000,
    0b0100_0100,
    0b0100_0100,
    0b0011_1000,
    0b0100_0100,
    0b0100_0100,
    0b0011_1000,
    0b0000_0000,
]];

/// char9：单字符 "9"（5×7 居中：行 0-6、列 bit6..bit2）
const RR3_FRAMES_CHAR_D9: [Rr3FaceFrame; 1] = [[
    0b0011_1000,
    0b0100_0100,
    0b0100_0100,
    0b0011_1100,
    0b0000_0100,
    0b0000_1000,
    0b0011_0000,
    0b0000_0000,
]];

/// 白名单序 → 帧序列表（与 RR3_FACE_EXPRESSIONS 下标对齐；blink/talk 双帧，
/// 七核心态与 char 单字符档全为单帧）
const RR3_FACE_FRAME_TABLE: [&[Rr3FaceFrame]; 43] = [
    &RR3_FRAMES_IDLE,
    &RR3_FRAMES_BLINK,
    &RR3_FRAMES_TALK,
    &RR3_FRAMES_THINK,
    &RR3_FRAMES_HAPPY,
    &RR3_FRAMES_LISTEN,
    &RR3_FRAMES_TURN,
    &RR3_FRAMES_CHAR_A,
    &RR3_FRAMES_CHAR_B,
    &RR3_FRAMES_CHAR_C,
    &RR3_FRAMES_CHAR_D,
    &RR3_FRAMES_CHAR_E,
    &RR3_FRAMES_CHAR_F,
    &RR3_FRAMES_CHAR_G,
    &RR3_FRAMES_CHAR_H,
    &RR3_FRAMES_CHAR_I,
    &RR3_FRAMES_CHAR_J,
    &RR3_FRAMES_CHAR_K,
    &RR3_FRAMES_CHAR_L,
    &RR3_FRAMES_CHAR_M,
    &RR3_FRAMES_CHAR_N,
    &RR3_FRAMES_CHAR_O,
    &RR3_FRAMES_CHAR_P,
    &RR3_FRAMES_CHAR_Q,
    &RR3_FRAMES_CHAR_R,
    &RR3_FRAMES_CHAR_S,
    &RR3_FRAMES_CHAR_T,
    &RR3_FRAMES_CHAR_U,
    &RR3_FRAMES_CHAR_V,
    &RR3_FRAMES_CHAR_W,
    &RR3_FRAMES_CHAR_X,
    &RR3_FRAMES_CHAR_Y,
    &RR3_FRAMES_CHAR_Z,
    &RR3_FRAMES_CHAR_D0,
    &RR3_FRAMES_CHAR_D1,
    &RR3_FRAMES_CHAR_D2,
    &RR3_FRAMES_CHAR_D3,
    &RR3_FRAMES_CHAR_D4,
    &RR3_FRAMES_CHAR_D5,
    &RR3_FRAMES_CHAR_D6,
    &RR3_FRAMES_CHAR_D7,
    &RR3_FRAMES_CHAR_D8,
    &RR3_FRAMES_CHAR_D9,
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
    /// 兜底表情（批N4 件五 I-17/M-3 数据化）：各回落点（end_talk 无挂起、
    /// begin_talk 幂等分支）的去处——装配期自 config `default_expression`
    /// 注入，构造器 [`Rr3Face::rr3_face_with_default`]；Default 是 legacy 回落
    /// （S-5 口径），不再是唯一真值点
    default_expr: &'static str,
}

impl Rr3Face {
    /// 带兜底表情的构造器（批N4 件五 I-17/M-3）：`default_expr` 须为在册值
    /// （装配期 parse 已对账白名单；此处机制面不再复检，值域由调用方保证）。
    /// 初态即兜底表情——新机器当前态就是人格取向本身。
    pub fn rr3_face_with_default(default_expr: &'static str) -> Self {
        Self {
            current: default_expr,
            suspended: None,
            frame_ix: 0,
            default_expr,
        }
    }
}

impl Default for Rr3Face {
    fn default() -> Self {
        // legacy 兜底 idle（一个事实一个真值点：legacy 值只在此处与 config.toml
        // 注释对表）
        Self::rr3_face_with_default("idle")
    }
}

impl Rr3Face {
    /// 点名切换表情：名字必须在白名单内。返回 `(切换后, 切换前)`。
    /// `non_pointable` = 本形态不可点名集（0.0.5 批M MC-10 I-11：config 注入，
    /// 默认含 `talk`——它是播放面的自动态）。命中排除集则拒（状态不动）；点名
    /// 切换会打断挂起态——显式指令优先于自动恢复，这是表情机的最小仲裁规则。
    pub fn rr3_face_set(
        &mut self,
        expr: &str,
        non_pointable: &[&str],
    ) -> Result<(&'static str, &'static str), String> {
        let expr = rr3_face_expression_lookup(expr).ok_or_else(|| {
            format!(
                "{RR3_FACE_TOPIC} 不认表情 {expr:?}（允许 {}）",
                RR3_FACE_EXPRESSIONS.join("/")
            )
        })?;
        if non_pointable.contains(&expr) {
            return Err(format!(
                "{expr} 不可被点名（本形态不可点名集 {}）",
                rr3_face_non_pointable_join(non_pointable)
            ));
        }
        let prev = core::mem::replace(&mut self.current, expr);
        self.suspended = None; // 显式指令打断 talk 挂起态
        self.frame_ix = 0; // 切换即从首帧重播
        Ok((expr, prev))
    }

    /// TTS 播放开始：切到 talk，返回被挂起的表情。已在 talk（嵌套播放）则幂等。
    pub fn rr3_face_begin_talk(&mut self) -> &'static str {
        if self.current == RR3_FACE_TALK {
            return self.suspended.unwrap_or(self.default_expr);
        }
        let prev = core::mem::replace(&mut self.current, RR3_FACE_TALK);
        self.suspended = Some(prev);
        self.frame_ix = 0; // 进 talk 从首帧（张口帧）播起
        prev
    }

    /// TTS 播放结束：恢复挂起前的表情。没有挂起态（如点名后直接调）则回兜底
    /// 表情（批N4 件五 I-17/M-3：default_expr 数据化，legacy 回落 idle）。
    pub fn rr3_face_end_talk(&mut self) -> &'static str {
        match self.suspended.take() {
            Some(prev) => {
                self.current = prev;
                self.frame_ix = 0;
                prev
            }
            None => {
                self.current = self.default_expr;
                self.frame_ix = 0;
                self.default_expr
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 缺省形态的不可点名集（config 档同源：`[rr3-interact.face] non_pointable`）
    const NON_TALK: &[&str] = &["talk"];

    /// 白名单口径：七核心态在前（帧表下标锚点，契约字面量不动），char 单字符
    /// 档 36 个随后（charA..charZ + char0..char9），全集 43。
    #[test]
    fn whitelist_covers_core_and_char_expressions() {
        assert_eq!(RR3_FACE_EXPRESSIONS.len(), 43);
        assert_eq!(
            &RR3_FACE_EXPRESSIONS[..7],
            ["idle", "blink", "talk", "think", "happy", "listen", "turn"]
        );
        for (ix, byte) in (b'A'..=b'Z').chain(b'0'..=b'9').enumerate() {
            let expected = format!("char{}", byte as char);
            assert_eq!(
                RR3_FACE_EXPRESSIONS[7 + ix],
                expected.as_str(),
                "白名单位次 7+{ix}"
            );
        }
        // 初始态 idle：首次切换的"切换前"就是它
        assert_eq!(
            Rr3Face::default()
                .rr3_face_set("blink", NON_TALK)
                .unwrap()
                .1,
            "idle"
        );
    }

    /// 兜底表情注入（批N4 件五 I-17/M-3）：`rr3_face_with_default` 注入后，
    /// 各回落点（end_talk 无挂起、begin_talk 幂等分支、切换前值）取注入值
    /// 而非硬编码 idle——机制零策略载荷的 mech 侧钉子。
    #[test]
    fn default_expression_injection_drives_fallbacks() {
        let mut f = Rr3Face::rr3_face_with_default("happy");
        // 初态即兜底表情：首次切换的"切换前"就是注入值
        assert_eq!(f.rr3_face_set("blink", NON_TALK).unwrap().1, "happy");
        // end_talk 无挂起：回落注入值（连续调用同样回落，不折 idle）
        assert_eq!(f.rr3_face_end_talk(), "happy");
        assert_eq!(f.rr3_face_end_talk(), "happy");
        // begin_talk 幂等分支：已在 talk 且无挂起时返回注入值
        f.rr3_face_begin_talk();
        assert_eq!(f.rr3_face_begin_talk(), "happy");
    }

    /// char 单字符档：帧表映射对得上 + 点名成功 + 单帧恒报 0（charH 横杠行钉死，
    /// 字模转换走偏在此最先响）
    #[test]
    fn char_expression_is_pointable_single_frame() {
        assert_eq!(rr3_face_frames("charH"), RR3_FRAMES_CHAR_H.as_slice());
        assert_eq!(
            RR3_FRAMES_CHAR_H[0][3], 0b0111_1100,
            "H 横杠行（列 bit6..bit2）"
        );
        let mut f = Rr3Face::default();
        assert_eq!(
            f.rr3_face_set("charH", NON_TALK).unwrap(),
            ("charH", "idle")
        );
        for _ in 0..3 {
            assert_eq!(f.rr3_face_tick(), ("charH", 0), "单帧态恒报帧 0");
        }
        assert_eq!(rr3_face_frames("char9").len(), 1, "数字档同构单帧");
    }

    #[test]
    fn new_states_are_point_namable() {
        let mut f = Rr3Face::default();
        assert_eq!(
            f.rr3_face_set("listen", NON_TALK).unwrap(),
            ("listen", "idle")
        );
        assert_eq!(
            f.rr3_face_set("turn", NON_TALK).unwrap(),
            ("turn", "listen")
        );
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
        f.rr3_face_set("blink", NON_TALK).unwrap();
        assert_eq!(f.rr3_face_tick(), ("blink", 0));
        assert_eq!(f.rr3_face_tick(), ("blink", 1));
        assert_eq!(f.rr3_face_tick(), ("blink", 0), "到尾回卷");
        assert_eq!(f.rr3_face_tick(), ("blink", 1));
    }

    #[test]
    fn switch_resets_frame_pointer() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("blink", NON_TALK).unwrap();
        f.rr3_face_tick(); // 指针已推到 1
        f.rr3_face_set("think", NON_TALK).unwrap();
        assert_eq!(f.rr3_face_tick(), ("think", 0), "切换后从首帧重播");
    }

    #[test]
    fn talk_window_ticks_talk_frames() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("happy", NON_TALK).unwrap();
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
        assert_eq!(
            f.rr3_face_set("happy", NON_TALK).unwrap(),
            ("happy", "idle")
        );
        assert_eq!(
            f.rr3_face_set("think", NON_TALK).unwrap(),
            ("think", "happy")
        );
    }

    #[test]
    fn set_rejects_unknown_and_point_named_talk() {
        let mut f = Rr3Face::default();
        assert!(f.rr3_face_set("wink", NON_TALK).is_err());
        assert!(f.rr3_face_set("", NON_TALK).is_err());
        assert!(
            f.rr3_face_set("talk", NON_TALK).is_err(),
            "talk 是播放面的自动态"
        );
        // 失败切换不动状态：下次成功切换的"切换前"仍是 idle
        assert_eq!(
            f.rr3_face_set("think", NON_TALK).unwrap(),
            ("think", "idle")
        );
    }

    /// I-11：排除规则从件内 if 迁出为注入集——改数据即改行为，件内无 `match` 分支
    #[test]
    fn injected_non_pointable_narrows_surface() {
        // 空集：本形态不排除任何表情点名，连 talk 都可点名
        let mut open = Rr3Face::default();
        assert_eq!(open.rr3_face_set("talk", &[]).unwrap(), ("talk", "idle"));
        // 换一个排除子集（点名规则随数据走）：此刻排除 blink、放开 talk
        let mut swap = Rr3Face::default();
        assert_eq!(
            swap.rr3_face_set("talk", &["blink"]).unwrap(),
            ("talk", "idle")
        );
        assert!(
            swap.rr3_face_set("blink", &["blink"]).is_err(),
            "blink 进了排除集"
        );
        // 非在册名与排除集无关：白名单是契约面，任何注入集都拒
        assert!(swap.rr3_face_set("wink", &[]).is_err());
    }

    /// I-11：拒因文案点名"本形态不可点名集"（空集渲染 `(空)`，见 face.rs 用例）
    #[test]
    fn non_pointable_reject_names_the_set() {
        let mut f = Rr3Face::default();
        let err = f.rr3_face_set("talk", NON_TALK).unwrap_err();
        assert!(err.contains("不可被点名"), "拒因点明规则: {err}");
        assert!(err.contains("talk"), "拒因回显排除集: {err}");
    }

    #[test]
    fn talk_suspends_and_restores_previous() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("think", NON_TALK).unwrap();
        assert_eq!(f.rr3_face_begin_talk(), "think", "挂起前的表情");
        assert_eq!(
            f.rr3_face_set("blink", NON_TALK).unwrap(),
            ("blink", "talk"),
            "talk 期间 current 是 talk"
        );
        // talk 期间被点名打断了挂起态，end_talk 只能回 idle——见 explicit_set 测试；
        // 这里另起一台机器验证正常恢复路径
        let mut g = Rr3Face::default();
        g.rr3_face_set("think", NON_TALK).unwrap();
        g.rr3_face_begin_talk();
        assert_eq!(g.rr3_face_end_talk(), "think", "播放结束恢复");
        assert_eq!(
            g.rr3_face_set("blink", NON_TALK).unwrap(),
            ("blink", "think")
        );
    }

    #[test]
    fn talk_without_suspend_restores_idle_and_is_idempotent() {
        let mut f = Rr3Face::default();
        // 无挂起态的 end_talk 回 idle
        assert_eq!(f.rr3_face_end_talk(), "idle");
        // 嵌套 begin_talk 幂等：不覆盖 suspended
        f.rr3_face_set("happy", NON_TALK).unwrap();
        f.rr3_face_begin_talk();
        assert_eq!(f.rr3_face_begin_talk(), "happy", "嵌套挂起不丢原表情");
        assert_eq!(f.rr3_face_end_talk(), "happy");
        assert_eq!(
            f.rr3_face_set("think", NON_TALK).unwrap(),
            ("think", "happy"),
            "恢复到位"
        );
    }

    #[test]
    fn explicit_set_interrupts_talk_suspend() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("blink", NON_TALK).unwrap();
        f.rr3_face_begin_talk();
        // 播放途中被点名 happy：挂起态作废，end_talk 不再回 blink
        let (cur, prev) = f.rr3_face_set("happy", NON_TALK).unwrap();
        assert_eq!((cur, prev), ("happy", "talk"));
        assert_eq!(f.rr3_face_end_talk(), "idle", "挂起态已被显式指令打断");
        assert_eq!(
            f.rr3_face_set("blink", NON_TALK).unwrap(),
            ("blink", "idle")
        );
    }

    // ---------- 两腿对拍（architecture §4.0.3 判据②，0.0.5 批K H1） ----------
    // 腿一·确定性：§8 设计行"set → begin_talk → set(listen) → end_talk"权威序
    // 序列喂两台新表情机，(调用结果, 相位, 帧指针轨迹) 逐拍相等。腿二·正确性：
    // 期望值引自裁定语义（interact 册 §2 权威序：挂起期被显式点名则挂起作废、
    // end_talk 回默认态、连续 begin 幂等）+ §5 载荷腿（应答体两字段，
    // additionalProperties=false 口径——形状断言在承载层 args_and_result_shapes，
    // 此处钉机制侧的转移与帧轨迹）。

    /// 腿一：权威序序列两跑轨迹相等
    #[test]
    fn parity_leg1_face_authority_order_train_is_deterministic() {
        fn run() -> Vec<(&'static str, &'static str, usize)> {
            let mut f = Rr3Face::default();
            let mut out = Vec::new();
            let (cur, prev) = f.rr3_face_set("happy", NON_TALK).unwrap();
            out.push((cur, prev, f.rr3_face_tick().1));
            out.push((f.rr3_face_begin_talk(), "", f.rr3_face_tick().1));
            let (cur, prev) = f.rr3_face_set("listen", NON_TALK).unwrap();
            out.push((cur, prev, f.rr3_face_tick().1));
            let back = f.rr3_face_end_talk();
            out.push((back, "", f.rr3_face_tick().1));
            out
        }
        assert_eq!(run(), run(), "同输入两跑逐拍相等（腿一·确定性）");
    }

    /// 腿二：权威序裁定逐点断言——显式点名打断挂起、end_talk 回 idle、
    /// 嵌套 begin 幂等、未 begin 就 end 回 idle（§8 边界集）
    #[test]
    fn parity_leg2_face_matches_authority_order() {
        // 挂起期被显式点名 → 挂起作废 → end_talk 回默认态 idle
        let mut f = Rr3Face::default();
        f.rr3_face_set("happy", NON_TALK).unwrap();
        f.rr3_face_begin_talk();
        f.rr3_face_set("listen", NON_TALK).unwrap();
        assert_eq!(
            f.rr3_face_end_talk(),
            "idle",
            "裁定：挂起被点名打断后回默认态"
        );
        // 连续两次 begin（幂等）：不覆盖挂起态
        let mut g = Rr3Face::default();
        g.rr3_face_set("think", NON_TALK).unwrap();
        assert_eq!(g.rr3_face_begin_talk(), "think");
        assert_eq!(g.rr3_face_begin_talk(), "think", "嵌套 begin 幂等");
        assert_eq!(g.rr3_face_end_talk(), "think", "挂起态未被嵌套覆盖");
        // 未 begin 就 end：回 idle 且无 panic（§8 边界）
        let mut h = Rr3Face::default();
        assert_eq!(h.rr3_face_end_talk(), "idle", "未 begin 就 end 回 idle");
        // 双帧态回绕 + 换态归零（§8 帧推进行）
        let mut k = Rr3Face::default();
        k.rr3_face_set("blink", NON_TALK).unwrap();
        assert_eq!(k.rr3_face_tick().1, 0);
        assert_eq!(k.rr3_face_tick().1, 1);
        assert_eq!(k.rr3_face_tick().1, 0, "双帧态到尾回卷");
        k.rr3_face_set("turn", NON_TALK).unwrap();
        assert_eq!(k.rr3_face_tick().1, 0, "换态归零");
    }
}
