// SPDX-License-Identifier: Apache-2.0
//! rrd 日志的落盘臂：常持句柄 + 进程内字节预算（续轮十四）。
//!
//! 原实现每行 `stat + open + write + close`（4 次系统调用，`rr3_log_rotate` 的
//! metadata 每行一次）。事件级频率下不算贵，但 fanout 满队列点名发生在 registry
//! 锁内、watchdog 判死也逐行落盘——锁内系统调用越少越好。现改为：定格时打开一次
//! 句柄，每行一次 `write_all`（1 次系统调用，仍无用户态缓冲——"打过的行当场就在
//! 文件里"的崩溃判据不变），轮转按进程内累计字节触发。`rr3_log_rotate` 保留给
//! supervisor 的 per-unit 文件（那是每次打开/关闭的独立文件面）。
//!
//! 未定格（Windows / `--manifest` 前段）= 纯内存；写失败进入 10s 冷却重试
//! （续轮二十三：瞬时故障不再永久关掉落盘，成功即回 `on`，只在转入/恢复时点名）。
//! 外部删除已打开文件时本进程继续写旧 inode
//! （句柄语义的已知边界）：轮转仍按累计字节发生，重启后回到新文件。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

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
    rr3_log_dir_group(&dir);
    Ok(dir)
}

/// 运维组 gid：首解析一次后进程内复用。`getgrnam` 走静态存储、多线程并发不安全，
/// 故用 OnceLock 定格——首次调用在 daemon 起工作线程前（socket ACL 同段），每次
/// spawn 的 ensure 都吃缓存。组不在位 = `None`（目录保持 root:root，留痕一次）。
#[cfg(unix)]
fn rr3_log_gid() -> Option<libc::gid_t> {
    static GID: OnceLock<Option<libc::gid_t>> = OnceLock::new();
    *GID.get_or_init(|| {
        let c = std::ffi::CString::new(crate::RR3_OPS_GROUP).ok()?;
        // SAFETY：c 活到调用结束；getgrnam 返回值指向静态存储，本处只读 gr_gid，
        // 且 OnceLock 保证全程只执行一次（启动段无并发 getgrnam）。
        let gr = unsafe { libc::getgrnam(c.as_ptr()) };
        if gr.is_null() {
            eprintln!(
                "rrd 日志目录属组 {} 不在位（跳过 chgrp，保持 root:root；组内用户需 sudo 读日志）",
                crate::RR3_OPS_GROUP
            );
            return None;
        }
        // SAFETY：gr 已判非空，只读一个字段。
        Some(unsafe { (*gr).gr_gid })
    })
}

/// 日志目录属组对齐运维组（与 socket ACL 同款机制）：失败只留痕，不挡起跑。
#[cfg(unix)]
fn rr3_log_dir_group(dir: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let Some(gid) = rr3_log_gid() else { return };
    let Ok(cpath) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return;
    };
    // SAFETY：cpath 活到调用结束；uid=MAX 按 POSIX 表示"不改属主"，chown 只读路径串。
    let rc = unsafe { libc::chown(cpath.as_ptr(), libc::uid_t::MAX, gid) };
    if rc != 0 {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        eprintln!(
            "rrd 日志目录 chgrp 失败（errno {errno}，保持 0750 root:root）: {}",
            dir.display()
        );
    }
}

/// rrd 自身日志的落盘文件路径（批N5 件二笔2，§17 运维留痕）：OnceLock 首份胜出，
/// daemon 装配链后定格一次。未定格（启动早于接线 / `--manifest` 前段 / Windows）
/// = 纯内存，`rr3_log` 行为与笔1 之前一字不变。回填读口用它。
static RR3_LOG_SINK: OnceLock<PathBuf> = OnceLock::new();

/// 落盘失败后的重试冷却（续轮二十三）：原口径"失败一次即本进程不再重试"会把
/// 一次瞬时故障（写抖动 / 暂时 ENOSPC / 目录被短暂挪走）变成"崩溃取证面永久关闭，
/// 只能重启恢复"。现改为有界重试：失败后冷却 10s，到点先重开句柄再试一行；
/// 成功即回 `on`，失败顺延一个冷却窗（只在转入降级与恢复时各点名一次）。
const RR3_SINK_RETRY_MS: u64 = 10_000;

/// 冷却比较用的进程内单调时钟（懒定格；零点与 daemon 起点无关，只做间隔比较）。
static RR3_SINK_T0: OnceLock<Instant> = OnceLock::new();

