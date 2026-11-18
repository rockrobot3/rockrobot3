//! 监听、accept、连接线程编排（spec §4）。
//!
//! Task 10 加了**唯一的** watchdog 线程（`rr3_spawn_watchdog`）；Task 11b 加了**唯一的**
//! supervisor 线程（`supervisor::rr3_spawn_supervisor`）与 boot 序。除这两条外本文件不养
//! 其它常驻线程——收尸、退避、重启全部串行在那一条上（E-5）。例外（0.0.5 增补轮四
//! 批W3，R26）：`counters-term-flush` 一条——SIGTERM/SIGINT 强制 flush 的 sleep-and-wake
//! 线程（self-pipe，见 [`ctl::rr3_flush_on_term`]），平时不醒，不算常驻负载，
//! 进程账按 +1 线程 +1 fd 记。
//! 本任务做的：清残留套接字 → bind → 起 supervisor → 起监视线程 → 投 boot 队列 → accept 循环。
//! 连接账（M-1）：每连接 **2 条线程 + 2 个 fd**，不是一条——accept 处 spawn 的 reader 线程，
//! 加上 `conn.rs` 里为同一条连接 spawn 的 writer 线程与它那份 `try_clone`（评审资源探针实测
//! 一条活跃连接 +2 线程 +2 fd）。Task 10/11 算并发与文件描述符账按这个数。
//! 单次 accept 失败只记日志不带走守护进程；协议错误只结束那一条连接。

mod boot;
mod ctl;
mod run;
mod task;
mod tick;

#[cfg(test)]
mod tests;

pub use run::rr3_daemon_run;
pub use tick::{rr3_now_ms, rr3_now_ns, rr3_power_frame, rr3_state_frame};
