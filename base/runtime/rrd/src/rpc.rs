// SPDX-License-Identifier: Apache-2.0
//! 模块 ↔ rrd 的上行 RPC：JSON-RPC 2.0（spec §5.0 + M3 的第 4 个方法）。
//!
//! 上行一律 `Rr3PayloadType::Request` + `dst_task = RR3_TASK_RRD`。
//! `rr3-runtime/system/heartbeat` 与 `rr3-runtime/module/state` 是 rrd 的**下行广播**，
//! 不是模块的上行心跳——模块要喂狗只能调 `rr3.heartbeat`。
//! `rr3.call` 是 M3 定型的模块间请求路由：rrd 按 params.topic 查被调方的
//! manifest `[rpc].exposes` 后定向转发，应答沿 trace_id 回路返回调用方
//! （rrd-rpc.md"trace_id 语义"）。
//! `rr3.ctl.*` 九方法是 F3 定型的控制查询（ADR-0010）：与模块方法同一解析器、
//! 同一白名单口径，但只对未注册的控制连接开放（分流在 conn 握手处）；
//! 0.0.4 批B append `rr3.ctl.counters`（运维计数读面，与 health 的心跳健康语义分开）、
//! `rr3.ctl.reboot`（系统重启，执行面带 cfg(linux)+env 双重门，缺省恒拒）；
//! 批N4/N5/N22 再 append `rr3.ctl.wiring` / `rr3.ctl.contracts` / `rr3.ctl.domain.set`
//! （装配快照、契约审计、策略量写口，判据源各见变体注释）。

