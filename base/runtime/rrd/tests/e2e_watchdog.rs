//! watchdog 的线上级证据（spec §5.0"心跳"段 / §9 的崩溃判死项）。
//!
//! 客户端一律是**第二个真进程** `sim_peer`（Task 9 立的夹具），不是测试自己手搓帧：
//! "不喂狗但连接还开着"这个形状必须由真进程维持打开的 fd 才成立——手搓帧的用例一注册完
//! 就 EOF 了，watchdog 在那种形状上根本无从判活。
//!
//! 时间口径：`E2E_TICK_MS = 20`、夹具 manifest `watchdog_ms = 500` → 到期上界 520ms。
//! **所有等待都是内容判据**（`rr3_wait_log` 3s / `SimPeer::rr3_wait` 5s），
//! 唯一的时长断言在 W1，且是粗上限（抓"线程根本没跑"，不抓"机器恰好慢"）。
//!
//! **本文件是 4 条而不是计划里的 5 条**：W5（`watchdog_tick_from_cli_reaches_the_thread`）
//! 与 `support` 里 `rr3_spawn_with_overlay` 末尾那句
//! `rr3_wait_log("watchdog 启动 tick={watchdog_tick_ms}ms")` 是同一条判据——本文件的 4 条
//! 已经把接线钉在 `20` 上，既有 19+1+5 条线级用例（`register_publish` / `socket_banner` /
//! `e2e_sim_peer`）又把它钉在 `3600000` 上，两个互不相同的值都只能
//! 由 CLI 传进来才对得上（线程里硬编码 200、或把 `pre_register_timeout_ms` 当 tick 传，
//! 都会让这两批用例同时超时红）。再加一条只 `assert!(socket.exists())` 的用例不新增任何
//! 判据（套接字存在已被"等到监听那一行"蕴含），按 Global Constraints 的"不许空转断言"
//! 这条就不写。裁定记录见本轮实施报告。
#![cfg(unix)]

mod support;

use rr_ipc::{rr3_body_split, Rr3PayloadType};
use serde_json::Value;
use std::time::Duration;
use support::*;

/// 静默 peer + 小 tick 的 rrd。返回顺序固定：先起 rrd，再起被观察的东西。
fn rr3_watched(tag: &str) -> (RrdTest, SimManifest) {
    let rrd = RrdTest::rr3_start_with_tick(tag, E2E_TICK_MS);
    let m = rr3_fixture_manifest(tag, "sima");
    (rrd, m)
}

/// 旁观连接：一个手搓帧注册的观察者。它的 manifest 带一个**远大于用例寿命**的
/// watchdog（60s），这样它自己永不参与判死——它是仪器，不是被测对象。
/// 刻意不把它改成"常驻喂狗的 sim_peer"：那会让"旁观者自己也进过判死"变成隐式前提，
/// 而 W3 的计数口径就建立在这个前提不存在之上。
/// 必须先起它、再起被观察的那个：`rr3_reg_fanout` 只发给已在册的连接，晚来的收不到过去的事。
/// 策略取 `never`：这条连接不该在任何"重启计划"的计数里出现，否则 W3 的广播数会被
/// 一个和判死无关的模块扰动。
fn rr3_bystander(rrd: &RrdTest, name: &str) -> RrdClient {
    let mut c = rrd.rr3_connect();
    rr3_send(&mut c, &rr3_register_frame(&rr3_manifest(name, 100, 60_000, "never"), name));
    rr3_read_state(&mut c, name, "registered");
    c
}

/// 判死日志里的 elapsed（内容判据的手）
fn rr3_elapsed_from_log(line: &str) -> u64 {
    line.split("距上次心跳 ")
        .nth(1)
        .and_then(|s| s.split("ms").next())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("判死日志里没有可解析的 elapsed：{line}"))
}

/// W1 — 静默即判死，且判死内容与广播都到位
#[test]
fn watchdog_expires_a_silent_peer_and_broadcasts_failed() {
    let (mut rrd, m) = rr3_watched("wd-expire");
    let mut b = rr3_bystander(&rrd, "rr3-simb");
    let mut peer = SimPeer::rr3_start(&rrd.socket, &m, "go_silent");
    peer.rr3_wait("静默中（已喂 1 次）");

    let line = rrd.rr3_wait_log("watchdog 判死 rr3-sima task=");
    let elapsed = rr3_elapsed_from_log(&line);
    assert!(line.contains("≥ 超时 500ms"), "必须点名该模块自己的 watchdog_ms: {line}");
    assert!(elapsed >= 500, "elapsed 必须 ≥ manifest 的 watchdog_ms，实际 {elapsed}: {line}");

    // 唯一的时长断言：判死必须**最终**发生（`rr3_wait_log` 自带 3s 上限即这条"最终"）。
    // 刻意不写"520ms 内判死"那种细上限：它抓不到任何本任务真正关心的缺陷
    // （线程没起 / 扫了但判错 / 判了不广播），却会把"机器恰好慢"变成随机红。
    // 3s 的粗上限足够抓"watchdog 压根没跑"。
    rr3_read_state(&mut b, "rr3-sima", "failed");
    assert!(rrd.rr3_wait_log("watchdog 广播 failed 到").contains("个连接"));
}

