#![cfg(unix)]
//! supervisor 的线上级证据（spec §5.3 重启/退避/启动序 + §5.4"rrd 是父进程、role 由 rrd 注入"
//! + §9 的"SIGKILL 夹具 → 判死 → 按策略重启 → 重新注册 → running""never 的夹具不重启并标 failed"）。
//!
//! 这是全项目**唯一真起子进程、真等退避**的测试目标：夹具由 rrd 自己 `Command::spawn`
//! （`--manifest` + `--boot-role`，E-6/E-7），身份来自 `tests/fixtures/*/module.toml`，
//! 行为脚本来自 `RR3_SIM_PEER_ROLE`。
//!
//! 三条口径先说清，后来者别改：
//! 1. **tick 必传小值**（`E2E_TICK_MS`）。Task 11b 的 E-5 让 supervisor 的**扫尸节拍**复用
//!    `--watchdog-tick-ms`，所以"安静 tick"在这里等于"收尸永远不发生"——boot 用例一律 20ms。
//! 2. **不靠 `sleep` 排序，也不写死 task/pid 号**：每一步都是 `rr3_wait_log`（内容）或
//!    `rr3_read_state`（内容），号一律 `rr3_field_from_log` 从日志里抽（A-5①）。
//!    线上帧同理按 `src_task` 比对：`rr3_reg_snapshot` 的补发会让一条晚到的旁观连接收到
//!    "上一世"的 state，只按到达序读就会把两世混成一世（`SCAN_BUDGET` 那套）。
//! 3. **否定 needle 只选不与别的文案共用子串的那几条**（A-5③ 第 3 条）：
//!    `spawn rr3-<名>`（只出现在成功起进程那一行）、`重启计划`（只出现在 conn 收尾那一路）、
//!    `跳过重启`/`取消重启`/`停止重启`（各一条自己的文案）。
//!    下面逐条注明它与哪条既有文案不重叠。
//!
//! **所有权边界**（E-1，与 `supervisor.rs` 文件头同一句话）：只有本守护起的模块会被重启，
//! 外来自注册的模块照样被记账、照样出计划，但执行时只打一行 `跳过重启`。S1/S3/S4 测 owned 路，
//! S2 把两条路放在同一框里对照。
mod support;

use std::process::Command;
use std::time::Duration;

use support::*;

/// 一条**只观察**的连接：注册完什么都不做，所以它的 `watchdog_timeout_ms` 取 60s
/// （远大于任何用例寿命）——它是仪器，不是被测对象；策略取 `never`，免得它出现在任何
/// 一条"重启计划/跳过重启"的计数里。
///
/// 与 `e2e_watchdog.rs` 的 `rr3_bystander` 同形状而**不共用**：本轮既禁改既有 28 条线级用例，
/// 就不能把那个函数搬进 `support`（搬动会改到 e2e_watchdog 的用例体）。这笔重复已记进实施报告，
/// 归 Task 13 的夹具整合。
fn rr3_observer(rrd: &mut RrdTest, name: &str) -> RrdClient {
    let mut c = rrd.rr3_connect();
    rr3_send(
        &mut c,
        &rr3_register_frame(&rr3_manifest(name, 100, 60_000, "never"), name),
    );
    // 等**关于自己**的那条 registered 广播，而不是 rrd 的日志：广播在 `rr3_reg_add` 之后才发，
    // 所以收到它就知道自己已在册（此后每条 fanout 都会算上这一份）。
    rr3_read_state(&mut c, name, "registered");
    c
}

/// 从"… 到 {n} 个连接"里抽那个数。写成数字断言而不是"这行里含有个连接"——
/// 后者与 needle 同源于一条格式串，抓不到任何事（`e2e_watchdog` W1 的同一口径）。
fn rr3_broadcast_count(line: &str) -> u64 {
    line.split("到 ")
        .nth(1)
        .and_then(|s| s.split(" 个连接").next())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("广播行里没有可解析的连接数：{line}"))
}

