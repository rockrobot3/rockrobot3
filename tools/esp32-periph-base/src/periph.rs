use std::thread;
use std::time::Duration;

use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::ledc::LedcDriver;
use esp_idf_hal::uart::UartDriver;

use crate::board::{
    DUTY_MAX, DUTY_MIN, OLED_COLS, OLED_CONTRAST_LEVELS, OLED_RENDER_ATTEMPTS, OLED_ROWS,
    RAIL_GATE_ARMED, RAIL_MILD_MV, RAIL_SEVERE_MV, RR3_BIND_ADC_RAIL, SERVO_SLEW_STEP_DEG,
    SERVO_SLEW_STEP_MS,
};
use crate::face::EyeStyle;
use crate::oled::{oled_capsules, oled_cmd, oled_frame, oled_init, oled_text};
use periph_mech::oled_codec::{rr3_contrast_command, rr3_oled_lines};
use periph_mech::periph_core::{
    rr3_servo_gate, rr3_servo_release, rr3_slew_steps, Rr3OledLadder, Rr3ServoCore,
};
use rr_periph_proto::ports::{Rr3AdcPort, Rr3PwmChannel, Rr3PwmPort};
use crate::power::{RailJudge, RailMonitor, RailTier};
use crate::servo::{axis_check, deg_to_duty, Axis};

// ---------------------------------------------------------------------------
// 外设面（控制面/运行面分离）：控制面独占 UART2，运行面独占 I2C0 + LEDC
// ---------------------------------------------------------------------------

/// 控制面独占的 UART2（指令面 io：行面直用固有方法，帧面经 [`UartIo`] 窄缝）。
/// 驱动 'static（外设 token 即 'static 无借用绑定；move 进线程后 esp_restart
/// 前永不 drop）。UartDriver 上游已标 `Send`，本结构自动 Send。
pub(crate) struct CtlUart {
    pub(crate) uart: UartDriver<'static>,
}

/// 运行面独占的外设面：I2C（OLED）+ 两轴 LEDC 舵机 + 5V 轨 ADC。**全部硬件慢动作在
/// 此面执行**——慢 I2C 与舵机整定都不阻塞控制面的命令吞吐。驱动 'static 同上；
/// I2cDriver/LedcDriver 上游已标 `Send`，本结构自动 Send（无需 unsafe impl）。
pub(crate) struct RtPeriph {
    pub(crate) i2c: I2cDriver<'static>,
    pub(crate) pan: LedcDriver<'static>,
    pub(crate) tilt: LedcDriver<'static>,
    /// 两轴**记账角**（缓速步进的起点：只有本面驱动舵机，故本面记账即真值；duty=0
    /// 未激磁期以中位起算，避免首帧从 0° 起跳的假行程）——机制核 [`Rr3ServoCore`]。
    pub(crate) servo: Rr3ServoCore,
    /// 当前 OLED 亮度档（整板电压降级用；见 [`OLED_CONTRAST_LEVELS`]）——机制核
    /// [`Rr3OledLadder`] 只管阶梯，档值真值仍在策略面。
    pub(crate) oled: Rr3OledLadder,
    /// 5V 轨采样器（方案A Q1）：装配失败为 `None`——按"打印不 panic"纪律不拖垮其余
    /// 外设面；采样路径据此回 Err（不冒领读数）。
    pub(crate) rail: Option<RailMonitor>,
    /// 下次 5V 轨周期采样时刻（运行面自主节拍；esp_timer ms，初值 0 = 首拍即采）。
    pub(crate) next_rail_ms: u64,
    /// 5V 轨欠压判定核（方案A Q2）：每次采样喂一拍，产出档位 + 越阈事件。纯机制件，
    /// 随采样在运行面推进（不读墙上时钟）。
    pub(crate) rail_judge: RailJudge,
    /// 5V 轨当前档位（快门值；`servo_set` 拒动判据，由运行面采样刷新）。boot 初值 `Ok`。
    pub(crate) rail_tier: RailTier,
}

impl RtPeriph {
    /// 装配套（驱动由 `main` 装好后移交；本构造只初始化机制核状态与判定核）。
    /// 记账角初值 = 各轴中位锚（策略面生成表），避免首帧从 0° 起跳的假行程。
    pub(crate) fn new(
        i2c: I2cDriver<'static>,
        pan: LedcDriver<'static>,
        tilt: LedcDriver<'static>,
        rail: Option<RailMonitor>,
    ) -> Self {
        Self {
            i2c,
            pan,
            tilt,
            servo: Rr3ServoCore::new(Axis::Pan.center_deg(), Axis::Tilt.center_deg()),
            oled: Rr3OledLadder::new(),
            rail,
            next_rail_ms: 0,
            rail_judge: RailJudge::new(RAIL_MILD_MV, RAIL_SEVERE_MV),
            rail_tier: RailTier::Ok,
        }
    }

