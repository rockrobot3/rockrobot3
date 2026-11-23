// SPDX-License-Identifier: Apache-2.0
//! 启动期 I2C0/OLED 装配：解卡 → 驱动安装 → 扫描点名 → init 重试 → boot 画面。
//!
//! 真板诊断面纪律：全链失败只打印 Err 明细并继续跑指令面，不 panic
//! （crate 以 `-Cpanic=immediate-abort` 构建，expect 文案不落地）。
//! 频率档见 board：50k 不是更稳档（真板实锤，见 [`I2C_BAUD_HZ`] 注）。

use std::thread;
use std::time::Duration;

use esp_idf_hal::gpio::{InputPin, OutputPin};
use esp_idf_hal::i2c::{I2cConfig, I2cDriver, I2C0};
use esp_idf_hal::peripheral::Peripheral;
use esp_idf_hal::units::Hertz;

use super::diag;
use crate::board::{BOOT_PAN_DEG, BOOT_TILT_DEG, I2C_BAUD_HZ, OLED_ADDR, UART2_BAUD_HZ};
use crate::oled::{i2c_bus_unstick, i2c_scan, oled_init, oled_text};

/// I2C 扫描结果点名（OLED 在列 ✓ / 不在列 ✗）。
fn i2c_scan_report(found: &[u8]) {
    if found.contains(&OLED_ADDR) {
        println!("[periph] I2C 扫描应答: {found:02X?} —— OLED 0x{OLED_ADDR:02X} 在列 ✓");
    } else {
        println!(
            "[periph] I2C 扫描应答: {found:02X?} —— 0x{OLED_ADDR:02X} 不在列 ✗（查接线/上拉）"
        );
    }
}

/// OLED init 重试：100k 档上电电荷泵未稳的瞬态失败，每次隔 1s、最多 5 次；
/// 真板诊断面纪律——失败打印 Err 明细不 panic（panic_abort 下 expect 无消息）。
fn oled_init_retry(i2c: &mut I2cDriver) -> bool {
    for attempt in 1..=5u32 {
        match oled_init(i2c) {
            Ok(()) => {
                println!("[periph] OLED init ✓（100k 档第 {attempt} 次过）");
                return true;
            }
            Err(e) => {
                println!("[periph] OLED 100k init 第 {attempt}/5 次失败: {e}");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
    false
}

/// boot 四行（4 行扩行能力的直证；行2/行3 从 board 真值拼出，防字面量漂移）。
fn oled_boot_rows(i2c: &mut I2cDriver) {
    let boot_rows: [(String, &str); 4] = [
        ("ROCK3 PERIPH BASE".into(), "boot 行0写失败"),
        ("4-ROW OLED READY".into(), "boot 行1写失败"),
        (format!("UART2 CMD @{UART2_BAUD_HZ}"), "boot 行2写失败"),
        (
            format!("P:{BOOT_PAN_DEG:03} T:{BOOT_TILT_DEG:03}"),
            "boot 角度行写失败",
        ),
    ];
    let mut boot_ok = true;
    for (row, (text, msg)) in boot_rows.iter().enumerate() {
        if let Err(e) = oled_text(i2c, row, text.as_bytes()) {
            println!("[periph] {msg}: {e}");
            boot_ok = false;
        }
    }
    if boot_ok {
        println!("[periph] OLED ✓ boot 画面已出（4 行）");
    }
}

/// OLED 装配面：扫描点名 → init 重试 → boot 画面；全程不 panic（终败继续跑指令面）。
fn oled_boot(i2c: &mut I2cDriver) {
    let found = i2c_scan(&mut crate::ports_impl::rr3_esp_i2c(&mut *i2c));
    i2c_scan_report(&found);
    if oled_init_retry(i2c) {
        oled_boot_rows(i2c);
    } else {
        println!(
            "[periph] OLED init 终败（400k/100k 双档）——不 panic 继续跑指令面（查上拉/接线/供电）"
        );
    }
}

/// I2C0 + OLED 装配：装驱动前先解卡（欠压复位后 SSD1306 常抱死 SDA→总线全黑）。
/// 批N41：注入优先、缺席回编译期装配锚（独立部署下 boot 期桥必未连，不能因此
/// 永久拒解）；false 只代表 9 时钟后 SDA 仍低（模块未上电/硬短/线松），不阻断 boot。
pub(super) fn open_oled<S, C>(
    i2c0: impl Peripheral<P = I2C0> + 'static,
    sda: impl Peripheral<P = S> + 'static,
    scl: impl Peripheral<P = C> + 'static,
) -> Result<I2cDriver<'static>, String>
where
    S: InputPin + OutputPin + 'static,
    C: InputPin + OutputPin + 'static,
{
    // SAFETY：启动期在 I2C 驱动安装之前、单线程上下文调用；函数只按注入/装配锚
    // 的引脚号操作 GPIO，不同时存在 I2C 事务（见 `i2c_bus_unstick` 的契约）。
    if unsafe { i2c_bus_unstick() } {
        println!("[periph] I2C boot 解卡 ✓（SDA 本已高或已释放）");
    } else {
        eprintln!("[periph] I2C boot 解卡未释（9 时钟后 SDA 仍低：模块未上电/硬短/线松）——继续 boot，运行面自愈将退避重试");
    }
    let config = I2cConfig::new()
        .baudrate(Hertz(I2C_BAUD_HZ))
        .sda_enable_pullup(true)
        .scl_enable_pullup(true);
    let mut i2c = I2cDriver::new(i2c0, sda, scl, &config).map_err(diag::i2c0_fail)?;
    // 扫描点名 → init 重试 → boot 画面（真板诊断面纪律：失败打印 Err 明细不 panic）
    oled_boot(&mut i2c);
    Ok(i2c)
}
