//! 注册面：发号/重复/保留名/坏 manifest/退订/应答点对点。

use super::register_sima;
use super::support::*;
use rr_bus::RR3_TASK_RRD;
use rr_ipc::Rr3StatusCode;
use serde_json::json;
use std::time::Duration;

/// 两个模块各拿一个不同 task，且互相看得见对方上线
#[test]
fn two_peers_get_distinct_tasks_and_online_broadcast() {
    let mut rrd = RrdTest::rr3_start("dual");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    let f = rr3_read_rpc_frame(&mut a);
    assert_eq!(f.header.src_task, RR3_TASK_RRD, "应答必须由 rrd 署名");
    assert_eq!(f.header.dst_task, 2, "应答必须点对点回给请求者");
    assert_eq!(
        rr3_json_of(&f)["result"]["src_task"],
        2,
        "第一个模块拿 RR3_TASK_FIRST_MODULE=2"
    );
    rr3_read_state(&mut a, "rr3-sima", "registered");

    let mut b = rrd.rr3_connect();
    rr3_send(
        &mut b,
        &rr3_register_frame(&rr3_manifest("rr3-simb", 100, 500, "always"), "rr3-simb"),
    );
    assert_eq!(
        rr3_read_rpc(&mut b)["result"]["src_task"],
        3,
        "task 号单调，不回收不重号"
    );
    let st = rr3_read_state(&mut b, "rr3-sima", "registered");
    assert_eq!(
        st["src_task"], 2,
        "A 的上线广播必须能到 B（rrd 下行，owner=rr3-runtime）"
    );
    assert!(rrd
        .rr3_logs(Duration::from_millis(100))
        .contains("注册 rr3-simb"));
}

/// 同名重复注册 = AIRY_EBUSY，且先来的那个不受牵连
#[test]
fn duplicate_name_is_busy_and_first_peer_survives() {
    let mut rrd = RrdTest::rr3_start("dup");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    assert_eq!(rr3_read_rpc(&mut a)["result"]["src_task"], 2);

    let mut b = rrd.rr3_connect();
    rr3_send(
        &mut b,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    let v = rr3_read_rpc(&mut b);
    assert_eq!(
        v["error"]["code"],
        Rr3StatusCode::Busy as i32,
        "重复注册 = AIRY_EBUSY(-16)"
    );
    assert!(
        v["error"]["message"].as_str().unwrap().contains("已在册"),
        "{v}"
    );

    rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 5, json!({}), 2));
    assert_eq!(
        rr3_read_rpc(&mut a)["result"]["beat"],
        "accepted",
        "拒第二条连接不该动第一条"
    );
    assert!(rrd.rr3_wait_log("注册被拒").contains("已在册"));
}

/// 冒充内建命名空间的模块名 = AIRY_EPERM
#[test]
fn registering_reserved_owner_is_eperm() {
    let rrd = RrdTest::rr3_start("reserved");
    let mut c = rrd.rr3_connect();
    rr3_send(
        &mut c,
        &rr3_register_frame(
            &rr3_manifest("rr3-runtime", 100, 500, "always"),
            "rr3-runtime",
        ),
    );
    let v = rr3_read_rpc(&mut c);
    assert_eq!(
        v["error"]["code"],
        Rr3StatusCode::Permission as i32,
        "保留名 = AIRY_EPERM(-22)"
    );
    assert!(
        v["error"]["message"].as_str().unwrap().contains("保留"),
        "{v}"
    );
}

/// 坏 manifest：文案必须点名具体字段；三次失败之后守护照常服务，且不占 task 号
#[test]
fn bad_manifests_name_the_field_and_keep_rrd_alive() {
    let mut rrd = RrdTest::rr3_start("bad");
    let good = rr3_manifest("rr3-sima", 100, 500, "always");
    let cases = [
        (rr3_manifest("rr3-sima", 500, 200, "always"), "heartbeat_ms"),
        (
            rr3_manifest("rr3-sima", 100, 500, "sometimes"),
            "restart_policy",
        ),
        (
            good.replace("entry = \"/bin/rr3-sima\"", "entry = \"bin/rr3-sima\""),
            "绝对路径",
        ),
        (good.replace("heartbeat_ms = 100\n", ""), "heartbeat_ms"),
    ];
    for (text, needle) in cases {
        let mut c = rrd.rr3_connect();
        rr3_send(&mut c, &rr3_register_frame(&text, "rr3-sima"));
        let v = rr3_read_rpc(&mut c);
        assert_eq!(
            v["error"]["code"],
            Rr3StatusCode::Invalid as i32,
            "{needle} 该回 EINVAL"
        );
        assert!(
            v["error"]["message"].as_str().unwrap().contains(needle),
            "文案必须点名 {needle}，实际 {v}"
        );
    }
    let mut ok = rrd.rr3_connect();
    rr3_send(
        &mut ok,
        &rr3_register_frame(&rr3_manifest("rr3-simb", 100, 500, "never"), "rr3-simb"),
    );
    assert_eq!(
        rr3_read_rpc(&mut ok)["result"]["src_task"],
        2,
        "被拒的注册不占 task 号"
    );
    assert!(
        rrd.rr3_logs(Duration::from_millis(200))
            .matches("注册被拒")
            .count()
            >= 4
    );
}

/// 主动退订：干净退出，绝不触发重启（哪怕 restart_policy=always）
#[test]
fn unregister_is_clean_exit_and_never_restarts() {
    let mut rrd = RrdTest::rr3_start("unreg");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut a);

    let mut b = rrd.rr3_connect();
    rr3_send(
        &mut b,
        &rr3_register_frame(&rr3_manifest("rr3-simb", 100, 500, "always"), "rr3-simb"),
    );
    rr3_read_rpc(&mut b);

    rr3_send(&mut a, &rr3_rpc_frame("rr3.unregister", 9, json!({}), 2));
    assert_eq!(rr3_read_rpc(&mut a)["result"]["removed"], true);

    let st = rr3_read_state(&mut b, "rr3-sima", "unregistered");
    assert_eq!(
        st["src_task"], 2,
        "退场广播由 rrd 下行发（owner=rr3-runtime）"
    );
    rrd.rr3_assert_no_log("重启计划", Duration::from_millis(400));
    assert!(rrd.rr3_logs(Duration::from_millis(100)).contains("无重启"));
}

/// 应答是点对点的（评审 A-1/I-2 的 m13）：`register_publish.rs` 里只有**注册应答**的 dst 被
/// 钉过（"应答必须点对点回给请求者"），心跳/退订应答的头字段今天没人看——
/// 把 `rr3_reply_ok` 的 dst 改成 `RR3_BROADCAST_DST_TASK` 就会在这里变红。
#[test]
fn replies_are_point_to_point() {
    let rrd = RrdTest::rr3_start("ptp");
    let mut a = rrd.rr3_connect();
    register_sima(&mut a);

    rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 11, json!({}), 2));
    let hb = rr3_read_rpc_frame(&mut a);
    assert_eq!(hb.header.dst_task, 2, "心跳应答只回给这一条连接");
    assert_eq!(rr3_json_of(&hb)["result"]["beat"], "accepted");

    rr3_send(&mut a, &rr3_rpc_frame("rr3.unregister", 12, json!({}), 2));
    let un = rr3_read_rpc_frame(&mut a);
    assert_eq!(
        un.header.dst_task, 2,
        "退订应答同理：dst=0 是 rrd 下行广播专用"
    );
    assert_eq!(rr3_json_of(&un)["result"]["removed"], true);
}
