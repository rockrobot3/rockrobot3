//! rrd 日志面：stdout / 文件落盘 / 内存环三面同源，含启动回填与启动摘要/降级快照。
//!
//! 2026-11-17 自 `lib.rs` 结构化拆分（纯移位）：公开路径由 `lib.rs` 的 re-export
//! 保持，`crate::rr3_log*` 调用点零改动；落盘**先于** stdout 的顺序见 [`rr3_log`]。
//! 2026-11-18 续轮十四把落盘臂（句柄 / 轮转 / 路径派生）拆到 `sink`，本文件只留
//! 出口、环、回填与启动快照；落盘从"每行 4 系统调用"改为常持句柄的 1 次写。

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::{Mutex, OnceLock};

use crate::rr3_lock;

#[cfg(unix)]
mod sink;
#[cfg(unix)]
pub(crate) use sink::{rr3_log_dir_ensure, rr3_log_rotate};
#[cfg(unix)]
pub use sink::{rr3_log_sink_info, rr3_log_sink_set, rr3_unit_log_dir};

/// 行字节（单点）：文本 + 换行一次成片。**分行两次 write 会在多线程日志下交错**
/// （A 行头 + B 行头 + 两个换行 ⇒ 排障看到被吞并的半行），且每次 write 各自
/// 一次系统调用；O_APPEND 的单次 write 是"行完整性"的唯一保证。
fn rr3_line_bytes(line: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(line.len() + 1);
    buf.extend_from_slice(line.as_bytes());
    buf.push(b'\n');
    buf
}

/// 统一日志出口：stdout、行首 `rrd: `。集成测试的可观察面全靠它。
/// 走 `io::Write` 逐行写（行缓冲，管道里也按行 flush），父进程逐行读得到。
///
/// ADR-0010 裁定 5：stdout 行为一字不动，同一行**追加**一份进固定容量日志环；
/// `rr3.ctl.logs` 读环的尾部副本。环在这里单点落——rr3_log 是全 daemon 唯一日志出口，
/// 在别的调用点各存一份迟早分叉。
///
/// 批N5 件二笔2（§17 运维留痕）：sink 已定格时同一行再同步 append 进
/// `<socket>.log/rrd.log`——stdout/环/文件三面同源，重启回填见 [`rr3_log_backfill`]。
pub fn rr3_log(msg: &str) {
    let line = format!("rrd: {msg}");
    // **落盘先于 stdout**：stdout 是可观察面，文件是崩溃后的取证面——若先打 stdout，
    // "看到了这行"不等于"这行已在文件里"，关停/崩溃窗口会把已打过的行从取证面丢掉
    // （2026-11-17 macOS 腿实证：收尸行先到 stdout，紧随的 SIGTERM 让第二世回填缺行）。
    // 落盘失败只降级自身（`rr3_log_sink_append` 内点名一次），不挡 stdout 与环。
    #[cfg(unix)]
    sink::rr3_log_sink_append(&line);
    // stdout 是观测通道：管道关闭（EPIPE）时 `println!` 会 panic，主线程/收件线程
    // 一 panic 就把整个守护带走——与落盘臂"通道故障降级、不放大成可用性故障"的
    // 纪律相悖。写失败静默降级，环照常。
    let _ = rr3_log_emit(&mut std::io::stdout(), &line);
    let mut ring = rr3_lock(rr3_log_ring());
    rr3_ring_push(&mut ring, line, rr3_log_ring_cap());
}

