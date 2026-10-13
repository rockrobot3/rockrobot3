use std::thread;
use std::time::Duration;

use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::ledc::LedcDriver;
use esp_idf_hal::uart::UartDriver;

use crate::board::{
    axis_check, deg_to_duty, Axis, OLED_COLS, OLED_CONTRAST_LEVELS, OLED_RENDER_ATTEMPTS,
    OLED_ROWS, RAIL_GATE_ARMED, SERVO_SLEW_STEP_DEG, SERVO_SLEW_STEP_MS,
};
use crate::oled::{oled_capsules, oled_cmd, oled_frame, oled_init, oled_text};
use crate::power::{RailJudge, RailMonitor, RailTier};

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
    /// 各轴**记账角**（缓速步进的起点：只有本面驱动舵机，故本面记账即真值；duty=0
    /// 未激磁期以中位起算，避免首帧从 0° 起跳的假行程）。
    pub(crate) pan_deg: u32,
    pub(crate) tilt_deg: u32,
    /// 当前 OLED 亮度档下标（整板电压降级用；见 [`OLED_CONTRAST_LEVELS`]）。
    pub(crate) contrast_idx: usize,
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
    /// 舵机转角（先过本轴行程裁决 → **缓速步进**逼近 → 返回实际设定角）。
    /// 协议级 0–180 之外的角先被拒；轴行程（中位 ± 摆幅）之外**点名拒收**，
    /// 不静默钳位——见文件头"分轴行程"段。缓速步进见 [`SERVO_SLEW_STEP_DEG`]
    /// （极限电源下的软启动：把整段跳变的电流尖峰摊成涓流）。
    pub(crate) fn servo_set(&mut self, axis: Axis, deg: u32) -> Result<u32, String> {
        let deg = axis_check(axis, deg)?;
        // 5V 轨**重度欠压 → 执行面拒动**（方案A Q2 / MC-5 语义）：已接闸（真板标定后）
        // 且档位为 `severe` 时拒收本次动作——fail-closed，绝不带着欠压去驱舵机、不去撞
        // BOD 复位。**妆面（OLED）不受影响**；**回阈后不自动重放**（调用方须重发）。
        // `RAIL_GATE_ARMED=false`（占位未标定）时不接闸，仅观测。
        if RAIL_GATE_ARMED && self.rail_tier == RailTier::Severe {
            return Err(format!(
                "5V 轨重度欠压（tier=severe）执行面拒动：{} 轴 {deg}° 拒收（回阈后请重发）",
                axis.name()
            ));
        }
        let cur = match axis {
            Axis::Pan => self.pan_deg,
            Axis::Tilt => self.tilt_deg,
        };
        let mut step = cur;
        while step != deg {
            step = if deg > step {
                (step + SERVO_SLEW_STEP_DEG).min(deg)
            } else {
                step.saturating_sub(SERVO_SLEW_STEP_DEG).max(deg)
            };
            let driver = match axis {
                Axis::Pan => &mut self.pan,
                Axis::Tilt => &mut self.tilt,
            };
            driver
                .set_duty(deg_to_duty(step))
                .map_err(|e| format!("LEDC set_duty 失败: {e:?}"))?;
            if step != deg {
                thread::sleep(Duration::from_millis(SERVO_SLEW_STEP_MS));
            }
        }
        match axis {
            Axis::Pan => self.pan_deg = deg,
            Axis::Tilt => self.tilt_deg = deg,
        }
        Ok(deg)
    }

    /// 下调一档 OLED 亮度（**整板电压降级**：见 [`OLED_CONTRAST_LEVELS`]）。已到地板
    /// 返回 false（调用方据此收手）；未到则写新档并返回 true。
    pub(crate) fn degrade_contrast(&mut self) -> bool {
        if self.contrast_idx + 1 >= OLED_CONTRAST_LEVELS.len() {
            return false;
        }
        self.contrast_idx += 1;
        let lv = OLED_CONTRAST_LEVELS[self.contrast_idx];
        match oled_cmd(&mut self.i2c, &[0x81, lv]) {
            Ok(()) => println!("[periph] OLED 降亮 → 0x{lv:02X}（轨压降级第 {} 档）", self.contrast_idx),
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
            self.contrast_idx = 0;
            return f(&mut self.i2c);
        }
        Err(last)
    }

    /// 渲染胶囊眼（**带恢复环**：降亮阶梯 + 模块 latch 重初始化）。
    pub(crate) fn capsules_degrading(&mut self, open: u32, dx: i16, dy: i16) -> Result<(), String> {
        self.render_with_recovery(|i2c| oled_capsules(i2c, open, dx, dy))
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
        let _ = self.pan.set_duty(0);
        let _ = self.tilt.set_duty(0);
    }

    /// 采样一次 5V 轨（**机制面**，方案A Q1）：返回 `(轨压 mV, 引脚 raw)`；未装配
    /// （ADC init 失败）返回 Err 文案（**不冒领读数**）。纯机制——不判阈值、不决策。
    pub(crate) fn rail_sample(&mut self) -> Result<(u16, u16), String> {
        match self.rail.as_mut() {
            Some(r) => r.sample().map_err(|e| format!("ADC 读失败: {e:?}")),
            None => Err("5V 轨监测未装配（查 ADC 组件/GPIO34 接线）".into()),
        }
    }

    /// OLED 文本 3 行（21 列硬切）+ 角度行（row 3）刷新（**带恢复环**）。
    pub(crate) fn oled_show(&mut self, text: &str, pan_deg: u32, tilt_deg: u32) -> Result<(), String> {
        // 先过滤成可见 ASCII（其余转空格，与 glyph 行为一致）再按 21 列硬切
        // 3 行——字节切片不劈多字节字符（char boundary 安全），行对齐不漂移。
        let ascii: Vec<u8> = text
            .chars()
            .map(|c| if (0x20..=0x7E).contains(&(c as u32)) { c as u8 } else { b' ' })
            .collect();
        let mut lines: Vec<Vec<u8>> = Vec::with_capacity(OLED_ROWS);
        for row in 0..(OLED_ROWS - 1) {
            let start = row * OLED_COLS;
            lines.push(if start < ascii.len() {
                ascii[start..(start + OLED_COLS).min(ascii.len())].to_vec()
            } else {
                Vec::new()
            });
        }
        lines.push(format!("P:{pan_deg:03} T:{tilt_deg:03}").into_bytes());
        self.render_with_recovery(|i2c| {
            for (row, line) in lines.iter().enumerate() {
                oled_text(i2c, row, line)?;
            }
            Ok(())
        })
    }
}
