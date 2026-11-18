//! rrd 日志的落盘臂：常持句柄 + 进程内字节预算（续轮十四）。
//!
//! 原实现每行 `stat + open + write + close`（4 次系统调用，`rr3_log_rotate` 的
//! metadata 每行一次）。事件级频率下不算贵，但 fanout 满队列点名发生在 registry
//! 锁内、watchdog 判死也逐行落盘——锁内系统调用越少越好。现改为：定格时打开一次
//! 句柄，每行一次 `write_all`（1 次系统调用，仍无用户态缓冲——"打过的行当场就在
//! 文件里"的崩溃判据不变），轮转按进程内累计字节触发。`rr3_log_rotate` 保留给
//! supervisor 的 per-unit 文件（那是每次打开/关闭的独立文件面）。
//!
//! 未定格（Windows / `--manifest` 前段）= 纯内存；写失败点名一次后置降级位，
//! 之后静默退纯内存（与旧口径一致）。外部删除已打开文件时本进程继续写旧 inode
//! （句柄语义的已知边界）：轮转仍按累计字节发生，重启后回到新文件。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use super::rr3_line_bytes;
use crate::rr3_lock;

/// per-unit 日志目录的派生单点：socket 全路径 + `.log` 后缀（批N4 件八定形，批N5
/// 件二笔2 迁本文件供两路共用——supervisor 的 per-unit 文件与 rrd 自身落盘同一
/// 目录）。同机多实例（测试 `/tmp/rr3-m2-*.sock`、产线 `/var/run/rock3-ipc.sock`）
/// 天然按实例隔离，不需要新 CLI 键；只算路径不建目录不打开，副作用留给调用方。
#[cfg(unix)]
pub fn rr3_unit_log_dir(bus_path: &Path) -> PathBuf {
    let mut dir = bus_path.as_os_str().to_os_string();
    dir.push(".log");
    PathBuf::from(dir)
}

/// 建 `<socket>.log` 目录并收紧到 0o750（属主 rwx / 属主组 r-x / other 无份）：
/// 日志面与 rrd socket 的 0o660 组访问模型对齐——同机无组身份的用户不得旁读
/// 运行日志（模块名/错误链/请求 id 都是运维内部面）。位元设置失败不阻断
/// （观测通道故障不放大成可用性故障），留痕后续沿用默认位。
#[cfg(unix)]
pub(crate) fn rr3_log_dir_ensure(bus_path: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = rr3_unit_log_dir(bus_path);
    std::fs::create_dir_all(&dir)?;
    if let Err(e) = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o750)) {
        eprintln!("rrd 日志目录收紧 0750 失败（继续，{}）: {e}", dir.display());
    }
    Ok(dir)
}

/// rrd 自身日志的落盘文件路径（批N5 件二笔2，§17 运维留痕）：OnceLock 首份胜出，
/// daemon 装配链后定格一次。未定格（启动早于接线 / `--manifest` 前段 / Windows）
/// = 纯内存，`rr3_log` 行为与笔1 之前一字不变。回填读口用它。
static RR3_LOG_SINK: OnceLock<PathBuf> = OnceLock::new();

/// 落盘写失败后的降级记账位：置位后本进程不再重试（磁盘故障不是每行日志都要
/// 重新撞一遍的墙），点名一次交给运维，之后退纯内存。
static RR3_LOG_SINK_DEAD: AtomicBool = AtomicBool::new(false);

/// 单日志文件体量上界（0.0.8c 审计 B-8）：SD 卡设备上 append 无界 = 磁盘耗尽
/// 炸弹（错误循环模块 10KB/s ≈ 900MB/天，盘满后 journal/OTA/配置写全灭）。
/// 2MiB×（rrd + ≤16 单元）×2 代 ≈ 68MB 最坏驻留，装机可承受。
pub(crate) const RR3_LOG_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// 常持句柄的落盘面（续轮十四）：`written` 含打开时的既有大小，越界即单代轮转。
/// 一把锁同时保护句柄与计数——多线程 `rr3_log` 的整行写仍靠 O_APPEND + 持锁，
/// 与旧实现"单次 write 落整行"的判据同源（见 `log::tests::log_line_is_single_write`）。
static RR3_LOG_SINK_FILE: OnceLock<Mutex<Rr3LogSink>> = OnceLock::new();

struct Rr3LogSink {
    file: std::fs::File,
    path: PathBuf,
    written: u64,
}

impl Rr3LogSink {
    /// 打开（create + append）并把既有大小记入预算：上一进程崩溃前留下的超界文件
    /// 在第一行落盘后立即轮转，不把无界增长带过来。
    fn rr3_open(path: PathBuf) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            file,
            path,
            written,
        })
    }

    fn rr3_append(&mut self, line: &str) -> std::io::Result<()> {
        let bytes = rr3_line_bytes(line);
        self.file.write_all(&bytes)?;
        self.written = self.written.saturating_add(bytes.len() as u64);
        if self.written > RR3_LOG_MAX_BYTES {
            self.rr3_rotate()?;
        }
        Ok(())
    }

    /// 单代轮转：rename 到 `<名>.1`（旧 `.1` 被覆盖）后重开新文件、预算清零。
    /// rename 失败不阻断（观测通道不打断业务主路）：照旧重开同名文件继续写；
    /// 重开失败才向上抛（调用方置降级位）。
    fn rr3_rotate(&mut self) -> std::io::Result<()> {
        if let Some(name) = self.path.file_name().and_then(|s| s.to_str()) {
            let _ = std::fs::rename(&self.path, self.path.with_file_name(format!("{name}.1")));
        }
        self.file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.written = 0;
        Ok(())
    }
}