fn rr3_sink_now_ms() -> u64 {
    RR3_SINK_T0.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// 单次落盘耗时窗口（续轮二十五）：日志写发生在模块线程与 registry 调用链上，
/// "写一行的阻塞窗"就是观测通道对业务主路的实际干扰。判据可证伪：稳态下
/// `max_us` 应远小于 50ms（`slow` 恒 0）；磁盘抖动时 `slow` 增长可读。
const RR3_SINK_SLOW_US: u64 = 50_000;

#[derive(Debug, Default)]
struct Rr3SinkLat {
    max_us: u64,
    slow: u64,
}

impl Rr3SinkLat {
    fn observe(&mut self, us: u64) {
        self.max_us = self.max_us.max(us);
        if us >= RR3_SINK_SLOW_US {
            self.slow = self.slow.saturating_add(1);
        }
    }
}

/// 落盘可用性闸：`down_until_ms` = 0 健康；非 0 表示冷却到该时刻再试。
#[derive(Debug, Default)]
struct Rr3SinkGate {
    down_until_ms: u64,
}

impl Rr3SinkGate {
    fn due(&self, now_ms: u64) -> bool {
        self.down_until_ms == 0 || now_ms >= self.down_until_ms
    }

    fn down(&self) -> bool {
        self.down_until_ms != 0
    }

    /// 记一次失败；返回是否**首次**转入降级（点名口径：转入时打一行，冷却期内静默）。
    fn on_fail(&mut self, now_ms: u64) -> bool {
        let first = self.down_until_ms == 0;
        self.down_until_ms = now_ms.saturating_add(RR3_SINK_RETRY_MS);
        first
    }

    /// 记一次成功；返回是否从降级恢复。
    fn on_ok(&mut self) -> bool {
        let was = self.down_until_ms != 0;
        self.down_until_ms = 0;
        was
    }

    fn state(&self) -> &'static str {
        if self.down_until_ms == 0 {
            "on"
        } else {
            "dead"
        }
    }
}

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
    gate: Rr3SinkGate,
    lat: Rr3SinkLat,
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
            gate: Rr3SinkGate::default(),
            lat: Rr3SinkLat::default(),
        })
    }

    /// 冷却到点的重开：写失败常见诱因是句柄坏 / 文件被挪走，重开一次比继续撞
    /// 旧句柄更可能恢复；预算按重开后的实际大小重记。
    fn rr3_reopen(&mut self) -> std::io::Result<()> {
        self.file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.written = self.file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(())
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

/// 落盘读面（`ctl.status`）：状态 + 单次写阻塞窗（`max_us` 行长）与超阈次数。
/// 写失败进入冷却（续轮二十三），`dead` 是"此刻不可用"而非终态。
#[derive(Debug, Clone, Copy)]
pub struct Rr3LogSinkInfo {
    pub state: &'static str,
    pub max_us: u64,
    pub slow: u64,
}

pub fn rr3_log_sink_info() -> Rr3LogSinkInfo {
    let Some(sink) = RR3_LOG_SINK_FILE.get() else {
        return Rr3LogSinkInfo {
            state: "off",
            max_us: 0,
            slow: 0,
        };
    };
    let g = rr3_lock(sink);
    Rr3LogSinkInfo {
        state: g.gate.state(),
        max_us: g.lat.max_us,
        slow: g.lat.slow,
    }
}

/// `rr3_log` 的落盘臂：一行一次 `write_all`，不缓冲不批写——"崩溃前最后 N 行"的
/// 判据要求打过的行当场就在文件里（无用户态缓冲，写即入内核）。写失败进入冷却：
/// 冷却期内静默退纯内存（不逐行撞盘），到点先重开再试一行，成功即恢复。
pub(super) fn rr3_log_sink_append(line: &str) {
    let Some(sink) = RR3_LOG_SINK_FILE.get() else {
        return;
    };
    let now = rr3_sink_now_ms();
    let mut sink = rr3_lock(sink);
    if !sink.gate.due(now) {
        return;
    }
    if sink.gate.down() {
        let _ = sink.rr3_reopen();
    }
    let path = sink.path.display().to_string();
    let mut note = None;
    let t0 = Instant::now();
    let res = sink.rr3_append(line);
    sink.lat.observe(t0.elapsed().as_micros() as u64);
    match res {
        Ok(()) => {
            if sink.gate.on_ok() {
                note = Some(format!("日志落盘恢复（{path}）"));
            }
        }
        Err(e) => {
            if sink.gate.on_fail(now) {
                // 先记账再点名：rr3_log 会再进本函数，此刻已在冷却窗内 → 只走
                // stdout 与日志环，不会再碰出错的落盘路径（无递归、无重入锁）。
                note = Some(format!(
                    "日志落盘写失败（{path}）: {e} —— {RR3_SINK_RETRY_MS}ms 后重试，成功即恢复"
                ));
            }
        }
    }
    drop(sink);
    if let Some(n) = note {
        crate::rr3_log(&n);
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

    /// 状态读口（续轮十七）：单测进程不定格 sink，必须是 off 而不是 on/dead。
    #[test]
    fn sink_state_is_off_until_fixed() {
        assert_eq!(rr3_log_sink_info().state, "off");
        assert_eq!(rr3_log_sink_info().max_us, 0, "未定格无写耗时");
    }

    /// 写阻塞窗（续轮二十五）：max 只记峰值、不在窗口内回落；≥50ms 才计一次慢写。
    #[test]
    fn sink_lat_tracks_peak_and_slow_count() {
        let mut l = Rr3SinkLat::default();
        l.observe(120);
        assert_eq!((l.max_us, l.slow), (120, 0), "低于阈只记峰值");
        l.observe(30);
        assert_eq!(l.max_us, 120, "峰值不回落到最近一次");
        l.observe(RR3_SINK_SLOW_US);
        assert_eq!((l.max_us, l.slow), (RR3_SINK_SLOW_US, 1), "到阈计一次慢写");
        l.observe(80_000);
        assert_eq!((l.max_us, l.slow), (80_000, 2));
    }

    /// 落盘冷却闸（续轮二十三）：失败后一个冷却窗内不重试（不逐行撞盘），到点
    /// 到期再试；成功即恢复；点名只在"首次转入降级"与"恢复"各一次。
    #[test]
    fn sink_gate_cools_down_then_recovers() {
        let mut g = Rr3SinkGate::default();
        assert!(g.due(0), "健康时随时可写");
        assert!(!g.down());
        assert!(g.on_fail(1_000), "首次失败要点名");
        assert!(g.down() && g.state() == "dead");
        assert!(!g.due(1_000 + RR3_SINK_RETRY_MS - 1), "冷却窗内不重试");
        assert!(g.due(1_000 + RR3_SINK_RETRY_MS), "冷却到点再试");
        assert!(!g.on_fail(2_000), "降级中的再次失败不再点名");
        assert!(g.on_ok(), "成功要从降级恢复（点名一次）");
        assert!(!g.down() && g.state() == "on");
        assert!(!g.on_ok(), "健康时成功不点名");
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