/// **卫生，不是判据**：尽力杀掉一个不属于本测试进程树的挂死夹具。
///
/// 为什么需要：`go_silent` 那一世睡在 `sleep(3600s)` 里、不再读 socket，rrd 死后没人收它
/// （std 的 `Child` drop 既不 kill 也不 wait，而 rrd 才是它的父进程）。S4 的判据链要求
/// "第三世也被起出来"，那第三世就是这条泄漏。
/// 为什么不算判据、也不算依赖：它不断言任何东西，失败一律忽略；且只动 `ps` 查出来
/// 确实是 `sim_peer` 的那一个——pid 被回收也不会误杀别人。
/// （`ps` 而不是 `/proc/{pid}/cmdline`：macOS 没有 /proc，照抄 Linux 写法这条卫生在
/// mac 上会静默变成 no-op，挂死夹具全部泄漏——S5 同一个教训。）
/// （产品侧的对应事实是已知边界③：rrd 不持有 SIGCHLD，也不在退出时杀子进程；S5 只管常驻支。）
fn rr3_kill_hung_fixture(pid: u64) {
    let ps = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output();
    let is_sim_peer = ps
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("sim_peer"))
        .unwrap_or(false);
    if is_sim_peer {
        let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
    }
}

/// S1 — DoD 主链：boot 起的模块真的上线（`running` 第一次有线上的脸），崩了按策略起第二世
#[test]
fn boot_spawned_module_reaches_running_then_restarts_after_crash() {
    let m = rr3_fixture_manifest("sr-s1", "sima"); // restart_policy = always
    let mut rrd = RrdTest::rr3_start_boot("sr-s1", &[&m.path], "abort", E2E_TICK_MS);
    let mut b = rr3_observer(&mut rrd, "rr3-obs");

    // ---- 第一世：boot 的那一次没有"第几次重启"可说 ----
    let spawn1 = rrd.rr3_wait_log("spawn rr3-sima pid=");
    assert!(
        !spawn1.contains("attempt="),
        "boot 的第一世不该带 attempt: {spawn1}"
    );
    assert!(
        spawn1.contains(&format!("entry={}", env!("CARGO_BIN_EXE_sim_peer"))),
        "spawn 行必须回显 manifest 里那条绝对 entry（占位符确实被替换过、路径确实按它起的）: {spawn1}"
    );
    let pid1 = rr3_field_from_log(&spawn1, "pid=");
    let reg1 = rrd.rr3_wait_log("注册 rr3-sima task=");
    assert!(
        reg1.contains("policy=always"),
        "身份与策略都来自 manifest 文件: {reg1}"
    );
    let task1 = rr3_field_from_log(&reg1, "task=");

    // ---- E-8：registered → running 第一次上线，日志有它、线上也有它 ----
    // **这一世不钉广播数**（写实，不是偷懒）：boot 队列在 accept 循环之前就投完
    // （`daemon.rs` 的序），于是"夹具的第一颗心跳"与"本用例这条旁观连接的注册"谁先谁后
    // 是两种合法时序——整个 workspace 并发跑时实测到过 `1`（旁观者尚未在册），
    // 单跑这个二进制时是 `2`。数字断言挪到第二世（下面 `run2`），那里的计数才有唯一答案。
    rrd.rr3_wait_log("rr3-sima → running（广播到");
    let on = rr3_read_state(&mut b, "rr3-sima", "running");
    assert_eq!(
        on["src_task"], task1,
        "线上那条 running 必须是第一世的（按 src_task 认，不按到达序猜）"
    );

    // ---- 终态：owned 的模块由退出码定性（spec §5.4 崩溃检测双路的那一路） ----
    let exit1 = rrd.rr3_wait_log("rr3-sima 子进程退出 status=signal: 6");
    assert!(
        exit1.contains("→ 广播 failed 到 1 个连接"),
        "abort 是信号死：status 必须是 signal: 6，且只发给旁观者（发起者已被 exclude）: {exit1}"
    );
    let on = rr3_read_state(&mut b, "rr3-sima", "failed");
    assert_eq!(on["src_task"], task1, "线上那条 failed 也必须指向第一世");
    // EOF 让位给退出码：conn 那一路对 owned 的模块一句都不广播（A-6 的分工）
    rrd.rr3_wait_log("rr3-sima 连接断开（干净收尾），终态交给子进程退出码那一路");

    // ---- 第二世：退避到期后真的起了另一个进程、重新注册、再次 running ----
    let spawn2 = rrd.rr3_wait_log("spawn rr3-sima pid=");
    let pid2 = rr3_field_from_log(&spawn2, "pid=");
    assert_ne!(pid1, pid2, "重启必须是一个新进程，不是把同一行读第二遍");
    assert!(
        spawn2.contains("attempt=1"),
        "第二次必须报出这是第几次重启: {spawn2}"
    );
    let reg2 = rrd.rr3_wait_log("注册 rr3-sima task=");
    let task2 = rr3_field_from_log(&reg2, "task=");
    assert_ne!(
        task1, task2,
        "task 由 rrd 递增分配，重启后是新号（旧号回收不得复用）"
    );
    let run2 = rrd.rr3_wait_log("rr3-sima → running（广播到");
    // 此刻在册且带句柄的恰好两条：旁观者 + 第二世自己（旁观者早在第一世之前就在册，
    // 第一世又活了整整一世，所以这一处的计数没有上面那种时序歧义）。
    // E-9 在这一路排的是 `RR3_TASK_RRD`，等于**不排除**——发起者自己也收到关于自己的
    // running；谁"顺手把发起者排除掉"会让这一条当场红，而 A-3 还没替这两处口径做决定。
    assert_eq!(
        rr3_broadcast_count(&run2),
        2,
        "running 该发给当时在册的两条连接: {run2}"
    );
    let on = rr3_read_state(&mut b, "rr3-sima", "running");
    assert_eq!(
        on["src_task"], task2,
        "§9 的\"重启成功后必须重新注册才恢复 running\"——线上那一帧属于第二世"
    );
}