use rr_ipc::Rr3StatusCode;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::registry::{Rr3PublishDeny, Rr3RegisterError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3RpcMethod {
    Register,
    Heartbeat,
    Unregister,
    Call,
    /// rr3.ctl.* 方法（ADR-0010）：控制查询，只对**未注册**的控制连接开放
    /// （conn 握手在方法分流处把 ctl 旁支接走）；已注册模块连接上行一律拒。
    /// 0.0.4 批B 增 `rr3.ctl.counters`/`rr3.ctl.reboot`；
    /// 批N4 件九笔4 增 `rr3.ctl.wiring`（装配快照读口，`[pins]×wiring` 全交叉
    /// 判果与四路底账一应答给出，判据源 = assembly.rs）；
    /// 批N5 件五笔4 增 `rr3.ctl.contracts`（契约完整性审计读口，Z-4"四册齐全
    /// 且 sha 对"判果与 sha 记账一应答给出，判据源 = contracts.rs）；
    /// 批N22 件一增 `rr3.ctl.domain.set`（装配侧策略量写口，敞口 15 写面半边：
    /// armed/severe_mv/current_ceil_ma 三写口，confirm 门＋证据引用四道核＋
    /// 装载链预核＋原子落盘"下次 boot 生效"，判据源 = domain_write.rs）。
    CtlStatus,
    CtlHealth,
    CtlLogs,
    CtlOta,
    CtlCounters,
    CtlReboot,
    CtlWiring,
    CtlContracts,
    CtlDomainSet,
}

/// 枚举 → 方法名（判别式即下标，与变体声明次序一一对应；`Call` 仍取 rr-bus 的
/// 唯一真值常量）。变体增减时长度断言与 `CtlDomainSet` 下标同时红。
const RR3_RPC_NAMES: [&str; 13] = [
    "rr3.register",
    "rr3.heartbeat",
    "rr3.unregister",
    rr_bus::RR3_METHOD_CALL,
    "rr3.ctl.status",
    "rr3.ctl.health",
    "rr3.ctl.logs",
    "rr3.ctl.ota",
    "rr3.ctl.counters",
    "rr3.ctl.reboot",
    "rr3.ctl.wiring",
    "rr3.ctl.contracts",
    "rr3.ctl.domain.set",
];

/// ctl 方法族在册表（ADR-0010）：解析面的 ctl 反查与 [`Rr3RpcMethod::is_ctl`]
/// 的成员判据共用本表；方法名的唯一真值在 [`RR3_RPC_NAMES`]（本表只列变体、
/// 不携字面名，名字漂移只在一处发生）。
const RR3_CTL_METHODS: [Rr3RpcMethod; 9] = [
    Rr3RpcMethod::CtlStatus,
    Rr3RpcMethod::CtlHealth,
    Rr3RpcMethod::CtlLogs,
    Rr3RpcMethod::CtlOta,
    Rr3RpcMethod::CtlCounters,
    Rr3RpcMethod::CtlReboot,
    Rr3RpcMethod::CtlWiring,
    Rr3RpcMethod::CtlContracts,
    Rr3RpcMethod::CtlDomainSet,
];

impl Rr3RpcMethod {
    pub fn as_str(&self) -> &'static str {
        RR3_RPC_NAMES[*self as usize]
    }

    /// 名字 → 变体：核心四方法直配，ctl 族按 [`RR3_CTL_METHODS`] 反查
    /// [`Self::as_str`]（解析面不另立 ctl 字面表——两张字面表并存必漂移）。
    fn parse(s: &str) -> Option<Self> {
        // 核心三方法查表（字面名 → 变体），ctl 族按 [`RR3_CTL_METHODS`] 反查
        const CORE: [(&str, Rr3RpcMethod); 3] = [
            ("rr3.register", Rr3RpcMethod::Register),
            ("rr3.heartbeat", Rr3RpcMethod::Heartbeat),
            ("rr3.unregister", Rr3RpcMethod::Unregister),
        ];
        if let Some((_, m)) = CORE.iter().find(|(k, _)| *k == s) {
            return Some(*m);
        }
        if s == rr_bus::RR3_METHOD_CALL {
            return Some(Self::Call);
        }
        RR3_CTL_METHODS.iter().copied().find(|m| m.as_str() == s)
    }

    /// 是否控制查询方法（ADR-0010 裁定 3 的分流判据）。成员判据与解析面共用
    /// [`RR3_CTL_METHODS`]：ctl 族增列一处，解析与分流同时生效。conn 的握手分流
    /// 经它；已注册连接的上行拒绝用显式 OR 臂（带守卫的臂不计入穷尽性）。
    pub fn is_ctl(&self) -> bool {
        RR3_CTL_METHODS.contains(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3RpcRequest {
    pub id: u64,
    pub method: Rr3RpcMethod,
    pub params: Value,
}

/// 解析上行 RPC。错误码走契约的 AIRY_E* 子集，不自造。
pub fn rr3_rpc_parse(payload: &[u8]) -> Result<Rr3RpcRequest, (i32, String)> {
    let v: Value = rr3_rpc_body(payload)?;
    rr3_rpc_ver(&v)?;
    let Some(id) = v["id"].as_u64() else {
        return Err((
            Rr3StatusCode::Invalid as i32,
            "缺 id（要求无符号整数）".to_string(),
        ));
    };
    let Some(method_str) = v["method"].as_str() else {
        return Err((
            Rr3StatusCode::Invalid as i32,
            "缺 method（要求字符串）".to_string(),
        ));
    };
    let Some(method) = Rr3RpcMethod::parse(method_str) else {
        return Err((
            Rr3StatusCode::Invalid as i32,
            format!("未知方法 {method_str}"),
        ));
    };
    Ok(Rr3RpcRequest {
        id,
        method,
        params: v.get("params").cloned().unwrap_or(Value::Null),
    })
}

/// 协议版本门：jsonrpc 必须等于 "2.0"。
fn rr3_rpc_ver(v: &Value) -> Result<(), (i32, String)> {
    if v["jsonrpc"] == "2.0" {
        return Ok(());
    }
    Err((
        Rr3StatusCode::Invalid as i32,
        "jsonrpc 必须等于 \"2.0\"".to_string(),
    ))
}

/// 传输层解码：UTF-8 文本 → JSON 值（两步形状错都折 Invalid 与逐字文案）。
fn rr3_rpc_body(payload: &[u8]) -> Result<Value, (i32, String)> {
    let text = std::str::from_utf8(payload).map_err(|e| {
        (
            Rr3StatusCode::Invalid as i32,
            format!("payload 非 UTF-8: {e}"),
        )
    })?;
    serde_json::from_str(text).map_err(|e| {
        (
            Rr3StatusCode::Invalid as i32,
            format!("JSON-RPC 解析失败: {e}"),
        )
    })
}

/// 字符串参数（借用取法）：形状判据单点在 `param_ref`，拥有版 `param_str` 只
/// 是它在需要所有权处的薄包装（Event 转发每帧取一次 topic，借用即省一次分配）。
fn param_ref<'a>(params: &'a Value, key: &str) -> Result<&'a str, (i32, String)> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            (
                Rr3StatusCode::Invalid as i32,
                format!("缺 params.{key}（要求非空字符串）"),
            )
        })
}

