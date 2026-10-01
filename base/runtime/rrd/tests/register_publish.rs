#![cfg(unix)]
//! 注册面与**发布面**的线级用例（spec §9 的注册/重复/保留名/坏 manifest/src_task 各条，
//! 加 Task 8 的发布前缀强制与内建命名空间冒充拒绝）。
//! 发布面共 7 条：合法转发、越权前缀、冒充 rr3-runtime/*、在册未登记主题、
//! 叠加层不是生产放行口、Event 体缺 topic 不牵连控制面、转发帧按内容而非到达序读。

mod support;

use rr_bus::RR3_TASK_RRD;
use rr_ipc::{rr3_body_split, Rr3PayloadType, Rr3StatusCode};
use serde_json::json;
use std::time::Duration;
use support::*;

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

/// 首帧必须是 rr3.register；心跳不能抢跑（legacy 教训 76c0ad8 的线上级写法）
#[test]
fn first_frame_must_be_register() {
    let mut rrd = RrdTest::rr3_start("order");
    let mut c = rrd.rr3_connect();
    rr3_send(&mut c, &rr3_rpc_frame("rr3.heartbeat", 1, json!({}), 0));
    let v = rr3_read_rpc(&mut c);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("rr3.register"),
        "{v}"
    );
    assert!(rrd.rr3_wait_log("注册被拒").contains("rr3.register"));

    let mut d = rrd.rr3_connect();
    rr3_send(
        &mut d,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    assert_eq!(
        rr3_read_rpc(&mut d)["result"]["src_task"],
        2,
        "被拒连接之后守护必须照常接受注册"
    );
}

/// 注册后伪造 src_task：拒这一帧、回原 id、不断连、不牵连后续心跳
#[test]
fn spoofed_src_task_is_rejected_without_killing_connection() {
    let mut rrd = RrdTest::rr3_start("spoof");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    assert_eq!(rr3_read_rpc(&mut a)["result"]["src_task"], 2);

    let f = {
        rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 7, json!({}), 99));
        rr3_read_rpc_frame(&mut a)
    };
    let v = rr3_json_of(&f);
    assert_eq!(v["id"], 7, "拒绝要回到请求者给的 id");
    assert_eq!(v["error"]["code"], Rr3StatusCode::Invalid as i32);
    assert!(
        v["error"]["message"].as_str().unwrap().contains("src_task"),
        "{v}"
    );
    assert_eq!(f.header.dst_task, 2, "拒绝也只回给这一条连接");

    rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 8, json!({}), 2));
    assert_eq!(
        rr3_read_rpc(&mut a)["result"]["beat"],
        "accepted",
        "一次伪造不该杀掉整条连接"
    );
    assert!(rrd
        .rr3_logs(Duration::from_millis(100))
        .contains("src_task"));
}

/// 上行只许 Request：把心跳塞进 Notification 帧里上来 → 拒（分派依据是前缀，不是内容）
#[test]
fn upstream_must_use_request_type() {
    let rrd = RrdTest::rr3_start("type");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut a);

    let body = rr3_request("rr3.heartbeat", 3, json!({}));
    rr3_send(
        &mut a,
        &rr3_json_frame(Rr3PayloadType::Notification, &body, 2, RR3_TASK_RRD),
    );
    let v = rr3_read_rpc(&mut a);
    assert_eq!(v["error"]["code"], Rr3StatusCode::Invalid as i32);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("上行不支持 payload 类型"),
        "{v}"
    );

    rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 4, json!({}), 2));
    assert_eq!(rr3_read_rpc(&mut a)["result"]["beat"], "accepted");
}