/// W2 — 健康模块不受牵连（正反两路同框）
#[test]
fn watchdog_leaves_a_beating_peer_alone() {
    let (mut rrd, ma) = rr3_watched("wd-healthy");
    let mb = rr3_fixture_manifest("wd-healthy-b", "simb");
    // 两个身份、两种结局，只差"喂不喂狗"这一维——所以这条抓的是"watchdog 会不会一律判死"。
    let mut silent = SimPeer::rr3_start(&rrd.socket, &ma, "go_silent");
    silent.rr3_wait("静默中（已喂 1 次）");
    let mut beating = SimPeer::rr3_start(&rrd.socket, &mb, "resident");
    beating.rr3_wait("连喂 3 次均被接受");

    rrd.rr3_wait_log("watchdog 判死 rr3-sima");
    // 否证而非计时：simb 一直在喂，任何时刻都不该被点名。不用 "sleep 之后还没死" 的写法，
    // 那是在猜"过了多久"。
    rrd.rr3_assert_no_log("watchdog 判死 rr3-simb", Duration::from_millis(300));
    assert!(beating.rr3_alive(), "健康模块的连接不该被 watchdog 收掉");
    // 静默的那条同样还活着：watchdog 判的是"在册相位"，不是"要不要关你的 fd"
    // （spec §5.0 只说"超时即判 crash"，断开连接是另一件事，本轮没做也不该被顺带做掉）。
    assert!(silent.rr3_alive(), "判死不等于断开连接");
}

/// W3 — 恰好一条 failed 广播、一条判死日志（跨 ≥3 tick 不重复回报）
#[test]
fn watchdog_broadcasts_exactly_one_failed_state() {
    let (mut rrd, m) = rr3_watched("wd-once");
    let mut b = rr3_bystander(&rrd, "rr3-simb");
    let mut peer = SimPeer::rr3_start(&rrd.socket, &m, "go_silent");
    peer.rr3_wait("静默中（已喂 1 次）");
    rrd.rr3_wait_log("watchdog 判死 rr3-sima");

    // 判死之后再跨 3 拍（60ms）收：每拍都重复回报的话，这里会数到 >1。
    // 窗口跨度由已知 tick 算出，不是猜被测行为的时间。
    let got = rr3_collect_within(&mut b, Duration::from_millis(60));
    let failed = got
        .iter()
        .filter(|f| match rr3_body_split(&f.payload) {
            Ok((Rr3PayloadType::Notification, body)) => serde_json::from_slice::<Value>(&body)
                .map(|v| v["phase"] == "failed" && v["module"] == "rr3-sima")
                .unwrap_or(false),
            _ => false,
        })
        .count();
    assert_eq!(failed, 1, "`rr3_reg_expire` 跳过 failed，第二拍起不得再回报同一条: {failed} 条");

    // 日志侧同样只能有一条判死行。needle 带到 `task=` 是为了与 W2 的口径一致
    // （点名身份，不点名泛化前缀）。**这条依赖上一行的 `rr3_wait_log` 已经把第一条消费掉**
    // （`seen`/通道语义见 Task 8 的实现），所以它测的是"有没有第二条"，
    // 不是"一条都不该有"——后来者必读。
    rrd.rr3_assert_no_log("watchdog 判死 rr3-sima task=", Duration::from_millis(300));
}

/// W4 — 判死后喂狗一律被拒、且不复活、不产重启计划
#[test]
fn beats_after_expiry_are_rejected_and_do_not_revive() {
    let (mut rrd, m) = rr3_watched("wd-revive");
    let mut peer = SimPeer::rr3_start(&rrd.socket, &m, "silent_then_beat");
    // 判据只取**最后一次**（第 1300ms 的那颗）：那一刻已静默 ≥780ms，远超到期上界 520ms，
    // 所以"被拒"不是运气。前四次不判——watchdog 恰好在两次喂狗之间到期是完全合法的时序。
    peer.rr3_wait("喂狗 #5 被拒");
    peer.rr3_wait("已试喂 5 次");

    let line = rrd.rr3_wait_log("watchdog 判死 rr3-sima");
    // 到期路径今天**不查 restart_policy**、不产重启计划。这是本任务显式留给 Task 11 的
    // 欠账，钉在这里是为了让它无法被悄悄宣布成"崩溃检测已完成"。
    assert!(!line.contains("attempt="), "判死路径今天不产重启计划（Task 11 的活）: {line}");
}
