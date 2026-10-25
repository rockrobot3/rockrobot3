//! face 原子件 · 机制核（批N23 件二，MC-7 缺② 收编原子模块形态）。
//!
//! **格位对齐申报**：本件服务 `ui.face` 格位（esp32-mini 装配单格主 = rr3-face
//! 宿主件，板侧经桥执行——宿主申报面在 modules/rr3-face/atoms/face，板侧执行面
//! **不冒领格主**、不立 atom.toml：`provides` 恰一格与板无关断言是格主面法则，
//! 见批N23 计划册 §2「三不三立」）。
//!
//! **机制与策略分离**（文件级，收编落点）：本文件收拢 ui.face 板侧执行面的
//! **面目几何**——胶囊眼位图算术（眼形/画布常量与求宽算法不可分，随件内聚，
//! 数值零改动）与表情基态预设（11 态映射：7 基态 + 4 情绪态）；零 I2C、零时序、
//! 零引脚（SSD1306 页写传输面留 [`crate::oled`] 薄壳，微动作**时间窗策略**——眨眼/
//! 扫视间隔与窗口——留 [`crate::state`] 决策状态机）。策略→机制单向依赖：
//! oled.rs 逐页取本件位图后只做页写。
//!
//! 表情白名单键与 `RR3_FACE_EXPRS` 生成物同源档案（contracts/face-expressions.toml，
//! 帧面/指令面守门）；本件 `expr_eye_base` 的 match 兜底 fail-safe（未知表情
//! 正目全开），白名单扩员不哑不红。

// —— 画布与眼形（ui.face 板侧执行面内在定义：0.96" 128×64 OLED 胶囊眼面目，
//      收编自 oled.rs oled_capsules，数值零改动；换 OLED 面板尺寸时本件与
//      oled.rs 传输面一并动——面目语义归本件，页写协议归传输面）——

/// 眼半宽（眼宽 18px）。
const W2: i32 = 9;
/// 满开眼高。
const H_FULL: i32 = 26;
/// 眼高上限（surprised 125% 取 32px，仍留足画布上下沿）。
const H_MAX: i32 = 40;
/// 眼中心行（64 行画布居中）。
const CY: i32 = 32;
/// 眼中心列（128 列画布左右分置）。
const EYES: [i32; 2] = [42, 86];
/// 眉基线行（dy=0 时眉毛首行；眉眼间距按满开眼顶 19 行留 5 行）。
const BROW_Y: i32 = 14;
/// 眉厚（行数）。
const BROW_THICK: i32 = 2;
/// 眉倾斜跨度（像素）：怒眉内端低、丧眉外端低。
const BROW_TILT: i32 = 4;

/// 眉形（情绪附加几何；7 基态与 surprised 无眉）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Brow {
    None,
    /// 怒眉：内端（朝两眼中间）压低。
    Angry,
    /// 丧眉：外端压低。
    Sad,
}

/// 一态胶囊眼基态（**策略参数**，机制 [`capsule_page`] 只消费不算）：开合% /
/// 水平扫视 / 垂直扫视 / 眼高% / 眉形。`height_pct = 100` 即 [`H_FULL`]。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct EyeStyle {
    pub(crate) open: u32,
    pub(crate) dx: i16,
    pub(crate) dy: i16,
    pub(crate) height_pct: u32,
    pub(crate) brow: Brow,
}

impl EyeStyle {
    const fn new(open: u32, dx: i16, dy: i16, height_pct: u32, brow: Brow) -> Self {
        Self {
            open,
            dx,
            dy,
            height_pct,
            brow,
        }
    }
}

/// 正目全开基态（boot 初值与未知表情兜底）。
pub(crate) const EYE_IDLE: EyeStyle = EyeStyle::new(100, 0, 0, 100, Brow::None);

