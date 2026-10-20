//! rrd 的平台无关部分：命令行参数、所有权表装载、manifest 运行期事实、在册表状态机与上行 RPC 编解码。
//!
//! 纯逻辑放在 lib 里，`tests/*.rs` 与后续 `rrd/src/bin/sim_peer.rs` 才能直接引用
//! （bin 里的模块对外不可见）。UDS、watchdog 与进程派生只在 `#[cfg(unix)]` 的
//! `conn`/`daemon`/`supervisor` 模块里（Task 7 落地监听、Task 11b 落地 spawn），
//! 非 Unix 平台它们根本不参与编译，
//! 所以 Windows 侧既能 `cargo check --all-targets` 也能跑到这里的全部测试——但那只证明
//! cfg 边界未破，`conn.rs`/`daemon.rs`/`supervisor.rs` 的每一行都要靠 Unix 侧（L2/L3）才有证据。

pub mod axis;
pub mod axis_write;
pub mod assembly;
pub mod cli;
pub mod counters;
pub mod manifest;
pub mod probe;
pub mod profile;
pub mod registry;
pub mod rpc;
pub mod topics_load;
pub mod wiring;

#[cfg(unix)]
pub mod conn;
#[cfg(unix)]
pub mod daemon;
#[cfg(unix)]
pub mod supervisor;

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// per-unit 日志目录的派生单点：socket 全路径 + `.log` 后缀（批N4 件八定形，批N5
/// 件二笔2 迁本文件供两路共用——supervisor 的 per-unit 文件与 rrd 自身落盘同一
/// 目录）。同机多实例（测试 `/tmp/rr3-m2-*.sock`、产线 `/var/run/rock3-ipc.sock`）
/// 天然按实例隔离，不需要新 CLI 键；只算路径不建目录不打开，副作用留给调用方。
#[cfg(unix)]
pub(crate) fn rr3_unit_log_dir(bus_path: &Path) -> PathBuf {
    let mut dir = bus_path.as_os_str().to_os_string();
    dir.push(".log");
    PathBuf::from(dir)
}

/// rrd 自身日志的落盘文件路径（批N5 件二笔2，§17 运维留痕）：OnceLock 首份胜出，
/// daemon 装配链后定格一次。未定格（启动早于接线 / `--manifest` 前段 / Windows）
/// = 纯内存，`rr3_log` 行为与笔1 之前一字不变。
static RR3_LOG_SINK: OnceLock<PathBuf> = OnceLock::new();

/// 落盘写失败后的降级记账位：置位后本进程不再重试（磁盘故障不是每行日志都要
/// 重新撞一遍的墙），点名一次交给运维，之后退纯内存。
static RR3_LOG_SINK_DEAD: AtomicBool = AtomicBool::new(false);

/// 定格 rrd 自身日志落盘（批N5 件二笔2）：`<socket>.log/rrd.log`，与 per-unit
/// 文件同目录同派生单点（[`rr3_unit_log_dir`]）。目录建不出 = 返回 Err，调用方
/// 点名降级纯内存——观测通道故障降级、不放大成可用性故障。`bus_path` 必须已
/// 绝对化（main 的 [`crate::cli::rr3_socket_absolute_path`]）——相对路径就是链条
/// 上游破了，fail-closed 拒定格（与 supervisor 的 `rr3_open_unit_log` 同一口径）。
#[cfg(unix)]
pub fn rr3_log_sink_set(bus_path: &Path) -> Result<(), String> {
    if !bus_path.is_absolute() {
        return Err("socket 路径非绝对（上游绝对化失效）".into());
    }
    let dir = rr3_unit_log_dir(bus_path);
    std::fs::create_dir_all(&dir).map_err(|e| format!("建目录 {}: {e}", dir.display()))?;
    RR3_LOG_SINK
        .set(dir.join("rrd.log"))
        .map_err(|_| "落盘路径已定格过（首份胜出，重复定格被拒）".to_string())
}