/// S2 — A-6 的对照：**同一个 abort role、两种所有权、两种线上相位**。
/// 删掉 owned 那半，这条就退化成 11a 之前的现值（"崩溃检测分不清被打死与体面退场"）。
#[test]
fn an_owned_crash_is_failed_while_a_foreign_one_is_unregistered() {
    let owned = rr3_fixture_manifest("sr-s2-own", "simb"); // 被 boot 的那一份
    let foreign = rr3_fixture_manifest("sr-s2-fgn", "sima"); // 测试自己起的那一份
                                                             // 夹具默认的 wd=500ms 是 e2e_watchdog 的判据，不是本用例要的：这里钉的是"退出码定性"，
                                                             // 必须把 watchdog 那一路从判据里排掉（负载下收尸可能晚于 500ms，谁先记就是掷硬币）。
    rr3_fixture_watchdog_ms(&owned, 60_000);
    rr3_fixture_watchdog_ms(&foreign, 60_000);
    let mut rrd = RrdTest::rr3_start_boot("sr-s2-own", &[&owned.path], "abort", E2E_TICK_MS);
    let mut b = rr3_observer(&mut rrd, "rr3-obs");

    // ---- owned：退出码是死因的权威 → failed ----
    // 这一行必须被**消费掉**（`rr3_assert_no_log` 扫的是累计已读行，见下面那条否定断言）；
    // boot 起的必须是 simb，行里的 `entry=` 已由 S1 钉过，本用例只取"起过的是谁"这一件事。
    let spawn = rrd.rr3_wait_log("spawn rr3-simb pid=");
    let own_task = rr3_field_from_log(&rrd.rr3_wait_log("注册 rr3-simb task="), "task=");
    let exit = rrd.rr3_wait_log("rr3-simb 子进程退出 status=signal: 6");
    assert!(
        exit.contains("→ 广播 failed"),
        "owned 的死因由退出码定性: {exit}"
    );
    let on = rr3_read_state(&mut b, "rr3-simb", "failed");
    assert_eq!(
        on["src_task"], own_task,
        "线上那条 failed 指向 boot 起的那一世"
    );

    // ---- foreign：只有 EOF 这一路信号 → unregistered，且计划被跳过 ----
    let mut peer = SimPeer::rr3_start(&rrd.socket, &foreign, "abort");
    peer.rr3_wait("即将 abort");
    let fgn_task = rr3_field_from_log(&rrd.rr3_wait_log("注册 rr3-sima task="), "task=");
    let plan = rrd.rr3_wait_log("rr3-sima 退出（干净收尾）→ 重启计划 attempt=1 delay_ms=20");
    assert!(
        plan.contains(&format!("entry={}", env!("CARGO_BIN_EXE_sim_peer")))
            && plan.contains("（非本守护起的模块不 spawn）"),
        "外来模块照样出计划、照样点名它为什么不执行: {plan}"
    );
    rrd.rr3_wait_log("跳过重启 rr3-sima");
    let on = rr3_read_state(&mut b, "rr3-sima", "unregistered");
    assert_eq!(
        on["src_task"], fgn_task,
        "同一台机器上两种结局：owned 的 simb 是 failed，非 owned 的 sima 只有 unregistered"
    );
    // 两条 owned 事实同框：起过的是 simb（上面那行 `spawn ` 就是它），而 sima 一次都没被起。
    // needle 选 `spawn rr3-sima`：`spawn ` 只在成功起进程那一行出现，且 `rr3-sima` 不是
    // `rr3-simb` 的前缀，不会被 owned 那一行顶掉。
    rrd.rr3_assert_no_log("spawn rr3-sima", Duration::from_millis(300));
    assert!(
        !spawn.contains("rr3-sima"),
        "boot 起的必须是 simb 而不是 sima（否则上面的否定断言什么也没测）: {spawn}"
    );
}

