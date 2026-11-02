//! 板侧策略量单点（Pi 4B 档）——**只住数据与换算常数，不住机制**。
//!
//! 机制件（`periph-mech`）、端口契约（`rr-periph-proto::ports`）与协议面
//! （`rr-periph-proto`）都不认识本文件；本文件只被适配层（`main`/`ports`/`oled`）
//! 引用。换板/换舵机/换测点 = 只改本文件。

/// 板↔宿主 UART 设备（Raspberry Pi OS：`/dev/serial0` → `ttyAMA0`，PL011）。
pub const UART_DEV: &str = "/dev/serial0";
/// 板↔宿主 UART 波特率（2026-10-30 三线直连定档 1.5M，与宿主同档）。
pub const UART_BAUD: u32 = 1_500_000;

/// Linux i2c-dev 设备（Pi 40-pin 排针 GPIO2/3 = I2C1，OLED + ADS1115 同总线）。
pub const I2C_DEV: &str = "/dev/i2c-1";

/// PWM sysfs 根（`dtoverlay=pwm-2chan` 的 pwmchip0；通道目录 pwm0/pwm1）。
pub const PWM_ROOT: &str = "/sys/class/pwm/pwmchip0";
/// sysfs 通道目录名（通道号 = 目录后缀）。
pub const PWM_CHANNELS: [&str; 2] = ["pwm0", "pwm1"];
/// 舵机帧周期（50Hz）。
pub const PWM_PERIOD_NS: u32 = 20_000_000;
/// 占空比刻度：与经典板 LEDC 14 位分辨率同口径（16384 tick / 20ms）。
/// 机制核产出 tick，端口按本值换算成 sysfs 纳秒——两板的角→tick 表因此逐值相同。
pub const PWM_RESOLUTION_TICKS: u32 = 16384;
/// 舵机脉宽下限（0.5ms，14 位刻度）。
pub const DUTY_MIN_TICKS: u32 = 410;
/// 舵机脉宽上限（2.5ms，14 位刻度）。
pub const DUTY_MAX_TICKS: u32 = 2048;
/// 缓速步进（度）与步间等待（ms）：电源软启动策略。
pub const SERVO_SLEW_STEP_DEG: u32 = 2;
pub const SERVO_SLEW_STEP_MS: u64 = 20;

/// SSD1306 7 位从地址。
pub const OLED_ADDR: u8 = 0x3C;
/// 文本行数（3 行文本 + 1 行角度）与每行列数。
pub const OLED_ROWS: usize = 4;
pub const OLED_COLS: usize = 21;
/// 初始亮度（中档：满亮大面亮点会反噬 I2C，与经典板同口径）。
pub const OLED_CONTRAST: u8 = 0x4F;
/// I2C 事务重试次数与间隔；整页重写轮数 / 分块字节数 / 块间隔。
pub const I2C_RETRY: u32 = 3;
pub const I2C_RETRY_DELAY_MS: u64 = 5;
pub const I2C_PAGE_ATTEMPTS: u32 = 3;
pub const I2C_PAGE_CHUNK: usize = 32;
pub const I2C_CHUNK_GAP_MS: u64 = 1;

/// ADS1115 7 位从地址（ADDR 接地 = 0x48）。
pub const ADS1115_ADDR: u8 = 0x48;
/// 轨压测点通道（A0；`Rr3AdcPort::pin` 的对账参数）。
pub const ADS1115_RAIL_CHANNEL: u8 = 0;
/// ADS1115 单端 ±4.096V 量程 + 128SPS + 单次转换 + 比较器关闭（0x8383）。
pub const ADS1115_CONFIG: [u8; 2] = [0x83, 0x83];
/// 单次转换等待（128SPS ≈ 7.8ms；取 12ms 留裕量）。
pub const ADS1115_WAIT_MS: u64 = 12;
/// 5V 轨 2:1 分压比。
pub const RAIL_DIVIDER_NUM: u32 = 2;
pub const RAIL_DIVIDER_DEN: u32 = 1;
/// 轨压阈值（声明预算位，标定后回填）。
pub const RAIL_MILD_MV: u16 = 4_600;
pub const RAIL_SEVERE_MV: u16 = 4_400;
/// 轨压拒动闸：未标定不武装（与经典板同口径：只观测不拒动）。
pub const RAIL_GATE_ARMED: bool = false;
/// 轨压周期采样间隔（主循环空闲拍）。
pub const RAIL_SAMPLE_PERIOD_MS: u64 = 500;

/// 板侧绑定对账锚（`rr3.set_bindings` 注入值与此逐键判等；不齐只计数不拒收）。
pub const BIND_I2C_SDA: u8 = 2;
pub const BIND_I2C_SCL: u8 = 3;
pub const BIND_UART2_TX: u8 = 14;
pub const BIND_UART2_RX: u8 = 15;

/// 行面预算（半行超时与每行上限）。
pub const FIRST_BYTE_TIMEOUT_MS: i32 = 10;
pub const LINE_BYTE_TIMEOUT_MS: i32 = 50;
pub const LINE_MAX: usize = 128;
/// 帧面坏帧后的残尾吞净上限。
pub const FRAME_DRAIN_MAX: usize = 4096;

/// 环境覆盖键（应急；缺省即本文件常数）。
pub const ENV_UART_DEV: &str = "RR3_PI_UART";
pub const ENV_UART_BAUD: &str = "RR3_PI_BAUD";
pub const ENV_I2C_DEV: &str = "RR3_PI_I2C_DEV";
pub const ENV_PWM_ROOT: &str = "RR3_PI_PWM_ROOT";

/// 读取字符串环境覆盖（trim 后非空才生效）。
pub fn env_str(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// 读取整数环境覆盖（解析失败按缺省；值域由消费面判）。
pub fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(default)
}

/// 占空 tick → sysfs 纳秒（纯换算；刻度 = 14 位分辨率）。
pub fn pwm_ticks_to_ns(ticks: u32) -> u32 {
    ((ticks as u64) * (PWM_PERIOD_NS as u64) / (PWM_RESOLUTION_TICKS as u64)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pwm_ticks_map_to_expected_ns() {
        assert_eq!(pwm_ticks_to_ns(0), 0);
        assert_eq!(pwm_ticks_to_ns(DUTY_MAX_TICKS), 2_500_000);
        assert_eq!(pwm_ticks_to_ns(DUTY_MIN_TICKS), 500_488);
    }
}
