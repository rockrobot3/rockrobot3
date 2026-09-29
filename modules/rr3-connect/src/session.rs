//! 云端会话：`rr3-connect/cloud/session`（request 事务面）。
//!
//! 会话建立/续期由 brain 侧发起，connect 只提供链路；凭据不经过本模块持久化。

pub const SURFACE: &[&str] = &["session: rpc rr3-connect/cloud/session (request)"];
