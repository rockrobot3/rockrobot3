// SPDX-License-Identifier: Apache-2.0
//! 模块侧同步总线客户端（architecture.md §6）。
//!
//! 总线的**承载契约面**：模块与 rrd 之间唯一的标准说话方式——连接 → 注册（manifest
//! 全文，entry 绝对化）→ 心跳线程 → 发布 / 点名调用 / 收帧。
//!
//! 并发形状（与 rrd conn.rs 同构、方向相反）：reader 线程独占读；写半边由
//! `Mutex<UnixStream>` 保护（一帧一次 `write_all`，整帧原子）。**应答与广播的相对
//! 次序不保证**，故 Response 按 JSON-RPC id 路由到 pending 表，其余帧进 inbox 由
//! `rr3_poll` 轮询——客户端永不假设帧序。
//!
//! 心跳失败 = stderr 一行 + `exit(1)`：模块不装"活着"，死交 supervisor 按 restart_policy 裁定。
//!
//! `rr3.call`（[`RR3_METHOD_CALL`]）是第 4 个上行 RPC（trace_id：0 = 非代理，代理请求 = 调用方 task）——被调模块用 [`Rr3BusClient::rr3_reply_call`] 顺原路回话。

mod conn;
mod parse;
mod publish;
mod sub;
#[cfg(all(test, unix))]
mod tests;

pub use conn::{Rr3BusClient, Rr3TimeBase};
pub use parse::{rr3_entry_absolutize, rr3_manifest_ident, Rr3BusIdentity};