/// `rr3_log` 的落盘臂：一行同步 append，不缓冲不批写——"崩溃前最后 N 行"的判据
/// 要求打过的行当场就在文件里。append 不 truncate 不轮转（批N4 件八同轨纪律）。
/// 每行现开现写：运维留痕是事件级频率，不值得常持句柄。打开或写失败 = 点名
/// 一次后置降级位，之后静默退纯内存。
fn rr3_log_sink_append(line: &str) {
    let Some(path) = RR3_LOG_SINK.get() else {
        return;
    };
    if RR3_LOG_SINK_DEAD.load(Ordering::Relaxed) {
        return;
    }
    let res = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(line.as_bytes()).and_then(|_| f.write_all(b"\n")));
    if let Err(e) = res {
        if !RR3_LOG_SINK_DEAD.swap(true, Ordering::Relaxed) {
            // eprintln 不走 rr3_log：降级说明本身不能再依赖降级中的通道（递归），
            // 也绝不进 stdout 管道（测试的可观察面是"rrd 说过什么"，不许混入）
            eprintln!(
                "rrd: 落盘写失败（{}）: {e} —— 运维留痕退纯内存，本进程不再重试",
                path.display()
            );
        }
    }
}

/// 统一日志出口：stdout、行首 `rrd: `。集成测试的可观察面全靠它。
/// 走 `println!`：Rust stdout 是行缓冲，管道里也按行 flush，父进程逐行读得到。
///
/// ADR-0010 裁定 5：stdout 行为一字不动，同一行**追加**一份进固定容量日志环；
/// `rr3.ctl.logs` 读环的尾部副本。环在这里单点落——rr3_log 是全 daemon 唯一日志出口，
/// 在别的调用点各存一份迟早分叉。
///
/// 批N5 件二笔2（§17 运维留痕）：sink 已定格时同一行再同步 append 进
/// `<socket>.log/rrd.log`——stdout/环/文件三面同源，重启回填见 [`rr3_log_backfill`]。
pub fn rr3_log(msg: &str) {
    let line = format!("rrd: {msg}");
    println!("{line}");
    rr3_log_sink_append(&line);
    let mut ring = rr3_lock(rr3_log_ring());
    rr3_ring_push(&mut ring, line, rr3_log_ring_cap());
}

/// 日志环容量兜底值（行数）。512 行对"运维看刚才发生了什么"足够，内存成本有界；
/// 满了丢最旧（环形语义，不是"到顶即停"）。装配单正门由 daemon 启动读
/// `[runtime].log_ring_cap` 一次定格（[`rr3_log_ring_cap_set`]，批N5 件二 F-30——
/// 判据"环容量 ∈ 装配单键集，装载期缺即红"）；`--manifest` 调试路径与装载完成前
/// 先打的行按本值收。
const RR3_LOG_RING_CAP_FALLBACK: usize = 512;

/// 日志环容量的进程级定格点（OnceLock 首份胜出，与装配快照同语义）。
static RR3_LOG_RING_CAP: OnceLock<usize> = OnceLock::new();

/// 定格日志环容量（行数）。daemon 装配链装载成功后调一次；重复调用失败返回
/// `false`——容量是 boot 期事实，运行期不改。
pub fn rr3_log_ring_cap_set(cap: usize) -> bool {
    RR3_LOG_RING_CAP.set(cap).is_ok()
}

/// 当前生效的环容量：定格值未落（未走装配单路径/尚未装载完）按兜底值收。
fn rr3_log_ring_cap() -> usize {
    *RR3_LOG_RING_CAP.get().unwrap_or(&RR3_LOG_RING_CAP_FALLBACK)
}

/// 日志环的单点入环：满则先丢最旧再入队。独立成纯函数（环 + 容量都是参数），
/// 裁剪语义才能在无全局态的单测里钉死——OnceLock 一进程只 set 得上一次，
/// 直接打全局的用例会互相污染。容量调小的一次性收敛也在本循环：环长超过
/// 新容量时连丢到容量以内，不会出现"只丢一个、环永远缩不回去"的僵局。
fn rr3_ring_push(ring: &mut VecDeque<String>, line: String, cap: usize) {
    while ring.len() >= cap {
        ring.pop_front();
    }
    ring.push_back(line);
}

