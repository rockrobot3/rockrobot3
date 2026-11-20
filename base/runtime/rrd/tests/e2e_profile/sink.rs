//! P5 rrd 自身日志落盘与重启回填用例（自 e2e_profile.rs 抽出，判据不变）。

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use rr_ipc::{rr3_body_split, Rr3PayloadType};
use serde_json::json;

use super::{rr3_status, Root, GOOD};
use crate::support::*;

/// P5 — rrd 自身日志落盘（批N5 件二笔2，§17 运维留痕同轨判据）：第一世的每一行
/// `rr3_log` 同步 append 进 `<socket>.log/rrd.log`；杀掉 rrd 后**同 socket** 重启
/// 第二世（spawn_inner 的 remove_file 只清 socket 文件，落盘目录 `.sock.log/` 不在
/// 清列——崩溃前行因此幸存，这正是被测语义），启动回填把文件尾置回环头 →
/// `rr3.ctl.logs` 取得到第一世的收尸行。
///
/// 独有性设计：第一世 kill 掉已 running 的 rr3-pb，supervisor 收尸打
/// `rr3-pb 子进程退出 status=...`——第二世模块全活，不可能有这行；若环里的它
/// 不是来自文件回填，本断言必红。时序断言：收尸行必须早于第二世最后一条 spawn
/// 行（回填行在环头、第二世行在其后——push_front 次序错的写法在这里红）。
/// cap=64：第一世 boot ~10 行 + 第二世 ~10 行远小于容量，两世行全在环内。
/// SIGTERM 优雅关停即可证：同步 append 下行早已当场在文件里，重启回收与
/// 关停形状无关（SIGKILL 崩溃只会更早截尾，不会多出别的路径）。
#[test]
fn rrd_reboot_recovers_previous_run_lines_from_disk() {
    let root = Root::new("sink");
    for name in ["rr3-pa", "rr3-pb", "rr3-pc"] {
        root.write_module(name);
    }
    // 落盘目录残根清掉（append 不 truncate：同 pid 同 tag 重跑时上一世的文件
    // 会混进判据面——本用例的独有行必须来自本世的第一世）
    let sink_dir = PathBuf::from(format!(
        "/tmp/rr3-m2-{}-prof-sink.sock.log",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&sink_dir);

    // 环容量 256 而不是 64：本用例靠"回填行仍在环里"取证，慢 runner 上
    // 第二世启动行多打几行就会把 64 格里的收尸行挤掉（CI 2026-11-17 实测
    // 一次红）；256 不改变断言，只去掉这类容量竞态。
    let profile = root.write_profile(&GOOD.replace("log_ring_cap = 512", "log_ring_cap = 256"));

    // —— 第一世：boot → rr3-pb running → kill -9 → 收尸行 → kill rrd ——
    let mut rrd1 =
        RrdTest::rr3_start_profile_env("prof-sink", &profile, E2E_TICK_MS, &[("RR3_PROBE", "sim")]);
    let spawn_pb = rrd1.rr3_wait_log("spawn rr3-pb pid=");
    let pb_running = rr3_wait_until(
        || {
            rr3_status(&rrd1)
                .iter()
                .any(|(n, p)| n == "rr3-pb" && p == "running")
        },
        Duration::from_secs(30),
    );
    assert!(
        pb_running,
        "rr3-pb 必须先到 running 再 kill：提前杀会走未注册即退出的另一条文案，判据不再独有；最后快照: {:?}",
        rr3_status(&rrd1)
    );
    let killed = Command::new("/bin/kill")
        .arg("-9")
        .arg(rr3_field_from_log(&spawn_pb, "pid=").to_string())
        .status()
        .expect("kill 前置失败");
    assert!(killed.success(), "kill 前置失败");
    rrd1.rr3_wait_log("rr3-pb 子进程退出");
    rrd1.rr3_sigterm_and_wait(); // 杀掉第一世；同步 append 下崩溃前行已全在文件里
    drop(rrd1);

    // —— 第二世：同 tag = 同 socket；ctl.logs 应含第一世崩溃前的行 ——
    let mut rrd2 =
        RrdTest::rr3_start_profile_env("prof-sink", &profile, E2E_TICK_MS, &[("RR3_PROBE", "sim")]);
    rrd2.rr3_wait_log("spawn rr3-pa pid=");
    let mut ctl = rrd2.rr3_connect();
    // 第二世的 spawn 行在 stdout 可见时刻未必已入环（`rr3_log` 的落盘 append 在
    // stdout 与入环之间）。按环内位置等可观测信号：只有出现"位于第二世监听行之后"
    // 的 spawn 行，才说明第二世行确实已入环——直接查询会偶发读到仅含回填块的旧环，
    // 把第一世的 spawn 行误当比较基线。
    let listen = format!("监听 {}", rrd2.socket.display());
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut joined: Vec<String>;
    loop {
        rr3_send(&mut ctl, &rr3_rpc_frame("rr3.ctl.logs", 2, json!({}), 0));
        let f = rr3_read_where(&mut ctl, "ctl.logs 应答", |fr| {
            rr3_body_split(&fr.payload)
                .map(|(ty, _)| ty == Rr3PayloadType::Response)
                .unwrap_or(false)
        });
        let v = rr3_json_of(&f);
        joined = v["result"]["lines"]
            .as_array()
            .expect("logs 应带 result.lines")
            .iter()
            .filter_map(|l| l.as_str().map(str::to_string))
            .collect();
        let spawn_at = joined.iter().rposition(|l| l.contains("spawn rr3-pa pid="));
        let listen_at = joined.iter().rposition(|l| l.contains(&listen));
        if matches!((spawn_at, listen_at), (Some(s), Some(b)) if s > b) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "第二世 spawn 行 10s 内未进入日志环（回填/入环次序异常）: {joined:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        joined.iter().any(|l| l.contains("rr3-pb 子进程退出")),
        "第二世 ctl.logs 必须取到第一世崩溃前的收尸行（rrd 自身日志落盘＋启动回填）: {joined:?}"
    );
    let corpse_idx = joined
        .iter()
        .position(|l| l.contains("rr3-pb 子进程退出"))
        .expect("上面断言已保证在场");
    assert!(
        joined
            .iter()
            .rposition(|l| l.contains("spawn rr3-pa pid="))
            .is_some_and(|last| corpse_idx < last),
        "第一世的行必须在第二世的行之前（回填行置环头，push_front 次序错了在这里红）: {joined:?}"
    );
}