/// 胶囊眼单页位图（128 列 × 8 行，页内 bit0=页顶）：两只竖直胶囊眼，开合
/// open_pct% 与扫视偏移 (dx,dy) 由表情基态 + 微动作动画合成，眼高 height_pct%
/// 与眉形 brow 是情绪附加几何（**纯函数**，不碰 I2C——调用方
/// [`crate::oled::oled_capsules`] 逐页取图后只做页写）。
/// 形状=圆角竖矩形（圆角半径取 min(半宽, 半高)：满高 26px 时即纯胶囊，眯缝
/// 低压扁成横缝不退化成圆盘）；行内实心段解析求宽（直段区全宽、圆角区 sqrt
/// 余量）。扫视出界自然裁剪（0..127 列钳制）不回卷。眉毛随 (dx,dy) 平移。
pub(crate) fn capsule_page(
    page: u8,
    open_pct: u32,
    dx: i16,
    dy: i16,
    height_pct: u32,
    brow: Brow,
) -> [u8; 128] {
    let full = (H_FULL * height_pct.clamp(50, 150) as i32 / 100).min(H_MAX);
    let h = (full * open_pct.clamp(4, 100) as i32 / 100).max(4);
    let half_h = h / 2;
    let rc = W2.min(half_h); // 圆角半径
    let straight = half_h - rc; // 垂直直段半长
    let mut buf = [0u8; 128];
    let y0 = page as i32 * 8;
    for y in y0..y0 + 8 {
        let py = (y - (CY + dy as i32)).abs();
        if py <= half_h {
            for &ecx in &EYES {
                let cx = ecx + dx as i32;
                let hw = if py <= straight {
                    W2
                } else {
                    let rem = rc * rc - (py - straight) * (py - straight);
                    if rem <= 0 {
                        continue;
                    }
                    (W2 - rc + (rem as f32).sqrt() as i32).min(W2)
                };
                for x in (cx - hw).max(0)..=(cx + hw).min(127) {
                    buf[x as usize] |= 1 << (y % 8);
                }
            }
        }
        if brow != Brow::None {
            for &ecx in &EYES {
                let cx = ecx + dx as i32;
                for x in (cx - W2).max(0)..=(cx + W2).min(127) {
                    // 斜率位：t=0 外端 → t=BROW_TILT 内端（左眼）／反之（右眼）
                    let t = ((x - (cx - W2)) * BROW_TILT) / (2 * W2);
                    let off = match (brow, cx < 64) {
                        (Brow::Angry, true) => t,                  // 左眼内端低
                        (Brow::Angry, false) => BROW_TILT - t,     // 右眼内端低
                        (Brow::Sad, true) => BROW_TILT - t,        // 左眼外端低
                        (Brow::Sad, false) => t,                   // 右眼外端低
                        (Brow::None, _) => 0,
                    };
                    let by = y - (BROW_Y + dy as i32 + off);
                    if (0..BROW_THICK).contains(&by) {
                        buf[x as usize] |= 1 << (y % 8);
                    }
                }
            }
        }
    }
    buf
}

