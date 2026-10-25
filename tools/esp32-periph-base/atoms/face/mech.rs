//! face 原子件 · 机制核（批N23 件二，MC-7 缺② 收编原子模块形态）。
//!
//! **格位对齐申报**：本件服务 `ui.face` 格位（esp32-mini 装配单格主 = rr3-face
//! 宿主件，板侧经桥执行——宿主申报面在 modules/rr3-face/atoms/face，板侧执行面
//! **不冒领格主**、不立 atom.toml：`provides` 恰一格与板无关断言是格主面法则，
//! 见批N23 计划册 §2「三不三立」）。
//!
//! **机制与策略分离**（文件级，收编落点）：本文件收拢 ui.face 板侧执行面的
//! **面目几何**——眼形位图算术（画布常量与形状求值不可分，随件内聚，7 基态
//! 数值零改动）与表情基态预设（16 态映射：7 基态 + 4 情绪态 + 5 情绪/彩蛋态）；
//! 零 I2C、零时序、零引脚（SSD1306 页写传输面留 [`crate::oled`] 薄壳，微动作
//! **时间窗策略**——眨眼/扫视间隔与窗口——留 [`crate::state`] 决策状态机）。
//! 策略→机制单向依赖：oled.rs 逐页取本件位图后只做页写。
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

/// 眉形（情绪附加几何；基态与 surprised 无眉）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Brow {
    None,
    /// 怒眉：内端（朝两眼中间）压低。
    Angry,
    /// 丧眉：外端压低。
    Sad,
    /// 专注眉：平直略压（无倾斜）。
    Focus,
    /// 困惑眉：左眉抬高、右眉压低（一高一低）。
    Confused,
}

/// 眼形（彩蛋态换图元；胶囊是基态/情绪态默认形）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum EyeShape {
    /// 圆角竖矩形（含眯缝扁化）。
    Capsule,
    /// 心形（love 彩蛋）。
    Heart,
    /// 同心双环（dizzy 彩蛋）。
    Spiral,
    /// 空心胶囊 + 中心瞳孔（scared 彩蛋）。
    Ring,
}

/// 一态胶囊眼基态（**策略参数**，机制 [`eye_page`] 只消费不算）：开合% /
/// 水平扫视 / 垂直扫视 / 眼高% / 眉形 / 眼形 / 左右眼开合差（asym，左加右减，
/// 0 = 对称）。`height_pct = 100` 即 [`H_FULL`]。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct EyeStyle {
    pub(crate) open: u32,
    pub(crate) dx: i16,
    pub(crate) dy: i16,
    pub(crate) height_pct: u32,
    pub(crate) brow: Brow,
    pub(crate) shape: EyeShape,
    pub(crate) asym: i16,
}

impl EyeStyle {
    /// 胶囊对称眼（7 基态 + 4 情绪态口径）。
    const fn new(open: u32, dx: i16, dy: i16, height_pct: u32, brow: Brow) -> Self {
        Self {
            open,
            dx,
            dy,
            height_pct,
            brow,
            shape: EyeShape::Capsule,
            asym: 0,
        }
    }

    /// 换眼形（彩蛋态）。
    const fn art(mut self, shape: EyeShape) -> Self {
        self.shape = shape;
        self
    }

    /// 左右眼开合差（困惑态：左眯右睁）。
    const fn uneven(mut self, asym: i16) -> Self {
        self.asym = asym;
        self
    }
}

/// 正目全开基态（boot 初值与未知表情兜底）。
pub(crate) const EYE_IDLE: EyeStyle = EyeStyle::new(100, 0, 0, 100, Brow::None);

