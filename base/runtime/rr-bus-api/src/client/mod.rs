//! 模块侧同步总线客户端（M3，architecture.md §6）。
//!
//! rr-bus-api 的文档声明落点（lib.rs 头注）：总线的**承载契约面**——客户端逐字自
//! `rr-bus` 迁入（批N9 件一，基-35 窄化②），是模块与 rrd 之间唯一的标准说话方式：
//! 连接 → 注册（manifest 全文，entry 绝对化）→ 心跳线程 → 发布 / 点名调用 / 收帧。
//!
//! 并发形状（与 rrd 的 conn.rs 同构、方向相反）：reader 线程独占读；写半边由
//! `Mutex<UnixStream>` 保护（一帧一次 `write_all`，整帧原子）。**应答与广播在同一
//! socket 上的相对次序不保证**（conn.rs 写面注释的同一条事实），所以 Response 按
//! JSON-RPC id 路由到 pending 表，其余帧进 inbox 由模块 `rr3_poll` 轮询——客户端
//! 永远不假设帧序。
//!
//! 心跳失败 = stderr 一行 + `exit(1)`：模块不装"活着"，把死交给 supervisor 按
//! restart_policy 裁定（rrd 的进程语义）。
//!
//! `rr3.call`（[`RR3_METHOD_CALL`])是 M3 定型的第 4 个上行 RPC 方法（trace_id
//! 语义：0 = 非代理；代理请求的 trace_id = 调用方 task，见 contracts/README.md
//! 演进登记）——被调模块用 [`Rr3BusClient::rr3_reply_call`] 顺原路回话。

mod conn;
mod parse;
mod publish;
mod sub;
#[cfg(all(test, unix))]
mod tests;

pub use conn::{Rr3BusClient, Rr3TimeBase};
pub use parse::{rr3_entry_absolutize, rr3_manifest_ident, Rr3BusIdentity};
