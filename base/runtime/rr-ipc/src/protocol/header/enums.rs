// SPDX-License-Identifier: Apache-2.0
//! A-IPC 协议词表：操作码、flags 位与 payload/status 枚举值（L1 冻结）。
//!
//! 本模块只放"取值即契约"的枚举与位表；128B 消息头的布局、编解码与
//! 编译期布局断言住父模块 [`super`]，两者共用一个公开路径。

/// 传输层操作码（与 payload 类型分属不同层次）
pub mod opcode {
    pub const SEND: u16 = 0x0001;
    pub const RECV: u16 = 0x0002;
    pub const SEND_BATCH: u16 = 0x0003;
    pub const CANCEL: u16 = 0x0004;
}

/// 头 flags 位。标准定义的全部 8 位；rock3 采用 NOWAIT/SIGNAL/ZEROCOPY/URGENT/NOREPLY，
/// CAP_CARRY/COMPRESS/ENCRYPT 已定义但不使用（ADR-0006 备选否决项）。
pub mod flags {
    pub const NOWAIT: u16 = 0x0001;
    pub const SIGNAL: u16 = 0x0002;
    pub const ZEROCOPY: u16 = 0x0004;
    pub const CAP_CARRY: u16 = 0x0008;
    pub const COMPRESS: u16 = 0x0010;
    pub const ENCRYPT: u16 = 0x0020;
    pub const URGENT: u16 = 0x0040;
    pub const NOREPLY: u16 = 0x0080;

    /// 标准已定义位掩码（AOS-STD-IPC-004 的前向兼容约定）。
    ///
    /// 实际行为（v0.0.1 不做运行时校验，也不得据此声称已强制）：
    /// `to_bytes`/`from_bytes` 对 `flags` 是**原样透传**——掩码之外的位既不报错也不清零，
    /// 编解码两个方向都保留（见 `header_unknown_flags`）。标准对发送方"应只置已定义位"
    /// 的期望属调用方自律与 M2 传输层的职责，本 crate 不拦截；接收方忽略未知位正是 IPC-T-010。
    pub const DEFINED: u16 =
        NOWAIT | SIGNAL | ZEROCOPY | CAP_CARRY | COMPRESS | ENCRYPT | URGENT | NOREPLY;
}

/// 5 种 payload 类型，枚举值 L1 永久稳定（AOS-STD-IPC-005）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3PayloadType {
    /// RPC 调用（JSON-RPC 2.0 body）
    Request = 0x0001,
    /// RPC 响应
    Response = 0x0002,
    /// 事件总线，尽力投递
    Event = 0x0003,
    /// 有序流：音频 PCM / 关节状态（实时控制面）
    Stream = 0x0004,
    /// 心跳 / 系统通知
    Notification = 0x0005,
}

impl Rr3PayloadType {
    /// 由类型枚举值解析。payload 体的前缀宽度已在 M2 冻结为 2 字节（`protocol::body`，
    /// `RR3_BODY_TYPE_PREFIX_LEN`），本函数就是它把线上字节还原成枚举的唯一入口。
    pub fn from_u16(v: u16) -> Option<Self> {
        let known: [(u16, Self); 5] = [
            (0x0001, Self::Request),
            (0x0002, Self::Response),
            (0x0003, Self::Event),
            (0x0004, Self::Stream),
            (0x0005, Self::Notification),
        ];
        known.iter().find(|(k, _)| *k == v).map(|(_, t)| *t)
    }
}

/// RESPONSE.status —— AIRY_E* 子集（ADR-0006 §3）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3StatusCode {
    Ok = 0,
    Invalid = -2,
    Busy = -16,
    Permission = -22,
    TimedOut = -1004,
}