    /// 本面记账角（运行面回写共享状态时用：只有成功移动才推进，读角永不复述
    /// 没到达的角）。
    pub(crate) fn pan_deg(&self) -> u32 {
        self.servo.deg(Rr3PwmChannel::Pan)
    }

    /// 见 [`Self::pan_deg`]。
    pub(crate) fn tilt_deg(&self) -> u32 {
        self.servo.deg(Rr3PwmChannel::Tilt)
    }

    /// 舵机转角（先过本轴行程裁决 → 轨压拒动闸 → **缓速步进**逼近 → 返回实际
    /// 设定角）。协议级 0–180 之外的角先被拒；轴行程（中位 ± 摆幅）之外**点名
    /// 拒收**，不静默钳位。缓速步进见 [`SERVO_SLEW_STEP_DEG`]（极限电源下的软启动：
    /// 把整段跳变的电流尖峰摊成涓流）；步进序列由机制核产出，本面只管写 duty 与
    /// 步间等待。
    pub(crate) fn servo_set(&mut self, axis: Axis, deg: u32) -> Result<u32, String> {
        let deg = axis_check(axis, deg)?;
        // 5V 轨**重度欠压 → 执行面拒动**（方案A Q2 / MC-5 语义）：已接闸（真板标定后）
        // 且档位为 `severe` 时拒收本次动作——fail-closed，绝不带着欠压去驱舵机、不去撞
        // BOD 复位。**妆面（OLED）不受影响**；**回阈后不自动重放**（调用方须重发）。
        // `RAIL_GATE_ARMED=false`（占位未标定）时不接闸，仅观测。
        rr3_servo_gate(RAIL_GATE_ARMED, self.rail_tier, axis.name(), deg)?;
        let ch = match axis {
            Axis::Pan => Rr3PwmChannel::Pan,
            Axis::Tilt => Rr3PwmChannel::Tilt,
        };
        for step in rr3_slew_steps(self.servo.deg(ch), deg, SERVO_SLEW_STEP_DEG) {
            self.rr3_pwm_duty(ch, deg_to_duty(step, DUTY_MIN, DUTY_MAX))?;
            if step != deg {
                thread::sleep(Duration::from_millis(SERVO_SLEW_STEP_MS));
            }
        }
        self.servo.commit(ch, deg);
        Ok(deg)
    }

    /// 下调一档 OLED 亮度（**整板电压降级**：见 [`OLED_CONTRAST_LEVELS`]）。已到地板
    /// 返回 false（调用方据此收手）；未到则写新档并返回 true。阶梯推进在机制核。
    pub(crate) fn degrade_contrast(&mut self) -> bool {
        let Some(idx) = self.oled.degrade(OLED_CONTRAST_LEVELS.len()) else {
            return false;
        };
        let lv = OLED_CONTRAST_LEVELS[idx];
        match oled_cmd(&mut self.i2c, &rr3_contrast_command(lv)) {
            Ok(()) => println!("[periph] OLED 降亮 → 0x{lv:02X}（轨压降级第 {idx} 档）"),
            Err(e) => eprintln!("[periph] 降亮写失败: {e}"),
        }
        true
    }

