// SPDX-License-Identifier: Apache-2.0
//! 自检面：逐腿输出、wiring 展开、verdict 等值与 relay 缺省语义。

use std::path::Path;

use super::*;

/// 假 rrd + 真总线客户端（RR3_BUS_PATH 注入，连上即撤 env）；`sock` 为唯一套接字名。
fn fake_bus(dir: &Path, sock: &str, canned: Value, contracts: Value) -> (BusHandle, String) {
    let bus_path = dir.join(sock);
    let listener = std::os::unix::net::UnixListener::bind(&bus_path).unwrap();
    std::thread::spawn(move || {
        rr3_i4_fake_rrd(
            listener,
            Arc::new(Mutex::new(Vec::new())),
            canned,
            contracts,
        )
    });
    std::env::set_var(rr_ipc::RR3_BUS_PATH_ENV, bus_path.to_str().unwrap());
    let bus = BusHandle::wrap(Rr3BusClient::rr3_connect(I4_MANIFEST).expect("连假 rrd 应成功"));
    std::env::remove_var(rr_ipc::RR3_BUS_PATH_ENV);
    (bus, bus_path.to_string_lossy().into_owned())
}

/// 五腿混合 pass/fail 的逐腿断言（speaker/face 主循环缺席、relay 走通、取数两腿 fail）。
fn legs_assert_mixed(v: &Value) {
    let legs = v["legs"].as_array().expect("逐腿数组");
    assert_eq!(legs.len(), 5, "并列五腿（三调＋wiring 腿＋contracts 腿）");
    assert_eq!(legs[0]["leg"], "speaker");
    assert_eq!(
        legs[0]["pass"], "fail",
        "主循环缺席腿如实 fail，不折算通过（D15）"
    );
    assert!(
        legs[0]["reason"].as_str().unwrap().contains("主循环"),
        "失败原因进 reason（本场景主循环缺席；内置测试音的播放结果由活主循环回）"
    );
    assert_eq!(legs[1]["leg"], "relay");
    assert_eq!(legs[1]["pass"], "pass", "假 rrd 应答 noop，on/off 两步都成");
    assert_eq!(legs[2]["leg"], "face");
    assert_eq!(legs[2]["pass"], "fail", "主循环不在，face 腿走业务 Err");
    assert_eq!(
        legs[3]["leg"], "wiring/取数",
        "假 rrd 未预置 wiring 应答（available 缺）→ 取数单腿 fail"
    );
    assert_eq!(legs[3]["pass"], "fail");
    assert_eq!(
        legs[4]["leg"], "contracts/取数",
        "假 rrd 未预置 contracts 应答（available 缺）→ 取数单腿 fail（腿五同口径）"
    );
    assert_eq!(legs[4]["pass"], "fail");
    assert_eq!(
        v["verdict"], "fail",
        "混合态 verdict 必 fail（speaker/face 不等值 + wiring/contracts 组含 fail）"
    );
}