/// S3 — `never` 策略的崩溃：一条 failed 广播 + 绝不再起一个进程
#[test]
fn never_policy_crash_does_not_spawn_again() {
    let m = rr3_fixture_manifest("sr-s3", "simb"); // restart_policy = never
                                                   // 同 S2：钉的是退出码那一路的定性 + never 不再起，500ms watchdog 抢记会把两路搅在一起
    rr3_fixture_watchdog_ms(&m, 60_000);
    let mut rrd = RrdTest::rr3_start_boot("sr-s3", &[&m.path], "abort", E2E_TICK_MS);
    let mut b = rr3_observer(&mut rrd, "rr3-obs");

    // boot 那一行必须被消费掉（`rr3_assert_no_log` 扫的是累计已读行）
    rrd.rr3_wait_log("spawn rr3-simb pid=");
    let task1 = rr3_field_from_log(&rrd.rr3_wait_log("注册 rr3-simb task="), "task=");
    rrd.rr3_wait_log("rr3-simb → running（广播到");
    let exit = rrd.rr3_wait_log("rr3-simb 子进程退出 status=signal: 6");
    assert!(
        exit.contains("→ 广播 failed 到 1 个连接"),
        "§9：never 的夹具照样标 failed: {exit}"
    );
    assert_eq!(
        rr3_read_state(&mut b, "rr3-simb", "failed")["src_task"],
        task1,
        "只广播这一条"
    );

    // 三条否定各钉一层，窗口都用"已知的退避基数"量不出来（判据是内容，不是时长）：
    // - `spawn rr3-simb`：确实没有第二个进程（`spawn ` 只属于成功起进程那一行）
    // - `重启计划`：策略这一层就说了不（不是所有权说的）——四字只出现在 conn 收尾那一路
    // - `跳过重启`：E-1 那一层也没被走到（四条文案互不重叠，见文件头顶上第 3 条）
    rrd.rr3_assert_no_log("spawn rr3-simb", Duration::from_millis(300));
    rrd.rr3_assert_no_log("重启计划", Duration::from_millis(150));
    rrd.rr3_assert_no_log("跳过重启", Duration::from_millis(150));
}

