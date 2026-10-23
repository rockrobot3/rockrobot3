//! servo 原子件 · 机制核（批N23 件一，MC-7 缺② 收编原子模块形态）。
//!
//! **格位对齐申报**：本件服务 `act.servo` 格位（esp32-mini 装配单格主 = rr3-act
//! 宿主件，板侧经桥执行——宿主申报面在 modules/rr3-act/atoms/servo，板侧执行面
//! **不冒领格主**、不立 atom.toml：`provides` 恰一格与板无关断言是格主面法则，
//! 见批N23 计划册 §2「三不三立」）。
//!
//! **机制与策略分离**（文件级，收编落点）：本文件零策略字面量——行程/中位读
//! 生成表（真值点 hardware/esp32-mini/axis/*.toml，批N3 门 g），duty 锚点
//! （`DUTY_MIN`/`DUTY_MAX`，SG90 脉宽 × LEDC 档位换算）由调用方
//! （[`crate::board`] 策略面）**按参传入**；换板/换舵机只动 board.rs，本文件
//! 零板名。行程生成物自本件引入（include! 路径相对本文件）；策略面经
//! `use crate::servo::RR3_AXIS_*` 取中位锚——策略→机制单向依赖。
//!
//! 轴行程**点名拒收**（fail-closed：静默钳到边界会让调用方误以为按给值生效），
//! 与控制面/帧面/运行面三处调用方共用同一判据同一文案（"一处真值、多个检查点
//! 复用同一判定"，杜绝控制/运行口径漂移）。

// 板侧行程常量表生成物（RR3_AXIS_<INSTANCE> 三元组 + rr3_axis_travel 查表）——
// 真值点 hardware/esp32-mini/axis/*.toml、生成器 tools/rr3-codegen（批N3 件五
// 门 g 收敛：行程唯一真值点在档案，任何文件不得手写行程字面量；WANDER/SCAN/
// 越程拒收的判据全部读生成表）。include! 路径相对本文件（atoms/servo/）。
include!("../../src/axis_gen.rs");

/// 轴身份（行程/中位/点名都从生成表查得——机制不写字面量；查无此轴 panic =
/// 生成物与本枚举失配，fail-closed 不静默）。
#[derive(Clone, Copy)]
pub(crate) enum Axis {
    Pan,
    Tilt,
}

impl Axis {
    /// 轴名（点名文案用，与帧面 `axis` 键取值同串）。
    pub(crate) fn name(self) -> &'static str {
        match self {
            Axis::Pan => "pan",
            Axis::Tilt => "tilt",
        }
    }

    /// 本轴绝对行程区间 `[lo, hi]`（WANDER 端点与 SCAN 判据点都从这里取，
    /// 机制不另写字面量）——生成表 [`rr3_axis_travel`] 查得；查无此轴 panic
    /// （Axis 枚举与生成表同源，此处失配 = 生成物缺失，fail-closed 不静默）。
    pub(crate) fn range(self) -> (u32, u32) {
        match rr3_axis_travel(self.name()) {
            Some((lo, hi, _)) => (lo, hi),
            None => panic!(
                "轴 {} 无行程表（axis_gen.rs 生成物与 Axis 枚举失配）",
                self.name()
            ),
        }
    }

    /// 本轴中位锚（boot 置位与角度行基准）——生成表查得，同上 fail-closed。
    pub(crate) fn center_deg(self) -> u32 {
        match rr3_axis_travel(self.name()) {
            Some((_, _, center)) => center,
            None => panic!(
                "轴 {} 无行程表（axis_gen.rs 生成物与 Axis 枚举失配）",
                self.name()
            ),
        }
    }
}

/// 角度 → duty ticks（线性 0–180 ↦ duty_min–duty_max；入参已由调用方 clamp
/// 0–180）。duty 锚点由调用方按参传入（策略量，见模块文档）。
pub(crate) fn deg_to_duty(deg: u32, duty_min: u32, duty_max: u32) -> u32 {
    duty_min + (deg * (duty_max - duty_min)) / 180
}

/// duty ticks → 角度（回读换算，与 [`deg_to_duty`] 互逆的整数近似，±1° 内）。
pub(crate) fn duty_to_deg(duty: u32, duty_min: u32, duty_max: u32) -> u32 {
    ((duty.saturating_sub(duty_min)) * 180 + (duty_max - duty_min) / 2) / (duty_max - duty_min)
}

