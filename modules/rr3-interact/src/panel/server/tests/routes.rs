//! 路由面：端点表、covers/applies 词表、越权形态与 HTTP 读请求。

use super::*;

/// BusCall 应答状态码裁定（I-21③）：载荷含 error = 422 业务失败，其余 200
#[test]
fn bus_reply_status_splits_business_error_from_success() {
    assert_eq!(
        rr3_bus_reply_status(&json!({ "played_ms": 120 })),
        (200, "OK"),
        "正常应答 200"
    );
    assert_eq!(
        rr3_bus_reply_status(&json!({ "expression": "talk", "previous": "idle" })),
        (200, "OK"),
        "多字段成功应答 200"
    );
    assert_eq!(
        rr3_bus_reply_status(&json!({ "error": "舵机越界拒收" })),
        (422, "Unprocessable Entity"),
        "业务错误 422（不得折 200+ok:true 假成功）"
    );
}

#[test]
fn route_covers_all_table_entries() {
    // 端点逐一点名（全集 = RR3_PANEL_ROUTES，表↔分派漂移由下一条门钉）：
    assert_eq!(rr3_panel_route("GET", "/", b""), Rr3PanelDispatch::Html);
    assert_eq!(
        rr3_panel_route("GET", "/api/status", b""),
        Rr3PanelDispatch::StatusQuery
    );
    assert_eq!(
        rr3_panel_route("GET", "/api/data", b""),
        Rr3PanelDispatch::CountersQuery
    );
    assert_eq!(
        rr3_panel_route("GET", "/api/logs", b""),
        Rr3PanelDispatch::LogsQuery
    );
    assert_eq!(
        rr3_panel_route("POST", "/api/relay", br#"{"on":true}"#),
        Rr3PanelDispatch::BusCall {
            topic: "rr3-act/relay/set".into(),
            args: json!({ "on": true })
        }
    );
    assert_eq!(
        rr3_panel_route("POST", "/api/beep", b""),
        Rr3PanelDispatch::PanelAct(Rr3PanelAction::Beep)
    );
    assert_eq!(
        rr3_panel_route("POST", "/api/face", br#"{"expression":"happy"}"#),
        Rr3PanelDispatch::PanelAct(Rr3PanelAction::Face {
            expression: "happy".into()
        })
    );
    assert_eq!(
        rr3_panel_route("POST", "/api/selftest", b""),
        Rr3PanelDispatch::SelfTest
    );
    assert_eq!(
        rr3_panel_route("POST", "/api/reboot", b""),
        Rr3PanelDispatch::Reboot
    );
    // 配网三端点（0.0.4 批D）：scan/forget 无参 BusCall，connect body 透传
    assert_eq!(
        rr3_panel_route("GET", "/api/wifi/scan", b""),
        Rr3PanelDispatch::BusCall {
            topic: "rr3-connect/wifi/scan".into(),
            args: json!({})
        }
    );
    assert_eq!(
        rr3_panel_route(
            "POST",
            "/api/wifi/connect",
            br#"{"ssid":"home","pass":"password8"}"#
        ),
        Rr3PanelDispatch::BusCall {
            topic: "rr3-connect/wifi/connect".into(),
            args: json!({ "ssid": "home", "pass": "password8" })
        }
    );
    assert_eq!(
        rr3_panel_route("POST", "/api/wifi/forget", b""),
        Rr3PanelDispatch::BusCall {
            topic: "rr3-connect/wifi/forget".into(),
            args: json!({})
        }
    );
    // 五模块 ops 读口（批N4 件九二）：无参 BusCall 透传，各点名 owner 模块
    assert_eq!(
        rr3_panel_route("GET", "/api/brain/ops", b""),
        Rr3PanelDispatch::BusCall {
            topic: "rr3-brain/ops/list".into(),
            args: json!({})
        }
    );
    // 配置面写口（件九三）：group/slot/value body 原样透传，点名 owner 写臂
    assert_eq!(
        rr3_panel_route(
            "POST",
            "/api/brain/ops",
            br#"{"group":"asr","slot":"asr_model","value":"m2"}"#
        ),
        Rr3PanelDispatch::BusCall {
            topic: "rr3-brain/ops/set".into(),
            args: json!({ "group": "asr", "slot": "asr_model", "value": "m2" })
        }
    );
    assert_eq!(
        rr3_panel_route("GET", "/api/act/ops", b""),
        Rr3PanelDispatch::BusCall {
            topic: "rr3-act/ops/list".into(),
            args: json!({})
        }
    );
    assert_eq!(
        rr3_panel_route("GET", "/api/connect/ops", b""),
        Rr3PanelDispatch::BusCall {
            topic: "rr3-connect/ops/list".into(),
            args: json!({})
        }
    );
    assert_eq!(
        rr3_panel_route("GET", "/api/interact/ops", b""),
        Rr3PanelDispatch::BusCall {
            topic: "rr3-interact/ops/list".into(),
            args: json!({})
        }
    );
    assert_eq!(
        rr3_panel_route("GET", "/api/power/ops", b""),
        Rr3PanelDispatch::BusCall {
            topic: "rr3-power/ops/list".into(),
            args: json!({})
        }
    );
    assert_eq!(
        rr3_panel_route("GET", "/api/sense/ops", b""),
        Rr3PanelDispatch::BusCall {
            topic: "rr3-sense/ops/list".into(),
            args: json!({})
        }
    );
    // 装配快照读口（批N4 件九笔4）：GET /api/wiring → ctl 七号方法透传
    assert_eq!(
        rr3_panel_route("GET", "/api/wiring", b""),
        Rr3PanelDispatch::WiringQuery
    );
    // 装配侧写口（批N22 件一）：POST /api/profile/domain → ctl.domain.set
    // 透传，body 原样进分派参数（门序裁决在 rrd 单点）
    assert_eq!(
            rr3_panel_route(
                "POST",
                "/api/profile/domain",
                r#"{"target":"domain","name":"rail5v","key":"armed","value":true,"confirm":true,"evidence":"实测/0001"}"#.as_bytes()
            ),
            Rr3PanelDispatch::ProfileDomainSet {
                params: json!({
                    "target": "domain", "name": "rail5v", "key": "armed",
                    "value": true, "confirm": true, "evidence": "实测/0001"
                })
            }
        );
    // 写口非 POST = 405（方法面收窄，与 relay/face 同口径）
    assert!(matches!(
        rr3_panel_route("GET", "/api/profile/domain", b""),
        Rr3PanelDispatch::MethodNotAllowed
    ));
}

/// 端点表 ↔ 路由分派漂移门（MC-8③/I-9）：表内每条必须分派成功（不是
/// 404/405），表长 pin 与自述文案同源（端点数由 len() 推导）——新增端点 =
/// 加表 + 改本 pin + 过本门，文案零改动。
#[test]
fn route_table_matches_dispatch() {
    assert_eq!(
            RR3_PANEL_ROUTES.len(),
            28,
            "端点表计数 pin：增删端点须同批改本 pin 与设计册 I-9 口径（26→28：增 GET /api/healthz + GET /api/version 探针端点，15b4556）"
        );
    for (method, path, _covers, _applies) in RR3_PANEL_ROUTES {
        let body: &[u8] = match (*method, *path) {
                ("POST", "/api/relay") => br#"{"on":true}"#,
                ("POST", "/api/face") => br#"{"expression":"happy"}"#,
                ("POST", "/api/wifi/connect") => br#"{"ssid":"s","pass":"password8"}"#,
                ("POST", "/api/model/key") => r#"{"ref":"测试键","value":"sk-x"}"#.as_bytes(),
                ("POST", "/api/brain/ops") => br#"{"group":"asr","slot":"asr_model","value":"m2"}"#,
                ("POST", "/api/brain/model") => {
                    r#"{"op":"activate","name":"DeepSeek官方"}"#.as_bytes()
                }
                ("POST", "/api/brain/model/test") => {
                    r#"{"name":"DeepSeek官方"}"#.as_bytes()
                }
                ("POST", "/api/profile/domain") => {
                    r#"{"target":"domain","name":"rail5v","key":"armed","value":true,"confirm":true,"evidence":"实测/0001"}"#.as_bytes()
                }
                _ => b"",
            };
        let d = rr3_panel_route(method, path, body);
        assert!(
            !matches!(
                &d,
                Rr3PanelDispatch::NotFound | Rr3PanelDispatch::MethodNotAllowed
            ),
            "{method} {path} 在端点表内但路由拒绝: {d:?}"
        );
    }
    // 反向：表外路径一律 404，不因加表而放开
    assert!(matches!(
        rr3_panel_route("GET", "/api/not-in-table", b""),
        Rr3PanelDispatch::NotFound
    ));
}

/// covers 申报词表形制门（I-19，批N4 件三）：每端点 covers 元素形制必须 =
/// `<RR3_PANEL_COVER_SETS 词表集合>/<单段字段>`——集合词表外、缺斜杠、多段
/// 都是申报面错，当场红（fail-closed；check.sh 步 ⑩ 侧另以同一词表导出复验，
/// 两处真值同源都是本常量）。
#[test]
fn covers_use_declared_vocabulary_only() {
    for (method, path, covers, _applies) in RR3_PANEL_ROUTES {
        for c in *covers {
            let (set, field) = c
                .split_once('/')
                .unwrap_or_else(|| panic!("{method} {path} 的 covers {c:?} 缺 '/'"));
            assert!(
                RR3_PANEL_COVER_SETS.contains(&set),
                "{method} {path} 的 covers {c:?} 集合 {set:?} 不在 RR3_PANEL_COVER_SETS"
            );
            assert!(
                !field.is_empty() && !field.contains('/'),
                "{method} {path} 的 covers {c:?} 字段段须非空且单段"
            );
        }
    }
}

/// applies 申报词表门（I-20，批N4 件四，全案 §19.3 判据 1）：第四列非空
/// 必须在 [`RR3_PANEL_APPLY_MODES`] 三态内；写口（非 GET）动了可配置集
/// （covers 非空）则 applies 必填——缺失即红。今日 covers 全空 ⇒ 判据
/// 后半 vacuous，但门先建：件九三面扩端点写 covers 的当场被兜住。
#[test]
fn applies_use_declared_vocabulary_only() {
    for (method, path, covers, applies) in RR3_PANEL_ROUTES {
        if !applies.is_empty() {
            assert!(
                RR3_PANEL_APPLY_MODES.contains(applies),
                "{method} {path} 的 applies {applies:?} 不在 RR3_PANEL_APPLY_MODES"
            );
        }
        if *method != "GET" && !covers.is_empty() {
            assert!(
                !applies.is_empty(),
                "{method} {path} 写口动了可配置集（covers 非空）却缺 applies 申报"
            );
        }
    }
    // 今日申报实况 pin：三条 applies = model-key 写口（live——secrets 落盘
    // 后 brain 每次现读，无需重启/重烧）＋ brain ops 写口（件九三，live 内存
    // 生效不落盘）＋ profile/domain 写口（批N22 件一，reboot——装配单落盘
    // 下次 rrd 装载投影）。改申报 = 同批改本 pin 与设计册 I-20。
    let declared: Vec<_> = RR3_PANEL_ROUTES
        .iter()
        .filter(|(_, _, _, a)| !a.is_empty())
        .collect();
    assert_eq!(
        declared,
        vec![
            &("POST", "/api/model/key", &[][..], "live"),
            &(
                "POST",
                "/api/brain/ops",
                &[
                    "ops/asr_model",
                    "ops/base_url",
                    "ops/llm_model",
                    "ops/max_utterance_ms",
                    "ops/timeout_s",
                    "ops/tts_model"
                ][..],
                "live"
            ),
            &("POST", "/api/brain/model", &[][..], "live"),
            &(
                "POST",
                "/api/profile/domain",
                &[
                    "domain/armed",
                    "domain/evidence",
                    "domain/severe_mv",
                    "axis/current_ceil_ma"
                ][..],
                "reboot"
            ),
        ],
        "applies 申报面漂移：新增写口申报须同批改本 pin"
    );
}

/// 越权拒的静态形态：路由器里没有"发布"这种产物——relay 只能以
/// BusCall（rr3.call 经 rrd 归属门）出去。这是类型级断言，不是约定。
#[test]
fn hardware_control_is_bus_call_only_never_publish() {
    match rr3_panel_route("POST", "/api/relay", br#"{"on":false}"#) {
        Rr3PanelDispatch::BusCall { topic, .. } => {
            assert_eq!(topic, "rr3-act/relay/set", "面板控制走 act 既有 exposes");
        }
        other => panic!("relay 必须折 BusCall（无发布面）: {other:?}"),
    }
}

#[test]
fn route_rejects_unknown_path_wrong_method_and_bad_body() {
    assert_eq!(
        rr3_panel_route("GET", "/api/none", b""),
        Rr3PanelDispatch::NotFound
    );
    assert_eq!(
        rr3_panel_route("DELETE", "/api/relay", b""),
        Rr3PanelDispatch::MethodNotAllowed
    );
    assert_eq!(
        rr3_panel_route("GET", "/", b""),
        Rr3PanelDispatch::Html,
        "GET / 不受其他端点方法约束"
    );
    // relay 缺 on / body 坏 JSON → 400 语义
    assert!(matches!(
        rr3_panel_route("POST", "/api/relay", b"{}"),
        Rr3PanelDispatch::BadRequest(_)
    ));
    assert!(matches!(
        rr3_panel_route("POST", "/api/relay", b"not-json"),
        Rr3PanelDispatch::BadRequest(_)
    ));
    assert!(matches!(
        rr3_panel_route("POST", "/api/face", b"{}"),
        Rr3PanelDispatch::BadRequest(_)
    ));
}

#[test]
fn http_read_request_reads_full_body_and_rejects_half() {
    // 全量：声明 12B 读满 12B
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let t = std::thread::spawn(move || {
        let (sock, _) = listener.accept().unwrap();
        rr3_http_read_req(&sock, &Rr3PanelLimits::default()).unwrap()
    });
    let mut c = TcpStream::connect(addr).unwrap();
    let full = b"POST /api/relay HTTP/1.1\r\nContent-Length: 11\r\n\r\n{\"on\":true}";
    c.write_all(full).unwrap();
    let req = t.join().unwrap();
    assert_eq!(req.method, "POST");
    assert_eq!(req.path, "/api/relay");
    assert_eq!(req.body, b"{\"on\":true}");

    // 半包：声明 12B 只发 4B → 读超时折 Err，不 panic 不挂死
    let listener2 = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr2 = listener2.local_addr().unwrap();
    let t2 = std::thread::spawn(move || {
        let (sock, _) = listener2.accept().unwrap();
        rr3_http_read_req(&sock, &Rr3PanelLimits::default())
    });
    let mut c2 = TcpStream::connect(addr2).unwrap();
    c2.write_all(b"POST /api/relay HTTP/1.1\r\nContent-Length: 11\r\n\r\n{\"on")
        .unwrap();
    let got = t2.join().unwrap();
    assert!(got.is_err(), "半包必须折 Err（读超时），不能装作读完");
}
