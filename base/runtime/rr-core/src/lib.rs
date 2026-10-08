//! # rr-core —— 机制核切片：总线语义层
//!
//! 承 [ADR-0018](../../../docs/process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md)
//! 裁定 1：机制核与操作系统无关、`no_std`（core+alloc）可达、契约级冻结。字节协议层
//! 已下沉（`rr-ipc` protocol，0.0.7 批B）；本 crate（0.0.5 第 II 轮批D₁）把**总线语义**
//! 从 `rr-bus`（std 客户端 crate）里抽出下沉：
//!
//! - **常量层**：task 号空间（0 广播 / 1 rrd / ≥2 模块）、EVENT 优先级占位带、主题分隔符、
//!   契约 schema 版本透传、模块间点名方法名——唯一真值在 `rr-core`，`rr-bus` re-export
//!   保持既有 import 路径逐字不变；
//! - **寻址规则**：task 号归属判定（[`rr3_task_kind`]）与 trace_id 直连语义
//!   （[`RR3_TRACE_ID_DIRECT`]）；
//! - **帧组装**：[`rr3_frame_compose`] —— opcode/flags/trace_id/**timestamp_ns**/src/dst
//!   全部由调用方注入，本 crate 不读任何时钟、不睡眠、不碰 io（ADR-0018 裁定 5 的
//!   机制侧前提）；payload 类型前缀复用 `rr-ipc` 的 body 编解码。
//!
//! `net.online` 格（contracts/slots.toml base 层）的机制核切片落位于
//! [`runtime`] 模块（批D₂）：执行环（连接→注册→心跳→分发→节拍）的连接态
//! 语义 [`runtime::Rr3LinkState`] / 心跳拍点调度 [`runtime::Rr3ModuleRuntime`] /
//! 收帧分发分类 [`runtime::rr3_dispatch_class`]；常量与寻址先行落位（批D₁）。
//!
//! 主承载层（std 客户端：UDS、线程、serde_json）不在这里——那是 `rr-bus` 的事；
//! 本 crate 是它在裸金属目标上可复用的语义内核。
//!
//! 零策略载荷：不得出现厂商名 / 产品名 / 业务阈值 / `if (vendor == …)`
//! （architecture §13 门禁，批E 接线）。

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

use rr_ipc::opcode;
use rr_ipc::{Rr3Frame, Rr3Header, Rr3PayloadType, RR3_CONTRACT_SCHEMA_VERSION};

pub mod runtime;

// ---------------------------------------------------------------------------
// 常量层（批D₁ 自 rr-bus 迁入；rr-bus re-export 保持对外路径不变）
// ---------------------------------------------------------------------------

/// 广播目标（A-IPC 标准：dst_task = 0 表示广播）
pub const RR3_BROADCAST_DST_TASK: u64 = 0;

/// `rrd` 自身的 task 号：0 已被广播占用，守护进程固定 1（spec §5.0）
pub const RR3_TASK_RRD: u64 = 1;

/// 模块 `src_task` 分配池的起点（0=广播、1=rrd 均保留）
pub const RR3_TASK_FIRST_MODULE: u64 = 2;

/// EVENT 优先级下界。
///
/// **占位值**：v0.0.1 没有任何契约/ADR/文档定义过优先级区间，这两个数只是给
/// `rr-ota`/`rrd` 留出的常量位；调度类与优先级语义随 M2 的调度 ADR 定稿后替换。
pub const RR3_PRIORITY_MIN: u32 = 0;

/// EVENT 优先级上界（同上：M2 调度 ADR 前的占位值，非对齐 Airy IPC 或 Linux 调度类）
pub const RR3_PRIORITY_MAX: u32 = 139;

/// 主题分隔符
pub const RR3_TOPIC_SEP: char = '/';

/// 契约 schema 版本透传（rr-core 侧的再导出；原 rr-bus re-export 名保持）
pub const RR3_BUS_CONTRACT_VERSION: u32 = RR3_CONTRACT_SCHEMA_VERSION;

/// 模块间请求路由的上行 RPC 方法名（rr-core 是唯一真值点，rrd 的 rpc.rs 经 rr-bus
/// re-export 引用）
pub const RR3_METHOD_CALL: &str = "rr3.call";

/// trace_id 直连语义（contracts/README.md 演进登记）：0 = 非代理；
/// 代理请求的 trace_id = 调用方 task（0 不可能是合法调用方 task，见
/// [`rr3_task_kind`]：0 是广播）。
pub const RR3_TRACE_ID_DIRECT: u64 = 0;

