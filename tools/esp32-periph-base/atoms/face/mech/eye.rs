// SPDX-License-Identifier: Apache-2.0
//! face 原子件 · 眼形几何（自 `mech.rs` 按职责拆出）。
//!
//! 只做形状求值与眉形像素落位，依赖画布常量与 [`super::EyeStyle`]；
//! 页面缓存布局与表情预设留在 `mech.rs`。

use super::{Brow, EyeShape, EyeStyle, BROW_THICK, BROW_TILT, BROW_Y, EYES, W2};

pub(super) struct EyePaint {
    pub(super) cx: i32,
    pub(super) cy: i32,
    pub(super) y: i32,
    pub(super) half_h: i32,
    pub(super) rc: i32,
    pub(super) straight: i32,
}

pub(super) fn paint_eye(buf: &mut [u8; 128], bit: u8, shape: EyeShape, p: &EyePaint) {
    match shape {
        EyeShape::Capsule => paint_capsule(buf, bit, p),
        EyeShape::Ring => paint_ring(buf, bit, p),
        EyeShape::Heart => paint_heart(buf, bit, p),
        EyeShape::Spiral => paint_spiral(buf, bit, p),
    }
}

fn put_px(buf: &mut [u8; 128], x: i32, bit: u8) {
    if (0..128).contains(&x) {
        buf[x as usize] |= bit;
    }
}

fn paint_capsule(buf: &mut [u8; 128], bit: u8, p: &EyePaint) {
    let py = (p.y - p.cy).abs();
    if py > p.half_h {
        return;
    }
    let hw = hw_at(py, p.rc, p.straight);
    for x in (p.cx - hw)..=(p.cx + hw) {
        put_px(buf, x, bit);
    }
}

fn paint_ring(buf: &mut [u8; 128], bit: u8, p: &EyePaint) {
    let py = (p.y - p.cy).abs();
    if py > p.half_h {
        return;
    }
    let hw = hw_at(py, p.rc, p.straight);
    let lid = py > p.half_h - 3;
    for x in (p.cx - hw)..=(p.cx + hw) {
        if lid || (x - p.cx).abs() > hw - 3 {
            put_px(buf, x, bit);
        }
    }
    // 中心瞳孔（5×5）：空心环里的小黑点
    let pupil = if p.half_h >= 8 { 2 } else { 1 };
    if py <= pupil {
        for x in (p.cx - pupil)..=(p.cx + pupil) {
            put_px(buf, x, bit);
        }
    }
}

fn paint_heart(buf: &mut [u8; 128], bit: u8, p: &EyePaint) {
    // 心形隐函数 (x²+y²-1)³ - x²y³ ≤ 0（y 向上为正）
    for x in (p.cx - W2)..=(p.cx + W2) {
        let nx = (x - p.cx) as f32 / W2 as f32;
        let ny = (p.cy - p.y) as f32 / p.half_h as f32;
        let a = nx * nx + ny * ny - 1.0;
        if a * a * a - nx * nx * ny * ny * ny <= 0.0 {
            put_px(buf, x, bit);
        }
    }
}

fn paint_spiral(buf: &mut [u8; 128], bit: u8, p: &EyePaint) {
    // 同心双环（眩晕纹）——椭圆化到眼框内
    for x in (p.cx - W2)..=(p.cx + W2) {
        let nx = (x - p.cx) as f32 / W2 as f32;
        let ny = (p.cy - p.y) as f32 / p.half_h as f32;
        let d = (nx * nx + ny * ny).sqrt();
        if (d - 0.38).abs() < 0.16 || (d - 0.76).abs() < 0.16 {
            put_px(buf, x, bit);
        }
    }
}

/// 胶囊行内半宽：直段区全宽，圆角区取 sqrt 余量；越界返回 -1（空区间）。
fn hw_at(py: i32, rc: i32, straight: i32) -> i32 {
    if py <= straight {
        W2
    } else {
        let rem = rc * rc - (py - straight) * (py - straight);
        if rem <= 0 {
            -1
        } else {
            (W2 - rc + (rem as f32).sqrt() as i32).min(W2)
        }
    }
}

pub(super) fn paint_brow(buf: &mut [u8; 128], bit: u8, y: i32, style: EyeStyle) {
    if style.brow == Brow::None {
        return;
    }
    for (ix, &ecx) in EYES.iter().enumerate() {
        let cx = ecx + style.dx as i32;
        for x in (cx - W2).max(0)..=(cx + W2).min(127) {
            // 斜率位：t=0 外端 → t=BROW_TILT 内端（左眼）／反之（右眼）
            let t = ((x - (cx - W2)) * BROW_TILT) / (2 * W2);
            let off = brow_off(style.brow, t, ix);
            let by = y - (BROW_Y + style.dy as i32 + off);
            if (0..BROW_THICK).contains(&by) {
                buf[x as usize] |= bit;
            }
        }
    }
}

fn brow_off(brow: Brow, t: i32, ix: usize) -> i32 {
    let (left, right) = match brow {
        Brow::Angry => (t, BROW_TILT - t),
        Brow::Sad => (BROW_TILT - t, t),
        Brow::Focus => (1, 1),
        Brow::Confused => (-2, 2),
        Brow::None => (0, 0),
    };
    if ix == 0 {
        left
    } else {
        right
    }
}
