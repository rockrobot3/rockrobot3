// SPDX-License-Identifier: Apache-2.0
//! P1 满配正门用例（自 e2e_profile.rs 抽出，判据不变）：enabled_slots 决定起谁，
//! 三条 spawn 行 + ctl.status 轮询全 running，并钉装配/契约两个读口的线上事实。

use std::time::Duration;

use rr_ipc::{rr3_body_split, Rr3PayloadType};
use serde_json::{json, Value};

use super::{rr3_status, Root, GOOD};
use crate::support::*;

/// 三条 spawn 行：模块名来自 enabled_slots，entry 回显夹具里的 sim_peer 绝对路径。
fn spawn_modules(rrd: &mut RrdTest) {
    rrd.rr3_wait_log("rrd: 装配单已加载: ");
    for name in ["rr3-pa", "rr3-pb", "rr3-pc"] {
        let spawn = rrd.rr3_wait_log(&format!("spawn {name} pid="));
        assert!(
            spawn.contains(&format!("entry={}", env!("CARGO_BIN_EXE_sim_peer"))),
            "spawn 行必须回显装配单解析出的 manifest 里的绝对 entry: {spawn}"
        );
    }
}

/// 有界轮询到三个模块全 running（判据是“成真”，dur 只是不再等的上限）。
fn wait_all_running(rrd: &RrdTest) {
    let all_running = rr3_wait_until(
        || {
            let status = rr3_status(rrd);
            ["rr3-pa", "rr3-pb", "rr3-pc"]
                .iter()
                .all(|name| status.iter().any(|(n, p)| n == name && p == "running"))
        },
        Duration::from_secs(20),
    );
    assert!(
        all_running,
        "三个 face 模块必须全部到达 running；最后一次快照: {:?}",
        rr3_status(rrd)
    );
}

/// 取 `rr3.ctl.wiring` 应答的 result 段（Response 帧过滤与 support 同口径）。
fn wiring_reply(rrd: &RrdTest) -> Value {
    let mut ctl = rrd.rr3_connect();
    rr3_send(&mut ctl, &rr3_rpc_frame("rr3.ctl.wiring", 2, json!({}), 0));
    let f = rr3_read_where(&mut ctl, "ctl.wiring 应答", |fr| {
        rr3_body_split(&fr.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    rr3_json_of(&f)["result"].clone()
}

/// 装配快照四路底账与 [pins]×wiring 全交叉判果（零 pin 零轴是如实投影）。
fn wiring_assert(w: &Value) {
    assert_eq!(w["available"], true, "{w}");
    assert_eq!(w["profile"], "default", "形态名 = 装配单文件 stem: {w}");
    assert_eq!(w["domains"][0]["name"], "rail5v", "{w}");
    assert_eq!(w["wires"][0]["connector"], "J-T", "{w}");
    // 15 = P1 件四四格能力面角色 + W5-3 ui.face.i2c 后的在册数
    assert_eq!(w["roles"].as_array().map(Vec::len), Some(15), "{w}");
    let checks = w["checks"].as_array().expect("checks 必须在场: {w}");
    let net_pass = checks
        .iter()
        .any(|c| c["item"] == "net:J-T:rail5v" && c["pass"] == "pass");
    let gnd_na = checks
        .iter()
        .any(|c| c["item"] == "signal:J-T[1]:gnd" && c["pass"] == "not-applicable");
    assert!(net_pass && gnd_na, "全交叉判果必须在场: {checks:?}");
}

/// 取 `rr3.ctl.contracts` 应答的 result 段（真仓契约目录；--profile 正门 boot 成功）。
fn contracts_reply(rrd: &RrdTest) -> Value {
    let mut ctl = rrd.rr3_connect();
    rr3_send(
        &mut ctl,
        &rr3_rpc_frame("rr3.ctl.contracts", 3, json!({}), 0),
    );
    let f = rr3_read_where(&mut ctl, "ctl.contracts 应答", |fr| {
        rr3_body_split(&fr.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    rr3_json_of(&f)["result"].clone()
}

/// 四册记账（基-40）：点名序 = RR3_CONTRACT_BOOKS，每册 64 字符 sha256。
fn contracts_books(c: &Value) {
    assert_eq!(c["available"], true, "{c}");
    let books = c["contracts"].as_array().expect("契约记账必须在场: {c}");
    assert_eq!(
        books
            .iter()
            .map(|b| b["file"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        rrd::profile::RR3_CONTRACT_BOOKS.to_vec(),
        "四册点名 = RR3_CONTRACT_BOOKS 清单序: {books:?}"
    );
    assert!(
        books
            .iter()
            .all(|b| b["sha256"].as_str().map(|s| s.len()) == Some(64)),
        "每册带 64 字符 sha256 十六进制: {books:?}"
    );
}

/// 真值集七成员记账与五条判果（boot 未动包内契约 → 全 pass）。
fn contracts_truth(c: &Value) {
    let ts = c["truth_set"].as_array().expect("真值集记账必须在场: {c}");
    assert_eq!(
        ts.iter()
            .map(|t| t["member"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec![
            "profiles/default.toml",
            "contracts/topics.toml",
            "contracts/slots.toml",
            "contracts/roles.toml",
            "contracts/devices.toml",
            "contracts/platform/rk3566.toml",
            "hardware/default/wiring.toml",
        ],
        "七成员点名 = 装载面真值集成员序: {ts:?}"
    );
    assert!(
        ts.iter()
            .all(|t| t["sha256"].as_str().map(|s| s.len()) == Some(64)),
        "每成员带 64 字符 sha256 十六进制: {ts:?}"
    );
    let cchecks = c["checks"].as_array().expect("契约判果必须在场: {c}");
    assert_eq!(cchecks.len(), 5, "四册逐条＋truth_set 汇总: {cchecks:?}");
    assert!(
        cchecks.iter().all(|x| x["pass"] == "pass"),
        "boot 未动包内契约 → 五条判果全 pass: {cchecks:?}"
    );
}

/// P1 — 满配：enabled_slots 决定起谁。三个格位各有一件、bindings 与 enabled_slots 互认领 →
/// 三条 spawn 行 + ctl.status 轮询到三个全 running。
#[test]
fn full_profile_spawns_every_face_module_and_all_reach_running() {
    let root = Root::new("full");
    for name in ["rr3-pa", "rr3-pb", "rr3-pc"] {
        root.write_module(name);
    }
    let profile = root.write_profile(GOOD);
    // env 显式钉 sim 探针（MC-4）：act.servo/power.charger 归 PWM/I2C 硬件面，
    // 不显式选 sim 会被 fail-closed 判缺格、模块不 spawn。
    let mut rrd =
        RrdTest::rr3_start_profile_env("prof-full", &profile, E2E_TICK_MS, &[("RR3_PROBE", "sim")]);
    spawn_modules(&mut rrd);
    wait_all_running(&rrd);
    wiring_assert(&wiring_reply(&rrd));
    let c = contracts_reply(&rrd);
    contracts_books(&c);
    contracts_truth(&c);
}