/// S4 — 第二条崩溃路：watchdog 到期判死一个 owned 的挂死模块 → **先杀再起** → 重新注册
#[test]
fn watchdog_expiry_of_an_owned_hang_kills_and_restarts() {
    let m = rr3_fixture_manifest("sr-s4", "sima"); // always：判死那一路的计划会执行
    let mut rrd = RrdTest::rr3_start_boot("sr-s4", &[&m.path], "go_silent", E2E_TICK_MS);

    let spawn1 = rrd.rr3_wait_log("spawn rr3-sima pid=");
    let pid1 = rr3_field_from_log(&spawn1, "pid=");
    let task1 = rr3_field_from_log(&rrd.rr3_wait_log("注册 rr3-sima task="), "task=");
    // needle 带 task 号点名是哪一世（W2/W3 的同一口径）
    let dead1 = rrd.rr3_wait_log(&format!("watchdog 判死 rr3-sima task={task1} "));
    assert!(!dead1.contains("attempt="), "判死行只描述判死本身: {dead1}");

    // "杀掉挂死的"这一行是唯一能区分"重起了"与"重起了但旧的去哪了"两种说法的证据：
    // std 的 `Child` 被覆盖不等于杀进程，删掉那一行这条用例必须红（brief Step 7 的另一半）。
    let kill1 = rrd.rr3_wait_log("杀掉挂死的 rr3-sima pid=");
    assert_eq!(
        rr3_field_from_log(&kill1, "pid="),
        pid1,
        "杀掉必须正是刚判死的那一世"
    );

    let spawn2 = rrd.rr3_wait_log("spawn rr3-sima pid=");
    let pid2 = rr3_field_from_log(&spawn2, "pid=");
    assert_ne!(pid1, pid2, "重起的是另一个进程");
    assert!(spawn2.contains("attempt=1"), "{spawn2}");
    let task2 = rr3_field_from_log(&rrd.rr3_wait_log("注册 rr3-sima task="), "task=");
    assert_ne!(
        task1, task2,
        "第一世的条目让位了才有新 task：同名注册被 EBUSY 挡住就是红在这里"
    );

    // 第二世**又**被判死（这条不许省）：杀不干净就会有"活着的"进程占着 name，
    // 第二世判死说明第一世的 fd/条目确实让位了。
    rrd.rr3_wait_log(&format!("watchdog 判死 rr3-sima task={task2} "));
    let kill2 = rrd.rr3_wait_log("杀掉挂死的 rr3-sima pid=");
    assert_eq!(
        rr3_field_from_log(&kill2, "pid="),
        pid2,
        "第二次杀的必须是第二世"
    );
    let spawn3 = rrd.rr3_wait_log("spawn rr3-sima pid=");
    let pid3 = rr3_field_from_log(&spawn3, "pid=");
    assert_ne!(
        pid2, pid3,
        "第二次重启的必须是第三个进程，不是把同一行读第二遍"
    );
    // 尾注**钉在 attempt=1 而不是"递增到 2"**（写实，不是猜）：11a 的 D-2 归零与判死同在一趟
    // 扫描里，段序是先归零后到期（`rr3_reg_expire`），而"注册→喂一颗→挂死"的模块到期时
    // `now - registered_at ≥ 500ms` 必然成立 → streak 每次都被清成 0 → 每一世的计划都是
    // attempt=1。真正"递增到上限"的形状是 crash loop（起来就死、活不满窗口），那条由
    // registry 的表级用例钉（含归零那一条），本用例不假装它在测这件事。
    assert!(
        spawn3.contains("attempt=1"),
        "到期那一路的计划要真的被执行: {spawn3}"
    );
    // 判据到此为止。第三世还是个挂死的夹具，测试进程是它名义上的"旁观者"却救不了它——
    // 只做卫生清理（不断言、不报错），别把一堆 sleep(3600s) 留给后面的用例与 CI。
    rr3_kill_hung_fixture(pid3);
}

/// S5 — 孤儿卫生：rrd 死了不许留下 boot 起的常驻夹具（CI 污染，不是理论风险）
#[test]
fn rrd_death_does_not_leave_boot_spawned_children() {
    let m = rr3_fixture_manifest("sr-s5", "sima");
    let mut rrd = RrdTest::rr3_start_boot("sr-s5", &[&m.path], "resident", E2E_TICK_MS);
    let spawn = rrd.rr3_wait_log("spawn rr3-sima pid=");
    let pid = rr3_field_from_log(&spawn, "pid=");
    rrd.rr3_wait_log("注册 rr3-sima task=");
    // 前置必须先成立：进程还活着，否则"它消失了"什么也没测。
    // 判活走 `rr3_pid_alive` 而不是 `/proc/{pid}`：macOS 没有 /proc，照抄 Linux 写法
    // 这条前置在 mac 上恒假，S5 从来没法在 mac 上跑到 drop 那一半。
    assert!(rr3_pid_alive(pid), "夹具必须还在: {spawn}");

    drop(rrd); // → `RrdTest::drop` 杀 rrd 并 unlink 套接字

    // 依据是读代码读来的，不是猜的：`sim_peer::rr3_resident` 每颗心跳都读应答，rrd 一死
    // socket 即 EOF → `rr3_wait_reply` 立刻 Err → 进程退出。
    // 有界轮询：判据是"消失"这件事本身，2s 只是不再等的上限（与 `rr3_wait_line` 同一口径）。
    let gone = rr3_wait_until(|| !rr3_pid_alive(pid), Duration::from_secs(2));
    assert!(
        gone,
        "rrd 死后 boot 起的常驻夹具必须自己退出；pid={pid} 两秒内没消失"
    );
}