/// 连上却不注册：读超时收掉，且只是那一条连接的事
#[test]
fn silent_connection_times_out_and_rrd_survives() {
    let mut rrd = RrdTest::rr3_start("idle");
    let _idle = rrd.rr3_connect(); // 一帧不发；--pre-register-timeout-ms 300
    assert!(rrd.rr3_wait_log("连接收尾").contains("首帧读取失败"));

    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    assert_eq!(rr3_read_rpc(&mut a)["result"]["src_task"], 2);
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

/// 首帧的 `dst_task` 必须是 rrd（评审 A-1/I-2 的 m08）：形状完全正确、只把 dst 填成 0 →
/// 拒 EINVAL 且文案点名 dst_task；这条被拒的连接不牵连后来的正常注册。
#[test]
fn first_frame_dst_must_be_rrd() {
    let mut rrd = RrdTest::rr3_start("dstfirst");
    let mut c = rrd.rr3_connect();
    // 与 rr3_register_frame 同形（src 按契约填 0），唯一差别是 dst_task=0 广播而不是 1=rrd
    let manifest = rr3_manifest("rr3-sima", 100, 500, "always");
    let body = rr3_request(
        "rr3.register",
        1,
        json!({"name": "rr3-sima", "manifest": manifest}),
    );
    rr3_send(
        &mut c,
        &rr3_json_frame(Rr3PayloadType::Request, &body, 0, 0),
    );
    // 握手期的拒答按对端自报的 src_task 回（conn.rs 的 rr3_refuse），所以照常用 rpc 读法就得到
    let v = rr3_read_rpc(&mut c);
    assert_eq!(
        v["error"]["code"],
        Rr3StatusCode::Invalid as i32,
        "dst 不是 rrd 的首帧该回 EINVAL（放行就等于替广播地址代收注册），实际 {v}"
    );
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("dst_task"),
        "文案必须点名 dst_task，实际 {v}"
    );
    assert!(rrd.rr3_wait_log("注册被拒").contains("dst_task"));

    let mut d = rrd.rr3_connect();
    rr3_send(&mut d, &rr3_register_frame(&manifest, "rr3-sima"));
    assert_eq!(
        rr3_read_rpc(&mut d)["result"]["src_task"],
        2,
        "被拒的 dst 错首帧不得占走 task 号，守护要照常接受注册"
    );
}

/// 注册后的上行 `dst_task` 也必须是 rrd（评审 A-1/I-2 的 m07）。这条路径是
/// `rr3_serve_frame` 返回 `Err` → 连接按崩溃收尾、**不回错误帧**，所以判据只能是否定式：
/// 连接被关掉之前不会收到 `beat=accepted` 的应答。删掉门禁即走到 `rr3_serve_request` 回 accepted。
#[test]
fn upstream_dst_must_be_rrd() {
    let mut rrd = RrdTest::rr3_start("updst");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    assert_eq!(rr3_read_rpc(&mut a)["result"]["src_task"], 2);

    rr3_send(
        &mut a,
        &rr3_json_frame(
            Rr3PayloadType::Request,
            &rr3_request("rr3.heartbeat", 6, json!({})),
            2,
            0,
        ),
    );
    let got = rr3_drain_until_closed(&mut a);
    let accepted = got
        .iter()
        .filter(|f| {
            rr3_body_split(&f.payload)
                .map(|(ty, _)| ty == Rr3PayloadType::Response)
                .unwrap_or(false)
                && rr3_json_of(f)["result"]["beat"] == "accepted"
        })
        .count();
    assert_eq!(
        accepted,
        0,
        "dst=0 的上行心跳不得被代收（那是跨模块串话的门）；连接被关掉前收到 {} 帧",
        got.len()
    );
    // 崩溃收尾的原因文案点名门禁（`rr3_conn_finish` 的"退出（原因）"）
    assert!(rrd.rr3_wait_log("上行 dst_task").contains("上行 dst_task"));

    let mut b = rrd.rr3_connect();
    rr3_send(
        &mut b,
        &rr3_register_frame(&rr3_manifest("rr3-simb", 100, 500, "always"), "rr3-simb"),
    );
    assert_eq!(
        rr3_read_rpc(&mut b)["result"]["src_task"],
        3,
        "崩掉一条连接之后守护照常发号"
    );
}

/// 应答是点对点的（评审 A-1/I-2 的 m13）：`register_publish.rs` 里只有**注册应答**的 dst 被
/// 钉过（"应答必须点对点回给请求者"），心跳/退订应答的头字段今天没人看——
/// 把 `rr3_reply_ok` 的 dst 改成 `RR3_BROADCAST_DST_TASK` 就会在这里变红。
#[test]
fn replies_are_point_to_point() {
    let rrd = RrdTest::rr3_start("ptp");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut a);

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

