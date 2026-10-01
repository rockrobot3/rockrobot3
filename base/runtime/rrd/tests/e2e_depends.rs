#![cfg(unix)]
//! ADR-0009 裁定的线级证据：boot 依赖仲裁（裁定 1/2）与降级传播（裁定 3/4）。
//!
//! 三条用例对应三个裁定面：
//! - D1 **链式启动序**（裁定 1/2 的"序"半边）：dependent 的 `start_priority` 故意比
//!   provider 低（10 vs 50）——旧代码按全局优先级升序排 boot 队列会先起 dependent，
//!   新仲裁的拓扑主键压过优先级，先起 provider。这是"拓扑主键 > start_priority"最强判据。
//! - D2 **环拒绝 boot**（裁定 2 的 fatal 半边）：两个互依赖的 manifest，rrd 必须
//!   非 0 退出（`main` 收到 `Err` 后 `ExitCode::FAILURE`）且日志点名"依赖环涉及"。
//! - D3 **降级→恢复**（裁定 3/4）：提供者被 SIGKILL → 依赖它的在册 Running 模块置
//!   Degraded（广播含依赖者自己，E-9 同一理据）；提供者按 always 策略自动重生回到
//!   Running → 解除。死亡走**测试侧主动 kill** 而不是 watchdog 到期：到期判死的时刻
//!   由 wd+tick 决定，"依赖者先到 Running"就变成与 exec 赛跑；kill 把"什么时候死"
//!   收进测试手里，判据里没有任何时间赌注（wd 拉到 60s，watchdog 全程静默）。
//!
//! 与 `e2e_spawn_restart.rs` 共用的三条口径（内容判据 / 不写死号 / 否定 needle 不共用
//! 子串）在那边有全文论证，此处不重复；夹具 manifest 由本文件的
//! `rr3_dep_manifest` 现场生成（带 `[provides]` / `[depends]` 的形状只有线级用例
//! 需要，不值得为它开 fixtures/ 目录），entry 用 `CARGO_BIN_EXE_sim_peer` 的绝对路径
//! 直填——占位符替换那套是给"夹具文件在仓内"的场景用的。
mod support;

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use support::*;

/// 生成一份带 `[provides]` / `[depends]` 的夹具 manifest（pid+tag 唯一 /tmp 路径，
/// `SimManifest` 守卫 Drop 时删除——断言 panic 也不会留残留，Task 7 评审 N-1 的教训）。
/// 段的取舍与 `rr3_manifest`（support）同形：publishes 空、订阅内建 state 主题，
/// `provides` / `depends` 为空时整段省略（schema 里都是可选段）。
fn rr3_dep_manifest(
    tag: &str,
    name: &str,
    start_priority: i64,
    provides: &[&str],
    requires: &[&str],
    policy: &str,
) -> SimManifest {
    let mut text = format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "{entry}"

[events]
publishes = []
subscribes = ["rr3-runtime/module/state"]

[resources]
restart_policy = "{policy}"
start_priority = {start_priority}
"#,
        entry = env!("CARGO_BIN_EXE_sim_peer"),
    );
    if !provides.is_empty() {
        let caps = provides.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(", ");
        text.push_str(&format!("\n[provides]\ncapabilities = [{caps}]\n"));
    }
    if !requires.is_empty() {
        let caps = requires.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(", ");
        text.push_str(&format!("\n[depends]\nrequired = [{caps}]\n"));
    }
    text.push_str("\n[health]\nheartbeat_ms = 100\nwatchdog_timeout_ms = 60000\n");
    let path = PathBuf::from(format!(
        "/tmp/rr3-f2-dep-{}-{tag}-{name}.toml",
        std::process::id()
    ));
    std::fs::write(&path, &text)
        .unwrap_or_else(|e| panic!("写夹具 manifest {} 失败: {e}", path.display()));
    SimManifest { path }
}

/// 一条**只观察**的连接（与 `e2e_spawn_restart` 的同名助手同形状不共用——
/// 那边的理由原文照抄：本轮禁改既有线级用例，就不能把函数搬进 support）。
fn rr3_observer(rrd: &mut RrdTest, name: &str) -> RrdClient {
    let mut c = rrd.rr3_connect();
    rr3_send(&mut c, &rr3_register_frame(&rr3_manifest(name, 100, 60_000, "never"), name));
    rr3_read_state(&mut c, name, "registered");
    c
}

/// 挂死夹具的卫生清理（S4/S5 同款，本文件自备一份）：go_silent 的第二世睡在
/// `sleep(3600s)` 里，rrd 被 Drop 杀死后没人收它。不断言、失败一律忽略——卫生不是判据。
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

