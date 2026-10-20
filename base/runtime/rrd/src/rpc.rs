//! 模块 ↔ rrd 的上行 RPC：JSON-RPC 2.0（spec §5.0 + M3 的第 4 个方法）。
//!
//! 上行一律 `Rr3PayloadType::Request` + `dst_task = RR3_TASK_RRD`。
//! `rr3-runtime/system/heartbeat` 与 `rr3-runtime/module/state` 是 rrd 的**下行广播**，
//! 不是模块的上行心跳——模块要喂狗只能调 `rr3.heartbeat`。
//! `rr3.call` 是 M3 定型的模块间请求路由：rrd 按 params.topic 查被调方的
//! manifest `[rpc].exposes` 后定向转发，应答沿 trace_id 回路返回调用方
//! （rrd-rpc.md"trace_id 语义"）。
//! `rr3.ctl.*` 六方法是 F3 定型的控制查询（ADR-0010）：与模块方法同一解析器、
//! 同一白名单口径，但只对未注册的控制连接开放（分流在 conn 握手处）；
//! 0.0.4 批B append `rr3.ctl.counters`（运维计数读面，与 health 的心跳健康语义分开）、
//! `rr3.ctl.reboot`（系统重启，执行面带 cfg(linux)+env 双重门，缺省恒拒）。

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
    /// 且 sha 对"判果与 sha 记账一应答给出，判据源 = contracts.rs）。
    CtlStatus,
    CtlHealth,
    CtlLogs,
    CtlOta,
    CtlCounters,
    CtlReboot,
    CtlWiring,
    CtlContracts,
}

impl Rr3RpcMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Register => "rr3.register",
            Self::Heartbeat => "rr3.heartbeat",
            Self::Unregister => "rr3.unregister",
            // 方法名的唯一真值点在 rr-bus（模块侧客户端与 rrd 共用一份）
            Self::Call => rr_bus::RR3_METHOD_CALL,
            Self::CtlStatus => "rr3.ctl.status",
            Self::CtlHealth => "rr3.ctl.health",
            Self::CtlLogs => "rr3.ctl.logs",
            Self::CtlOta => "rr3.ctl.ota",
            Self::CtlCounters => "rr3.ctl.counters",
            Self::CtlReboot => "rr3.ctl.reboot",
            Self::CtlWiring => "rr3.ctl.wiring",
            Self::CtlContracts => "rr3.ctl.contracts",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "rr3.register" => Some(Self::Register),
            "rr3.heartbeat" => Some(Self::Heartbeat),
            "rr3.unregister" => Some(Self::Unregister),
            s if s == rr_bus::RR3_METHOD_CALL => Some(Self::Call),
            "rr3.ctl.status" => Some(Self::CtlStatus),
            "rr3.ctl.health" => Some(Self::CtlHealth),
            "rr3.ctl.logs" => Some(Self::CtlLogs),
            "rr3.ctl.ota" => Some(Self::CtlOta),
            "rr3.ctl.counters" => Some(Self::CtlCounters),
            "rr3.ctl.reboot" => Some(Self::CtlReboot),
            "rr3.ctl.wiring" => Some(Self::CtlWiring),
            "rr3.ctl.contracts" => Some(Self::CtlContracts),
            _ => None,
        }
    }

    /// 是否控制查询方法（ADR-0010 裁定 3 的分流判据）。conn 的握手分流经它；
    /// 已注册连接的上行拒绝用显式 OR 臂（带守卫的臂不计入穷尽性），两处由
    /// 编译器共同看护：新增 ctl 变体漏改任何一处都会变红。
    pub fn is_ctl(&self) -> bool {
        matches!(
            self,
            Self::CtlStatus
                | Self::CtlHealth
                | Self::CtlLogs
                | Self::CtlOta
                | Self::CtlCounters
                | Self::CtlReboot
                | Self::CtlWiring
                | Self::CtlContracts
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3RpcRequest {
    pub id: u64,
    pub method: Rr3RpcMethod,
    pub params: Value,
}

/// 解析上行 RPC。错误码走契约的 AIRY_E* 子集，不自造。
pub fn rr3_rpc_parse_request(payload: &[u8]) -> Result<Rr3RpcRequest, (i32, String)> {
    let text = std::str::from_utf8(payload).map_err(|e| {
        (
            Rr3StatusCode::Invalid as i32,
            format!("payload 非 UTF-8: {e}"),
        )
    })?;
    let v: Value = serde_json::from_str(text).map_err(|e| {
        (
            Rr3StatusCode::Invalid as i32,
            format!("JSON-RPC 解析失败: {e}"),
        )
    })?;
    if v["jsonrpc"] != "2.0" {
        return Err((
            Rr3StatusCode::Invalid as i32,
            "jsonrpc 必须等于 \"2.0\"".to_string(),
        ));
    }
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

fn param_str(params: &Value, key: &str) -> Result<String, (i32, String)> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            (
                Rr3StatusCode::Invalid as i32,
                format!("缺 params.{key}（要求非空字符串）"),
            )
        })
}

