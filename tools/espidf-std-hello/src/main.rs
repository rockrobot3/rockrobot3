//! espidf-std-hello —— 0.0.5 增补轮四 批W1 espidf std 构建链冒烟件。
//!
//! 职责（全案 §3.3 C1）：证明 `cargo +esp build --target xtensa-esp32-espidf`
//! 整链可行——rustc（esp fork，build-std 从捆绑 rust-src 现编 std）→
//! esp-idf-sys 0.35.0 构建脚本（IDF_PATH 指本地 ESP-IDF v5.2.2 克隆，cmake
//! 编 IDF C 库）→ bindgen 出绑定 → ldproxy + Xtensa esp-gcc 链接出 ELF。
//! 本文件只写最小观测面：两枚 IDF C API（版本串/空闲堆）经 std println 出帧。
//! 哨兵 std 运行时（Q2 裁定 a）归 0.0.7 批D1——本件不承载任何业务语义。
//!
//! 外置不进仓（Q5 纪律）：ESP-IDF v5.2.2 与 esp-gcc 全在 `~/.espressif/`，
//! 版本/路径钉死登记见本目录 README.md。

use std::ffi::CStr;

use esp_idf_sys::{esp_get_free_heap_size, esp_get_idf_version};

fn main() {
    let idf_ver = unsafe { CStr::from_ptr(esp_get_idf_version()) }
        .to_string_lossy()
        .into_owned();
    let free_heap = unsafe { esp_get_free_heap_size() };
    println!("[espidf-std-hello] IDF={idf_ver} free_heap={free_heap}B std-println-OK");
}
