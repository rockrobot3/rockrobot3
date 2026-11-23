// SPDX-License-Identifier: Apache-2.0
//! PSRAM 定容缓冲（W6 内存门）：大块音频缓冲放外部 PSRAM，内部 SRAM 只留栈与
//! 热路径（计划册 §3.2 内存策略）。
//!
//! 载体仍是 `Vec<T>`（Slice 语义不变），底层块由 `heap_caps_malloc(MALLOC_CAP_SPIRAM)`
//! 提供；容量一次定死、调用方不得越界 push（超出会触发 Vec 重新分配，退回内部堆）。
//! PSRAM 不可用（classic / sdkconfig 未生效）时回退普通 `Vec::with_capacity`，
//! 行为不静默消失——boot 的 PSRAM 点名行与 RAM 点会同时显形。

use esp_idf_sys::{heap_caps_get_total_size, heap_caps_malloc, MALLOC_CAP_SPIRAM};

/// PSRAM 总容量（0 = 未启用；调用方据此显式降级，不赌分配成功）。
pub(crate) fn rr3_psram_total() -> usize {
    // SAFETY：无参只读的 IDF 堆账查询。
    unsafe { heap_caps_get_total_size(MALLOC_CAP_SPIRAM) as usize }
}

/// 定容向量（PSRAM 优先；`cap` 为元素个数）。
pub(crate) fn rr3_psram_vec<T>(cap: usize) -> Vec<T> {
    if cap == 0 {
        return Vec::new();
    }
    let bytes = cap.saturating_mul(core::mem::size_of::<T>());
    // SAFETY：heap_caps_malloc 返回 null 或一块至少 bytes 字节、按平台最大对齐的
    // 可写内存；null 走回退分支，不做解引用。
    let ptr = unsafe { heap_caps_malloc(bytes, MALLOC_CAP_SPIRAM) } as *mut T;
    if ptr.is_null() {
        return Vec::with_capacity(cap);
    }
    // SAFETY：ptr 来自 heap_caps_malloc（同 malloc 家族，free 可释放），长度 0、
    // 容量恰为 cap；所有权自此归 Vec，容量耗尽前不会 realloc。
    unsafe { Vec::from_raw_parts(ptr, 0, cap) }
}