static RR3_LOG_RING: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn rr3_log_ring() -> &'static Mutex<VecDeque<String>> {
    RR3_LOG_RING.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// 日志环的尾部副本（`rr3.ctl.logs` 的数据源）。只有 `conn`（`#[cfg(unix)]`）读它，
/// 所以本函数同样 cfg——Windows 侧无人调用，不 cfg 就是 `dead_code` 警告。
#[cfg(unix)]
pub fn rr3_log_tail() -> Vec<String> {
    rr3_lock(rr3_log_ring()).iter().cloned().collect()
}

/// 启动回填（批N5 件二笔2，§17 运维留痕同轨判据）：读落盘文件尾，把崩溃前的最后
/// 若干行**静默**置回环头——不 println、不再落盘（行已在文件里）。未定格 sink =
/// no-op。次序语义：文件行必比本次启动已打的行旧，[`rr3_backfill_lines`] 给出的
/// 消费序保证 push_front 后最旧行恰在环头；超容量由下面的裁剪收敛，环恒是
/// "最后 cap 行"。
#[cfg(unix)]
pub fn rr3_log_backfill() {
    let Some(path) = RR3_LOG_SINK.get() else {
        return;
    };
    let Ok(mut f) = std::fs::File::open(path) else {
        return;
    };
    let len = match f.metadata() {
        Ok(m) => m.len(),
        Err(_) => return,
    };
    // 从尾块取（append 不轮转下文件无界，整读违背"有界"本意）。64KB 对兜底
    // 容量 512 行 × 常见行长 ~80B 满配有余；起点不在文件头时块首行可能截断，
    // 丢弃（rr3_backfill_lines 的 drop_first）。
    const RR3_LOG_BACKFILL_TAIL_BYTES: u64 = 64 * 1024;
    let start = len.saturating_sub(RR3_LOG_BACKFILL_TAIL_BYTES);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return;
    }
    let mut buf = String::new();
    if f.read_to_string(&mut buf).is_err() {
        return;
    }
    let cap = rr3_log_ring_cap();
    let lines = rr3_backfill_lines(&buf, start > 0, cap);
    let mut ring = rr3_lock(rr3_log_ring());
    for line in lines {
        ring.push_front(line);
    }
    while ring.len() > cap {
        ring.pop_front();
    }
}

/// 回填行集（纯函数）：文本按行取尾 `cap` 行，按 push_front 消费序返回——
/// 最新在前，依次 push_front 后时序与文件一致（最旧在环头）。`drop_first` =
/// 尾块起点不在文件头：块首行可能是截断的半行，丢弃。独立纯函数是为单测能
/// 无全局态地钉选取序（同 [`rr3_ring_push`] 的理由）。
fn rr3_backfill_lines(text: &str, drop_first: bool, cap: usize) -> Vec<String> {
    let mut lines: Vec<&str> = text.lines().collect();
    if drop_first {
        lines.remove(0);
    }
    lines.reverse();
    lines.into_iter().take(cap).map(str::to_string).collect()
}

/// 锁中毒的唯一处理口径：中毒只可能来自 panic 的持锁者，继续用比弃连接/比整个守护停摆好。
/// **单点**原因：`conn.rs`（应答与收尾）、`daemon.rs`（watchdog 扫描与广播）与 `supervisor.rs`
/// （收尸与重启）共用同一本在册表锁，三处各写一份"怎么处理中毒"迟早分叉——与 Task 9 抽
/// `rr3_spawn_line_reader` 是同一条理由。
///
/// 原 `#[cfg(unix)]` 已随 F3（ADR-0010 裁定 5）摘除：`rr3_log` 的日志环让本函数在
/// 平台无关代码里也有了调用方，cfg 反而会在 Windows 上变成 `unused` 警告。
pub(crate) fn rr3_lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// 非 Unix 平台的唯一出口文案（Task 3 Step 6 的实证按它断言，别改字面量）
pub const RRD_UNSUPPORTED: &str =
    "rrd: 本平台不支持 UDS 总线（需 Unix；Windows 侧只编译协议层与平台无关逻辑）";

