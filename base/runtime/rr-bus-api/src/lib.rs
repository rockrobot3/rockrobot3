// SPDX-License-Identifier: Apache-2.0
//! # rr-bus-api
//!
//! 总线承载契约面（模块侧同步 UDS 客户端，基-35 窄化②）：五单元的承载面直依本
//! crate 而不再绑 `rr-bus`，客户端见 [`client`]（连接 → 注册 → 心跳 → 发布 /
//! `rr3.call` / 收帧）。
//!
//! `#[cfg(unix)]` 门随迁：非 unix 目标本 crate 编译为空壳——承载是平台事，UDS
//! 承载只对 unix 目标在册。
//!
//! 分工不变线：总线语义层真值单点在 `rr-core`（常量/寻址/帧组装），消费方直引
//! `rr_core`；本 crate 不 re-export 语义常量，不给同一真值立第二份供给路径。
//! `rr-bus` 以 re-export 保持 rrd/tools 的既有 import 路径。

#[cfg(unix)]
pub mod client;

#[cfg(unix)]
pub use client::{
    rr3_entry_absolutize, rr3_manifest_ident, Rr3BusClient, Rr3BusIdentity, Rr3TimeBase,
};
