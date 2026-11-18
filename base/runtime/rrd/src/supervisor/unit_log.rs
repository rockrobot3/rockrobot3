//! per-unit 日志文件的定位与打开（append），供子进程 stderr 落盘。

use std::path::{Path, PathBuf};

/// per-unit 日志文件的定位与打开（append）：`<socket 路径>.log/<unit>.log`。
///
/// 门 r 留痕通道定形（批N4 件八）：目录名由 socket 全路径加 `.log` 后缀派生——同机
/// 多实例（测试 `/tmp/rr3-m2-*.sock`、产线 `/var/run/rock3-ipc.sock`）天然按实例隔离，
/// 不需要新 CLI 键；append 不 truncate，重启历史保留在文件里。目录派生单点在
/// lib.rs 的 `rr3_unit_log_dir`（批N5 件二笔2 迁入）：rrd 自身日志落盘与 per-unit
/// 文件同目录同规则，路径规则一处定形。这里只负责"打开"；
/// 打不开的错误交调用方裁决（回退 inherit，不挡起跑）。
/// 超上界单代轮转（0.0.8c 审计 B-8）在打开前做——子进程持 fd 直写，rrd 只能在
/// spawn 窗口收口体量；长跑期间体量由模块侧留痕纪律（事件级频率）约束。
pub(super) fn rr3_open_unit_log(bus_path: &Path, unit: &str) -> std::io::Result<std::fs::File> {
    let path = rr3_unit_log_path(bus_path, unit)?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// 日志文件路径：bus_path 必须绝对（fail-closed），建目录 + 轮转后交出待开路径。
fn rr3_unit_log_path(bus_path: &Path, unit: &str) -> std::io::Result<PathBuf> {
    // fail-closed：bus_path 由 daemon 侧绝对化（cli.rs 的 rr3_socket_abs），
    // 相对/空路径到这里就是链条上游破了——拒开，绝不相对 CWD 悄悄建目录
    rr3_log_abs(bus_path)?;
    let dir = crate::rr3_log_dir_ensure(bus_path)?;
    let path = dir.join(format!("{unit}.log"));
    crate::rr3_log_rotate(&path);
    Ok(path)
}

/// 绝对路径门：非绝对即拒（上游绝对化失效的信号）。
fn rr3_log_abs(bus_path: &Path) -> std::io::Result<()> {
    if bus_path.is_absolute() {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "socket 路径非绝对（上游绝对化失效）",
    ))
}