/// 启动横幅。主题数来自装载结果，不得写死 21。
pub fn rr3_banner(contract: u32, manifest: u32, bus: &str, topics: usize) -> String {
    format!(
        "rrd 0.0.1 (contracts schema v{contract}, manifest schema v{manifest}, bus {bus}, {topics} topics)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_names_both_schemas_and_topic_count() {
        let s = rr3_banner(1, 1, "/var/run/rock3-ipc.sock", 21);
        assert!(s.contains("contracts schema v1"), "{s}");
        assert!(s.contains("manifest schema v1"), "{s}");
        assert!(s.contains("rock3-ipc.sock"), "{s}");
        assert!(s.contains("21 topics"), "横幅必须打印在册主题数: {s}");
    }

    /// 主题数必须来自装载结果（把 21 写死进文案就抓不到叠加层生效与否）
    #[test]
    fn banner_topic_count_is_not_hardcoded() {
        assert!(rr3_banner(1, 1, "/tmp/x", 24).contains("24 topics"));
    }

    /// 环形裁剪语义（批N5 件二）：满则丢最旧、尾部恒是新行；容量调小时环一次性
    /// 收敛到容量以内（批N5 件二笔1 的 while 循环是钉这条的——只丢一个的写法
    /// 会让"cap 从 512 改 8"之后环永远缩不回 8）
    #[test]
    fn ring_push_keeps_last_cap_lines_and_converges_on_shrink() {
        let mut ring = VecDeque::new();
        for i in 0..4 {
            rr3_ring_push(&mut ring, format!("l{i}"), 2);
        }
        assert_eq!(
            ring.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["l2", "l3"],
            "满则丢最旧，环里恒是最后 cap 行"
        );
        // 容量调小的一次性收敛：环长 2 > 新 cap 1，下一推连丢到容量以内
        rr3_ring_push(&mut ring, "l4".into(), 1);
        assert_eq!(
            ring.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["l4"],
            "容量调小后环必须收敛到新容量以内"
        );
    }

    /// 容量定格一次（首份胜出）：重复 set 不覆盖——容量是 boot 期事实，运行期不改。
    /// 全 crate 单测只有本用例 set（OnceLock 进程级单次，别处再 set 会互相污染）。
    #[test]
    fn log_ring_cap_sets_once_and_first_wins() {
        assert!(rr3_log_ring_cap_set(8), "首次 set 必须成功");
        assert!(!rr3_log_ring_cap_set(999), "重复 set 必须失败（首份胜出）");
        assert_eq!(rr3_log_ring_cap(), 8);
    }

    /// 回填行集（批N5 件二笔2）：取尾 cap 行、push_front 消费序（最新在前）；
    /// drop_first 只丢块首可能截断的半行；空文本回填空。
    #[test]
    fn backfill_lines_take_tail_in_front_push_order() {
        let text = "l0\nl1\nl2\nl3";
        assert_eq!(
            rr3_backfill_lines(text, false, 2),
            vec!["l3".to_string(), "l2".to_string()],
            "尾 cap 行、最新在前：依次 push_front 后 l2 在环头、l3 在环尾（时序与文件一致）"
        );
        assert_eq!(
            rr3_backfill_lines(text, true, 3),
            vec!["l3".to_string(), "l2".to_string(), "l1".to_string()],
            "drop_first 只丢 l0，不影响尾部选取"
        );
        assert!(rr3_backfill_lines("", false, 4).is_empty(), "空文件回填空");
    }

    /// 目录派生单点：`<socket>.log` 后缀直拼，不解析不 canonicalize——supervisor
    /// 的 per-unit 文件与 rrd 自身落盘共用同一条路径规则（一处定形）。
    #[cfg(unix)]
    #[test]
    fn unit_log_dir_appends_log_suffix() {
        assert_eq!(
            rr3_unit_log_dir(Path::new("/tmp/rr3-a.sock")),
            PathBuf::from("/tmp/rr3-a.sock.log")
        );
    }
}