    /// OLED 渲染通用恢复环（**降亮阶梯 + 模块 latch 重初始化**）：闭包 `f` 执行一次
    /// 完整渲染，失败即降一档亮度重试；到亮度地板仍败则重走 [`oled_init`] 复位 SSD1306
    /// 状态机**一次**、再试一次即收手（真板实锤 2026-10-13：大面亮点页丢拍会把模块
    /// latch 成"此后连 2 字节命令都恒 NACK"，降亮与总线自愈都救不回，重初始化只作最后
    /// 手段、并非稳过）。**本环只治"拍丢/latch"子症状，不治根因**——根因是大面亮点帧拉
    /// 塌轨压触发 ESP32 BOD 复位，归"电源/负载均衡"机制。`OLED_RENDER_ATTEMPTS` 为轮数
    /// 上限（防死循环）。
    fn render_with_recovery<F>(&mut self, mut f: F) -> Result<(), String>
    where
        F: FnMut(&mut I2cDriver<'static>) -> Result<(), String>,
    {
        // **机制与策略分离**：本函数只是"失败 → 按策略降亮/重初始化 → 再试"的骨架，
        // 轮数与档数等策略量全在 [`crate::board`]。**每轮至多一次模块重初始化**：落到
        // 亮度地板就 `oled_init` 一次 + 再试一次即收手（返回其结果，不再回降亮循环）。
        // 真板实锤（2026-10-13）：旧版隐式计数会在一拍内触发两次 `oled_init`、30s 内
        // 56 次重初始化风暴——既救不回 latch 又白烧轨压（见 `OLED_RENDER_ATTEMPTS`）。
        let mut last = String::new();
        for _ in 0..OLED_RENDER_ATTEMPTS {
            match f(&mut self.i2c) {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
            if self.degrade_contrast() {
                continue; // 还有更暗的档：下一轮直接用新档重试
            }
            // 已到亮度地板：重初始化复位模块一次，再试一次即收手
            eprintln!("[periph] OLED 渲染到亮度地板仍败 → 重走 oled_init 复位模块（每轮至多一次）");
            let _ = oled_init(&mut self.i2c);
            self.oled.reset();
            return f(&mut self.i2c);
        }
        Err(last)
    }

    /// 渲染胶囊眼（**带恢复环**：降亮阶梯 + 模块 latch 重初始化）。
    pub(crate) fn capsules_degrading(&mut self, style: EyeStyle) -> Result<(), String> {
        self.render_with_recovery(|i2c| oled_capsules(i2c, style))
    }

    /// 渲染 8x8 点阵全屏放大（**带恢复环**）。
    pub(crate) fn render_frame(&mut self, frame: &[u8; 8]) -> Result<(), String> {
        let frame = *frame;
        self.render_with_recovery(|i2c| oled_frame(i2c, &frame))
    }

    /// 舵机让位（**供电分时仲裁**）：板载 5V 轨上 SSD1306 电荷泵与舵机保持电流
    /// **不能共存**——真板实锤（2026-10-12）：两轴激磁时 OLED 页写/命令笔恒
    /// `ESP_FAIL`（地址单笔因弱上拉假 ACK 仍"通过"，具欺骗性），而**掉 duty 后同一笔
    /// 立刻 Ok**；且激磁越久越顽固（一度到掉 duty 也不复、需重走 init）。
    /// 故除"正在移动"外一律释放两轴：把 5V 轨整段让给 OLED。运行面单线程、渲染与
    /// 移动在时间上本就串行，故本函数在**每次动作收尾**调用即可保证两者零重叠。
    pub(crate) fn servo_release(&mut self) {
        rr3_servo_release(self);
    }

    /// 采样一次 5V 轨（**机制面**，方案A Q1）：返回 `(轨压 mV, 引脚 raw)`；未装配
    /// （ADC init 失败）返回 Err 文案（**不冒领读数**）。纯机制——不判阈值、不决策。
    pub(crate) fn rail_sample(&mut self) -> Result<(u16, u16), String> {
        match self.rail.as_mut() {
            Some(r) => r.rr3_adc_read_mv(RR3_BIND_ADC_RAIL),
            None => Err("5V 轨监测未装配（查 ADC 组件/GPIO34 接线）".into()),
        }
    }

    /// OLED 文本 3 行（21 列硬切）+ 角度行（row 3）刷新（**带恢复环**）。
    pub(crate) fn oled_show(
        &mut self,
        text: &str,
        pan_deg: u32,
        tilt_deg: u32,
    ) -> Result<(), String> {
        // 排版机制在 periph-mech（P0-A A3：可见 ASCII 过滤 + 21 列硬切 + 角度行），
        // 列数/行数策略量按参喂入；本面只做逐行传输。
        let lines = rr3_oled_lines(text, pan_deg, tilt_deg, OLED_ROWS, OLED_COLS);
        self.render_with_recovery(|i2c| {
            for (row, line) in lines.iter().enumerate() {
                oled_text(i2c, row, line)?;
            }
            Ok(())
        })
    }
}

impl Rr3PwmPort for RtPeriph {
    /// LEDC 两通道即 PWM 端口：按通道选驱动设占空。错误文案保留原样
    /// （`servo_set` 的上级文案依赖 `LEDC set_duty 失败` 前缀）。
    fn rr3_pwm_duty(&mut self, channel: Rr3PwmChannel, duty: u32) -> Result<(), String> {
        let driver = match channel {
            Rr3PwmChannel::Pan => &mut self.pan,
            Rr3PwmChannel::Tilt => &mut self.tilt,
        };
        driver
            .set_duty(duty)
            .map_err(|e| format!("LEDC set_duty 失败: {e:?}"))
    }
}
