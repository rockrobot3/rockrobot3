//! 舵机与降档核心机制（P0-A A2 自板档适配层迁入）——纯状态/纯计算，不碰驱动、
//! 不睡眠、不读时钟。
//!
//! 适配层把策略量按参喂进来（行程端点/中位、duty 锚点、缓速步进、亮度阶梯长度），
//! 时间等待与端口写由适配层在拿到本模块产出的步进后执行。

use rr_periph_proto::ports::{Rr3PwmChannel, Rr3PwmPort};

use crate::rail_judge::RailTier;

/// 轴行程校验（**纯函数**：只算不碰硬件，控制面能在下发作业前即时拒）。
/// 协议级 0–180 之外的角先拒；轴行程 `[lo, hi]` 之外**点名拒收**，不静默钳位
/// （静默钳到边界会让调用方误以为按给值生效）。`center` 只进文案。
pub fn rr3_axis_check(name: &str, lo: u32, hi: u32, center: u32, deg: u32) -> Result<u32, String> {
    if deg > 180 {
        return Err(format!("角度超范围 0-180: {deg}"));
    }
    rr3_axis_limit(name, lo, hi, center, deg)?;
    Ok(deg)
}

/// 轴行程门：落在 `[lo, hi]` 之外点名拒收（`center` 只进文案）。
fn rr3_axis_limit(name: &str, lo: u32, hi: u32, center: u32, deg: u32) -> Result<(), String> {
    if !(lo..=hi).contains(&deg) {
        return Err(format!(
            "{name} 轴超出行程 [{lo}, {hi}]（中位 {center}°）: {deg}"
        ));
    }
    Ok(())
}

/// 角度 → duty ticks（线性 0–180 ↦ duty_min–duty_max；入参须已过行程校验）。
pub fn rr3_deg_to_duty(deg: u32, duty_min: u32, duty_max: u32) -> u32 {
    duty_min + (deg * (duty_max - duty_min)) / 180
}

/// duty ticks → 角度（回读换算，与 [`rr3_deg_to_duty`] 互逆的整数近似，±1° 内）。
pub fn rr3_duty_to_deg(duty: u32, duty_min: u32, duty_max: u32) -> u32 {
    ((duty.saturating_sub(duty_min)) * 180 + (duty_max - duty_min) / 2) / (duty_max - duty_min)
}

/// 缓速步进序列（**不含起点、含终点**）：从 `from` 每步挪 `step` 度逼近 `to`，
/// 越界时贴到端点。`from == to` 产出空序列（调用方零写零等）。步进 0 按 1 兜底
/// （否则永不抵达目标 = 调用方死循环，机制不给策略留这个坑）。
pub struct Rr3SlewSteps {
    cur: u32,
    target: u32,
    step: u32,
}

impl Iterator for Rr3SlewSteps {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        if self.cur == self.target {
            return None;
        }
        self.cur = rr3_slew_step(self.cur, self.target, self.step);
        Some(self.cur)
    }
}

/// 单步逼近（越界贴端点；步进 0 已在构造面兜底为 1）。
fn rr3_slew_step(cur: u32, target: u32, step: u32) -> u32 {
    if target > cur {
        (cur + step).min(target)
    } else {
        cur.saturating_sub(step).max(target)
    }
}

/// 建缓速步进序列（起点 `from`、目标 `to`、每步 `step` 度）。
pub fn rr3_slew_steps(from: u32, to: u32, step: u32) -> Rr3SlewSteps {
    Rr3SlewSteps {
        cur: from,
        target: to,
        step: step.max(1),
    }
}

/// 轨压拒动闸（**安全核语义**：接闸且重度欠压 → 执行面拒动；回阈不自动重放）。
/// 未接闸（阈值/分压比仍是占位未标定）只观测不拒——浮空测点的伪欠压不能把
/// 一台能跑的板子打死。文案与调用方口径同源（含轴名与请求角，便于现场归因）。
pub fn rr3_servo_gate(
    armed: bool,
    tier: RailTier,
    axis_name: &str,
    deg: u32,
) -> Result<(), String> {
    if armed && tier == RailTier::Severe {
        return Err(format!(
            "5V 轨重度欠压（tier=severe）执行面拒动：{axis_name} 轴 {deg}° 拒收（回阈后请重发）"
        ));
    }
    Ok(())
}

/// 两轴记账角（缓速步进的起点：只有驱动面推角度，故记账即真值；duty=0 未激磁
/// 期以中位起算，避免首帧从 0° 起跳的假行程）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ServoCore {
    pan_deg: u32,
    tilt_deg: u32,
}

impl Rr3ServoCore {
    pub const fn new(pan_deg: u32, tilt_deg: u32) -> Self {
        Self { pan_deg, tilt_deg }
    }

    /// 本通道的记账角。
    pub fn deg(&self, channel: Rr3PwmChannel) -> u32 {
        match channel {
            Rr3PwmChannel::Pan => self.pan_deg,
            Rr3PwmChannel::Tilt => self.tilt_deg,
        }
    }

    /// 一次移动成功后提交记账角（失败路径不得调用——读角永不复述没到达的角）。
    pub fn commit(&mut self, channel: Rr3PwmChannel, deg: u32) {
        match channel {
            Rr3PwmChannel::Pan => self.pan_deg = deg,
            Rr3PwmChannel::Tilt => self.tilt_deg = deg,
        }
    }
}

