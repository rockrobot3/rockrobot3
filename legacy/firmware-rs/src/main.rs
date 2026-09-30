//! RockRobot3 Rust 步骤1：relay + data + applog 三模块板测编排
//!
//! 迁移边界：hal_esp/* 为平台绑定层（RK3566 上整体重写），
//! cloud/dialog 为平台无关层（Linux 迁移零改动）。
//! 本阶段验证：GPIO21 继电器开合、NVS 计数器跨重启持久、日志环形缓冲导出。
mod applog;
mod data;
mod dialog;
mod hal_esp;

use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::PinDriver;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::link_patches;
use log::info;

use crate::hal_esp::relay::Relay;

fn main() -> anyhow::Result<()> {
    // esp-idf-sys 运行时补丁（必须最先调用）
    link_patches();
    // applog 接管 log crate 输出：串口 + RAM 环形缓冲双写
    applog::init();

    info!("=== RockRobot3 Rust 步骤1：relay + data + applog ===");

    let peripherals = Peripherals::take().unwrap();
    let nvs = EspDefaultNvsPartition::take()?;

    // data：加载 NVS 计数器，boots 自增并持久化
    let mut data = data::Data::new(nvs)?;
    info!("开机次数 boots={}", data.get(data::Key::Boots));

    // relay：板载继电器 REL 针 → GPIO21（状态软件保存）
    let mut relay = Relay::new(PinDriver::output(peripherals.pins.gpio21)?);

    let mut n: u32 = 0;
    loop {
        // 每 3s 切一次继电器并计数（relay_on/relay_off 持久化）
        let on = !relay.get();
        relay.set(on)?;
        if on {
            data.inc(data::Key::RelayOn);
        } else {
            data.inc(data::Key::RelayOff);
        }
        info!(
            "继电器 {} relay_on={} relay_off={} data={}",
            if on { "ON" } else { "OFF" },
            data.get(data::Key::RelayOn),
            data.get(data::Key::RelayOff),
            data.json()
        );
        // 每 5 轮导出最近 5 条日志（验证环形缓冲 + dump_json）
        if n % 5 == 4 {
            applog::write_line(&format!("最近日志={}", applog::dump_json(5)));
        }
        FreeRtos::delay_ms(3000);
        n += 1;
    }
}