fn param_str(params: &Value, key: &str) -> Result<String, (i32, String)> {
    param_ref(params, key).map(str::to_owned)
}

pub fn rr3_rpc_manifest(params: &Value) -> Result<String, (i32, String)> {
    param_str(params, "manifest")
}

pub fn rr3_rpc_topic_param(params: &Value) -> Result<String, (i32, String)> {
    param_str(params, "topic")
}

/// `params.topic` 的借用取法（Event 转发热路径专用：每帧一条，不复制字符串）。
pub fn rr3_rpc_topic_ref(params: &Value) -> Result<&str, (i32, String)> {
    param_ref(params, "topic")
}

/// `rr3.call` 的参数形状：`topic` 必填非空字符串，`args` 可选（缺省 `Null`）。
/// 被调方收到的转发体里 params 原样带过去——这里的解析只服务 rrd 的**路由**判定
/// （查 callee 用 topic），不重写调用方的参数。
pub fn rr3_rpc_call_params(params: &Value) -> Result<(String, Value), (i32, String)> {
    let topic = rr3_rpc_topic_param(params)?;
    let args = params.get("args").cloned().unwrap_or(Value::Null);
    Ok((topic, args))
}

/// `rr3.heartbeat` 的可选 `counters` 字段（0.0.4 批B，Q1 裁定）：`{string: u64}` 映射，
/// 模块自报的运维计数。取值口径：
/// - 字段缺省 / 非对象 / 空对象 → `None`（无上报，旧模块兼容，线上形状 `{}`）；
/// - 合法键值（非空字符串键 + `as_u64`）逐对收；**非法值静默跳过**——计数是运维账，
///   脏键值不值得让一拍心跳折错误（心跳失败 = 模块 exit，客户端侧语义太重）。
///   聚合与键归属归 conn/registry，这里只做形状拆包。
pub fn rr3_rpc_counters(params: &Value) -> Option<BTreeMap<String, u64>> {
    let obj = params.get("counters")?.as_object()?;
    let mut out = BTreeMap::new();
    for (k, v) in obj.iter().filter(|(k, _)| !k.is_empty()) {
        if let Some(n) = v.as_u64() {
            out.insert(k.clone(), n);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// JSON-RPC 2.0 应答帧。`Value` 的 `Display` 输出紧凑 JSON 且不返回错误
/// （`to_vec` 的错误面对 `Value` 不可达）——用 Display 让"序列化不可失败"
/// 由类型保证，而不是靠 expect 断言。
pub fn rr3_rpc_build_result(id: u64, result: &Value) -> Vec<u8> {
    serde_json::json!({"jsonrpc":"2.0","id":id,"result":result})
        .to_string()
        .into_bytes()
}

pub fn rr3_rpc_build_error(id: u64, code: Rr3StatusCode, message: &str) -> Vec<u8> {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code as i32, "message": message}
    })
    .to_string()
    .into_bytes()
}

pub fn rr3_rpc_reg_status(e: &Rr3RegisterError) -> Rr3StatusCode {
    match e {
        Rr3RegisterError::Duplicate { .. } | Rr3RegisterError::Capacity { .. } => {
            Rr3StatusCode::Busy
        }
        Rr3RegisterError::ReservedName { .. } => Rr3StatusCode::Permission,
    }
}

pub fn rr3_rpc_deny_status(e: &Rr3PublishDeny) -> Rr3StatusCode {
    match e {
        Rr3PublishDeny::ReservedNamespace { .. } | Rr3PublishDeny::ForeignPrefix { .. } => {
            Rr3StatusCode::Permission
        }
        Rr3PublishDeny::NotRegistered { .. } => Rr3StatusCode::Invalid,
    }
}

#[cfg(test)]
mod tests;
