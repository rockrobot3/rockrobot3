//! blink — 工具链验证：板载 LED（GPIO2）闪烁 + 串口 hello
#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock,
    gpio::{Level, Output, OutputConfig},
    main,
    time::{Duration, Instant},
};
use esp_println::println;

#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // 板载 LED 在 GPIO2，高电平点亮
    let mut led = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());

    println!("MakeRobot Rust 工具链 OK，LED 开始闪烁");

    let mut n: u32 = 0;
    loop {
        led.toggle();
        let now = Instant::now();
        while now.elapsed() < Duration::from_millis(500) {}
        n += 1;
        if n % 2 == 0 {
            println!("blink {}", n / 2);
        }
    }
}
