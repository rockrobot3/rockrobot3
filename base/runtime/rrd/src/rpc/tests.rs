// SPDX-License-Identifier: Apache-2.0
//! rpc 单测（文件头：本文件只放测试，生产面在 `rpc.rs`）。
//!
//! 拆分动因：`rpc.rs` 553 行、内联测试自 264 行起占近半，先按仓内同形搬出。

use super::*;
use serde_json::json;

fn bytes(v: Value) -> Vec<u8> {
    v.to_string().into_bytes()
}

/// 三个方法各自解析成功，id 与 params 原样带上
#[test]
fn parses_three_upstream_methods() {
    for m in ["rr3.register", "rr3.heartbeat", "rr3.unregister"] {
        let got = rr3_rpc_parse(&bytes(
            json!({"jsonrpc":"2.0","id":7,"method":m,"params":{}}),
        ))
        .unwrap_or_else(|e| panic!("{m} 应能解析: {e:?}"));
        assert_eq!(got.id, 7);
        assert_eq!(got.method.as_str(), m);
    }
    assert_eq!(Rr3RpcMethod::Register.as_str(), "rr3.register");
}

/// 名字表覆盖全部变体：长度对齐、判别式连续（末变体下标 = 表长 - 1）
#[test]
fn name_table_covers_all_variants() {
    assert_eq!(RR3_RPC_NAMES.len(), 13);
    assert_eq!(Rr3RpcMethod::CtlDomainSet as usize, RR3_RPC_NAMES.len() - 1);
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
        let (code, msg) = rr3_rpc_parse(&bytes(v.clone())).expect_err("畸形必须拒");
        assert_eq!(code, Rr3StatusCode::Invalid as i32, "{v}");
        assert!(msg.contains(hint), "文案缺 {hint}: {msg}");
    }
}

/// 未知方法必须拒（不许当 no-op 放行）
#[test]
fn rejects_unknown_method() {
    let (code, msg) = rr3_rpc_parse(&bytes(
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
        let (code, _) = rr3_rpc_parse(&bad).expect_err("坏 payload 必须拒");
        assert_eq!(code, Rr3StatusCode::Invalid as i32);
    }
}

/// register 的 manifest 文本取值：缺参 / 类型错 / 空串都拒
#[test]
fn manifest_param_is_strict() {
    assert_eq!(
        rr3_rpc_manifest(&json!({"name":"rr3-sima","manifest":"[manifest]\n"})).unwrap(),
        "[manifest]\n"
    );
    for v in [
        json!({"name":"rr3-sima"}),
        json!({"manifest":5}),
        json!({"manifest":""}),
    ] {
        let (code, msg) = rr3_rpc_manifest(&v).expect_err("{v} 必须拒");
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
    let got = rr3_rpc_parse(&bytes(json!({
        "jsonrpc":"2.0","id":3,"method":rr_bus::RR3_METHOD_CALL,
        "params":{"topic":"rr3-interact/tts/text","args":{"text":"hi"}}
    })))
    .expect("rr3.call 应能解析");
    assert_eq!(got.method, Rr3RpcMethod::Call);
    assert_eq!(Rr3RpcMethod::Call.as_str(), "rr3.call");
}

/// F3（ADR-0010）：ctl 方法走同一解析器，is_ctl 的判据与方法面一致
/// （0.0.4 批B 起 `rr3.ctl.counters`/`rr3.ctl.reboot` 入列；批N4 件九笔4 起
/// `rr3.ctl.wiring` 入列；批N5 件五笔4 起 `rr3.ctl.contracts` 入列；批N22 件一
/// 起 `rr3.ctl.domain.set` 入列，白名单九 ctl）
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
        "rr3.ctl.domain.set",
    ] {
        let got = rr3_rpc_parse(&bytes(
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
    // ctl 在册表 ↔ 名字真值互证：表内每变体经 as_str 反查必回本变体
    // （表漏列/名漂移在此一处即红，不靠人工对齐两张清单）
    for m in RR3_CTL_METHODS {
        assert!(m.is_ctl(), "{m:?} 在册必判 ctl");
        assert_eq!(
            Rr3RpcMethod::parse(m.as_str()),
            Some(m),
            "{m:?} 名字反查必须回到本变体"
        );
    }
}

/// 心跳可选 counters 字段的形状拆包（0.0.4 批B）：缺省/非对象/空 → None；
/// 合法键值逐对收；非法值与空键静默跳过（脏拍不折心跳错误）
#[test]
fn heartbeat_counters_param_is_optional_and_lenient() {
    assert!(rr3_rpc_counters(&json!({})).is_none(), "缺省字段");
    assert!(
        rr3_rpc_counters(&json!({"counters": {}})).is_none(),
        "空映射等价无上报"
    );
    assert!(
        rr3_rpc_counters(&json!({"counters": 3})).is_none(),
        "非对象忽略"
    );
    let got = rr3_rpc_counters(&json!({
        "counters": {"asr_ok": 2, "asr_fail": 0}
    }))
    .expect("合法映射应拆出");
    assert_eq!(got["asr_ok"], 2);
    assert_eq!(got["asr_fail"], 0, "0 也是合法计数值");
    let got = rr3_rpc_counters(&json!({
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
        rr3_rpc_reg_status(&Rr3RegisterError::Duplicate {
            name: "a".into(),
            src_task: 2
        }),
        Rr3StatusCode::Busy
    );
    assert_eq!(
        rr3_rpc_reg_status(&Rr3RegisterError::Capacity { limit: 16 }),
        Rr3StatusCode::Busy
    );
    assert_eq!(
        rr3_rpc_reg_status(&Rr3RegisterError::ReservedName {
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
        rr3_rpc_deny_status(&Rr3PublishDeny::ReservedNamespace { topic: "t".into() }),
        Rr3StatusCode::Permission
    );
    assert_eq!(
        rr3_rpc_deny_status(&Rr3PublishDeny::ForeignPrefix {
            topic: "t".into(),
            owner: "o".into(),
            sender: "s".into()
        }),
        Rr3StatusCode::Permission
    );
    assert_eq!(
        rr3_rpc_deny_status(&Rr3PublishDeny::NotRegistered { topic: "t".into() }),
        Rr3StatusCode::Invalid
    );
}

/// topic 借用取法与拥有版同一判据：同值、同错误文案、空串/非串同拒
/// （Event 转发热路径用借用版省一次 String 分配，口径不许分家）
#[test]
fn topic_ref_and_owned_param_share_one_judgement() {
    let ok = json!({"topic": "rr3-sense/capture/dump"});
    assert_eq!(rr3_rpc_topic_ref(&ok).unwrap(), "rr3-sense/capture/dump");
    assert_eq!(rr3_rpc_topic_param(&ok).unwrap(), "rr3-sense/capture/dump");
    for bad in [json!({}), json!({"topic": ""}), json!({"topic": 1})] {
        let e_ref = rr3_rpc_topic_ref(&bad).unwrap_err();
        let e_own = rr3_rpc_topic_param(&bad).unwrap_err();
        assert_eq!(e_ref, e_own, "两取法必须同一判据: {bad}");
        assert_eq!(e_ref.0, Rr3StatusCode::Invalid as i32);
        assert!(e_ref.1.contains("params.topic"), "{}", e_ref.1);
    }
}