// ---------------------------------------------------------------------------
// 寻址规则
// ---------------------------------------------------------------------------

/// task 号的归属判定（0 广播 / 1 rrd / ≥2 模块池；三段不得重叠）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3TaskKind {
    /// dst_task = 0：广播
    Broadcast,
    /// task 号 = 1：守护进程 rrd
    Daemon,
    /// task 号 ≥ 2：模块池（由 rrd 分配）
    Module,
}

/// 判定一个 task 号的归属。
pub fn rr3_task_kind(task: u64) -> Rr3TaskKind {
    match task {
        RR3_BROADCAST_DST_TASK => Rr3TaskKind::Broadcast,
        RR3_TASK_RRD => Rr3TaskKind::Daemon,
        _ => Rr3TaskKind::Module,
    }
}

// ---------------------------------------------------------------------------
// 帧组装（时钟注入；payload 类型前缀复用 rr-ipc body 编解码）
// ---------------------------------------------------------------------------

/// 组装一帧上行 SEND（机制核切片：不读钟、不睡眠、不碰 io）。
///
/// - `timestamp_ns` 由调用方注入（Linux 面是墙钟 ns，裸机面是平台单调时基的读数）；
/// - `payload` 是**已完成 body 编解码**的字节（[`rr3_body_encode`] 产物——payload
///   类型前缀 + 体）；`payload_len` 在此回填为 `payload.len()`；
/// - 返回 [`Rr3Frame`]，整帧字节化走 `rr_ipc::rr3_frame_to_bytes`（同在 no_std 面）。
pub fn rr3_frame_compose(
    op: u16,
    flags: u16,
    trace_id: u64,
    timestamp_ns: u64,
    src_task: u64,
    dst_task: u64,
    payload: Vec<u8>,
) -> Rr3Frame {
    Rr3Frame {
        header: Rr3Header {
            opcode: op,
            flags,
            trace_id,
            timestamp_ns,
            src_task,
            dst_task,
            payload_len: payload.len() as u32,
        },
        payload,
    }
}

/// 组装一条上行 SEND 的 Event 发布帧（[`RR3_METHOD_CALL`] 之外的总线出站原语族
/// 共用此形状：topic 在 body 内，归属判定是 rrd 的事）。
pub fn rr3_event_compose(
    timestamp_ns: u64,
    src_task: u64,
    payload: Vec<u8>,
) -> Rr3Frame {
    rr3_frame_compose(
        opcode::SEND,
        0,
        RR3_TRACE_ID_DIRECT,
        timestamp_ns,
        src_task,
        RR3_TASK_RRD,
        payload,
    )
}

/// 上行 RPC（Request）的 payload 类型速记——保留语义名供组装侧引用，避免散落字面量。
pub const RR3_PAYLOAD_REQUEST: Rr3PayloadType = Rr3PayloadType::Request;
/// 上行应答（Response）的 payload 类型速记。
pub const RR3_PAYLOAD_RESPONSE: Rr3PayloadType = Rr3PayloadType::Response;
/// 上行事件（Event）的 payload 类型速记。
pub const RR3_PAYLOAD_EVENT: Rr3PayloadType = Rr3PayloadType::Event;

#[cfg(test)]
mod tests {
    use super::*;
    use rr_ipc::opcode;
    use rr_ipc::rr3_body_encode;

    // no_std 显式化：std 面只在测试需要
    use alloc::vec;

    /// task 号三段不得重叠：0 广播 / 1 rrd / ≥2 模块（原 rr-bus lib 用例语义随迁）
    #[test]
    fn task_id_space_is_disjoint() {
        assert_eq!(RR3_BROADCAST_DST_TASK, 0);
        assert_eq!(RR3_TASK_RRD, 1);
        assert_eq!(RR3_TASK_FIRST_MODULE, 2);
        const {
            assert!(
                RR3_TASK_FIRST_MODULE > RR3_TASK_RRD,
                "模块池必须让开两个保留号"
            );
        }
        assert_eq!(rr3_task_kind(0), Rr3TaskKind::Broadcast);
        assert_eq!(rr3_task_kind(1), Rr3TaskKind::Daemon);
        assert_eq!(rr3_task_kind(2), Rr3TaskKind::Module);
        assert_eq!(rr3_task_kind(u64::MAX), Rr3TaskKind::Module);
    }