/// 表情→胶囊眼基态预设：11 态白名单各有面目（7 基态 + 4 情绪态），微动作动画
/// 在此基态上叠加（眨眼/扫视时间窗策略在 state.rs `PeriphState::anim_decide`，
/// 见模块文档的分离裁定）。情绪态语义：sleepy 半合下沉、surprised 眼高放大上提、
/// angry 窄眼压眉、sad 半垂垂眉。
pub(crate) fn expr_eye_base(expr: &str) -> EyeStyle {
    match expr {
        "blink" => EyeStyle::new(4, 0, 0, 100, Brow::None), // 眯缝
        "happy" => EyeStyle::new(55, 0, 2, 100, Brow::None), // 眯眼微垂
        "talk" => EyeStyle::new(70, 0, 0, 100, Brow::None), // 说话态半开
        "think" => EyeStyle::new(100, 6, -3, 100, Brow::None), // 抬眼右上
        "listen" => EyeStyle::new(100, -6, 0, 100, Brow::None), // 侧耳向左
        "turn" => EyeStyle::new(100, 8, 0, 100, Brow::None), // 回望右方
        "sleepy" => EyeStyle::new(22, 0, 2, 100, Brow::None), // 半合下垂
        "surprised" => EyeStyle::new(100, 0, -2, 125, Brow::None), // 眼高放大上提
        "angry" => EyeStyle::new(45, 0, 1, 100, Brow::Angry), // 窄眼压眉
        "sad" => EyeStyle::new(65, 0, 3, 100, Brow::Sad),   // 半垂垂眉
        _ => EYE_IDLE, // idle 与兜底：正目全开
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expr_eye_base_eleven_states_and_fallback() {
        let s = expr_eye_base;
        assert_eq!(s("blink"), EyeStyle::new(4, 0, 0, 100, Brow::None));
        assert_eq!(s("happy"), EyeStyle::new(55, 0, 2, 100, Brow::None));
        assert_eq!(s("talk"), EyeStyle::new(70, 0, 0, 100, Brow::None));
        assert_eq!(s("think"), EyeStyle::new(100, 6, -3, 100, Brow::None));
        assert_eq!(s("listen"), EyeStyle::new(100, -6, 0, 100, Brow::None));
        assert_eq!(s("turn"), EyeStyle::new(100, 8, 0, 100, Brow::None));
        assert_eq!(s("sleepy"), EyeStyle::new(22, 0, 2, 100, Brow::None));
        assert_eq!(s("surprised"), EyeStyle::new(100, 0, -2, 125, Brow::None));
        assert_eq!(s("angry"), EyeStyle::new(45, 0, 1, 100, Brow::Angry));
        assert_eq!(s("sad"), EyeStyle::new(65, 0, 3, 100, Brow::Sad));
        assert_eq!(s("idle"), EyeStyle::new(100, 0, 0, 100, Brow::None));
        // 未知表情兜底 fail-safe：正目全开（白名单守门在帧面/指令面，本件不拒）
        assert_eq!(s("no-such"), EyeStyle::new(100, 0, 0, 100, Brow::None));
    }

    /// 满开 100%：眼中心行（直段区）实心段 = 两眼各 19 列（cx±9 含端点），
    /// 段外与两眼间隙全零。
    #[test]
    fn capsule_full_open_center_row_geometry() {
        // y=32（页 4 bit0）= 满开眼中心行（CY=32）
        let buf = capsule_page(4, 100, 0, 0, 100, Brow::None);
        for x in 0..128 {
            let lit = buf[x] & 1 == 1;
            let expect = (33..=51).contains(&x) || (77..=95).contains(&x);
            assert_eq!(lit, expect, "列 {x} 置位不符（满开中心行几何）");
        }
    }

    /// 眨眼 4%：h = max(⌊26×4/100⌋, 4) = 4 → 仅眼心行上下各一行实心
    /// （y∈[31,33]，圆角 rem=0 截断 y=30/34）：页 3 bit7 + 页 4 bit0/bit1，
    /// 圆角行列宽收窄，其余页全零。
    #[test]
    fn capsule_blink_is_two_row_slit() {
        let p3 = capsule_page(3, 4, 0, 0, 100, Brow::None);
        let p4 = capsule_page(4, 4, 0, 0, 100, Brow::None);
        // y=32（页4 bit0）：py=0 ≤ straight=0 → 全宽直段（列 33..51 含 42/33）
        // y=31/33（py=1）：圆角 rem=3 → hw=8 → 列 34..50（含 42 不含 33）
        assert_eq!(p3[42] & 0b1000_0000, 0b1000_0000, "y=31 圆角行（页3 bit7）");
        assert_eq!(p3[33] & 0b1000_0000, 0, "y=31 段宽收窄：列 33 落段外");
        assert_eq!(p4[42] & 0b11, 0b11, "y=32/33 两行实心");
        assert_eq!(p4[33] & 0b11, 0b01, "y=33 段自列 34 起（y=32 全宽）");
        assert_eq!(p4[42] & 0b100, 0, "y=34 无像素（圆角 rem=0 截断）");
        for page in [0u8, 1, 2, 5, 6, 7] {
            assert!(
                capsule_page(page, 4, 0, 0, 100, Brow::None)
                    .iter()
                    .all(|&b| b == 0),
                "眨眼页 {page} 应全零"
            );
        }
    }

    /// 扫视出界自然裁剪（不回卷）：水平大偏移两眼全出画布 → 全零；垂直上移
    /// 超出画布上沿 → 全零；垂直下移 30 → 像素只落在画布底行，页 6/7 的
    /// 置位分布与圆角收窄一致。
    #[test]
    fn capsule_scan_offset_clamps_without_wrap() {
        for page in 0u8..8 {
            assert!(
                capsule_page(page, 100, 120, 0, 100, Brow::None)
                    .iter()
                    .all(|&b| b == 0),
                "dx=+120 两眼出界，页 {page} 应全零（不回卷）"
            );
            assert!(
                capsule_page(page, 100, 0, -60, 100, Brow::None)
                    .iter()
                    .all(|&b| b == 0),
                "dy=-60 眼形出画布上沿，页 {page} 应全零"
            );
        }
        // dy=+30：眼心行 62 → 实心行 y∈[49,63]（y=49 圆角 rem=0 截断）
        // 页 6（y 48..55）：y=50..55 置位 → bit2..bit7（y=48/49 空）
        // 页 7（y 56..63）：直段+圆角全实 → bit0..bit7
        let p6 = capsule_page(6, 100, 0, 30, 100, Brow::None);
        assert_eq!(p6[42], 0b1111_1100, "页 6 置位 y=50..55（bit2..bit7）");
        let p7 = capsule_page(7, 100, 0, 30, 100, Brow::None);
        assert_eq!(p7[42], 0xFF, "页 7 置位 y=56..63（bit0..bit7）");
    }

    /// 眼高缩放（surprised 125%）：满开眼高 32px → 眼心行上下各 16 行，
    /// 中心行仍全宽；页 2（y 16..23）首行即眼形顶（y=16 圆角截断，y=17 起）。
    #[test]
    fn capsule_tall_eye_extends_height() {
        let p2 = capsule_page(2, 100, 0, 0, 125, Brow::None);
        assert_eq!(p2[42] & 1, 0, "y=16 圆角 rem=0 截断（眼高 32 顶行）");
        assert_eq!(p2[42] & 0b10, 0b10, "y=17 圆角行起");
        // 页 4/5 中心区直段全宽（y 32..40 落在直段）
        let p4 = capsule_page(4, 100, 0, 0, 125, Brow::None);
        assert_ne!(p4[42], 0, "页 4 中心行实心");
        let p5 = capsule_page(5, 100, 0, 0, 125, Brow::None);
        assert_ne!(p5[42], 0, "页 5 仍属 32px 眼形（y 40..47）");
    }

    /// 眉形几何：怒眉内端低、丧眉外端低；无眉态页 1 全零。
    #[test]
    fn capsule_brow_slant_direction() {
        // 页 1 = y 8..15：眉基线 14（dy=0）→ 只有 y=14/15 两行；怒左眼内端
        // （x 靠 51）斜率下压，故内端像素落 y=16+，页 1 内端不亮、外端亮。
        let angry = capsule_page(1, 45, 0, 0, 100, Brow::Angry);
        assert_ne!(angry[34], 0, "怒眉左眼外端（x=34）在页 1 亮");
        assert_eq!(angry[50], 0, "怒眉左眼内端（x=50）下压出页 1");
        let sad = capsule_page(1, 65, 0, 0, 100, Brow::Sad);
        assert_eq!(sad[34], 0, "丧眉左眼外端下压出页 1");
        assert_ne!(sad[50], 0, "丧眉左眼内端（x=50）在页 1 亮");
        assert!(
            capsule_page(1, 100, 0, 0, 100, Brow::None)
                .iter()
                .all(|&b| b == 0),
            "无眉态页 1 全零"
        );
    }
}