/// 自检逐腿输出（批N4 件五 I-18③/D15）：应答 `{legs:[{leg,pass,reason?}],verdict}`——
/// speaker 腿（主循环缺席场景）如实 fail 不折算通过；verdict = 结果集
/// 与预期集等值（批N5 件五 F-31），不等值即 fail。
#[test]
fn selftest_legs_fail() {
    // 假 rrd 不预置 wiring/contracts（available 缺）→ 取数两腿如实 fail；
    // actions 接收者即刻 drop——speaker/face 两腿走"主循环不在"业务 Err。
    let dir = std::env::temp_dir().join(format!("panel-selftest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (bus, bus_path) = fake_bus(&dir, "st.sock", json!({}), json!({}));

    let (actions_tx, _rx) = mpsc::channel(); // 接收者即刻 drop（下行）：主循环不在
    drop(_rx); // 显式 drop 保 send 失败立即折 Err，不撞 5s 调用耐心
    let shared = panel_shared(bus, bus_path, actions_tx);
    let (addr, jh) = panel_serve_once(shared);
    let v = selftest_roundtrip(addr, jh);
    legs_assert_mixed(&v);
}

/// 三态 wiring 与 contracts 预置应答（pass / n-a / 未知值 + 两条契约判果）。
fn wiring_canned() -> (Value, Value) {
    let canned = json!({
        "available": true,
        "profile": "default",
        "checks": [
            { "item": "net:J-T:rail5v", "pass": "pass" },
            { "item": "signal:J-T[1]:gnd", "pass": "not-applicable",
              "reason": "gnd-only 单线组不判导体" },
            { "item": "pin:J-T[2]:vcc", "pass": "weird-unknown", "reason": "未知值兜底" }
        ]
    });
    // 腿五 contracts 的预置应答：available:true + 两条全 pass 判果。
    let canned_contracts = json!({
        "available": true,
        "contracts": [
            { "file": "topics.toml", "sha256": "64hex", "bytes": 1 }
        ],
        "checks": [
            { "item": "contract:topics.toml", "pass": "pass", "reason": "sha256 对上" },
            { "item": "contract:slots.toml", "pass": "pass", "reason": "sha256 对上" }
        ]
    });
    (canned, canned_contracts)
}

/// 八腿断言：三调腿 + wiring 三条逐条摊开 + contracts 两条；verdict fail。
fn wiring_legs_assert(v: &Value) {
    let legs = v["legs"].as_array().expect("逐腿数组");
    assert_eq!(
        legs.len(),
        8,
        "三调腿＋wiring 三条逐条摊开＋contracts 两条: {v}"
    );
    assert_eq!(legs[0]["leg"], "speaker");
    assert_eq!(legs[0]["pass"], "fail", "主循环不在，如实 fail");
    assert_eq!(legs[1]["leg"], "relay");
    assert_eq!(legs[1]["pass"], "pass", "假 rrd noop 兜底应答，两步都成");
    assert_eq!(legs[2]["leg"], "face");
    assert_eq!(legs[2]["pass"], "fail", "主循环不在，如实 fail");
    assert_eq!(legs[3]["leg"], "wiring/net:J-T:rail5v");
    assert_eq!(legs[3]["pass"], "pass", "rrd 判 pass 如实转述");
    assert_eq!(legs[4]["leg"], "wiring/signal:J-T[1]:gnd");
    assert_eq!(
        legs[4]["pass"], "n-a",
        "not-applicable 映射 n-a，不折算 fail"
    );
    assert_eq!(legs[4]["reason"], "gnd-only 单线组不判导体", "reason 透传");
    assert_eq!(legs[5]["leg"], "wiring/pin:J-T[2]:vcc");
    assert_eq!(legs[5]["pass"], "fail", "未知 pass 值 fail-closed 折 fail");
    assert_eq!(legs[5]["reason"], "未知值兜底");
    // 腿五 contracts：预置判果逐条摊 contracts/<item>，reason 透传。
    assert_eq!(legs[6]["leg"], "contracts/contract:topics.toml");
    assert_eq!(legs[6]["pass"], "pass");
    assert_eq!(legs[6]["reason"], "sha256 对上");
    assert_eq!(legs[7]["leg"], "contracts/contract:slots.toml");
    assert_eq!(legs[7]["pass"], "pass");
    assert_eq!(
        v["verdict"], "fail",
        "wiring 组含 fail → verdict fail（零 fail 判据；n-a 不救场也不拖累）"
    );
}

/// 自检腿四展开（批N4 件九笔4，全案 §7 ⑤ + F-31）：假 rrd 预置
/// available:true + 三态混合 checks——pass 如实 pass、not-applicable 映射
/// n-a（不拖累不救场）、未知值 fail-closed 折 fail（恒败腿不得折算通过）；
/// 判果是 rrd 的产出，面板只转述（leg = "wiring/<item>" 逐条摊开）。
#[test]
fn selftest_leg_states() {
    let dir = std::env::temp_dir().join(format!("panel-wlegs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (canned, canned_contracts) = wiring_canned();
    let (bus, bus_path) = fake_bus(&dir, "wl.sock", canned, canned_contracts);

    let (actions_tx, _rx) = mpsc::channel();
    drop(_rx); // 接收者即刻 drop：speaker/face 走业务 Err
    let shared = panel_shared(bus, bus_path, actions_tx);
    let (addr, jh) = panel_serve_once(shared);
    let v = selftest_roundtrip(addr, jh);
    wiring_legs_assert(&v);
}

/// 构造一条腿（逐用例共用）。
fn leg(name: &str, pass: &str) -> Value {
    json!({ "leg": name, "pass": pass })
}

/// 等值两例：缺省预期全等值；预期改数据（F-31）后等值即过。
fn verdict_equal_cases() {
    let legs = vec![
        leg("speaker", "pass"),
        leg("relay", "pass"),
        leg("face", "pass"),
        leg("wiring/net:J-T:rail5v", "pass"),
        leg("wiring/signal:J-T[1]:gnd", "n-a"),
    ];
    assert_eq!(
        rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
        "pass",
        "结果集与缺省预期全等值"
    );
    let expect = Rr3SelftestExpect {
        speaker: "fail",
        face: "n-a",
        ..Default::default()
    };
    let legs = vec![
        leg("speaker", "fail"),
        leg("relay", "pass"),
        leg("face", "n-a"),
    ];
    assert_eq!(
        rr3_selftest_verdict(&legs, &expect),
        "pass",
        "预期表改数据即改判据，结果与预期等值即过"
    );
}

/// 固定腿三态不等值与 wiring 组含 fail 两例。
fn verdict_mismatch_cases() {
    let legs = vec![
        leg("speaker", "fail"),
        leg("relay", "pass"),
        leg("face", "pass"),
    ];
    assert_eq!(
        rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
        "fail",
        "固定腿三态不等值即红"
    );
    let legs = vec![
        leg("speaker", "pass"),
        leg("relay", "pass"),
        leg("face", "pass"),
        leg("wiring/pin:J-T[2]:vcc", "fail"),
    ];
    assert_eq!(
        rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
        "fail",
        "wiring 组零 fail 判据"
    );
}

/// contracts 组：零 fail 即组过；含 fail 即红。
fn verdict_group_cases() {
    let legs = vec![
        leg("speaker", "pass"),
        leg("relay", "pass"),
        leg("face", "pass"),
        leg("contracts/contract:topics.toml", "pass"),
        leg("contracts/contract:roles.toml", "pass"),
    ];
    assert_eq!(
        rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
        "pass",
        "contracts 组零 fail 即组过"
    );
    let legs = vec![
        leg("speaker", "pass"),
        leg("relay", "pass"),
        leg("face", "pass"),
        leg("contracts/contract:roles.toml", "fail"),
    ];
    assert_eq!(
        rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
        "fail",
        "contracts 组零 fail 判据"
    );
}

/// 边界两例：结果集缺固定腿即红；腿册外未知腿名 fail-closed。
fn verdict_edge_cases() {
    let legs = vec![leg("speaker", "pass"), leg("relay", "pass")];
    assert_eq!(
        rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
        "fail",
        "缺腿即红"
    );
    let legs = vec![
        leg("speaker", "pass"),
        leg("relay", "pass"),
        leg("face", "pass"),
        leg("arm", "pass"),
    ];
    assert_eq!(
        rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
        "fail",
        "腿册外未知腿名 fail-closed"
    );
}

/// 自检 verdict 预期表等值判定（批N5 件五 F-31，纯函数钉门）：等值 pass／
/// 固定腿不等值 fail／缺腿 fail／wiring 组含 fail fail／腿册外未知腿名
/// fail-closed fail；预期可改数据——恒败腿如实申报期望 fail 即等值过
/// （改数据不改码）。
#[test]
fn selftest_set_table() {
    verdict_equal_cases();
    verdict_mismatch_cases();
    verdict_group_cases();
    verdict_edge_cases();
}

/// relay 腿能力面（N44 后续）：rr3-act 缺席或配置显式 n-a → n-a；
/// 在册且未显式 n-a → 沿用预期表（classic 仍 pass/fail 照跑）。
#[test]
fn selftest_configured() {
    assert_eq!(rr3_relay_expect(false, "pass"), "n-a");
    assert_eq!(rr3_relay_expect(false, "fail"), "n-a");
    assert_eq!(rr3_relay_expect(true, "pass"), "pass");
    assert_eq!(rr3_relay_expect(true, "fail"), "fail");
    assert_eq!(rr3_relay_expect(true, "n-a"), "n-a");
}

/// 假总线 + ctl 错误信封夹具（ctl.status 取数失败）。
fn relay_ctl_down(dir: &Path) -> (BusHandle, String, std::thread::JoinHandle<()>) {
    // 总线连接只用于构造 BusHandle：本腿在 ctl 失败分支不会触总线
    let (bus, _bus_path) = fake_bus(dir, "bus.sock", json!({}), json!({}));
    let ctl_path = dir.join("ctl.sock");
    let ctl_listener = std::os::unix::net::UnixListener::bind(&ctl_path).unwrap();
    let jh = rr3_fake_ctl_rrd(
        ctl_listener,
        json!({
            "jsonrpc": "2.0", "id": 6,
            "error": { "code": 3, "message": "ctl.status 不可用" }
        }),
    );
    (bus, ctl_path.to_string_lossy().into_owned(), jh)
}

/// 取数失败必须 fail-closed：腿如实 fail、期望态不被改写、verdict 必红。
fn relay_fail_closed_assert(leg: Value, want: &'static str) {
    assert_eq!(leg["pass"], "fail", "取数失败如实记 fail");
    assert_eq!(want, "pass", "期望态不得被记成 fail 抵消");
    let legs = vec![
        json!({ "leg": "speaker", "pass": "pass" }),
        leg,
        json!({ "leg": "face", "pass": "pass" }),
    ];
    let expect = Rr3SelftestExpect {
        relay: want,
        ..Default::default()
    };
    assert_eq!(
        rr3_selftest_verdict(&legs, &expect),
        "fail",
        "能力核验故障不得假绿"
    );
}

/// relay 腿 ctl 取数失败必须 fail-closed（0.0.8c 审计）：腿如实 fail，
/// 期望态不得记成 fail 抵消等值——能力面不可核验时 verdict 必红。
#[test]
fn selftest_fail_closed() {
    let dir = std::env::temp_dir().join(format!("panel-relay-fail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (bus, ctl_path, jh) = relay_ctl_down(&dir);

    let (actions_tx, _rx) = mpsc::channel();
    let shared = panel_shared(bus, ctl_path, actions_tx);
    let (leg, want) = rr3_leg_relay(&shared);
    relay_fail_closed_assert(leg, want);
    jh.join().unwrap();
}

/// verdict 对 relay=n-a 的 host-only 形态等值即过（n-a 不拖累）。
#[test]
fn selftest_ok_n_host() {
    let legs = vec![
        json!({ "leg": "speaker", "pass": "pass" }),
        json!({ "leg": "relay", "pass": "n-a" }),
        json!({ "leg": "face", "pass": "pass" }),
        json!({ "leg": "wiring/net:J0:rail5v", "pass": "pass" }),
    ];
    let expect = Rr3SelftestExpect {
        relay: "n-a",
        ..Default::default()
    };
    assert_eq!(rr3_selftest_verdict(&legs, &expect), "pass");
}

/// 接线图 HTTP 透传（批N4 件九笔4）：GET /api/wiring → ctl.wiring 一问一答，
/// rrd 应答原样 200 回浏览器（available:true 快照在册面）。
#[test]
fn wiring_snapshot() {
    let dir = std::env::temp_dir().join(format!("panel-wpass-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let canned = json!({
        "available": true,
        "profile": "default",
        "pins": [],
        "domains": [ { "name": "rail5v", "nominal_mv": 5000 } ],
        "wires": [ { "connector": "J-T", "net": "rail5v" } ],
        "checks": [ { "item": "net:J-T:rail5v", "pass": "pass" } ]
    });
    let expect = canned.clone();
    let (bus, bus_path) = fake_bus(&dir, "wp.sock", canned, json!({}));

    let (actions_tx, _rx) = mpsc::channel();
    let shared = panel_shared(bus, bus_path, actions_tx);
    let (addr, jh) = panel_serve_once(shared);

    let mut c = TcpStream::connect(addr).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    c.write_all(b"GET /api/wiring HTTP/1.1\r\n\r\n").unwrap();
    let _ = c.shutdown(std::net::Shutdown::Write);
    let mut buf = Vec::new();
    c.read_to_end(&mut buf).expect("接线图应答必须完整可读");
    jh.join().unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "透传 200: {text}");
    let body = text.split("\r\n\r\n").nth(1).expect("应答带 body");
    let v: Value = serde_json::from_str(body).expect("应答是 JSON: {body}");
    assert_eq!(v, expect, "rrd 应答原样透传，面板不重判不改写");
}

/// covers 计数 pin（批N4 件九笔4，门 d 分母同源）：/api/wiring 申报
/// [pins]×wiring 全交叉读口的四族词表全集——pin 11 + role 7 + domain 12 +
/// axis 14 = 44，与 check.sh 步 ⑩ 的分母导出逐项对齐（红账 43 全转绿的
/// 面板侧申报表；domain 12 = 批N21 件二 evidence 键随批N21 件四补申报）；
/// 增删字段 = 改端点表 + 改本 pin + 过门 d。
#[test]
fn wiring_vocabulary() {
    let entry = RR3_PANEL_ROUTES
        .iter()
        .find(|(_, p, _, _)| *p == "/api/wiring")
        .expect("端点表必含 /api/wiring");
    let covers: Vec<&str> = entry.2.to_vec();
    assert_eq!(covers.len(), 44, "四族全集 44 项: {covers:?}");
    let count = |prefix: &str| covers.iter().filter(|c| c.starts_with(prefix)).count();
    assert_eq!(count("pin/"), 11, "pin 族 11 字段（RR3_PIN_KEYS 同源）");
    assert_eq!(
        count("role/"),
        7,
        "role 族 7 字段（roles.toml requires 同源）"
    );
    assert_eq!(
        count("domain/"),
        12,
        "domain 族 12 字段（RR3_DOMAIN_KEYS 同源）"
    );
    assert_eq!(
        count("axis/"),
        14,
        "axis 族 14 字段（RR3_ACT_PROFILE_FIELDS 同源）"
    );
    let mut sorted = covers.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), covers.len(), "covers 申报不得重复");
}