/// 两轴让位（**供电分时仲裁**）：把 5V 轨整段让给妆面（OLED），空闲期不激磁。
/// 返回值丢弃——释放路径是收尾动作，单通道失败不阻断另一通道。
pub fn rr3_servo_release(pwm: &mut impl Rr3PwmPort) {
    let _ = pwm.rr3_pwm_duty(Rr3PwmChannel::Pan, 0);
    let _ = pwm.rr3_pwm_duty(Rr3PwmChannel::Tilt, 0);
}

/// OLED 亮度降档阶梯（**整板电压降级**）：从当前档逐级下调，到地板返回 `None`
/// （调用方据此收手，不再写亮度命令）；重初始化后由调用方 [`Self::reset`] 回顶档。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rr3OledLadder {
    idx: usize,
}

impl Rr3OledLadder {
    pub const fn new() -> Self {
        Self { idx: 0 }
    }

    /// 当前档下标。
    pub const fn idx(self) -> usize {
        self.idx
    }

    /// 下调一档：还有更暗的档 → 前进并回新下标；已到地板 → `None`（下标不动）。
    pub fn degrade(&mut self, levels: usize) -> Option<usize> {
        if self.idx + 1 >= levels {
            return None;
        }
        self.idx += 1;
        Some(self.idx)
    }

    /// 回顶档（模块重初始化后调用）。
    pub fn reset(&mut self) {
        self.idx = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试自带策略值（不引板档常量，与 atoms 机制核同口径）。
    const DUTY_MIN: u32 = 410;
    const DUTY_MAX: u32 = 2048;

    #[test]
    fn axis_check_rejects_out_of_travel_named() {
        assert_eq!(
            rr3_axis_check("pan", 60, 120, 90, 200).unwrap_err(),
            "角度超范围 0-180: 200"
        );
        assert_eq!(
            rr3_axis_check("pan", 60, 120, 90, 59).unwrap_err(),
            "pan 轴超出行程 [60, 120]（中位 90°）: 59"
        );
        assert_eq!(
            rr3_axis_check("tilt", 80, 100, 90, 101).unwrap_err(),
            "tilt 轴超出行程 [80, 100]（中位 90°）: 101"
        );
        assert_eq!(rr3_axis_check("pan", 60, 120, 90, 60), Ok(60));
        assert_eq!(rr3_axis_check("tilt", 80, 100, 90, 90), Ok(90));
    }

    #[test]
    fn duty_conversion_endpoints_and_roundtrip() {
        assert_eq!(rr3_deg_to_duty(0, DUTY_MIN, DUTY_MAX), DUTY_MIN);
        assert_eq!(rr3_deg_to_duty(180, DUTY_MIN, DUTY_MAX), DUTY_MAX);
        assert_eq!(rr3_deg_to_duty(90, DUTY_MIN, DUTY_MAX), 1229);
        for deg in [0u32, 60, 90, 120, 180] {
            let back =
                rr3_duty_to_deg(rr3_deg_to_duty(deg, DUTY_MIN, DUTY_MAX), DUTY_MIN, DUTY_MAX);
            assert!(
                (back as i32 - deg as i32).abs() <= 1,
                "{deg}° 往返漂移超 ±1°: {back}°"
            );
        }
    }

    #[test]
    fn slew_steps_approach_target_in_both_directions() {
        // 上行 60 → 65，步进 10：一步贴端点即止
        assert_eq!(rr3_slew_steps(60, 65, 10).collect::<Vec<_>>(), vec![65]);
        // 下行 90 → 84，步进 2：每步 2°，末步贴端点
        assert_eq!(
            rr3_slew_steps(90, 84, 2).collect::<Vec<_>>(),
            vec![88, 86, 84]
        );
        // 同角：空序列（零写零等）
        assert_eq!(rr3_slew_steps(90, 90, 2).count(), 0);
        // 步进 0 兜底按 1：不产生死循环（1 → 2 仍在一拍内抵达）
        assert_eq!(rr3_slew_steps(1, 2, 0).collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn servo_gate_only_trips_when_armed_and_severe() {
        assert_eq!(
            rr3_servo_gate(true, RailTier::Severe, "pan", 95).unwrap_err(),
            "5V 轨重度欠压（tier=severe）执行面拒动：pan 轴 95° 拒收（回阈后请重发）"
        );
        assert!(rr3_servo_gate(false, RailTier::Severe, "pan", 95).is_ok());
        assert!(rr3_servo_gate(true, RailTier::Mild, "pan", 95).is_ok());
        assert!(rr3_servo_gate(true, RailTier::Ok, "tilt", 90).is_ok());
    }

    #[test]
    fn servo_core_bookkeeping_is_per_channel() {
        let mut core = Rr3ServoCore::new(90, 90);
        core.commit(Rr3PwmChannel::Pan, 120);
        assert_eq!(core.deg(Rr3PwmChannel::Pan), 120);
        assert_eq!(core.deg(Rr3PwmChannel::Tilt), 90, "另一通道不被带着动");
    }

    #[test]
    fn oled_ladder_walks_to_floor_and_resets() {
        let mut ladder = Rr3OledLadder::new();
        assert_eq!(ladder.idx(), 0);
        assert_eq!(ladder.degrade(4), Some(1));
        assert_eq!(ladder.degrade(4), Some(2));
        assert_eq!(ladder.degrade(4), Some(3));
        assert_eq!(ladder.degrade(4), None, "地板不再前进");
        assert_eq!(ladder.idx(), 3, "地板下标不动");
        ladder.reset();
        assert_eq!(ladder.idx(), 0);
        // 单档阶梯（策略面只配一档）：构造即地板
        assert_eq!(Rr3OledLadder::new().degrade(1), None);
    }
}
