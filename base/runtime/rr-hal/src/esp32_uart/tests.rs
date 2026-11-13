//! esp32_uart 的单测（自同名单测模块原样抽出，判据不变）。

use super::*;
// 协议件（信封编码/载荷类型）与直连 trace：生产面已收拢到 rr-periph-proto，
// 测试侧显式取用（判据不变）。
use rr_ipc::{rr3_body_encode, Rr3PayloadType};
use rr_periph_proto::consts::RR3_TRACE_DIRECT;

/// 轴 env 取值裁决：两档内放行，trim 宽容；其余全拒（落缺省 pan 的正门）。
#[test]
fn periph_axis_parse_accepts_pan_tilt_only() {
    assert_eq!(rr3_axis_parse("pan"), Some("pan"));
    assert_eq!(rr3_axis_parse("tilt"), Some("tilt"));
    assert_eq!(rr3_axis_parse(" pan "), Some("pan"));
    for bad in ["", "PAN", "roll", "yaw", "1"] {
        assert_eq!(rr3_axis_parse(bad), None, "{bad}");
    }
}

/// 波特率 env 取值裁决（批D②）：档位表内放行、trim 宽容；档位外/垃圾全拒
/// （真值单点在 rr-ipc 档位表，本侧只做解析）。
#[test]
fn periph_baud_parse_accepts_whitelist_only() {
    assert_eq!(rr3_baud_parse("115200"), Some(115200));
    assert_eq!(rr3_baud_parse(" 921600 "), Some(921600));
    assert_eq!(rr3_baud_parse("9600"), Some(9600));
    // 1.5M/2M 高速档（2026-10-30 提档实验进白名单；1.5M 为当前定档）
    assert_eq!(rr3_baud_parse("1500000"), Some(1_500_000));
    assert_eq!(rr3_baud_parse("2000000"), Some(2_000_000));
    for bad in ["", "0", "230400", "921600x", "-1"] {
        assert_eq!(rr3_baud_parse(bad), None, "{bad}");
    }
}

/// 请求帧信封形状（B批 b1 起 `rr3_bridge_call` 是 rr3.call 的生产单点，
/// 原 `rr3_bridge_request_body` 已并入）：method 恒 rr3.call、主题在
/// params.topic、args 原样内嵌（conn.rs 转发形状的分发依据；漂移此处即红）。
#[test]
fn bridge_request_body_is_rr3_call_with_topic_in_params() {
    let frame = rr3_bridge_call(7, RR3_SERVO_TOPIC, &json!({ "angle_deg": 90 }));
    assert_eq!(frame.header.src_task, RR3_TASK_HOST);
    assert_eq!(frame.header.dst_task, RR3_PERIPH_TASK);
    assert_eq!(frame.header.trace_id, RR3_TRACE_DIRECT);
    let (ty, body) = rr3_body_split(&frame.payload).expect("payload 前缀合法");
    assert_eq!(ty, Rr3PayloadType::Request);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], 7);
    assert_eq!(v["method"], "rr3.call");
    assert_eq!(v["params"]["topic"], RR3_SERVO_TOPIC);
    assert_eq!(v["params"]["args"]["angle_deg"], 90);
}

/// method 面请求体信封：method/params 原样内嵌（rr3.set_bindings 的协议形状
/// 锚——载荷住 params.args，rr-ipc bindings 模块定形）。
#[test]
fn bridge_request_body_params_is_generic_method_envelope() {
    let s = rr3_bridge_req_body(9, "rr3.set_bindings", &json!({ "args": {} }));
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], 9);
    assert_eq!(v["method"], "rr3.set_bindings");
    assert_eq!(v["params"]["args"], json!({}));
}

/// 板侧绑定载荷（纯函数）：板侧键 Some 者逐键入载荷、值保持字符串（描述符同形
/// 纪律）；全缺 = None（无板侧配置可注）；部分注入只带已有键；cred_store
/// 字符串键随键（批N17 件二）。
#[test]
fn board_bindings_payload_carries_string_values_only() {
    let mut b = Rr3HalBindings::default();
    assert_eq!(rr3_board_payload(&b), None, "三键全缺 = 无载荷");
    b.i2c_sda = Some(21);
    b.adc_rail_pin = Some(34);
    let p = rr3_board_payload(&b).expect("有键必须有载荷");
    assert_eq!(p["i2c_sda"], "21", "值保持字符串（描述符同形纪律）");
    assert_eq!(p["adc_rail_pin"], "34");
    assert!(p.get("i2c_scl").is_none(), "未注入键不入载荷");
    assert!(
        p.get("cred_store").is_none(),
        "未注入键不入载荷（cred_store 同）"
    );
    b.cred_store = Some("nvs:rr3".to_string());
    let p = rr3_board_payload(&b).expect("有键必须有载荷");
    assert_eq!(p["cred_store"], "nvs:rr3", "cred_store 字符串值随键");
}

/// 应答门序：Response + id 回显 + result 在位放行；类型错/id 错/缺 result/
/// result.error 四路全点名（板侧业务性失败折形状的协议面锚）。
#[test]
fn bridge_parse_response_gates() {
    let ok = json!({"jsonrpc": "2.0", "id": 3, "result": {"angle_deg": 90}});
    let (ty, raw) = (
        Rr3PayloadType::Response,
        rr3_body_encode(Rr3PayloadType::Response, ok.to_string().as_bytes()),
    );
    let v = rr3_bridge_decode(ty, &raw[2..], 3).unwrap();
    assert_eq!(v["angle_deg"], 90);

    // 类型错
    let ev = rr3_body_encode(Rr3PayloadType::Event, ok.to_string().as_bytes());
    assert!(rr3_bridge_decode(Rr3PayloadType::Event, &ev[2..], 3).is_err());
    // id 错
    assert!(rr3_bridge_decode(ty, &raw[2..], 4).is_err());
    // 缺 result
    let no_res = json!({"jsonrpc": "2.0", "id": 3});
    let nr = rr3_body_encode(ty, no_res.to_string().as_bytes());
    assert!(rr3_bridge_decode(ty, &nr[2..], 3).is_err());
    // result.error 折 Err
    let err_body = json!({"jsonrpc": "2.0", "id": 3, "result": {"error": "角度超范围 0-180: 200"}});
    let eb = rr3_body_encode(ty, err_body.to_string().as_bytes());
    let e = rr3_bridge_decode(ty, &eb[2..], 3).unwrap_err();
    assert!(e.contains("板侧业务失败"), "{e}");
    assert!(e.contains("角度超范围"), "{e}");
}

/// 真值锚定：宿主/板上 task 位形与 rr-core 的机制真值逐值一致（本 crate
/// 不加依赖边的补偿断言——漂移即红，防两处常量静默分叉）。
#[test]
fn task_and_trace_constants_match_rr_core_truth() {
    assert_eq!(RR3_TASK_HOST, 1, "宿主位形=rrd（rr-core::RR3_TASK_RRD）");
    assert_eq!(
        RR3_TRACE_DIRECT, 0,
        "直连 trace（rr-core::RR3_TRACE_ID_DIRECT）"
    );
    assert_eq!(RR3_PERIPH_TASK, 2, "板上执行面占首模块池位形");
    assert_eq!(
        RR3_BRIDGE_BAUD, 1_500_000,
        "桥面定档 1.5M：与板固件 UART2_BAUD_HZ 共用 rr-periph-proto 常量"
    );
}