/// 轴行程校验（**纯函数**：只算不碰硬件，控制面能在下发作业前即时拒）。
/// 与运行面 [`crate::periph::RtPeriph::servo_set`] 共用同一判据、同一文案。
pub(crate) fn axis_check(axis: Axis, deg: u32) -> Result<u32, String> {
    if deg > 180 {
        return Err(format!("角度超范围 0-180: {deg}"));
    }
    let Some((lo, hi, center)) = rr3_axis_travel(axis.name()) else {
        return Err(format!(
            "{} 轴无行程表（axis_gen.rs 生成物缺轴）",
            axis.name()
        ));
    };
    if !(lo..=hi).contains(&deg) {
        return Err(format!(
            "{} 轴超出行程 [{lo}, {hi}]（中位 {center}°）: {deg}",
            axis.name()
        ));
    }
    Ok(deg)
}

/// 轴行程五等分判据点（SCAN 判据：端点 + 三分点，全部落在本轴行程内——
/// 旧版写死的 0/45/135/180 越出机械行程，正是堵转来源）。
pub(crate) fn axis_scan_points(axis: Axis) -> [u32; 5] {
    let (lo, hi) = axis.range();
    let span = hi - lo;
    [lo, lo + span / 4, lo + span / 2, lo + span * 3 / 4, hi]
}

/// 扫摆步进：deg ± step，轴行程端点折返后返回新角度。dir 应为 ±1
/// （WANDER 开关随机起步；0 防御上不会出现——0 时原地不动）。
/// `bounds` 由 [`Axis::range`] 给出，端点即机械安全边界。
pub(crate) fn wander_step(deg: u32, dir: &mut i8, step: i32, bounds: (u32, u32)) -> u32 {
    let (lo, hi) = bounds;
    let mut next = deg as i32 + *dir as i32 * step;
    if next >= hi as i32 {
        next = hi as i32;
        *dir = -1;
    } else if next <= lo as i32 {
        next = lo as i32;
        *dir = 1;
    }
    next as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    // duty 锚点测试值（SG90 @ 50Hz/14bit 的 board.rs 策略值同款——机制测试
    // 自带参数，不引策略常量）。
    const DUTY_MIN: u32 = 410;
    const DUTY_MAX: u32 = 2048;

    #[test]
    fn duty_conversion_endpoints_and_roundtrip() {
        assert_eq!(deg_to_duty(0, DUTY_MIN, DUTY_MAX), DUTY_MIN);
        assert_eq!(deg_to_duty(180, DUTY_MIN, DUTY_MAX), DUTY_MAX);
        assert_eq!(deg_to_duty(90, DUTY_MIN, DUTY_MAX), 1229);
        for deg in [0u32, 60, 90, 120, 180] {
            let back = duty_to_deg(deg_to_duty(deg, DUTY_MIN, DUTY_MAX), DUTY_MIN, DUTY_MAX);
            assert!(
                (back as i32 - deg as i32).abs() <= 1,
                "{deg}° 往返漂移超 ±1°: {back}°"
            );
        }
    }

    #[test]
    fn axis_check_rejects_out_of_travel_named() {
        // 协议级越界（>180）先行拒
        assert_eq!(
            axis_check(Axis::Pan, 200).unwrap_err(),
            "角度超范围 0-180: 200"
        );
        // 行程越界点名拒收（fail-closed，不静默钳位）——文案与调用方断言同源
        assert_eq!(
            axis_check(Axis::Pan, 59).unwrap_err(),
            "pan 轴超出行程 [60, 120]（中位 90°）: 59"
        );
        assert_eq!(
            axis_check(Axis::Tilt, 101).unwrap_err(),
            "tilt 轴超出行程 [80, 100]（中位 90°）: 101"
        );
        assert_eq!(axis_check(Axis::Pan, 60).unwrap(), 60);
        assert_eq!(axis_check(Axis::Tilt, 90).unwrap(), 90);
    }

    #[test]
    fn scan_points_stay_in_travel() {
        for axis in [Axis::Pan, Axis::Tilt] {
            let (lo, hi) = axis.range();
            let pts = axis_scan_points(axis);
            assert_eq!(pts[0], lo);
            assert_eq!(pts[4], hi);
            for w in pts.windows(2) {
                assert!(w[0] < w[1], "{:?} 判据点非严格递增: {pts:?}", axis.name());
            }
        }
        assert_eq!(axis_scan_points(Axis::Tilt), [80, 85, 90, 95, 100]);
    }

    #[test]
    fn wander_step_folds_back_at_bounds() {
        let mut dir: i8 = 1;
        assert_eq!(wander_step(115, &mut dir, 10, (60, 120)), 120);
        assert_eq!(dir, -1, "上端折返后方向须翻转");
        assert_eq!(wander_step(65, &mut dir, 10, (60, 120)), 60);
        assert_eq!(dir, 1, "下端折返后方向须翻转");
        assert_eq!(wander_step(90, &mut dir, 10, (60, 120)), 100);
    }
}