pub fn rr3_rpc_manifest_param(params: &Value) -> Result<String, (i32, String)> {
    param_str(params, "manifest")
}

pub fn rr3_rpc_topic_param(params: &Value) -> Result<String, (i32, String)> {
    param_str(params, "topic")
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
pub fn rr3_rpc_heartbeat_counters(params: &Value) -> Option<BTreeMap<String, u64>> {
    let obj = params.get("counters")?.as_object()?;
    let mut out = BTreeMap::new();
    for (k, v) in obj {
        if k.is_empty() {
            continue;
        }
        if let Some(n) = v.as_u64() {
            out.insert(k.clone(), n);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub fn rr3_rpc_build_result(id: u64, result: &Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":id,"result":result}))
        .expect("serde_json 序列化自身构造的值不会失败")
}

pub fn rr3_rpc_build_error(id: u64, code: Rr3StatusCode, message: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code as i32, "message": message}
    }))
    .expect("serde_json 序列化自身构造的值不会失败")
}

pub fn rr3_rpc_status_for_register(e: &Rr3RegisterError) -> Rr3StatusCode {
    match e {
        Rr3RegisterError::Duplicate { .. } | Rr3RegisterError::Capacity { .. } => {
            Rr3StatusCode::Busy
        }
        Rr3RegisterError::ReservedName { .. } => Rr3StatusCode::Permission,
    }
}