/// 单行写出（含换行与 flush，**一次 write 落整行**——见 [`rr3_line_bytes`]）：
/// 返回 false = 观测通道写失败，调用方降级不 panic。
fn rr3_log_emit(out: &mut dyn Write, line: &str) -> bool {
    out.write_all(&rr3_line_bytes(line))
        .and_then(|()| out.flush())
        .is_ok()
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
/// `false`——容量是 boot 期事实，运行期不改。0 拒定格：入环的"满则丢最旧"在
/// 0 容量下无元素可丢，会持环锁死转；解析侧已把 0 判红，这里守 API 边界。
pub fn rr3_log_ring_cap_set(cap: usize) -> bool {
    cap > 0 && RR3_LOG_RING_CAP.set(cap).is_ok()
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

/// 本次装配摘要的一行定格：daemon 启动时 set 一次，`rr3.ctl.status` 读回。
/// 远程诊断不必先翻日志；首份胜出（进程内装配事实，运行期不改）。
#[cfg(unix)]
static RR3_BOOT_SUMMARY: OnceLock<String> = OnceLock::new();

/// 定格启动摘要（daemon 装配完成后调一次）；重复调用被拒。
#[cfg(unix)]
pub fn rr3_boot_summary_set(line: String) -> bool {
    RR3_BOOT_SUMMARY.set(line).is_ok()
}

/// 读启动摘要：未定格（非装配路径/尚未启动完）返回 None，不造假值。
#[cfg(unix)]
pub fn rr3_boot_summary_get() -> Option<String> {
    RR3_BOOT_SUMMARY.get().cloned()
}

/// 降级原因定格（模块, 类别, 点名文案）：daemon 装配完成后 set 一次，
/// `rr3.ctl.health` 读回。与启动摘要同源同刻；未定格返回 None（后门路径），不造假值。
#[cfg(unix)]
static RR3_DEGRADE_NOTES: OnceLock<serde_json::Value> = OnceLock::new();

/// 定格降级原因（daemon 装配完成后调一次）；重复调用被拒。
#[cfg(unix)]
pub fn rr3_degrade_set(v: serde_json::Value) -> bool {
    RR3_DEGRADE_NOTES.set(v).is_ok()
}

/// 读降级原因：未定格返回 None，不造假值。
#[cfg(unix)]
pub fn rr3_degrade_get() -> Option<serde_json::Value> {
    RR3_DEGRADE_NOTES.get().cloned()
}

fn rr3_log_ring() -> &'static Mutex<VecDeque<String>> {
    RR3_LOG_RING.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// 日志环查询（`rr3.ctl.logs` 的数据源）：`prefix` 非空时只回包含该子串的行，
/// `tail > 0` 时只回最后 tail 行（0 = 不限）。只有 `conn`（`#[cfg(unix)]`）读它，
/// 所以本函数同样 cfg——Windows 侧无人调用，不 cfg 就是 `dead_code` 警告。
#[cfg(unix)]
pub fn rr3_log_query(prefix: &str, tail: usize) -> Vec<String> {
    rr3_log_filter(&rr3_lock(rr3_log_ring()), prefix, tail)
}

/// 查询的纯函数核（锁外可测）：过滤与尾行裁剪，不改环。
#[cfg(unix)]
fn rr3_log_filter(lines: &VecDeque<String>, prefix: &str, tail: usize) -> Vec<String> {
    let mut out: Vec<String> = lines
        .iter()
        .filter(|line| prefix.is_empty() || line.contains(prefix))
        .cloned()
        .collect();
    if tail > 0 && out.len() > tail {
        out.drain(..out.len() - tail);
    }
    out
}

/// 启动回填（批N5 件二笔2，§17 运维留痕同轨判据）：读落盘文件尾，把崩溃前的最后
/// 若干行**静默**置回环头——不 println、不再落盘（行已在文件里）。未定格 sink =
/// no-op。次序语义：文件行必比本次启动已打的行旧，[`rr3_backfill_lines`] 给出的
/// 消费序保证 push_front 后最旧行恰在环头；超容量由下面的裁剪收敛，环恒是
/// "最后 cap 行"。
#[cfg(unix)]
pub fn rr3_log_backfill() {
    let Some(path) = sink::rr3_log_sink_path() else {
        return;
    };
    let Some((text, partial)) = rr3_backfill_tail(path) else {
        return;
    };
    let cap = rr3_log_ring_cap();
    let lines = rr3_backfill_lines(&text, partial, cap);
    let mut ring = rr3_lock(rr3_log_ring());
    for line in lines {
        ring.push_front(line);
    }
    while ring.len() > cap {
        ring.pop_front();
    }
}

/// 读日志尾块（有界 64KB）：返回（有损解码文本，块首是否残行）；任何 IO 失败
/// 回 None——回填是观测面兜底，不阻断 boot。
fn rr3_backfill_tail(path: &std::path::Path) -> Option<(String, bool)> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    // 从尾块取（append 不轮转下文件无界，整读违背"有界"本意）。64KB 对兜底
    // 容量 512 行 × 常见行长 ~80B 满配有余；起点不在文件头时块首行可能截断，
    // 丢弃（rr3_backfill_lines 的 drop_first）。
    const RR3_LOG_BACKFILL_TAIL_BYTES: u64 = 64 * 1024;
    let start = len.saturating_sub(RR3_LOG_BACKFILL_TAIL_BYTES);
    // 起点前一个字节决定块首行是否残行：上一行以 '\n' 收尾即块首行完整，
    // 不必丢（旧序无条件丢，每重启白丢一行历史）；读不到按残行保守处理。
    let mut prev = None;
    if start > 0 {
        let mut one = [0u8; 1];
        if f.seek(SeekFrom::Start(start - 1)).is_ok() && f.read_exact(&mut one).is_ok() {
            prev = Some(one[0]);
        }
    }
    f.seek(SeekFrom::Start(start)).ok()?;
    // 按字节读到尾块、有损解码：尾块起点常落在多字节字符中间（中文日志每行
    // 都可能是），旧序 `read_to_string` 遇非 UTF-8 边界整块 Err，回填被静默
    // 跳过＝重启后环里没有崩溃前的行；块首残行由 drop_first 丢弃。
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Some((text, rr3_tail_partial(prev, start)))
}

/// 尾块首行是否残行（纯函数）：起点在文件头不算；否则看起点前一字节——上一行
/// 以 '\n' 收尾即块首行完整。读不到前一字节（`prev=None`）按残行保守处理。
fn rr3_tail_partial(prev: Option<u8>, start: u64) -> bool {
    start > 0 && prev != Some(b'\n')
}

/// 回填行集（纯函数）：文本按行取尾 `cap` 行，按 push_front 消费序返回——
/// 最新在前，依次 push_front 后时序与文件一致（最旧在环头）。`drop_first` =
/// 尾块起点不在文件头：块首行可能是截断的半行，丢弃。独立纯函数是为单测能
/// 无全局态地钉选取序（同 [`rr3_ring_push`] 的理由）。
fn rr3_backfill_lines(text: &str, drop_first: bool, cap: usize) -> Vec<String> {
    let mut lines: Vec<&str> = text.lines().collect();
    if drop_first && !lines.is_empty() {
        lines.remove(0);
    }
    lines.reverse();
    lines.into_iter().take(cap).map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// 日志查询核（ctl.logs）：前缀过滤 + 尾行裁剪；0 = 不限，空前缀 = 不过滤。
    #[cfg(unix)]
    #[test]
    fn log_filter_prefix_and_tail() {
        let lines: VecDeque<String> = [
            "rrd: 启动摘要 x",
            "rrd: 轮次 1: ASR 失败",
            "rrd: 启动摘要 y",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            rr3_log_filter(&lines, "", 0).len(),
            3,
            "空前缀不限尾数须全回"
        );
        assert_eq!(
            rr3_log_filter(&lines, "启动摘要", 0),
            vec!["rrd: 启动摘要 x", "rrd: 启动摘要 y"],
            "前缀过滤按子串命中"
        );
        assert_eq!(
            rr3_log_filter(&lines, "", 1),
            vec!["rrd: 启动摘要 y"],
            "尾行裁剪取最后 N 行"
        );
        assert_eq!(
            rr3_log_filter(&lines, "启动摘要", 1),
            vec!["rrd: 启动摘要 y"],
            "过滤后裁剪"
        );
        assert!(rr3_log_filter(&lines, "不存在", 0).is_empty());
    }

    /// 容量定格一次（首份胜出）：重复 set 不覆盖——容量是 boot 期事实，运行期不改。
    /// 全 crate 单测只有本用例 set（OnceLock 进程级单次，别处再 set 会互相污染）。
    #[test]
    fn log_ring_cap_sets_once_and_first_wins() {
        assert!(!rr3_log_ring_cap_set(0), "0 容量拒定格（入环会持锁死转）");
        assert!(rr3_log_ring_cap_set(8), "首次 set 必须成功");
        assert!(!rr3_log_ring_cap_set(999), "重复 set 必须失败（首份胜出）");
        assert_eq!(rr3_log_ring_cap(), 8);
    }

    /// 日志目录落位与收紧（0750）：other 无份，与 socket 0o660 的组模型对齐。
    #[cfg(unix)]
    #[test]
    fn log_dir_ensure_locks_mode() {
        use std::os::unix::fs::PermissionsExt;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base =
            std::env::temp_dir().join(format!("rr3-rrd-logdir-{}-{nanos}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = rr3_log_dir_ensure(&base.join("rr3.sock")).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o750, "日志目录须 0750（other 无份）");
        let _ = std::fs::remove_dir_all(&base);
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
        // 尾块切在多字节字符中间：有损解码后块首是残行（替换符），drop_first 丢它
        assert_eq!(
            rr3_backfill_lines("\u{FFFD}\u{FFFD}行\nl1\nl2", true, 9),
            vec!["l2".to_string(), "l1".to_string()],
            "块首残行丢弃，其后行照常回填"
        );
        assert!(
            rr3_backfill_lines("", true, 4).is_empty(),
            "空尾块 + drop_first 不得越界"
        );
        // 块首是否残行：文件头不丢；前一字节是换行 = 完整行保留；否则丢；
        // 前一字节读不到按残行保守处理
        assert!(!rr3_tail_partial(Some(b'\n'), 0), "文件头无残行");
        assert!(
            !rr3_tail_partial(Some(b'\n'), 99),
            "起点前一字节换行 = 块首行完整，不丢"
        );
        assert!(rr3_tail_partial(Some(b'x'), 99), "切在行中 = 残行");
        assert!(rr3_tail_partial(None, 99), "前字节取不到按残行保守");
    }

    /// 目录派生单点：`<socket>.log` 后缀直拼，不解析不 canonicalize——supervisor
    /// 的 per-unit 文件与 rrd 自身落盘共用同一条路径规则（一处定形）。
    #[cfg(unix)]
    #[test]
    fn unit_log_dir_appends_log_suffix() {
        use std::path::Path;
        assert_eq!(
            rr3_unit_log_dir(Path::new("/tmp/rr3-a.sock")),
            std::path::PathBuf::from("/tmp/rr3-a.sock.log")
        );
    }

    /// stdout 写失败必须降级为 false 而不是 panic：观测通道（管道关闭/EPIPE）
    /// 不得把守护进程带走（与落盘臂的降级纪律同源）。
    #[test]
    fn log_emit_returns_false_on_broken_channel() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert!(!rr3_log_emit(&mut Broken, "rrd: x"), "写失败须返回 false");
        let mut ok = Vec::new();
        assert!(rr3_log_emit(&mut ok, "rrd: x"), "可写通道须成功");
        assert_eq!(ok, b"rrd: x\n", "逐行写出，带换行分隔");
    }

    /// 行必须一次 write 落整（B40 审计）：分行两次 write 在多线程下会交错，
    /// 落盘档出现"两行头 + 两个换行"的坏行；单次 O_APPEND 写才保证行完整。
    #[test]
    fn log_line_is_single_write() {
        struct Counting {
            bytes: Vec<u8>,
            calls: usize,
        }
        impl Write for Counting {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.calls += 1;
                self.bytes.extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut c = Counting {
            bytes: Vec::new(),
            calls: 0,
        };
        assert!(rr3_log_emit(&mut c, "rrd: 一行日志"));
        assert_eq!(c.calls, 1, "整行须一次 write: {:?}", c.bytes);
        assert_eq!(c.bytes, "rrd: 一行日志\n".as_bytes());
    }
}