/// 线上注册一个**依赖者**并手动喂到 Running：注册 → 等自己的 registered 广播 →
/// 一颗心跳（Registered → Running 的唯一升级时刻在 `rr3_reg_beat`，一拍即 Promoted）。
/// 返回连接与 rrd 分配的 task。心跳 id 从 2 起（注册帧占了 1）。
fn rr3_register_dependent(rrd: &mut RrdTest, name: &str, requires: &[&str]) -> (RrdClient, u64) {
    let m = rr3_dep_manifest("dep-online", name, 100, &[], requires, "never");
    let mut c = rrd.rr3_connect();
    rr3_send(&mut c, &rr3_register_frame(&std::fs::read_to_string(&m.path).expect("读在线夹具"), name));
    rr3_read_state(&mut c, name, "registered");
    let reply = rr3_read_rpc(&mut c);
    let task = reply["result"]["src_task"]
        .as_u64()
        .unwrap_or_else(|| panic!("注册应答里没有 src_task: {reply}"));
    rr3_send(&mut c, &rr3_rpc_frame("rr3.heartbeat", 2, serde_json::json!({}), task));
    let _ = rr3_read_rpc(&mut c); // 心跳应答
    (c, task)
}

/// D1 — 拓扑主键压过 start_priority：provider prio=50、dependent prio=10，
/// 旧的全局优先级排序会先起 dependent；仲裁序必须先起 provider。
#[test]
fn boot_order_follows_dependency_not_priority() {
    let prov = rr3_dep_manifest("dep-d1", "rr3-chainp", 50, &["cap.chain"], &[], "never");
    let dep = rr3_dep_manifest("dep-d1", "rr3-chaind", 10, &[], &["cap.chain"], "never");
    let mut rrd = RrdTest::rr3_start_boot(
        "dep-d1",
        &[&dep.path, &prov.path], // CLI 序故意把 dependent 放前面：序只能来自仲裁，不能来自装载序
        "resident",
        E2E_TICK_MS,
    );
    let mut b = rr3_observer(&mut rrd, "rr3-depobs");

    // 两条 spawn 行都到齐后比对**位置**：spawn 行由唯一的 supervisor 线程串行打出，
    // 行序 = boot 序，这是唯一不带竞态的序证据（注册序会因两个子进程并行 exec 而抖动）。
    let mut text = String::new();
    let both = rr3_wait_until(
        || {
            text = rrd.rr3_logs(Duration::from_millis(10));
            text.contains("spawn rr3-chainp pid=") && text.contains("spawn rr3-chaind pid=")
        },
        Duration::from_secs(10),
    );
    assert!(both, "两条 boot spawn 都该出现；已读到:\n{text}");
    let p = text.find("spawn rr3-chainp pid=").expect("上面已确认存在");
    let d = text.find("spawn rr3-chaind pid=").expect("上面已确认存在");
    assert!(
        p < d,
        "cap.chain 的提供者必须先于依赖者起（拓扑主键；旧的全局优先级排序此处是反的）:\n{text}"
    );

    // 双双上线：日志各有一次 running 转移，线上帧也各自可读（src_task 按到达序无关地挑）。
    rrd.rr3_wait_log("rr3-chainp → running（广播到");
    rrd.rr3_wait_log("rr3-chaind → running（广播到");
    rr3_read_state(&mut b, "rr3-chainp", "running");
    rr3_read_state(&mut b, "rr3-chaind", "running");
}