/// 眼形单页位图（128 列 × 8 行，页内 bit0=页顶）：按 [`EyeStyle`] 逐眼求形——
/// 开合%（左右可不等）、扫视偏移 (dx,dy)、眼高%、眉形、眼形（胶囊/心形/双环/
/// 空心环+瞳孔）。**纯函数**，不碰 I2C——调用方 [`crate::oled::oled_capsules`]
/// 逐页取图后只做页写。胶囊形状=圆角竖矩形（半径取 min(半宽, 半高)：满高 26px
/// 即纯胶囊，眯缝扁成横缝不退化成圆盘）；行内实心段解析求宽（直段区全宽、
/// 圆角区 sqrt 余量）。扫视出界自然裁剪（0..127 列钳制）不回卷。眉毛随 (dx,dy)
/// 平移，并按眉形一高一低/内低外低倾斜。
pub(crate) fn eye_page(page: u8, style: EyeStyle) -> [u8; 128] {
    let base_h = (H_FULL * style.height_pct.clamp(50, 150) as i32 / 100).min(H_MAX);
    let mut buf = [0u8; 128];
    let y0 = page as i32 * 8;
    let cy = CY + style.dy as i32;
    for y in y0..y0 + 8 {
        let bit = 1u8 << (y % 8);
        let py = (y - cy).abs();
        for (ix, &ecx) in EYES.iter().enumerate() {
            let cx = ecx + style.dx as i32;
            // 左右眼开合差：asym 加在左眼、减在右眼（困惑态左眯右睁）
            let asym = if ix == 0 {
                style.asym as i32
            } else {
                -(style.asym as i32)
            };
            let open = (style.open as i32 + asym).clamp(4, 100);
            let half_h = (base_h * open / 100).max(4) / 2;
            let rc = W2.min(half_h);
            let straight = half_h - rc;
            let mut put = |x: i32| {
                if (0..128).contains(&x) {
                    buf[x as usize] |= bit;
                }
            };
            match style.shape {
                EyeShape::Capsule => {
                    if py > half_h {
                        continue;
                    }
                    for x in (cx - hw_at(py, rc, straight))..=(cx + hw_at(py, rc, straight)) {
                        put(x);
                    }
                }
                EyeShape::Ring => {
                    if py > half_h {
                        continue;
                    }
                    let hw = hw_at(py, rc, straight);
                    let lid = py > half_h - 3;
                    for x in (cx - hw)..=(cx + hw) {
                        if lid || (x - cx).abs() > hw - 3 {
                            put(x);
                        }
                    }
                    // 中心瞳孔（5×5）：空心环里的小黑点
                    let pupil = if half_h >= 8 { 2 } else { 1 };
                    if py <= pupil {
                        for x in (cx - pupil)..=(cx + pupil) {
                            put(x);
                        }
                    }
                }
                EyeShape::Heart => {
                    // 心形隐函数 (x²+y²-1)³ - x²y³ ≤ 0（y 向上为正）
                    for x in (cx - W2)..=(cx + W2) {
                        let nx = (x - cx) as f32 / W2 as f32;
                        let ny = (cy - y) as f32 / half_h as f32;
                        let a = nx * nx + ny * ny - 1.0;
                        if a * a * a - nx * nx * ny * ny * ny <= 0.0 {
                            put(x);
                        }
                    }
                }
                EyeShape::Spiral => {
                    // 同心双环（眩晕纹）——椭圆化到眼框内
                    for x in (cx - W2)..=(cx + W2) {
                        let nx = (x - cx) as f32 / W2 as f32;
                        let ny = (cy - y) as f32 / half_h as f32;
                        let d = (nx * nx + ny * ny).sqrt();
                        if (d - 0.38).abs() < 0.16 || (d - 0.76).abs() < 0.16 {
                            put(x);
                        }
                    }
                }
            }
        }
        if style.brow != Brow::None {
            for (ix, &ecx) in EYES.iter().enumerate() {
                let cx = ecx + style.dx as i32;
                for x in (cx - W2).max(0)..=(cx + W2).min(127) {
                    // 斜率位：t=0 外端 → t=BROW_TILT 内端（左眼）／反之（右眼）
                    let t = ((x - (cx - W2)) * BROW_TILT) / (2 * W2);
                    let off = match (style.brow, ix) {
                        (Brow::Angry, 0) => t,              // 左眼内端低
                        (Brow::Angry, _) => BROW_TILT - t,  // 右眼内端低
                        (Brow::Sad, 0) => BROW_TILT - t,    // 左眼外端低
                        (Brow::Sad, _) => t,                // 右眼外端低
                        (Brow::Focus, _) => 1,              // 平直略压
                        (Brow::Confused, 0) => -2,          // 左眉抬高
                        (Brow::Confused, _) => 2,           // 右眉压低
                        (Brow::None, _) => 0,
                    };
                    let by = y - (BROW_Y + style.dy as i32 + off);
                    if (0..BROW_THICK).contains(&by) {
                        buf[x as usize] |= bit;
                    }
                }
            }
        }
    }
    buf
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

/// 表情→眼形基态预设：16 态白名单各有面目（7 基态 + 4 情绪态 + 5 情绪/彩蛋态），
/// 微动作动画在此基态上叠加（眨眼/扫视时间窗策略在 state.rs `PeriphState::anim_decide`，
/// 见模块文档的分离裁定）。情绪态语义：sleepy 半合下沉、surprised 眼高放大上提、
/// angry 窄眼压眉、sad 半垂垂眉；二批：confused 左眯右睁一高一低眉、focus 窄眼平眉、
/// love 心形眼、dizzy 同心环、scared 空心大眼小瞳孔。
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
        "confused" => EyeStyle::new(90, 0, 1, 100, Brow::Confused).uneven(-35), // 左眯右睁
        "focus" => EyeStyle::new(45, 0, 0, 100, Brow::Focus), // 窄眼平眉
        "love" => EyeStyle::new(100, 0, 0, 100, Brow::None).art(EyeShape::Heart), // 心形眼
        "dizzy" => EyeStyle::new(100, 0, 0, 100, Brow::None).art(EyeShape::Spiral), // 同心环
        "scared" => EyeStyle::new(100, 0, 0, 110, Brow::Sad).art(EyeShape::Ring), // 空心大眼
        _ => EYE_IDLE, // idle 与兜底：正目全开
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expr_eye_base_sixteen_states_and_fallback() {
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
        assert_eq!(
            s("confused"),
            EyeStyle::new(90, 0, 1, 100, Brow::Confused).uneven(-35)
        );
        assert_eq!(s("focus"), EyeStyle::new(45, 0, 0, 100, Brow::Focus));
        assert_eq!(
            s("love"),
            EyeStyle::new(100, 0, 0, 100, Brow::None).art(EyeShape::Heart)
        );
        assert_eq!(
            s("dizzy"),
            EyeStyle::new(100, 0, 0, 100, Brow::None).art(EyeShape::Spiral)
        );
        assert_eq!(
            s("scared"),
            EyeStyle::new(100, 0, 0, 110, Brow::Sad).art(EyeShape::Ring)
        );
        assert_eq!(s("idle"), EyeStyle::new(100, 0, 0, 100, Brow::None));
        // 未知表情兜底 fail-safe：正目全开（白名单守门在帧面/指令面，本件不拒）
        assert_eq!(s("no-such"), EyeStyle::new(100, 0, 0, 100, Brow::None));
    }

    /// 满开 100%：眼中心行（直段区）实心段 = 两眼各 19 列（cx±9 含端点），
    /// 段外与两眼间隙全零。
    #[test]
    fn capsule_full_open_center_row_geometry() {
        // y=32（页 4 bit0）= 满开眼中心行（CY=32）
        let buf = eye_page(4, EyeStyle::new(100, 0, 0, 100, Brow::None));
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
        let blink = EyeStyle::new(4, 0, 0, 100, Brow::None);
        let p3 = eye_page(3, blink);
        let p4 = eye_page(4, blink);
        // y=32（页4 bit0）：py=0 ≤ straight=0 → 全宽直段（列 33..51 含 42/33）
        // y=31/33（py=1）：圆角 rem=3 → hw=8 → 列 34..50（含 42 不含 33）
        assert_eq!(p3[42] & 0b1000_0000, 0b1000_0000, "y=31 圆角行（页3 bit7）");
        assert_eq!(p3[33] & 0b1000_0000, 0, "y=31 段宽收窄：列 33 落段外");
        assert_eq!(p4[42] & 0b11, 0b11, "y=32/33 两行实心");
        assert_eq!(p4[33] & 0b11, 0b01, "y=33 段自列 34 起（y=32 全宽）");
        assert_eq!(p4[42] & 0b100, 0, "y=34 无像素（圆角 rem=0 截断）");
        for page in [0u8, 1, 2, 5, 6, 7] {
            assert!(
                eye_page(page, blink).iter().all(|&b| b == 0),
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
                eye_page(page, EyeStyle::new(100, 120, 0, 100, Brow::None))
                    .iter()
                    .all(|&b| b == 0),
                "dx=+120 两眼出界，页 {page} 应全零（不回卷）"
            );
            assert!(
                eye_page(page, EyeStyle::new(100, 0, -60, 100, Brow::None))
                    .iter()
                    .all(|&b| b == 0),
                "dy=-60 眼形出画布上沿，页 {page} 应全零"
            );
        }
        // dy=+30：眼心行 62 → 实心行 y∈[49,63]（y=49 圆角 rem=0 截断）
        // 页 6（y 48..55）：y=50..55 置位 → bit2..bit7（y=48/49 空）
        // 页 7（y 56..63）：直段+圆角全实 → bit0..bit7
        let p6 = eye_page(6, EyeStyle::new(100, 0, 30, 100, Brow::None));
        assert_eq!(p6[42], 0b1111_1100, "页 6 置位 y=50..55（bit2..bit7）");
        let p7 = eye_page(7, EyeStyle::new(100, 0, 30, 100, Brow::None));
        assert_eq!(p7[42], 0xFF, "页 7 置位 y=56..63（bit0..bit7）");
    }

    /// 眼高缩放（surprised 125%）：满开眼高 32px → 眼心行上下各 16 行，
    /// 中心行仍全宽；页 2（y 16..23）首行即眼形顶（y=16 圆角截断，y=17 起）。
    #[test]
    fn capsule_tall_eye_extends_height() {
        let tall = EyeStyle::new(100, 0, 0, 125, Brow::None);
        let p2 = eye_page(2, tall);
        assert_eq!(p2[42] & 1, 0, "y=16 圆角 rem=0 截断（眼高 32 顶行）");
        assert_eq!(p2[42] & 0b10, 0b10, "y=17 圆角行起");
        // 页 4/5 中心区直段全宽（y 32..40 落在直段）
        assert_ne!(eye_page(4, tall)[42], 0, "页 4 中心行实心");
        assert_ne!(eye_page(5, tall)[42], 0, "页 5 仍属 32px 眼形（y 40..47）");
    }

    /// 眉形几何：怒眉内端低、丧眉外端低、专注眉平压、困惑眉左高一低；
    /// 无眉态页 1 全零。
    #[test]
    fn capsule_brow_slant_direction() {
        // 页 1 = y 8..15：眉基线 14（dy=0）→ 只有 y=14/15 两行；怒左眼内端
        // （x 靠 51）斜率下压，故内端像素落 y=16+，页 1 内端不亮、外端亮。
        let angry = eye_page(1, EyeStyle::new(45, 0, 0, 100, Brow::Angry));
        assert_ne!(angry[34], 0, "怒眉左眼外端（x=34）在页 1 亮");
        assert_eq!(angry[50], 0, "怒眉左眼内端（x=50）下压出页 1");
        let sad = eye_page(1, EyeStyle::new(65, 0, 0, 100, Brow::Sad));
        assert_eq!(sad[34], 0, "丧眉左眼外端下压出页 1");
        assert_ne!(sad[50], 0, "丧眉左眼内端（x=50）在页 1 亮");
        // 专注眉：整条平压一行（y=15）
        assert_ne!(
            eye_page(1, EyeStyle::new(45, 0, 0, 100, Brow::Focus))[42],
            0,
            "专注眉（y=15）"
        );
        // 困惑眉：左眉抬高（y=12/13 落页 1）、右眉压低（y=16/17 出页 1）
        let conf = EyeStyle::new(90, 0, 0, 100, Brow::Confused).uneven(-35);
        assert_ne!(eye_page(1, conf)[42], 0, "困惑左眉抬高在页 1");
        assert_eq!(eye_page(1, conf)[86], 0, "困惑右眉压低出页 1");
        assert_ne!(eye_page(2, conf)[86], 0, "困惑右眉落页 2");
        assert!(
            eye_page(1, EyeStyle::new(100, 0, 0, 100, Brow::None))
                .iter()
                .all(|&b| b == 0),
            "无眉态页 1 全零"
        );
    }

    /// 困惑态左右眼不等开合：右眼（open 100%）纵向比左眼（55%）高出一截。
    #[test]
    fn confused_eyes_are_asymmetric() {
        let conf = EyeStyle::new(90, 0, 0, 100, Brow::Confused).uneven(-35);
        // y=22（页 2 bit6）：右眼（半高 13）在形内、左眼（半高 7）已在形外
        let p2 = eye_page(2, conf);
        assert_ne!(p2[86] & 0b0100_0000, 0, "右眼 y=22 仍有像素");
        assert_eq!(p2[42] & 0b0100_0000, 0, "左眼 y=22 已收口");
    }

    /// 彩蛋眼形：love 心形（中心实、下尖收口）、dizzy 同心环（中心空、环上亮）、
    /// scared 空心环 + 瞳孔（边框亮、环内空、中心瞳孔亮）。
    #[test]
    fn easter_egg_shapes_geometry() {
        // love：页 4 中心行全宽（列 33..51），页 7（y56..63，ny<-1.8）无像素
        let love = EyeStyle::new(100, 0, 0, 100, Brow::None).art(EyeShape::Heart);
        let p4 = eye_page(4, love);
        assert_ne!(p4[33], 0, "心形中心行左端实心");
        assert_ne!(p4[42], 0, "心形中心实心");
        assert!(
            eye_page(7, love).iter().all(|&b| b == 0),
            "心形下尖在页 7 之上收口"
        );
        // dizzy：中心（nx=ny=0）不在环上 → 暗；离中心 0.38 半径处亮
        let dizzy = EyeStyle::new(100, 0, 0, 100, Brow::None).art(EyeShape::Spiral);
        let p4 = eye_page(4, dizzy);
        assert_eq!(p4[42] & 1, 0, "同心环中心暗");
        assert_ne!(p4[45], 0, "同心环第一圈（x=45）亮");
        // scared：空心环边框亮、环内空、中心瞳孔亮（5×5）
        let scared = EyeStyle::new(100, 0, 0, 110, Brow::Sad).art(EyeShape::Ring);
        let p4 = eye_page(4, scared);
        assert_ne!(p4[33], 0, "空心环左边框亮（x=33）");
        assert_eq!(p4[39], 0, "环内空心（x=39）");
        assert_ne!(p4[42], 0, "中心瞳孔亮（x=42）");
    }
}
