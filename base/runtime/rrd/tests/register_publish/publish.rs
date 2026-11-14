//! 发布面：前缀强制、命名空间冒充拒绝、主题在册与转发读法。

use super::register_sima;
use super::support::*;
use rr_bus::RR3_TASK_RRD;
use rr_ipc::{Rr3PayloadType, Rr3StatusCode};
use serde_json::json;
use std::time::Duration;

/// 合法发布：叠加层在册 + 前缀是自己 → 转发给别的连接，且不回 ACK
#[test]
fn legal_publish_is_forwarded_to_other_peers() {
    let mut rrd = RrdTest::rr3_start("pub-ok");
    let (mut a, mut b) = (rrd.rr3_connect(), rrd.rr3_connect());
    register_sima(&mut a);
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
    // needle 收窄（基-23 夹具适配）：监听面夹具改走缺格跳过装配单正门，启动期会打
    // 一行"拒绝启动 rr3-listenstub：缺格 act.servo"（见 support::rr3_listen_profile），
    // rr3_logs 全量回放必把它带进来。这条断言抓的是"合法发布被 rrd 拒"——发布拒因
    // 实词见 registry.rs（"未注册主题 …，拒绝发布"），用"拒绝发布"仍抓得住拒发，
    // 不再误伤启动期的"拒绝启动"（判据收窄，不变弱）。
    rrd.rr3_assert_no_log("拒绝发布", Duration::from_millis(150));

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
    register_sima(&mut a);

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
    register_sima(&mut a);

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
    register_sima(&mut a);

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
    register_sima(&mut a);
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