// ---------- 发布面（Task 8）----------

/// 合法发布：叠加层在册 + 前缀是自己 → 转发给别的连接，且不回 ACK
#[test]
fn legal_publish_is_forwarded_to_other_peers() {
    let mut rrd = RrdTest::rr3_start("pub-ok");
    let (mut a, mut b) = (rrd.rr3_connect(), rrd.rr3_connect());
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut a);
    rr3_send(
        &mut b,
        &rr3_register_frame(&rr3_manifest("rr3-simb", 100, 500, "always"), "rr3-simb"),
    );
    rr3_read_rpc(&mut b);

    let ping = json!({"topic": "rr3-sima/ping/state", "seq": 1});
    rr3_send(
        &mut a,
        &rr3_json_frame(Rr3PayloadType::Event, &ping, 2, RR3_TASK_RRD),
    );
    let got = rr3_read_where(&mut b, " sima 的 ping 转发", |f| {
        rr3_json_of(f)["topic"] == "rr3-sima/ping/state"
    });
    assert_eq!(
        got.header.src_task, 2,
        "转发必须保留发布者 task 号（rrd 分配值，非自设）"
    );
    assert_eq!(
        got.header.dst_task,
        rr_bus::RR3_BROADCAST_DST_TASK,
        "发布是广播语义"
    );
    assert_eq!(rr3_json_of(&got)["seq"], 1, "体必须原样转发");
    rrd.rr3_assert_no_log("拒绝", Duration::from_millis(150));

    // F1（评审 I-1）：b 侧的正向断言只证明"帧到了别人"，不证明"没回到自己手上"。
    // "排除项 = 发布者 task" 是 `conn.rs` 的接线选择（`rr3_reg_fanout(&frame, me.task)`），
    // 只有 registry 级用例钉过它；改成不排除（`exclude_task = 0`，0 不是任何模块的 task）时
    // 上面三条全绿。这条否定断言就是那一个变异的红证：自回显会让"订阅后转发"型模块形成回环。
    let mine = rr3_collect_within(&mut a, Duration::from_millis(150));
    let my_topics: Vec<String> = mine
        .iter()
        .filter_map(|f| rr3_json_of(f)["topic"].as_str().map(str::to_string))
        .collect();
    assert!(
        !my_topics.iter().any(|t| t == "rr3-sima/ping/state"),
        "发布者不得收到自己的转发（fanout 排除项 = me.task）；a 侧 150 ms 内收到的 {} 帧 topic = {my_topics:?}",
        mine.len()
    );
}

/// 越权前缀：owner 是 simb 的主题由 sima 发 → AIRY_EPERM + 计数 + 不断连
///
/// 两个变量各打一层：`pong/state` 在册（三层顺序里的第一道"前缀不是自己"就把它拒了），
/// `pong/typo` **两条都不成立**（首段不是自己、主题又没登记）。少了"前缀"这一道就会
/// 落到"未注册主题"那条 EINVAL 上去——定性从"越权"被改成"拼错了"，是 M-C 变异的对象。
/// 只测在册的那一条会漏掉这个分支：末段的 owner 比对是**在册主题**上的兜底，
/// 对不在册的主题它根本不会执行。
#[test]
fn foreign_prefix_publish_is_eperm_and_counted() {
    let mut rrd = RrdTest::rr3_start("pub-foreign");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut a);

    for topic in ["rr3-simb/pong/state", "rr3-simb/pong/typo"] {
        rr3_send(
            &mut a,
            &rr3_json_frame(
                Rr3PayloadType::Event,
                &json!({"topic": topic}),
                2,
                RR3_TASK_RRD,
            ),
        );
        let v = rr3_read_rpc(&mut a);
        assert_eq!(
            v["error"]["code"],
            Rr3StatusCode::Permission as i32,
            "{topic} 越权 = AIRY_EPERM(-22)"
        );
        assert!(
            v["error"]["message"].as_str().unwrap().contains("越权发布"),
            "{topic}: {v}"
        );
    }
    assert!(rrd.rr3_wait_log("越权发布").contains("rr3-simb/pong/state"));
    // 不牵连：越权之后仍能正常喂狗
    rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 11, json!({}), 2));
    assert_eq!(rr3_read_rpc(&mut a)["result"]["beat"], "accepted");
}

