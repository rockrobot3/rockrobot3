#![cfg(unix)]
//! 跨进程端到端：第二个**真进程**（sim_peer）走 rrd 的线上入口完成 注册→心跳→发布→退场。
//!
//! 前面 19 条 `register_publish` 的客户端是**测试自己**（同进程、同二进制、手搓帧）。
//! 这一组换成另一个二进制去连：它自带一份独立的编解码调用链，"协议层不是测试自己跟自己
//! 说话"这句话第一次有证据。它也顺带独立复证了 Task 7 的并发形状——夹具按 id 挑应答、
//! 不按到达序，第二份实现写出来还能绿，才说明那条约定不是只在测试脑子里成立。
//!
//! 身份来自 manifest 文件（`RR3_MANIFEST`），`RR3_SIM_PEER_ROLE` 只选行为脚本。

mod support;

use rr_ipc::{rr3_body_split, Rr3PayloadType};
use std::time::Duration;
use support::*;

/// 真进程完成握手、task 取自 rrd 的应答（不是自设），心跳被连续接受
#[test]
fn sim_peer_registers_and_beats_are_accepted() {
    let mut rrd = RrdTest::rr3_start("e2e-beat");
    let manifest = rr3_fixture_manifest("beat", "sima");
    let mut peer = SimPeer::rr3_start(&rrd.socket, &manifest, "resident");

    let line = rrd.rr3_wait_log("注册 rr3-sima");
    // 相等这一条同时钉住两件事：夹具真的从线上应答里读出了 src_task（不是自己猜的），
    // 且它后来喂狗用的就是这个值（用错会被 "src_task 由 rrd 分配" 挡回，挡回就打不出下一行）。
    let echoed = peer.rr3_wait("注册成功 name=rr3-sima");
    let task_in_rrd = line
        .split("task=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or("")
        .to_string();
    assert_eq!(
        echoed.split("task=").nth(1).unwrap_or("").trim(),
        task_in_rrd,
        "夹具回显的 task 必须等于 rrd 分配的 task；rrd: {line} / peer: {echoed}"
    );

    peer.rr3_wait("连喂 3 次均被接受");
    assert!(peer.rr3_alive(), "喂狗期间夹具不该退出");
    rrd.rr3_assert_no_log("退出", Duration::from_millis(300));
}

/// 真进程发布 → **早已在册**的旁观连接收到转发帧，体原样、类型仍是 Event、src_task 是发布者的分配值
#[test]
fn sim_peer_publish_reaches_another_connection() {
    let mut rrd = RrdTest::rr3_start("e2e-pub");

    // 旁观连接必须先上线：转发只发给"当时在册"的连接，晚注册的收不到（Task 7 的 fanout 语义）。
    // 这也决定了 task 号：b 先到拿 2，夹具稍后拿 3 —— 所以断言比对的是 rrd 日志里那个分配值，
    // 不是写死的 2。
    let mut b = rrd.rr3_connect();
    rr3_send(
        &mut b,
        &rr3_register_frame(&rr3_manifest("rr3-simb", 100, 500, "always"), "rr3-simb"),
    );
    rr3_read_rpc(&mut b);

    let manifest = rr3_fixture_manifest("pub", "sima");
    let mut peer = SimPeer::rr3_start(&rrd.socket, &manifest, "publish_once");
    peer.rr3_wait("已发布 rr3-sima/ping/state");
    let publisher_task = rrd
        .rr3_wait_log("注册 rr3-sima task=")
        .split("task=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or("")
        .to_string();

    let got = rr3_read_where(&mut b, " sima 发布的 ping", |f| {
        rr3_json_of(f)["topic"] == "rr3-sima/ping/state"
    });
    assert_eq!(
        got.header.src_task.to_string(),
        publisher_task,
        "转发帧的 src_task 必须等于 rrd 分配给发布者的 task"
    );
    assert_eq!(rr3_json_of(&got)["seq"], 1, "体必须逐字穿过 rrd，不被改写");
    let (ty, _) = rr3_body_split(&got.payload).expect("前缀合法");
    assert_eq!(ty, Rr3PayloadType::Event, "转发不改 payload 类型");
}

/// 主动 unregister → 恰好一条 unregistered 广播；never 策略不产生重启计划
#[test]
fn sim_peer_clean_exit_broadcasts_unregistered_and_never_restarts() {
    let mut rrd = RrdTest::rr3_start("e2e-clean");

    // 同上：收广播的连接必须在夹具退场之前就已在册
    let mut b = rrd.rr3_connect();
    rr3_send(
        &mut b,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut b);

    let manifest = rr3_fixture_manifest("clean", "simb"); // never 策略
    let mut peer = SimPeer::rr3_start(&rrd.socket, &manifest, "exit_clean");
    peer.rr3_wait("已主动退场");

    rr3_read_state(&mut b, "rr3-simb", "unregistered");
    rrd.rr3_wait_log("rr3-simb 退出（干净收尾）");
    rrd.rr3_assert_no_log("重启计划", Duration::from_millis(300));
}

/// 特征化用例（**不是**验收"崩溃检测已做好"）：abort 与主动退场在这条连接上同为一次 EOF。
/// 为什么 11b 之后它仍然是 EOF：这个进程**不是 rrd 起的**（E-1）——退出码握在别人的父进程里，
/// rrd 只能给出 `unregistered` 加一行 `跳过重启`。owned 那一路（真退出码 → `failed`）
/// 由 `e2e_spawn_restart.rs` 的 S2 与它同框对照。
#[test]
fn killed_foreign_peer_is_only_an_eof_and_its_plan_is_skipped() {
    let mut rrd = RrdTest::rr3_start("e2e-abort");

    let mut b = rrd.rr3_connect();
    rr3_send(
        &mut b,
        &rr3_register_frame(&rr3_manifest("rr3-simb", 100, 500, "always"), "rr3-simb"),
    );
    rr3_read_rpc(&mut b);

    let manifest = rr3_fixture_manifest("abort", "sima"); // always 策略
    let mut peer = SimPeer::rr3_start(&rrd.socket, &manifest, "abort");
    peer.rr3_wait("即将 abort");

    // EOF 走 conn.rs 的 `Rr3FrameError::Closed` 分支 → clean_exit = true：
    // 广播 phase 是 unregistered（不是 failed），而 always 策略照样给出重启计划。
    // rrd 分不清"被信号杀掉"与"体面退出"，因为这条连接上它只看得见 EOF；
    // 退出码在**父进程**手里，而那个父进程不是 rrd（E-1 的所有权边界）——所以这一格
    // 在 11b 之后仍然只有 `unregistered`，spec §5.4 的"崩溃检测双路"里只有 conn 这一路被走到。
    rr3_read_state(&mut b, "rr3-sima", "unregistered");

    let plan = rrd.rr3_wait_log("重启计划 attempt=1 delay_ms=20");
    // 这一条把"被打死走的到底是哪条读分支"也钉住：夹具在 abort 前把接收队列排空，于是内核以
    // FIN 收尾 → `Rr3FrameError::Closed` → `clean_exit = true` → 文案是"干净收尾"。
    // 队列里留着未读广播时改成 RST → rrd 读到 ECONNRESET → 这里是"帧读取失败…→ failed 广播"，
    // 本用例的 unregistered 判据就会随机红（实测未排空 10 次红 1 次）。RST 那一支**仍然没人测**
    // ——11b 把"退出码定性"补给了 owned 那一路（`e2e_spawn_restart` 的 S1/S2），而这条连接是
    // 外来的，它连 RST 都只会读成一次 EOF。这里排掉混淆项，不是把它判给"不存在"。
    assert!(plan.contains("退出（干净收尾）"), "现值：EOF 才叫干净收尾；实际 {plan}");
    assert!(
        plan.contains(&format!("entry={}", env!("CARGO_BIN_EXE_sim_peer"))),
        "重启计划必须回显 manifest 里那条绝对 entry（占位符确实被替换过的证据）: {plan}"
    );
    // 11b 的新事实：计划真的投递出去了，而 supervisor 不执行它（`jobs` 里没有这个 name，E-1）。
    // 正面钉这一行是把"接线断了"与"接线在、但被判成外来"分开——只留上面那两条 `contains`，
    // `sup.rr3_restart(p)` 被删掉这条用例照样绿。
    rrd.rr3_wait_log("跳过重启 rr3-sima");
}

/// 同一个二进制、两份 manifest → 两个身份；role 只改行为、不改身份（两边都用正面断言）
#[test]
fn identity_comes_from_manifest_not_from_the_role_env() {
    let mut rrd = RrdTest::rr3_start("e2e-identity");
    let ma = rr3_fixture_manifest("id-a", "sima");
    let mb = rr3_fixture_manifest("id-b", "simb");

    // 两个 resident 进程，role 完全相同；身份与策略都来自各自 manifest
    let mut p1 = SimPeer::rr3_start(&rrd.socket, &ma, "resident");
    p1.rr3_wait("注册成功 name=rr3-sima");
    let mut p2 = SimPeer::rr3_start(&rrd.socket, &mb, "resident");
    p2.rr3_wait("注册成功 name=rr3-simb");

    // task 号各不同且按注册序递增（rrd 分配，与被测进程无关）
    assert!(rrd.rr3_wait_log("注册 rr3-sima task=2").contains("policy=always"));
    assert!(rrd.rr3_wait_log("注册 rr3-simb task=3").contains("policy=never"));

    // 换 role 不换身份：行为变了（它会发布），name 仍是 manifest 里那个。
    // 先 drop p1：同名重复注册会被 EBUSY 挡住（Task 7 已钉），那样测不到 role 这一维。
    drop(p1);
    // 再等 rrd 把 p1 那条连接的收尾做完才起 p3：同名在册条目要等 conn 线程走到
    // `rr3_conn_finish` 才腾出来。不等就是拿 fork/exec 的先后当同步手段——本轮 20 次实测全绿
    // 也只说明运气好，不说明判据成立。needle 只取"退出"两字，因为 EOF 与 ECONNRESET
    // 两支文案不同（干净收尾 / 帧读取失败），而这里要钉的是"收尾发生过"，不是"怎么收的"。
    rrd.rr3_wait_log("rr3-sima 退出");
    let mut p3 = SimPeer::rr3_start(&rrd.socket, &ma, "publish_once");
    p3.rr3_wait("注册成功 name=rr3-sima");
    p3.rr3_wait("已发布 rr3-sima/ping/state");
}
