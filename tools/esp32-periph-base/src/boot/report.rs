//! 启动期观测行（自 boot.rs 按"装配序 / 观测面"拆分）：堆账三点采样、芯片自证
//! 横幅、PSRAM 点名。只读 IDF 账，不参与分配、不改装配序。
//!
//! 观测行字段固定、一行一阶段，供 W7 内存门与证据记录逐阶段对账；格式变动必须
//! 同批更新证据侧解析口径，不静默改列。

use esp_idf_sys::{esp_get_free_heap_size, esp_get_minimum_free_heap_size};

use crate::board::UART2_BAUD_HZ;

/// 三点堆采样观测行（批D 三点 RAM 复测沿用本法，批B 先落观测面）。
pub(super) fn ram_point(point: &str) {
    // SAFETY：四个堆查询都是无参只读的 IDF 系统调用，不触碰 Rust 内存状态。
    let free = unsafe { esp_get_free_heap_size() };
    let min_free = unsafe { esp_get_minimum_free_heap_size() };
    // PSRAM 进 malloc 后 esp_get_free_heap_size 含外部堆；内存门看**内部 SRAM**
    // 单列（计划册 §3.2：内部只留栈与热路径），避免"PSRAM 掩盖 SRAM 见底"。
    let int_free =
        unsafe { esp_idf_sys::heap_caps_get_free_size(esp_idf_sys::MALLOC_CAP_INTERNAL) };
    let int_min =
        unsafe { esp_idf_sys::heap_caps_get_minimum_free_size(esp_idf_sys::MALLOC_CAP_INTERNAL) };
    println!(
        "[periph] RAM point={point} free={free}B int_free={int_free}B int_min={int_min}B min_free={min_free}B"
    );
}

/// 启动横幅（引脚号不上横幅：I2C/轨测点真值经 rr3.set_bindings 注入；芯片档自证行
/// 让证据记录直接可辨 classic / S3，引脚真值以 board/pins_*.rs 对账锚为准）。
/// n20-live 标记：重烧后此行在册＝新固件上板实证；同屏「凭据源=nvs」＝NVS 凭据跨
/// 重烧保留实证（升级即重烧语义）。
pub(super) fn boot_banner() {
    println!(
        "[periph] surface=esp-rs-std crate=esp32-periph-base oled=i2c0 servo=ledc rail=adc1 cmd=uart2@{UART2_BAUD_HZ} line+frame planes=ctrl+runtime serial-servo n20-live"
    );
    // 控制台自证（W7 现场收口）：本行本身经 UART0 输出——能读到即证明"日志口 =
    // UART0"且终端 baud 与固件一致；波特率取自 sdkconfig defaults（build.rs 注入），
    // 不手抄常量，改配置时本行随动。副通道恒 none（S3 在目标档显式关闭）。
    println!(
        "[periph] console=uart0@{} secondary=none（读到本行即日志口正确）",
        env!("RR3_CONSOLE_BAUD")
    );
    #[cfg(feature = "esp32")]
    println!("[periph] chip=esp32 pins=classic");
    #[cfg(feature = "esp32s3")]
    {
        psram_point();
        println!("[periph] chip=esp32s3 pins=s3 flash=16MB psram=8MB-octal");
    }
    ram_point("1-boot");
}

/// PSRAM 点名行（W6 内存门证据）：sdkconfig.defaults 未生效时这里为 0——把
/// "配置没吃到"从隐性变成 boot 第一屏可见。
#[cfg(feature = "esp32s3")]
fn psram_point() {
    // SAFETY：两个 heap_caps 查询都是无参只读的 IDF 堆账读取。
    let total = unsafe { esp_idf_sys::heap_caps_get_total_size(esp_idf_sys::MALLOC_CAP_SPIRAM) };
    let free = unsafe { esp_idf_sys::heap_caps_get_free_size(esp_idf_sys::MALLOC_CAP_SPIRAM) };
    println!("[periph] PSRAM total={total}B free={free}B");
}