/// 冒充内建命名空间（最危险的伪造面）：rr3-runtime/* 是 rrd 的下行广播，上行必拒
#[test]
fn impersonating_runtime_namespace_is_refused() {
    let rrd = RrdTest::rr3_start("pub-impersonate");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut a);

    for topic in ["rr3-runtime/system/heartbeat", "rr3-runtime/module/state"] {
        rr3_send(
            &mut a,
            &rr3_json_frame(
                Rr3PayloadType::Event,
                &json!({"topic": topic}),
                2,
                RR3_TASK_RRD,
            ),
        );
        let v = rr3_read_rpc(&mut a);
        assert_eq!(
            v["error"]["code"],
            Rr3StatusCode::Permission as i32,
            "{topic} 冒充必须 EPERM"
        );
        assert!(
            v["error"]["message"].as_str().unwrap().contains("冒充"),
            "{v}"
        );
    }
    // 冒充也只拒这一帧：同一条连接之后照常喂狗（与越权前缀同一口径，控制面不牵连）
    rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 13, json!({}), 2));
    assert_eq!(rr3_read_rpc(&mut a)["result"]["beat"], "accepted");
}

/// 在册但前缀对、主题没登记（拼错域名）→ AIRY_EINVAL，不是静默丢弃
#[test]
fn unregistered_topic_is_einval() {
    let rrd = RrdTest::rr3_start("pub-unknown");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut a);

    rr3_send(
        &mut a,
        &rr3_json_frame(
            Rr3PayloadType::Event,
            &json!({"topic": "rr3-sima/ping/typo"}),
            2,
            RR3_TASK_RRD,
        ),
    );
    let v = rr3_read_rpc(&mut a);
    assert_eq!(v["error"]["code"], Rr3StatusCode::Invalid as i32);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("未注册主题"),
        "{v}"
    );
}

/// 生产契约的主题不能被夹具发：没带 --topics-overlay 时 rr3-sima/* 根本不在册，
/// 而 rr3-sense/* 在册但 owner 不是 sima → 两种拒法都要成立。
/// 这条专治"叠加层被当成生产放行口"（K4）。
#[test]
fn overlay_is_not_a_production_pass() {
    let rrd = RrdTest::rr3_start_no_overlay("pub-noov");
    let mut a = rrd.rr3_connect();
    // 夹具 manifest 里没有 publishes，校验只对契约负责 → 注册本身合法
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut a);

    rr3_send(
        &mut a,
        &rr3_json_frame(
            Rr3PayloadType::Event,
            &json!({"topic": "rr3-sima/ping/state"}),
            2,
            RR3_TASK_RRD,
        ),
    );
    let v = rr3_read_rpc(&mut a);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("未注册主题"),
        "叠加层没加载就不该在册: {v}"
    );

    rr3_send(
        &mut a,
        &rr3_json_frame(
            Rr3PayloadType::Event,
            &json!({"topic": "rr3-sense/env/temperature"}),
            2,
            RR3_TASK_RRD,
        ),
    );
    let v2 = rr3_read_rpc(&mut a);
    assert!(
        v2["error"]["message"]
            .as_str()
            .unwrap()
            .contains("越权发布"),
        "生产主题 owner 是 sense: {v2}"
    );
}

/// Event 体里**没有 `topic` 键**：这是数据面的形状错误，只回错误帧，
/// **绝不按崩溃收尾**（评审裁定 R5）——连接留着，心跳照常被接受。
/// brief 的代码在这里用了 `?` 早退，会让 `rr3_serve_frame` 返回 `Err` → 断连 + 记一次崩溃 +
/// 可能触发重启，与它自己的文档注释和房内风格（src_task 伪造、payload 类型不支持都是
/// "回错误帧 + Keep"）冲突，故取注释为准。
#[test]
fn event_without_topic_key_replies_error_and_keeps_connection() {
    let mut rrd = RrdTest::rr3_start("pub-notopic");
    let mut a = rrd.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut a);

    rr3_send(
        &mut a,
        &rr3_json_frame(Rr3PayloadType::Event, &json!({"seq": 1}), 2, RR3_TASK_RRD),
    );
    let v = rr3_read_rpc(&mut a);
    assert_eq!(
        v["error"]["code"],
        Rr3StatusCode::Invalid as i32,
        "缺 topic 是形状错，不是越权: {v}"
    );
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("params.topic"),
        "文案必须点名缺哪个参数（rr3_rpc_topic_param 的既有文案）: {v}"
    );

    rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 14, json!({}), 2));
    assert_eq!(
        rr3_read_rpc(&mut a)["result"]["beat"],
        "accepted",
        "缺 topic 的 Event 不得带走连接"
    );
    rrd.rr3_assert_no_log("连接收尾", Duration::from_millis(150));
}