/// D2 — 环拒整个 boot：两个互依赖的 manifest，rrd 非 0 退出且逐条点名。
/// 直接 `Command` 起 rrd 读 stdout 到 EOF——这条用例里 rrd 的生命结束方式**就是被测行为**，
/// `RrdTest` 的"等就绪行"框架用不上（永远等不到监听后的正常运行）。
#[test]
fn dependency_cycle_rejects_the_whole_boot() {
    let a = rr3_dep_manifest("dep-d2", "rr3-loopa", 50, &["cap.loopa"], &["cap.loopb"], "never");
    let b = rr3_dep_manifest("dep-d2", "rr3-loopb", 50, &["cap.loopb"], &["cap.loopa"], "never");
    let socket = PathBuf::from(format!("/tmp/rr3-f2-cycle-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);

    let mut child = Command::new(env!("CARGO_BIN_EXE_rrd"))
        .arg("--socket")
        .arg(&socket)
        .arg("--manifest")
        .arg(&a.path)
        .arg("--manifest")
        .arg(&b.path)
        .arg("--boot-role")
        .arg("resident")
        .args(["--restart-base-ms", "20"])
        .args(["--watchdog-tick-ms", &E2E_TICK_MS.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null()) // main 的 eprintln 细节走 stderr，判据全在 stdout 日志里
        .spawn()
        .expect("起 rrd 失败");
    let lines = rr3_spawn_line_reader(child.stdout.take().expect("stdout 必须是管道"), "rrd");

    // 读到 EOF（进程自己退出）为止。20s 上界只防"拒绝路径坏了、进程照常起服务"的退化：
    // 那时 kill 掉并 panic，而不是让整个测试目标挂到 cargo 超时。
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut text = String::new();
    loop {
        match lines.recv_timeout(Duration::from_millis(100)) {
            Ok(l) => {
                text.push_str(&l);
                text.push('\n');
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(_) if Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("rrd 未在期限内退出——环没有被拒；已读到:\n{text}");
            }
            Err(_) => {}
        }
    }
    let status = child.wait().expect("wait rrd");
    assert!(!status.success(), "环必须让 boot 非 0 退出（main 的 ExitCode::FAILURE）: {status}");
    assert!(text.contains("拒绝 boot："), "fatal 先逐条打到 stdout 日志再退出:\n{text}");
    assert!(text.contains("依赖环涉及"), "{text}");
    assert!(text.contains("rr3-loopa") && text.contains("rr3-loopb"), "两个环成员都要点名:\n{text}");
    assert!(
        !text.contains("spawn rr3-loopa") && !text.contains("spawn rr3-loopb"),
        "拒 boot 的进程一个子进程都不能起:\n{text}"
    );
    let _ = std::fs::remove_file(&socket);
}

/// D3 — 裁定 3/4 的全链：提供者被 kill → 依赖者 Degraded（自己也能听到）→
/// always 策略自动重生 → 依赖者解除回 Running。全程 watchdog 静默（wd=60s），
/// "死亡时刻"由测试的 kill 决定，判据里没有时间赌注。
#[test]
fn provider_death_degrades_dependent_and_recovery_undegrades() {
    let prov = rr3_dep_manifest("dep-d3", "rr3-degp", 50, &["cap.deg"], &[], "always");
    let mut rrd = RrdTest::rr3_start_boot("dep-d3", &[&prov.path], "go_silent", E2E_TICK_MS);
    let mut b = rr3_observer(&mut rrd, "rr3-degobs");

    // 提供者第一世上线：go_silent 注册后喂一颗就静默——那一颗正好把它 Promoted 到 Running
    let spawn1 = rrd.rr3_wait_log("spawn rr3-degp pid=");
    let pid1 = rr3_field_from_log(&spawn1, "pid=");
    let reg1 = rrd.rr3_wait_log("注册 rr3-degp task=");
    let task1 = rr3_field_from_log(&reg1, "task=");
    rrd.rr3_wait_log("rr3-degp → running（广播到");

    // 依赖者线上注册并手动喂到 Running（此刻它必须已在册且 Running，降级传播才轮得到它）
    let (mut dep_conn, dep_task) = rr3_register_dependent(&mut rrd, "rr3-degd", &["cap.deg"]);
    rrd.rr3_wait_log("rr3-degd → running（广播到");

    // 提供者死于非命（SIGKILL → 收尸那一路的退出码定性，signal: 9）
    let killed = Command::new("/bin/kill")
        .args(["-9", &pid1.to_string()])
        .status()
        .expect("kill 前置失败");
    assert!(killed.success(), "kill -9 {pid1} 没成功——前置不成立，后面的判据全部作废");
    let exit1 = rrd.rr3_wait_log("rr3-degp 子进程退出 status=signal: 9");
    assert!(exit1.contains("→ 广播 failed"), "owned 模块由退出码定性: {exit1}");

    // 裁定 3 断供半边：依赖者被置 Degraded，广播**不排除依赖者自己**（E-9 理据：
    // 它没有别的途径查自己的相位），旁观者与依赖者各自读一条
    let degraded = rrd.rr3_wait_log("rr3-degd 依赖断供 → degraded（提供者 rr3-degp 离开 Running）");
    assert!(
        degraded.contains("广播到"),
        "置位必须伴一次广播: {degraded}"
    );
    let on = rr3_read_state(&mut dep_conn, "rr3-degd", "degraded");
    assert_eq!(on["src_task"], dep_task, "线上那条 degraded 必须指向依赖者自己的 task");
    rr3_read_state(&mut b, "rr3-degd", "degraded");
    // 裁定 4：降级不是崩溃——不产重启计划、不经 supervisor（`重启计划` 四字只出现在
    // conn 收尾那一路，是 A-5③ 批准的唯一子串）
    rrd.rr3_assert_no_log("重启计划", Duration::from_millis(200));

    // always 策略把提供者起回来（attempt=1 的新进程），重新注册、再次 running
    let spawn2 = rrd.rr3_wait_log("spawn rr3-degp pid=");
    let pid2 = rr3_field_from_log(&spawn2, "pid=");
    assert_ne!(pid1, pid2, "重起的必须是新进程");
    assert!(spawn2.contains("attempt=1"), "{spawn2}");
    let reg2 = rrd.rr3_wait_log("注册 rr3-degp task=");
    assert_ne!(rr3_field_from_log(&reg2, "task="), task1, "重注册必须换新 task");
    rrd.rr3_wait_log("rr3-degp → running（广播到");

    // 裁定 3 恢复半边：全部 required 就位 → 解除回 Running（同样不排除依赖者自己）
    rrd.rr3_wait_log("rr3-degd 依赖恢复 → running（提供者 rr3-degp 回到 Running）");
    let on = rr3_read_state(&mut dep_conn, "rr3-degd", "running");
    assert_eq!(on["src_task"], dep_task, "解除帧仍指向依赖者自己的 task");
    rr3_read_state(&mut b, "rr3-degd", "running");

    // 卫生（S4 同款）：第二世又是个静默挂死的夹具，rrd 被 Drop 杀死后没人收它
    rr3_kill_hung_fixture(pid2);
}
