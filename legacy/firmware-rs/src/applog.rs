//! applog：日志环形缓冲（移植自 legacy/core/applog）
//!
//! C 版通过 esp_log_set_vprintf 钩子截获 esp_log 输出（va_list 回调 Rust 无法实现），
//! Rust 版改为实现 log crate 的 Log trait：串口照常输出，同时写入 RAM 环形缓冲。
//! 对外提供 count()/dump_json()（供将来 panel /api/logs 使用）与 write_line()。

use std::collections::VecDeque;
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

/// 环形缓冲最大行数（C 版 LOG_MAX_LINES）
pub const MAX_LINES: usize = 100;
/// 单行最大长度，超长截断并标记 '~'（C 版 LOG_LINE_MAX）
pub const LINE_MAX: usize = 160;

static RING: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

fn push_ring(line: &str) {
    let mut g = RING.lock().unwrap();
    if g.len() >= MAX_LINES {
        g.pop_front();
    }
    g.push_back(line.to_string());
}

struct Applog;

impl Log for Applog {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // 开机至今秒数（对应 C 版 esp_timer_get_time 时间戳）
        let us = unsafe { esp_idf_svc::sys::esp_timer_get_time() };
        let line = format!("[{:>8.3}] {:<5} {}", us as f64 / 1e6, record.level(), record.args());
        println!("{}", line); // 串口同步可见
        push_ring(&line);
    }

    fn flush(&self) {}
}

/// 初始化日志：接管 log crate 输出（替代 EspLogger::initialize_default）
pub fn init() {
    let _ = log::set_boxed_logger(Box::new(Applog));
    log::set_max_level(LevelFilter::Info);
}

/// 直接写一条日志（对应 C 版 log_writef，不走 level 机制，串口+缓冲双写）
pub fn write_line(line: &str) {
    let us = unsafe { esp_idf_svc::sys::esp_timer_get_time() };
    let full = format!("[{:>8.3}] {}", us as f64 / 1e6, line);
    println!("{}", full);
    push_ring(&full);
}

/// 环形缓冲当前行数
pub fn count() -> usize {
    RING.lock().unwrap().len()
}

/// JSON 字符串转义
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// 导出最近 max_lines 条日志为 JSON 数组，新的在前（对应 C 版 log_dump_json）
pub fn dump_json(max_lines: usize) -> String {
    let g = RING.lock().unwrap();
    let n = max_lines.min(g.len());
    let mut out = String::from("[");
    for (i, line) in g.iter().rev().take(n).enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&json_escape(line));
        out.push('"');
    }
    out.push(']');
    out
}
