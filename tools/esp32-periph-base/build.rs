//! 链接参数接力（同 tools/espidf-sentinel/build.rs，批W1 同款）。
//!
//! esp-idf-sys 的 build 脚本经 `cargo:rustc-link-arg` 发射的 `--ldproxy-linker`
//! 与 sysroot 参数只作用于它自己包内的 bins/examples（它没有 bin，全被丢弃）；
//! 依赖方 bins 必须经 `links = "esp_idf"` 元数据取回转发，否则 ldproxy 因收不到
//! `--ldproxy-linker <gcc>` 而 panic（官方 esp-idf-template 同款 build.rs）。
//!
//! 另职责：从 `sdkconfig.defaults[.<mcu>]` 取控制台波特率注入 `RR3_CONSOLE_BAUD`，
//! 供 boot 横幅打印"本固件日志口归属"（W7 现场"日志读不到"的第一判据）。叠加序
//! 与 IDF 一致：common 先、目标专属后写者胜；缺项回落 IDF 缺省 115200。

use std::fs;

/// 取 `CONFIG_ESP_CONSOLE_UART_BAUDRATE`（形制错/缺项即回落 115200，不 panic）。
fn console_baud(mcu: &str) -> String {
    let mut baud = String::from("115200");
    for f in [
        "sdkconfig.defaults".to_string(),
        format!("sdkconfig.defaults.{mcu}"),
    ] {
        if let Ok(txt) = fs::read_to_string(&f) {
            for line in txt.lines() {
                if let Some(v) = line
                    .trim()
                    .strip_prefix("CONFIG_ESP_CONSOLE_UART_BAUDRATE=")
                {
                    baud = v.trim().to_string();
                }
            }
        }
        println!("cargo:rerun-if-changed={f}");
    }
    baud
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    embuild::build::LinkArgs::output_propagated("ESP_IDF")?;
    let mcu = std::env::var("MCU").unwrap_or_else(|_| "esp32".into());
    println!("cargo:rustc-env=RR3_CONSOLE_BAUD={}", console_baud(&mcu));
    Ok(())
}
