//! httpd 纯逻辑层单测（自同名单测模块原样抽出，判据不变）。

use super::*;

/// P1-3 写口令牌校验：合法 token 通过、缺/错/坏 JSON 拒绝。
#[test]
fn admin_token_check() {
    let ok = format!("{{\"token\":\"{}\"}}", RR3_BOARD_ADMIN_TOKEN);
    assert!(rr3_token_check(ok.as_bytes()), "合法 token 应通过");
    assert!(!rr3_token_check(b"{\"token\":\"wrong\"}"), "错 token 应拒");
    assert!(!rr3_token_check(b"{}"), "缺 token 应拒");
    assert!(!rr3_token_check(b"not json"), "坏 JSON 应拒");
    assert!(!rr3_token_check(br#"{"token":123}"#), "非串 token 应拒");
}

/// 表即分派真值：表内每行可查、method/path 交叉必拒（防"GET 走写口"的
/// 白名单越界——W4-2 起第五列处理臂就是分派本身，结构上无表臂漂移）。
#[test]
fn route_table_is_dispatch_truth() {
    for (method, path, _, _, _) in RR3_BOARD_ROUTES {
        assert!(
            rr3_route_lookup(method, path).is_some(),
            "表内行查不到: {method} {path}"
        );
    }
    assert!(rr3_route_lookup("GET", "/api/board/nope").is_none());
    assert!(
        rr3_route_lookup("POST", "/api/board/state").is_none(),
        "读端点不受 POST——写口白名单外 fail-closed"
    );
    assert!(
        rr3_route_lookup("GET", "/api/board/wifi").is_none(),
        "写端点不受 GET"
    );
    assert!(rr3_route_lookup("PUT", "/api/board/wifi").is_none());
    assert!(rr3_route_lookup("GET", "/api/brain/ops").is_none());
    assert!(rr3_route_lookup("GET", "/api/board/state/extra").is_none());
}

/// covers 形制（同 check.sh ⑩ 扫描执法语义）：带斜杠且不以 `/` 开头；
/// applies 只收 `live`/空；method 词表只收 GET/POST（注册面映射与 handler
/// 识别共用同一词表——批N14 件二扩 POST 行时钉死）。
#[test]
fn covers_wordform_shape() {
    for (method, path, covers, applies, _) in RR3_BOARD_ROUTES {
        assert!(
            *method == "GET" || *method == "POST",
            "method 词表外: {path} {method}"
        );
        assert!(!path.is_empty(), "空 path 行");
        assert!(
            applies.is_empty() || *applies == "live",
            "applies 形制乱写: {path} {applies}"
        );
        for c in *covers {
            assert!(
                c.contains('/') && !c.starts_with('/'),
                "covers 形制乱写: {path} {c}（应为 <路>/<字段>）"
            );
        }
    }
}

/// 配网页字段名与写口解析器同源（2026-11-20 真板实锤：页面发 `password`、
/// 写口认 `pass`，保存恒回 `stage:"body"`）。改任一侧必须同步。
#[test]
fn index_form_posts_pass_field() {
    assert!(
        RR3_BOARD_INDEX_HTML.contains("pass:pass.value"),
        "配网页必须发 `pass` 字段（写口 rr3_wifi_write_body 只认 pass）"
    );
    assert!(
        !RR3_BOARD_INDEX_HTML.contains("password:pass.value"),
        "配网页不得再发 `password`（会被写口判 body 解析失败）"
    );
}

/// 写口 body 解析域（纯函数）：合法/空密码合法（开放网络）/坏 JSON/缺字段/
/// 非串字段全点名。
#[test]
fn wifi_write_body_domain() {
    let (s, p) = rr3_wifi_write_body(br#"{"ssid":"ap","pass":"pw"}"#).unwrap();
    assert_eq!((s.as_str(), p.as_str()), ("ap", "pw"));
    assert!(
        rr3_wifi_write_body(br#"{"ssid":"ap","pass":""}"#).is_ok(),
        "空密码合法（开放网络）"
    );
    assert!(rr3_wifi_write_body(b"not json").is_err(), "坏 JSON 必须红");
    assert!(
        rr3_wifi_write_body(br#"{"ssid":"ap"}"#).is_err(),
        "pass 缺席必须红"
    );
    assert!(
        rr3_wifi_write_body(br#"{"pass":"pw"}"#).is_err(),
        "ssid 缺席必须红"
    );
    assert!(
        rr3_wifi_write_body(br#"{"ssid":1,"pass":"pw"}"#).is_err(),
        "非串字段必须红"
    );
}

/// 写口应答 JSON 形状：四字段名/类型钉死（拒绝态与成功态同形状——
/// 宿主消费面契约面）。
#[test]
fn wifi_write_json_shape() {
    let mk = |r: &Rr3WifiWriteResult| {
        let v: serde_json::Value = serde_json::from_str(&rr3_wifi_write_json(r)).unwrap();
        v
    };
    let v = mk(&Rr3WifiWriteResult {
        ok: true,
        connected: true,
        reverted: false,
        stage: "apply",
    });
    assert_eq!(v["ok"], true);
    assert_eq!(v["connected"], true);
    assert_eq!(v["reverted"], false);
    assert_eq!(v["stage"], "apply");
    assert_eq!(v.as_object().unwrap().len(), 4);
    let v = mk(&Rr3WifiWriteResult::rejected("form"));
    assert_eq!(v["ok"], false);
    assert_eq!(v["connected"], false);
    assert_eq!(v["reverted"], false);
    assert_eq!(v["stage"], "form");
    assert_eq!(v.as_object().unwrap().len(), 4);
}

/// state JSON 形状：字段名/类型钉死（宿主消费面契约面）。
#[test]
fn state_json_shape() {
    let s = Rr3BoardStateSnapshot {
        pan_deg: 90,
        tilt_deg: 91,
        frame_mode: false,
        expr_mode: true,
        wander_on: false,
        rail_mv: 5123,
        rail_raw: 2048,
        rail_valid: true,
        rail_tier: "Ok",
        rail_low_events: 0,
        move_fail_events: 2,
        selfheal_skipped: 0,
        binding_mismatch: 1,
        audio_on: true,
        audio_seq: 7,
        audio_sent: 100,
        audio_dropped: 3,
        tts_gate: false,
        tts_played: 5,
        tts_fail: 1,
    };
    let v: serde_json::Value = serde_json::from_str(&rr3_state_json(&s)).unwrap();
    assert_eq!(v["pan_deg"], 90);
    assert_eq!(v["rail_tier"], "Ok");
    assert_eq!(v["rail_valid"], true);
    assert_eq!(v["move_fail_events"], 2);
    assert_eq!(v["binding_mismatch"], 1);
    assert_eq!(v["audio_sent"], 100);
    assert_eq!(v["expr_mode"], true);
    assert_eq!(v["tts_played"], 5);
    assert_eq!(v["tts_fail"], 1);
    assert_eq!(v.as_object().unwrap().len(), 20);
}

/// bindings JSON 形状：未注入整体 null；注入态十一键全名＋None→null；GPIO
/// 十键数字值、cred_store 字符串值（批N17 件四——键名与 rr-ipc 在册表同源）。
#[test]
fn bindings_json_shape() {
    assert_eq!(rr3_bindings_json(None), "null");
    let b = Rr3BoardBindings {
        i2c_sda: Some(21),
        i2c_scl: Some(22),
        adc_rail_pin: Some(34),
        uart2_tx: Some(18),
        uart2_rx: Some(19),
        ledc_pan: Some(32),
        ledc_tilt: Some(33),
        i2s_bclk: Some(4),
        i2s_sd: Some(27),
        i2s_ws: None,
        cred_store: Some("nvs:rr3".into()),
    };
    let v: serde_json::Value = serde_json::from_str(&rr3_bindings_json(Some(&b))).unwrap();
    assert_eq!(v["i2c_sda"], 21);
    assert_eq!(v["i2s_ws"], serde_json::Value::Null);
    assert_eq!(v["cred_store"], "nvs:rr3", "cred_store 字符串值（非 GPIO）");
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "adc_rail_pin",
            "cred_store",
            "i2c_scl",
            "i2c_sda",
            "i2s_bclk",
            "i2s_sd",
            "i2s_ws",
            "ledc_pan",
            "ledc_tilt",
            "uart2_rx",
            "uart2_tx",
        ],
        "恰十一键（rr-ipc 在册表封闭域）"
    );
    assert_eq!(v.as_object().unwrap().len(), 11);
}