/// 同一条连接上现在同时有**应答 / 状态广播 / 发布转发**三类帧（Task 8 之后才成立），
/// 所以"挑帧"必须按内容而不是按到达序。这条是 Task 7 顺序无关性的第二型反例（遗留 ⑤）：
/// sima 连发两条不同 seq 的 Event，b 侧**先挑 seq=2 再挑 seq=1**（逆着到达序），
/// 之后再回头挑一条 Response —— 证明"先挑 Event"不会把后面的读法饿掉。
/// 夹具若退回"不匹配的帧即丢"，第二、三次读法就等不到帧。
#[test]
fn forwarded_events_are_read_by_content_not_arrival_order() {
    let rrd = RrdTest::rr3_start("pub-order");
    let (mut a, mut b) = (rrd.rr3_connect(), rrd.rr3_connect());
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut a);
    rr3_send(
        &mut b,
        &rr3_register_frame(&rr3_manifest("rr3-simb", 100, 500, "always"), "rr3-simb"),
    );
    // 必须先把 simb 的注册应答读掉再让 sima 发：在册是"应答写出去之前"发生的，
    // 不读就等于把两条 Event 的转发和第二条连接的入库放在一起赛跑。
    assert_eq!(rr3_read_rpc(&mut b)["result"]["src_task"], 3);

    for seq in [1u64, 2] {
        rr3_send(
            &mut a,
            &rr3_json_frame(
                Rr3PayloadType::Event,
                &json!({"topic": "rr3-sima/ping/state", "seq": seq}),
                2,
                RR3_TASK_RRD,
            ),
        );
    }

    // 逆序取：先要后发的那一条
    let second = rr3_read_where(&mut b, " seq=2 的转发", |f| {
        rr3_json_of(f)["topic"] == "rr3-sima/ping/state" && rr3_json_of(f)["seq"] == 2
    });
    let first = rr3_read_where(&mut b, " seq=1 的转发", |f| {
        rr3_json_of(f)["topic"] == "rr3-sima/ping/state" && rr3_json_of(f)["seq"] == 1
    });
    assert_eq!(second.header.src_task, 2);
    assert_eq!(
        first.header.src_task, 2,
        "两条转发的署名都是发布者的 task 号"
    );
    assert_eq!(second.header.dst_task, rr_bus::RR3_BROADCAST_DST_TASK);
    // 挑过两次 Event 之后，这条连接上还能按类型挑回 Response（b 自己发的心跳），
    // 压在队列里的那些状态广播一条也没被"扫掉"
    rr3_send(&mut b, &rr3_rpc_frame("rr3.heartbeat", 16, json!({}), 3));
    let hb = rr3_read_rpc(&mut b);
    assert_eq!(hb["id"], 16);
    assert_eq!(hb["result"]["beat"], "accepted");
    // 发布不回 ACK：a 侧的注册应答已被读走，此刻下一条应答必须正是心跳那一条（id=15）。
    // 应答都由这条连接的 reader 线程按处理序写，所以 Event 若被多回一帧，这里读到的就是它。
    rr3_send(&mut a, &rr3_rpc_frame("rr3.heartbeat", 15, json!({}), 2));
    let ack = rr3_read_rpc(&mut a);
    assert_eq!(
        ack["id"], 15,
        "Event 之后不该多出一条应答（发布是单向的）: {ack}"
    );
    assert_eq!(ack["result"]["beat"], "accepted");
}
