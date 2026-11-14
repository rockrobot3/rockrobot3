//! 白名单序帧表与表情名查表：52 项帧序列，下标与白名单对齐。

use super::super::{Rr3FaceFrame, RR3_FACE_EXPRESSIONS};
use super::frames_core::*;
use super::frames_emotion::*;
use super::frames_glyph::*;

/// 白名单序 → 帧序列表（与 RR3_FACE_EXPRESSIONS 下标对齐；blink/talk 双帧，
/// 七核心态 + 9 情绪/彩蛋态与 char 单字符档全为单帧）
pub(super) const RR3_FACE_FRAME_TABLE: [&[Rr3FaceFrame]; 52] = [
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
    &RR3_FRAMES_SLEEPY,
    &RR3_FRAMES_SURPRISED,
    &RR3_FRAMES_ANGRY,
    &RR3_FRAMES_SAD,
    &RR3_FRAMES_CONFUSED,
    &RR3_FRAMES_FOCUS,
    &RR3_FRAMES_LOVE,
    &RR3_FRAMES_DIZZY,
    &RR3_FRAMES_SCARED,
];

/// 表情名 → 帧序列。未知表情回退 idle 帧（调用方已过白名单，此为兜底）。
pub fn rr3_face_frames(expr: &str) -> &'static [Rr3FaceFrame] {
    let ix = RR3_FACE_EXPRESSIONS
        .iter()
        .position(|&e| e == expr)
        .unwrap_or(0);
    RR3_FACE_FRAME_TABLE[ix]
}
