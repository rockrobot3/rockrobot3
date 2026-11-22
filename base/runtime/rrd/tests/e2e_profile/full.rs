//! P1 满配正门用例（自 e2e_profile.rs 抽出，判据不变）：enabled_slots 决定起谁，
//! 三条 spawn 行 + ctl.status 轮询全 running，并钉装配/契约两个读口的线上事实。

use std::time::Duration;

use rr_ipc::{rr3_body_split, Rr3PayloadType};
use serde_json::json;

use super::{rr3_status, Root, GOOD};
use crate::support::*;

/// P1 — 满配：enabled_slots 决定起谁。三个格位各有一件、bindings 与 enabled_slots 互认领 →
/// 三条 spawn 行 + ctl.status 轮询到三个全 running。
#[test]
fn full_profile_spawns_every_face_module_and_all_reach_running() {
    let root = Root::new("full");
    for name in ["rr3-pa", "rr3-pb", "rr3-pc"] {
        root.write_module(name);
    }
    let profile = root.write_profile(GOOD);
    // env 显式钉 sim 探针（MC-4：非 test 面不再默认 sim）：act.servo/power.charger 归
    // PWM/I2C 硬件面，开发机与 CI runner 无这些 linux sysfs 设备，不显式选 sim 会被
    // fail-closed 判缺格、模块不 spawn（被测行为之外的夹具前提）。
    let mut rrd =
        RrdTest::rr3_start_profile_env("prof-full", &profile, E2E_TICK_MS, &[("RR3_PROBE", "sim")]);

    // 装配单凭证行（横幅之后、监听行之前的 stdout 事实）：证明这条 boot 走的是正门，
    // 不是"给了 --profile 却按 --manifest 空表静默起了一个空 daemon"。
    rrd.rr3_wait_log("rrd: 装配单已加载: ");

    // 三条 spawn 行：模块名来自 enabled_slots，entry 回显夹具里的 sim_peer 绝对路径——
    // 证明"profile 解析出的 manifest 真的被装载、真按它起了进程"，不是碰巧的同名者。
    for name in ["rr3-pa", "rr3-pb", "rr3-pc"] {
        let spawn = rrd.rr3_wait_log(&format!("spawn {name} pid="));
        assert!(
            spawn.contains(&format!("entry={}", env!("CARGO_BIN_EXE_sim_peer"))),
            "spawn 行必须回显装配单解析出的 manifest 里的绝对 entry: {spawn}"
        );
    }

    // running 的可观察面：轮询到三个模块全 running 为止。有界等待（判据是"成真"，
    // dur 只是不再等的上限，与 rr3_wait_line 同一口径）。
    let all_running = rr3_wait_until(
        || {
            let status = rr3_status(&rrd);
            ["rr3-pa", "rr3-pb", "rr3-pc"]
                .iter()
                .all(|name| status.iter().any(|(n, p)| n == name && p == "running"))
        },
        Duration::from_secs(20),
    );
    assert!(
        all_running,
        "三个 face 模块必须全部到达 running；最后一次快照: {:?}",
        rr3_status(&rrd)
    );

    // 装配快照读口（批N4 件九笔4）：--profile 正门 boot 成功 → `rr3.ctl.wiring`
    // 应答 available:true，四路底账（形态名/域/线组/角色）与 [pins]×wiring 全交叉
    // 判果同帧在线。夹具无 [[pin]]/axis 目录——零 pin 零轴是如实投影（装载面
    // role_missing 非致命），vcc/gnd 豁免导体判 not-applicable、在册域判 pass。
    let mut ctl = rrd.rr3_connect();
    rr3_send(&mut ctl, &rr3_rpc_frame("rr3.ctl.wiring", 2, json!({}), 0));
    let f = rr3_read_where(&mut ctl, "ctl.wiring 应答", |fr| {
        rr3_body_split(&fr.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    let v = rr3_json_of(&f);
    let w = &v["result"];
    assert_eq!(w["available"], true, "{v}");
    assert_eq!(w["profile"], "default", "形态名 = 装配单文件 stem: {v}");
    assert_eq!(w["domains"][0]["name"], "rail5v", "{v}");
    assert_eq!(w["wires"][0]["connector"], "J-T", "{v}");
    // 15 = P1 件四四格能力面角色（net.wifi.link / audio.capture.mic /
    // audio.playback.spk / audio.vad.gate）+ W5-3 ui.face.i2c 后的在册数
    assert_eq!(w["roles"].as_array().map(Vec::len), Some(15), "{v}");
    let checks = w["checks"].as_array().expect("checks 必须在场: {v}");
    let net_pass = checks
        .iter()
        .any(|c| c["item"] == "net:J-T:rail5v" && c["pass"] == "pass");
    let gnd_na = checks
        .iter()
        .any(|c| c["item"] == "signal:J-T[1]:gnd" && c["pass"] == "not-applicable");
    assert!(net_pass && gnd_na, "全交叉判果必须在场: {checks:?}");

    // 契约完整性审计（批N5 件五笔4，Z-4；批N8 件一②扩面，基-40）：--profile 正门
    // boot 成功 → 真值集记账定格，`rr3.ctl.contracts` available:true，四行四册记账
    // （真仓契约目录，e2e 不传 --contracts）+ 七行真值集成员记账（基-40 成员名 =
    // 装载面真值集相对名）+ 五条判果"齐全且 sha 对"全 pass。
    let mut ctl2 = rrd.rr3_connect();
    rr3_send(
        &mut ctl2,
        &rr3_rpc_frame("rr3.ctl.contracts", 3, json!({}), 0),
    );
    let f = rr3_read_where(&mut ctl2, "ctl.contracts 应答", |fr| {
        rr3_body_split(&fr.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    let v = rr3_json_of(&f);
    let c = &v["result"];
    assert_eq!(c["available"], true, "{v}");
    let books = c["contracts"].as_array().expect("契约记账必须在场: {v}");
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
    // 基-40 成员完整性段：七成员点名（成员名 = rr3_truth_set 相对名，顺序 =
    // 成员名单点序），每成员带 64 字符 sha256——与四册账同源同值（四册成员重复记账）。
    let ts = c["truth_set"].as_array().expect("真值集记账必须在场: {v}");
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
    let cchecks = c["checks"].as_array().expect("契约判果必须在场: {v}");
    assert_eq!(cchecks.len(), 5, "四册逐条＋truth_set 汇总: {cchecks:?}");
    assert!(
        cchecks.iter().all(|x| x["pass"] == "pass"),
        "boot 未动包内契约 → 五条判果全 pass: {cchecks:?}"
    );
}
