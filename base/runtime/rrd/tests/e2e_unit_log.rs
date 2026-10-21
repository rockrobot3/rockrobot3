#![cfg(unix)]
//! 门 r 留痕通道定形的线上级证据（批N4 件八）：rrd spawn 的子进程 stderr 落进
//! per-unit 日志文件 `<socket>.log/<unit>.log`（append）——模块留痕不再进被丢弃的流
//! （stdout 是 `Stdio::null()`，原先 45 处 `println!` 按设计不可达任何去处）。
//!
//! 夹具用自写 shell 脚本当 entry（打一行 stderr 即退）——不用 `sim_peer`：它只打
//! stdout（仍进 null），证不了 stderr 那条腿。`restart_policy = "never"`：脚本退场
//! 不重启，用例收尾零泄漏。

mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use support::*;

fn e2e_unit_log_dir(socket: &Path) -> PathBuf {
    let mut p = socket.to_path_buf().into_os_string();
    p.push(".log");
    PathBuf::from(p)
}

#[test]
fn spawned_module_stderr_lands_in_per_unit_log() {
    // ---- 夹具：打一行 stderr 即退的脚本 + 绝对 entry 的 manifest ----
    let script = std::env::temp_dir().join(format!(
        "rr3-unit-log-probe-{}-{}.sh",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("时钟在 UNIX_EPOCH 之前")
            .as_nanos()
    ));
    std::fs::write(&script, "#!/bin/sh\necho unit-log-probe >&2\nexit 0\n").expect("写夹具脚本");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .expect("脚本可执行位");

    let manifest = std::env::temp_dir().join(format!(
        "rr3-unit-log-{}-{}.toml",
        std::process::id(),
        script
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("probe")
    ));
    std::fs::write(
        &manifest,
        format!(
            "[manifest]\n\
             schema_version = 1\n\
             name = \"rr3-ulog\"\n\
             version = \"0.0.1\"\n\
             type = \"module\"\n\
             lang = \"rust\"\n\
             entry = \"{}\"\n\
             \n\
             [events]\n\
             publishes = []\n\
             subscribes = [\"rr3-runtime/module/state\"]\n\
             \n\
             [resources]\n\
             restart_policy = \"never\"\n\
             \n\
             [health]\n\
             heartbeat_ms = 100\n\
             watchdog_timeout_ms = 500\n",
            script.display()
        ),
    )
    .expect("写夹具 manifest");

    // ---- boot 直驱：spawn 发生即应建 per-unit 文件，stderr 行落盘 ----
    let mut rrd = RrdTest::rr3_start_boot("ulog", &[&manifest], "abort", E2E_TICK_MS);
    let spawn = rrd.rr3_wait_log("spawn rr3-ulog pid=");
    assert!(
        spawn.contains(&format!("entry={}", script.display())),
        "spawn 行必须回显脚本 entry: {spawn}"
    );

    let log_file = e2e_unit_log_dir(&rrd.socket).join("rr3-ulog.log");
    let landed = rr3_wait_until(
        || {
            std::fs::read_to_string(&log_file)
                .map(|t| t.contains("unit-log-probe"))
                .unwrap_or(false)
        },
        Duration::from_secs(5),
    );
    assert!(
        landed,
        "子进程 stderr 未落 per-unit 日志 {}（门 r 通道断）",
        log_file.display()
    );
    let _ = std::fs::remove_file(&script);
    let _ = std::fs::remove_file(&manifest);
    let _ = std::fs::remove_dir_all(e2e_unit_log_dir(&rrd.socket));
}
