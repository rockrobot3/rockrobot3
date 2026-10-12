use std::thread;
use std::time::Duration;

use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::ledc::LedcDriver;
use esp_idf_hal::uart::UartDriver;

use crate::board::{
    axis_check, deg_to_duty, Axis, OLED_COLS, OLED_CONTRAST_LEVELS, OLED_ROWS,
    SERVO_SLEW_STEP_DEG, SERVO_SLEW_STEP_MS,
};
use crate::oled::{oled_capsules, oled_cmd, oled_text};

// ---------------------------------------------------------------------------
// 外设面（控制面/运行面分离）：控制面独占 UART2，运行面独占 I2C0 + LEDC
// ---------------------------------------------------------------------------

/// 控制面独占的 UART2（指令面 io：行面直用固有方法，帧面经 [`UartIo`] 窄缝）。
/// 驱动 'static（外设 token 即 'static 无借用绑定；move 进线程后 esp_restart
/// 前永不 drop）。UartDriver 上游已标 `Send`，本结构自动 Send。
pub(crate) struct CtlUart {
    pub(crate) uart: UartDriver<'static>,
}

/// 运行面独占的外设面：I2C（OLED）+ 两轴 LEDC 舵机。**全部硬件慢动作在此面执行**
/// ——慢 I2C 与舵机整定都不阻塞控制面的命令吞吐。驱动 'static 同上；
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
}

impl RtPeriph {
    /// 舵机转角（先过本轴行程裁决 → **缓速步进**逼近 → 返回实际设定角）。
    /// 协议级 0–180 之外的角先被拒；轴行程（中位 ± 摆幅）之外**点名拒收**，
    /// 不静默钳位——见文件头"分轴行程"段。缓速步进见 [`SERVO_SLEW_STEP_DEG`]
    /// （极限电源下的软启动：把整段跳变的电流尖峰摊成涓流）。
    pub(crate) fn servo_set(&mut self, axis: Axis, deg: u32) -> Result<u32, String> {
        let deg = axis_check(axis, deg)?;
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

    /// 渲染胶囊眼（**带整板电压降级**）：整屏渲染失败即降一档亮度重来，到地板仍败
    /// 才回 Err。真板主故障（大面亮点 → 轨压/地弹 → I2C 丢拍）由此闭环自愈。
    pub(crate) fn capsules_degrading(&mut self, open: u32, dx: i16, dy: i16) -> Result<(), String> {
        loop {
            match oled_capsules(&mut self.i2c, open, dx, dy) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if !self.degrade_contrast() {
                        return Err(e);
                    }
                }
            }
        }
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

    /// OLED 文本 3 行（21 列硬切）+ 角度行（row 3）刷新。
    pub(crate) fn oled_show(&mut self, text: &str, pan_deg: u32, tilt_deg: u32) -> Result<(), String> {
        // 先过滤成可见 ASCII（其余转空格，与 glyph 行为一致）再按 21 列硬切
        // 3 行——字节切片不劈多字节字符（char boundary 安全），行对齐不漂移。
        let ascii: String = text
            .chars()
            .map(|c| {
                if (0x20..=0x7E).contains(&(c as u32)) {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        let bytes = ascii.as_bytes();
        for row in 0..(OLED_ROWS - 1) {
            let start = row * OLED_COLS;
            let chunk = if start < bytes.len() {
                let end = (start + OLED_COLS).min(bytes.len());
                &bytes[start..end]
            } else {
                b"" as &[u8]
            };
            oled_text(&mut self.i2c, row, chunk)?;
        }
        let angle_line = format!("P:{pan_deg:03} T:{tilt_deg:03}");
        oled_text(&mut self.i2c, OLED_ROWS - 1, angle_line.as_bytes())
    }
}