pub fn rr3_rpc_status_for_publish_deny(e: &Rr3PublishDeny) -> Rr3StatusCode {
    match e {
        Rr3PublishDeny::ReservedNamespace { .. } | Rr3PublishDeny::ForeignPrefix { .. } => {
            Rr3StatusCode::Permission
        }
        Rr3PublishDeny::NotRegistered { .. } => Rr3StatusCode::Invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bytes(v: Value) -> Vec<u8> {
        v.to_string().into_bytes()
    }

    /// 三个方法各自解析成功，id 与 params 原样带上
    #[test]
    fn parses_three_upstream_methods() {
        for m in ["rr3.register", "rr3.heartbeat", "rr3.unregister"] {
            let got = rr3_rpc_parse_request(&bytes(
                json!({"jsonrpc":"2.0","id":7,"method":m,"params":{}}),
            ))
            .unwrap_or_else(|e| panic!("{m} 应能解析: {e:?}"));
            assert_eq!(got.id, 7);
            assert_eq!(got.method.as_str(), m);
        }
        assert_eq!(Rr3RpcMethod::Register.as_str(), "rr3.register");
    }

    /// 畸形 JSON-RPC：一律 Invalid，文案指出缺哪一项
    #[test]
    fn rejects_malformed_request() {
        for (v, hint) in [
            (
                json!({"jsonrpc":"1.0","id":1,"method":"rr3.heartbeat"}),
                "jsonrpc",
            ),
            (json!({"id":1,"method":"rr3.heartbeat"}), "jsonrpc"),
            (json!({"jsonrpc":"2.0","method":"rr3.heartbeat"}), "id"),
            (
                json!({"jsonrpc":"2.0","id":"x","method":"rr3.heartbeat"}),
                "id",
            ),
            (json!({"jsonrpc":"2.0","id":1}), "method"),
        ] {
            // `bytes` 按值收 Value，而下一行的断言消息还要用 `{v}` 指名是哪一样，
            // 故此处传克隆（不克隆就是 E0382 borrow-after-move）。
            let (code, msg) = rr3_rpc_parse_request(&bytes(v.clone())).expect_err("畸形必须拒");
            assert_eq!(code, Rr3StatusCode::Invalid as i32, "{v}");
            assert!(msg.contains(hint), "文案缺 {hint}: {msg}");
        }
    }

    /// 未知方法必须拒（不许当 no-op 放行）
    #[test]
    fn rejects_unknown_method() {
        let (code, msg) = rr3_rpc_parse_request(&bytes(
            json!({"jsonrpc":"2.0","id":1,"method":"rr3.shUTDOWN","params":{}}),
        ))
        .expect_err("未知方法必须拒");
        assert_eq!(code, Rr3StatusCode::Invalid as i32);
        assert!(msg.contains("未知方法 rr3.shUTDOWN"), "{msg}");
    }

    /// 坏 payload（非 UTF-8 / 非 JSON / 空）一律 Invalid，不 panic
    #[test]
    fn rejects_bad_payloads() {
        for bad in [vec![0xFFu8, 0xFE], b"{".to_vec(), Vec::new()] {
            let (code, _) = rr3_rpc_parse_request(&bad).expect_err("坏 payload 必须拒");
            assert_eq!(code, Rr3StatusCode::Invalid as i32);
        }
    }

    /// register 的 manifest 文本取值：缺参 / 类型错 / 空串都拒
    #[test]
    fn manifest_param_is_strict() {
        assert_eq!(
            rr3_rpc_manifest_param(&json!({"name":"rr3-sima","manifest":"[manifest]\n"})).unwrap(),
            "[manifest]\n"
        );
        for v in [
            json!({"name":"rr3-sima"}),
            json!({"manifest":5}),
            json!({"manifest":""}),
        ] {
            let (code, msg) = rr3_rpc_manifest_param(&v).expect_err("{v} 必须拒");
            assert_eq!(code, Rr3StatusCode::Invalid as i32);
            assert!(msg.contains("params.manifest"), "{msg}");
        }
    }

    /// Event payload 的 topic 取值同样严格（Task 8 的发布面靠它）
    #[test]
    fn topic_param_is_strict() {
        assert_eq!(
            rr3_rpc_topic_param(&json!({"topic":"rr3-sima/ping/state"})).unwrap(),
            "rr3-sima/ping/state"
        );
        assert!(rr3_rpc_topic_param(&json!({})).is_err());
        assert!(rr3_rpc_topic_param(&json!({"topic": 3})).is_err());
    }

    /// 第 4 个上行方法：rr3.call 能解析（方法名走 rr-bus 的唯一真值点）
    #[test]
    fn parses_call_method() {
        let got = rr3_rpc_parse_request(&bytes(json!({
            "jsonrpc":"2.0","id":3,"method":rr_bus::RR3_METHOD_CALL,
            "params":{"topic":"rr3-interact/tts/text","args":{"text":"hi"}}
        })))
        .expect("rr3.call 应能解析");
        assert_eq!(got.method, Rr3RpcMethod::Call);
        assert_eq!(Rr3RpcMethod::Call.as_str(), "rr3.call");
    }

    /// F3（ADR-0010）：ctl 方法走同一解析器，is_ctl 的判据与方法面一致
    /// （0.0.4 批B 起 `rr3.ctl.counters`/`rr3.ctl.reboot` 入列；批N4 件九笔4 起
    /// `rr3.ctl.wiring` 入列；批N5 件五笔4 起 `rr3.ctl.contracts` 入列，白名单八 ctl）
    #[test]
    fn parses_ctl_methods_and_flags_them() {
        for m in [
            "rr3.ctl.status",
            "rr3.ctl.health",
            "rr3.ctl.logs",
            "rr3.ctl.ota",
            "rr3.ctl.counters",
            "rr3.ctl.reboot",
            "rr3.ctl.wiring",
            "rr3.ctl.contracts",
        ] {
            let got = rr3_rpc_parse_request(&bytes(
                json!({"jsonrpc":"2.0","id":5,"method":m,"params":{}}),
            ))
            .unwrap_or_else(|e| panic!("{m} 应能解析: {e:?}"));
            assert_eq!(got.method.as_str(), m);
            assert!(got.method.is_ctl(), "{m} 必须被 is_ctl 判为控制方法");
        }
        assert!(!Rr3RpcMethod::Register.is_ctl());
        assert!(!Rr3RpcMethod::Heartbeat.is_ctl());
        assert!(!Rr3RpcMethod::Unregister.is_ctl());
        assert!(!Rr3RpcMethod::Call.is_ctl());
    }

    /// 心跳可选 counters 字段的形状拆包（0.0.4 批B）：缺省/非对象/空 → None；
    /// 合法键值逐对收；非法值与空键静默跳过（脏拍不折心跳错误）
    #[test]
    fn heartbeat_counters_param_is_optional_and_lenient() {
        assert!(rr3_rpc_heartbeat_counters(&json!({})).is_none(), "缺省字段");
        assert!(
            rr3_rpc_heartbeat_counters(&json!({"counters": {}})).is_none(),
            "空映射等价无上报"
        );
        assert!(
            rr3_rpc_heartbeat_counters(&json!({"counters": 3})).is_none(),
            "非对象忽略"
        );
        let got = rr3_rpc_heartbeat_counters(&json!({
            "counters": {"asr_ok": 2, "asr_fail": 0}
        }))
        .expect("合法映射应拆出");
        assert_eq!(got["asr_ok"], 2);
        assert_eq!(got["asr_fail"], 0, "0 也是合法计数值");
        let got = rr3_rpc_heartbeat_counters(&json!({
            "counters": {"ok": 1, "bad": "x", "": 5, "neg": -3}
        }))
        .expect("有合法键值即拆出");
        assert_eq!(got.len(), 1, "非法键值静默跳过");
        assert_eq!(got["ok"], 1);
    }

    /// rr3.call 的参数：topic 必填，args 缺省 Null 且原样透传
    #[test]
    fn call_params_need_topic_and_args_default_to_null() {
        let (topic, args) =
            rr3_rpc_call_params(&json!({"topic":"t/a","args":{"text":"hi"}})).expect("应有参数");
        assert_eq!(topic, "t/a");
        assert_eq!(args["text"], "hi");
        let (topic, args) = rr3_rpc_call_params(&json!({"topic":"t/a"})).expect("args 可省");
        assert_eq!(topic, "t/a");
        assert_eq!(args, Value::Null);
        let (code, msg) = rr3_rpc_call_params(&json!({"args":{}})).expect_err("缺 topic 必须拒");
        assert_eq!(code, Rr3StatusCode::Invalid as i32);
        assert!(msg.contains("params.topic"), "{msg}");
    }

    /// 应答形状：JSON-RPC 2.0 + 同 id + result；注册回带的 src_task 是唯一的 task 来源
    #[test]
    fn result_shape_is_jsonrpc2() {
        let out = rr3_rpc_build_result(9, &json!({"src_task": 2}));
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 9);
        assert_eq!(v["result"]["src_task"], 2);
        assert!(v.get("error").is_none(), "成功应答不得带 error 字段");
    }

    /// 错误应答用契约里的 AIRY_E* 判别值（不许自造码）
    #[test]
    fn error_shape_uses_contract_status_codes() {
        let out = rr3_rpc_build_error(9, Rr3StatusCode::Busy, "已在册");
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(
            v["error"]["code"],
            Rr3StatusCode::Busy as i32,
            "AIRY_EBUSY = -16"
        );
        assert_eq!(v["error"]["message"], "已在册");
        assert!(v.get("result").is_none());

        let p = rr3_rpc_build_error(0, Rr3StatusCode::Permission, "越权");
        assert_eq!(
            serde_json::from_slice::<Value>(&p).unwrap()["error"]["code"],
            Rr3StatusCode::Permission as i32,
            "AIRY_EPERM = -22"
        );
    }

    /// 注册错误到状态码的映射（重复/超额=忙，保留名=权限）
    #[test]
    fn register_error_maps_to_status_code() {
        use crate::registry::Rr3RegisterError;
        assert_eq!(
            rr3_rpc_status_for_register(&Rr3RegisterError::Duplicate {
                name: "a".into(),
                src_task: 2
            }),
            Rr3StatusCode::Busy
        );
        assert_eq!(
            rr3_rpc_status_for_register(&Rr3RegisterError::Capacity { limit: 16 }),
            Rr3StatusCode::Busy
        );
        assert_eq!(
            rr3_rpc_status_for_register(&Rr3RegisterError::ReservedName {
                name: "rr3-runtime".into()
            }),
            Rr3StatusCode::Permission
        );
    }

    /// 发布拒绝到状态码：三种拒法都是权限/非法，不是忙
    #[test]
    fn publish_deny_maps_to_status_code() {
        use crate::registry::Rr3PublishDeny;
        assert_eq!(
            rr3_rpc_status_for_publish_deny(&Rr3PublishDeny::ReservedNamespace {
                topic: "t".into()
            }),
            Rr3StatusCode::Permission
        );
        assert_eq!(
            rr3_rpc_status_for_publish_deny(&Rr3PublishDeny::ForeignPrefix {
                topic: "t".into(),
                owner: "o".into(),
                sender: "s".into()
            }),
            Rr3StatusCode::Permission
        );
        assert_eq!(
            rr3_rpc_status_for_publish_deny(&Rr3PublishDeny::NotRegistered { topic: "t".into() }),
            Rr3StatusCode::Invalid
        );
    }
}