/// 定格 rrd 自身日志落盘（批N5 件二笔2）：`<socket>.log/rrd.log`，与 per-unit
/// 文件同目录同派生单点（[`rr3_unit_log_dir`]）。目录建不出 / 文件打不开 = 返回
/// Err，调用方点名降级纯内存——观测通道故障降级、不放大成可用性故障。`bus_path`
/// 必须已绝对化（main 的 [`crate::cli::rr3_socket_abs`]）——相对路径就是链条
/// 上游破了，fail-closed 拒定格（与 supervisor 的 `rr3_open_unit_log` 同一口径）。
#[cfg(unix)]
pub fn rr3_log_sink_set(bus_path: &Path) -> Result<(), String> {
    if !bus_path.is_absolute() {
        return Err("socket 路径非绝对（上游绝对化失效）".into());
    }
    if RR3_LOG_SINK.get().is_some() {
        return Err("落盘路径已定格过（首份胜出，重复定格被拒）".into());
    }
    let dir = rr3_log_dir_ensure(bus_path).map_err(|e| format!("建日志目录失败: {e}"))?;
    let path = dir.join("rrd.log");
    let sink = Rr3LogSink::rr3_open(path.clone()).map_err(|e| format!("打开落盘文件失败: {e}"))?;
    RR3_LOG_SINK
        .set(path)
        .map_err(|_| "落盘路径已定格过（首份胜出，重复定格被拒）".to_string())?;
    let _ = RR3_LOG_SINK_FILE.set(Mutex::new(sink));
    Ok(())
}

/// 落盘路径读口（回填用）：未定格返回 None，不造假值。
pub(super) fn rr3_log_sink_path() -> Option<&'static Path> {
    RR3_LOG_SINK.get().map(PathBuf::as_path)
}

/// `rr3_log` 的落盘臂：一行一次 `write_all`，不缓冲不批写——"崩溃前最后 N 行"的
/// 判据要求打过的行当场就在文件里（无用户态缓冲，写即入内核）。打开或写失败 =
/// 点名一次后置降级位，之后静默退纯内存。
pub(super) fn rr3_log_sink_append(line: &str) {
    if RR3_LOG_SINK_DEAD.load(Ordering::Relaxed) {
        return;
    }
    let Some(sink) = RR3_LOG_SINK_FILE.get() else {
        return;
    };
    let mut sink = rr3_lock(sink);
    let Err(e) = sink.rr3_append(line) else {
        return;
    };
    let path = RR3_LOG_SINK
        .get()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "?".to_string());
    drop(sink);
    if !RR3_LOG_SINK_DEAD.swap(true, Ordering::Relaxed) {
        // eprintln 不走 rr3_log：降级说明本身不能再依赖降级中的通道（递归），
        // 也绝不进 stdout 管道（测试的可观察面是"rrd 说过什么"，不许混入）
        eprintln!("rrd: 落盘写失败（{path}）: {e} —— 运维留痕退纯内存，本进程不再重试");
    }
}

/// 单代轮转（per-unit 文件面，supervisor 每次打开/关闭时调用）：文件超上界即
/// rename 到 `<名>.1`（旧 `.1` 被覆盖），下一段写新文件。失败只留痕不阻断——
/// 观测通道绝不允许因磁盘抖动打断业务主路；容量判据是尽力而为的防线，不是事务。
pub(crate) fn rr3_log_rotate(path: &std::path::Path) {
    let Ok(md) = std::fs::metadata(path) else {
        return;
    };
    if md.len() <= RR3_LOG_MAX_BYTES {
        return;
    }
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    let _ = std::fs::rename(path, path.with_file_name(format!("{name}.1")));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rr3-rrd-sink-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 单代轮转（B-8，per-unit 面）：超上界 rename 到 `.1`（旧 `.1` 覆盖），未超界
    /// 不动；文件不存在 = 静默返回。
    #[test]
    fn log_rotate_triggers_only_over_cap() {
        let dir = tmpdir("rot");
        let f = dir.join("u.log");
        std::fs::write(&f, b"short").unwrap();
        rr3_log_rotate(&f);
        assert!(f.exists() && !dir.join("u.log.1").exists(), "界内不轮转");
        std::fs::write(&f, vec![b'x'; (RR3_LOG_MAX_BYTES + 1) as usize]).unwrap();
        rr3_log_rotate(&f);
        assert!(!f.exists(), "超界原文件已让位");
        assert_eq!(
            std::fs::read(dir.join("u.log.1")).unwrap().len(),
            RR3_LOG_MAX_BYTES as usize + 1
        );
        rr3_log_rotate(&f);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 常持句柄（续轮十四）：一行一次写、内容逐行完整；打开时既有大小计入预算，
    /// 超界在第一行后即轮转（上一进程留下的超界文件不把无界增长带过来）。
    #[test]
    fn sink_appends_and_rotates_by_byte_budget() {
        let dir = tmpdir("append");
        let path = dir.join("rrd.log");
        std::fs::write(&path, vec![b'y'; (RR3_LOG_MAX_BYTES + 1) as usize]).unwrap();
        let mut sink = Rr3LogSink::rr3_open(path.clone()).expect("应能打开");
        sink.rr3_append("rrd: 第一行").expect("写应成功");
        let rotated = dir.join("rrd.log.1");
        assert_eq!(
            std::fs::metadata(&rotated).unwrap().len(),
            RR3_LOG_MAX_BYTES + 1 + "rrd: 第一行\n".len() as u64,
            "打开时既有大小计入预算，首行写入后立即轮转"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "",
            "轮转后的新文件从零开始"
        );
        sink.rr3_append("rrd: 第二行").expect("写应成功");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "rrd: 第二行\n",
            "常持句柄逐行追加"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
