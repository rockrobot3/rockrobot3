//! 内部 SRAM / PSRAM 堆账采样（W7 内存门观测面）。
//!
//! 只读 IDF 堆账，不改分配；一行一阶段、字段固定，供 W7 Go/No-Go 逐阶段对账。
//! `int_min` 是启动以来的内部堆最低水位（不是当前窗口），用于判断 TLS/云调用
//! 是否触底；classic 无 PSRAM 时 `psram_*` 恒 0，不伪造外部堆。
//!
//! 采样点只放阶段边界（云调用首包/收尾、对话轮、播放、httpd 起栈），不放音频
//! 热路径——日志在 1.5M 串口上与音频上行抢带宽的教训在案。

use esp_idf_sys::{
    esp_get_free_heap_size, esp_get_minimum_free_heap_size, heap_caps_get_free_size,
    heap_caps_get_minimum_free_size, heap_caps_get_total_size, MALLOC_CAP_INTERNAL,
    MALLOC_CAP_SPIRAM,
};

/// 打一行阶段堆账：`stage` 是业务阶段，`phase` 是阶段内点位（首包/收尾等）。
pub(crate) fn rr3_heap_mark(stage: &str, phase: &str) {
    // SAFETY：六个堆账查询都是无参只读的 IDF 系统调用，不触碰 Rust 内存状态；
    // 返回值按 IDF 契约是字节数或 0（PSRAM 未启用）。
    let free = unsafe { esp_get_free_heap_size() };
    let min_free = unsafe { esp_get_minimum_free_heap_size() };
    let int_free = unsafe { heap_caps_get_free_size(MALLOC_CAP_INTERNAL) };
    let int_min = unsafe { heap_caps_get_minimum_free_size(MALLOC_CAP_INTERNAL) };
    let psram_free = unsafe { heap_caps_get_free_size(MALLOC_CAP_SPIRAM) };
    let psram_total = unsafe { heap_caps_get_total_size(MALLOC_CAP_SPIRAM) };
    println!(
        "[mem] stage={stage} phase={phase} free={free}B int_free={int_free}B int_min={int_min}B psram_free={psram_free}B psram_total={psram_total}B min_free={min_free}B"
    );
}
