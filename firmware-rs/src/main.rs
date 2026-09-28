use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::PinDriver;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::sys::link_patches;
use log::info;

fn main() -> anyhow::Result<()> {
    // esp-idf-sys 运行时补丁（必须最先调用）
    link_patches();
    // 绑定 log 到 ESP-IDF 日志系统
    EspLogger::initialize_default();

    info!("RockRobot3 Rust 阶段1：工具链验证，板载 LED 闪烁");

    let peripherals = Peripherals::take().unwrap();
    // 板载 LED 在 GPIO2，高电平点亮
    let mut led = PinDriver::output(peripherals.pins.gpio2)?;

    let mut n: u32 = 0;
    loop {
        led.set_high()?;
        FreeRtos::delay_ms(500);
        led.set_low()?;
        FreeRtos::delay_ms(500);
        n += 1;
        if n % 2 == 0 {
            info!("blink {}", n / 2);
        }
    }
}