    /// 占位窗口要有内容可言：区间非空（MIN < MAX），且两个值都落在常量文档自己声明的
    /// 0..=139 占位带内（原 rr-bus lib 用例语义随迁）
    #[test]
    fn priority_window_is_nonempty_placeholder_band() {
        const BAND: core::ops::RangeInclusive<u32> = 0..=139; // 常量文档声明的占位带
        const {
            assert!(
                RR3_PRIORITY_MIN < RR3_PRIORITY_MAX,
                "占位窗口为空（MIN >= MAX）"
            );
        }
        assert!(
            BAND.contains(&RR3_PRIORITY_MIN) && BAND.contains(&RR3_PRIORITY_MAX),
            "占位值越出常量文档声明的 {BAND:?}：MIN={} MAX={}",
            RR3_PRIORITY_MIN,
            RR3_PRIORITY_MAX
        );
    }

    #[test]
    fn contract_version_is_passed_through() {
        assert_eq!(RR3_BUS_CONTRACT_VERSION, RR3_CONTRACT_SCHEMA_VERSION);
        assert_eq!(RR3_BUS_CONTRACT_VERSION, 1);
    }

    #[test]
    fn method_call_is_the_wire_name() {
        assert_eq!(RR3_METHOD_CALL, "rr3.call");
    }

    /// trace_id 直连语义：0 = 非代理；且 0 是广播号，不可能与"调用方 task"混淆
    #[test]
    fn trace_id_direct_is_zero_and_not_a_caller_task() {
        assert_eq!(RR3_TRACE_ID_DIRECT, 0);
        assert_eq!(
            rr3_task_kind(RR3_TRACE_ID_DIRECT),
            Rr3TaskKind::Broadcast,
            "0 已被广播占用，不可能被分配为调用方 task"
        );
    }

    /// 帧组装：payload_len 回填 + 全字段注入逐项落位（时钟是参数不是读取）
    #[test]
    fn frame_compose_fills_every_field_from_arguments() {
        let f = rr3_frame_compose(
            opcode::SEND,
            0,
            42,
            1_000_000,
            7,
            RR3_TASK_RRD,
            vec![1, 2, 3],
        );
        assert_eq!(f.header.opcode, opcode::SEND);
        assert_eq!(f.header.flags, 0);
        assert_eq!(f.header.trace_id, 42);
        assert_eq!(f.header.timestamp_ns, 1_000_000);
        assert_eq!(f.header.src_task, 7);
        assert_eq!(f.header.dst_task, RR3_TASK_RRD);
        assert_eq!(f.header.payload_len, 3, "payload_len 必须回填实际长度");
        assert_eq!(f.payload, vec![1, 2, 3]);
        // 空载荷也必须一致（心跳 {}/无载荷应答形状）
        let empty = rr3_frame_compose(opcode::SEND, 0, 0, 0, 1, RR3_TASK_RRD, Vec::new());
        assert_eq!(empty.header.payload_len, 0);
        assert!(empty.payload.is_empty());
    }

    /// Event 组装形状：SEND / trace_id=直连 / dst=rrd，与 rr-bus 客户端既有线上
    /// 形状逐字段一致（客户端改造后由同一函数产出，此用例钉形状守恒）
    #[test]
    fn event_compose_matches_client_wire_shape() {
        let payload = rr3_body_encode(
            Rr3PayloadType::Event,
            b"{\"topic\":\"rr3-t/x\"}",
        );
        let f = rr3_event_compose(77, 9, payload);
        assert_eq!(f.header.opcode, opcode::SEND);
        assert_eq!(f.header.flags, 0);
        assert_eq!(f.header.trace_id, RR3_TRACE_ID_DIRECT);
        assert_eq!(f.header.timestamp_ns, 77);
        assert_eq!(f.header.src_task, 9);
        assert_eq!(f.header.dst_task, RR3_TASK_RRD);
        let (ty, body) = rr_ipc::rr3_body_split(&f.payload).unwrap();
        assert_eq!(ty, Rr3PayloadType::Event);
        assert_eq!(body, b"{\"topic\":\"rr3-t/x\"}");
    }

    /// 组装产物可整帧字节化并与 golden 头区对拍（机制核切片的编译期+运行期双面之一）
    #[test]
    fn composed_frame_is_writable_and_matches_golden_header_size() {
        let f = rr3_event_compose(100, 7, rr3_body_encode(Rr3PayloadType::Event, b"{}"));
        let bytes = rr_ipc::rr3_frame_to_bytes(&f).unwrap();
        assert_eq!(bytes.len(), rr_ipc::RR3_IPC_HDR_SIZE + f.payload.len());
        assert_eq!(&bytes[..4], &[0x31, 0x45, 0x52, 0x41], "magic LE 线上字节");
    }
}
