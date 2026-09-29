//! 反射环宿主进程内的 1k–10kHz 闭环。
//!
//! **永不进总线**（ADR-0007 裁定 1）：这里只允许在异常/饱和时发
//! `rr3-act/motor/reflex` 通告，闭环数据本身不过 IPC。

pub const SURFACE: &[&str] =
    &["reflex: 1k-10kHz 闭环，进程内 | 异常通告 -> rr3-act/motor/reflex (notification)"];
