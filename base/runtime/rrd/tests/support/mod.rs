//! 集成测试助手：起**真 rrd 子进程** + 裸 UDS 客户端，断言线级行为。
//!
//! 只在 Unix 编译（每个测试文件顶层 `#![cfg(unix)]`），Windows 侧这个模块整体不参与编译，
//! 所以 `cargo check --all-targets` 在 Windows 上是绿的——那不是证据，是盲区。
//!
//! 顺序约定（F1 之后是**真的**与顺序无关，不再只是注释）：应答由 conn 线程写、广播由这条连接的
//! writer 线程写，同一 socket 上**只保证整帧原子，不保证相对顺序**（Task 7 的并发形状，brief 明令；
//! 入队次序 ≠ 上线次序，见 `rrd/src/conn.rs` 的同一处注释）。
//! 所以本夹具按连接维护一条 `VecDeque<Rr3Frame>` 待处理队列：`rr3_read_where` 把谓词不成立的帧
//! **存回队列**而不是从 socket 上丢掉，"先按类型读应答、再按字段读 state"这类两阶段读因此与到达
//! 次序无关。读不到帧时给的是"没等到 X；队列里已积压 N 帧"的诊断，不是一句裸 io panic。
//! 没有"读第 N 帧"的助手——想这么写说明测试在赌时序。

#![allow(dead_code)] // 各测试文件按需取用，没用到不算死码

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame,
    Rr3FrameError, Rr3Header, Rr3PayloadType,
};
use serde_json::{json, Value};

/// 叠加层（Task 2 提交的那份）：夹具主题只在测试里在册
pub const RR3_OVERLAY_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/sim-topics.toml");

/// 等到 rrd 打出"监听 <path>"的超时
const READY_TIMEOUT: Duration = Duration::from_secs(5);
/// 单条日志等待
const LOG_TIMEOUT: Duration = Duration::from_secs(3);

pub struct RrdTest {
    pub socket: PathBuf,
    child: Child,
    lines: mpsc::Receiver<String>,
    seen: Vec<String>,
}

impl RrdTest {
    /// 起 rrd：套接字用 pid+tag 唯一名（Global Constraints 8），退避基数压到 20ms
    /// （Task 11 要跑完 1..60 档），注册前读超时 300ms（本任务测试 8 用它）。
    pub fn rr3_start(tag: &str) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        // 就绪判据用日志而非 connect：connect 探测会留下一条"首帧读取失败"噪声日志
        RrdTest::rr3_spawn(&socket, None, &socket)
    }

    /// 真正的起进程处。三个路径各管各的：
    /// - `socket_arg` 原样写进 `--socket`（可以是相对路径，CF-3 就靠这一点）
    /// - `cwd` 是子进程的工作目录（`None` = 继承测试进程的），相对 `socket_arg` 按它解析
    /// - `socket` 是**期望的绝对路径**：用于清残留、连它、以及等横幅回显它
    ///
    /// 把"传给 CLI 的原词"和"期望的绝对路径"分成两个参数，正是为了让用例能断言
    /// 横幅回显的是后者而不是前者（`rrd/tests/socket_banner.rs`）。
    ///
    /// 参数表里没有 `tag`：套接字名由**调用方**决定（`rr3_start` 把 tag 拼进路径，
    /// `socket_banner.rs` 自己拼名字），起进程这一层用不上它（M-5：死参数已删）。
    pub fn rr3_spawn(socket: &Path, cwd: Option<&Path>, socket_arg: &Path) -> RrdTest {
        let _ = std::fs::remove_file(socket);
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rrd"));
        cmd.arg("--socket")
            .arg(socket_arg)
            .arg("--topics-overlay")
            .arg(RR3_OVERLAY_PATH)
            .args(["--restart-base-ms", "20"])
            .args(["--pre-register-timeout-ms", "300"])
            .stdout(Stdio::piped())
            // stderr 直接继承：出错就在 cargo 输出里，不必再开一条管道读（也避免缓冲区写满把 rrd 卡死）
            .stderr(Stdio::inherit());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let mut child = cmd
            .spawn()
            .unwrap_or_else(|e| panic!("起 rrd 失败: {e}"));

        let (tx, rx) = mpsc::channel::<String>();
        let mut log = BufReader::new(child.stdout.take().expect("stdout 必须是管道"));
        thread::spawn(move || loop {
            let mut line = String::new();
            match log.read_line(&mut line) {
                Ok(0) => break, // EOF：子进程已退出
                Ok(_) => {
                    let t = line.trim_end().to_string();
                    if !t.is_empty() && tx.send(t).is_err() {
                        break; // 接收端已 drop
                    }
                }
                Err(_) => break,
            }
        });

        let mut me = RrdTest { socket: socket.to_path_buf(), child, lines: rx, seen: Vec::new() };
        me.rr3_wait_log(&format!("监听 {}", socket.display()));
        me
    }

    /// 裸客户端：5s 读超时，卡住即失败而不是挂住整个测试目标。
    /// 返回的是带待处理队列的 `RrdClient`（F1），不是裸 `UnixStream`。
    pub fn rr3_connect(&self) -> RrdClient {
        let s = UnixStream::connect(&self.socket).unwrap_or_else(|e| {
            panic!("连接 {} 失败: {e}；rrd 已打印:\n{}", self.socket.display(), self.seen.join("\n"))
        });
        s.set_read_timeout(Some(Duration::from_secs(5))).expect("set_read_timeout");
        RrdClient { stream: s, pending: VecDeque::new() }
    }

    fn rr3_next_line(&mut self, dur: Duration) -> Option<String> {
        match self.lines.recv_timeout(dur) {
            Ok(l) => Some(l),
            // 注：Rust 的模式里不能写 `RecvTimeoutError::{Timeout, Disconnected}`（简写只在
            // 表达式位置成立），这里用完整的 `|` 分支——语义与 brief 一致：超时与断开都当"暂无"。
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    /// 等到一条含 needle 的日志（先查已缓冲的）；超时把全部已读日志打进 panic，便于定位
    pub fn rr3_wait_log(&mut self, needle: &str) -> String {
        if let Some(i) = self.seen.iter().position(|l| l.contains(needle)) {
            return self.seen.remove(i);
        }
        let deadline = Instant::now() + LOG_TIMEOUT;
        loop {
            match self.rr3_next_line(Duration::from_millis(50)) {
                Some(l) if l.contains(needle) => return l,
                Some(l) => self.seen.push(l),
                None if Instant::now() > deadline => panic!(
                    "{LOG_TIMEOUT:?} 内没等到日志 {needle:?}；已读到:\n{}",
                    self.seen.join("\n")
                ),
                None => {}
            }
        }
    }

    /// 排干 dur 内的全部输出（等异步副作用：watchdog、重启），返回拼接文本
    pub fn rr3_logs(&mut self, dur: Duration) -> String {
        let started = Instant::now();
        while started.elapsed() < dur {
            if let Some(l) = self.rr3_next_line(Duration::from_millis(20)) {
                self.seen.push(l);
            }
        }
        self.seen.join("\n")
    }

    /// 断言 dur 内**没有**某串（重启/广播这类"不该发生"的副作用靠它抓）
    pub fn rr3_assert_no_log(&mut self, needle: &str, dur: Duration) {
        let logs = self.rr3_logs(dur);
        assert!(!logs.contains(needle), "不该出现 {needle:?}；实际日志:\n{logs}");
    }
}

impl Drop for RrdTest {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

// ---------- manifest 文本 ----------

/// 合法的最小可用 manifest。改这里的字面量必须同步改测试里的 `.replace(...)` 目标。
pub fn rr3_manifest(name: &str, hb: u64, wd: u64, policy: &str) -> String {
    format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/bin/{name}"

[events]
publishes = []
subscribes = ["rr3-runtime/module/state"]

[resources]
restart_policy = "{policy}"

[health]
heartbeat_ms = {hb}
watchdog_timeout_ms = {wd}
"#
    )
}

// ---------- 帧 ----------

pub fn rr3_request(method: &str, id: u64, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

pub fn rr3_json_frame(ty: Rr3PayloadType, body: &Value, src_task: u64, dst_task: u64) -> Rr3Frame {
    let payload = rr3_body_encode(ty, body.to_string().as_bytes());
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: 0,
            src_task,
            dst_task,
            payload_len: payload.len() as u32,
        },
        payload,
    }
}

/// 上行 RPC（默认 dst=rrd）
pub fn rr3_rpc_frame(method: &str, id: u64, params: Value, src_task: u64) -> Rr3Frame {
    rr3_json_frame(
        Rr3PayloadType::Request,
        &rr3_request(method, id, params),
        src_task,
        rr_bus::RR3_TASK_RRD,
    )
}

/// 注册帧（此时模块还没有 task，按契约填 0——"用户态不得自设"的另一种写法）
pub fn rr3_register_frame(manifest: &str, name: &str) -> Rr3Frame {
    rr3_rpc_frame("rr3.register", 1, json!({"name": name, "manifest": manifest}), 0)
}

/// 一条连接上的裸客户端 + **按连接存在的待处理队列**（F1）。
///
/// 为什么队列必须挂在连接上而不是全局：fd 会被复用，全局 map 会把上一条连接遗留的帧投给新连接
/// （评审裁定里明令不换 fd-keyed 全局 map）。也不动生产代码——顺序无关性本就该由读侧承担。
pub struct RrdClient {
    stream: UnixStream,
    pending: VecDeque<Rr3Frame>,
}

/// 一次"挑帧"最多扫多少帧。扫不到就是**夹具 bug**（谓词写错、或 rrd 真没发这一帧），
/// 不是把预算调大就能糊过去的偶发；提为常量是因为 Task 8 加 Event 分支、Task 10 加 watchdog
/// 广播之后一条连接上几秒内的帧数会超过旧的字面值 16（M-4）。
const SCAN_BUDGET: usize = 64;

/// 帧的一行摘要，只用于失败诊断
fn rr3_frame_brief(f: &Rr3Frame) -> String {
    format!("{:?} {}", f.header, String::from_utf8_lossy(&f.payload))
}

/// 队列现状的一句话诊断（失败文案里"没等到 X"之后紧跟它，省得再猜哪些帧被扫过）
fn rr3_pending_dump(c: &RrdClient) -> String {
    if c.pending.is_empty() {
        return "队列里已积压 0 帧".to_string();
    }
    format!(
        "队列里已积压 {} 帧:\n{}",
        c.pending.len(),
        c.pending.iter().map(rr3_frame_brief).collect::<Vec<_>>().join("\n---\n")
    )
}

/// 非 panic 版读帧：`Ok` 一帧，`Err` 把底层错误原样交回（EOF 与超时都在此分流）。
/// 队列里有存货就先交存货——这是"不匹配的帧绝不丢"的落点。
pub fn rr3_try_recv(c: &mut RrdClient) -> Result<Rr3Frame, Rr3FrameError> {
    if let Some(f) = c.pending.pop_front() {
        return Ok(f);
    }
    rr3_frame_read(&mut c.stream)
}

pub fn rr3_send(c: &mut RrdClient, frame: &Rr3Frame) {
    rr3_frame_write(&mut c.stream, frame).unwrap_or_else(|e| panic!("写帧失败: {e:?}"));
}

pub fn rr3_recv(c: &mut RrdClient) -> Rr3Frame {
    match rr3_try_recv(c) {
        Ok(f) => f,
        Err(e) => panic!("读帧失败: {e:?}"),
    }
}

/// 拆出类型 + JSON 体（本套件的体一律 JSON）
pub fn rr3_json_of(frame: &Rr3Frame) -> Value {
    let (_, body) = rr3_body_split(&frame.payload).unwrap_or_else(|e| panic!("前缀非法: {e:?}"));
    serde_json::from_slice(&body).expect("体必须是 JSON")
}

/// 按谓词挑帧（最多 `SCAN_BUDGET` 帧）。为什么必须挑：见本文件头注释的"顺序约定"。
/// 谓词不成立的帧**存回队列**（`push_front` 回原位，保持到达次序），后来的读法还能读到它们。
///
/// 实现要点：本轮扫过的帧先攒在 `deferred` 里、**扫完才回队列**。要是边扫边 `push_back`，
/// 下一轮 `rr3_try_recv` 会优先弹队列，于是同一帧被反复弹出又塞回，永远读不到 socket 上
/// 后面的帧（第一版就栽在这里：`duplicate_name…` 等 6 条用例以"64 帧内没等到"的形式变红）。
pub fn rr3_read_where<F: FnMut(&Rr3Frame) -> bool>(
    c: &mut RrdClient,
    what: &str,
    mut pred: F,
) -> Rr3Frame {
    let mut deferred: Vec<Rr3Frame> = Vec::new();
    let mut outcome: Result<Rr3Frame, String> = Err(format!(
        "{SCAN_BUDGET} 帧内没等到{what}（超出预算即视为夹具 bug：谓词写错或 rrd 确实没发这一帧）"
    ));
    for _ in 0..SCAN_BUDGET {
        match rr3_try_recv(c) {
            Ok(frame) => {
                if pred(&frame) {
                    outcome = Ok(frame);
                    break;
                }
                deferred.push(frame);
            }
            // 对端关闭（Closed / Torn*）与 5 s 读超时（Io os error 11）都不是"没等到 X"的
            // 替代品：要说清没等到什么、哪些帧还压在队列里。
            Err(e) => {
                outcome = Err(format!("没等到{what}（读帧提前结束: {e:?}）"));
                break;
            }
        }
    }
    // 本轮取走的帧按到达次序回到队列头部（倒序 push_front 正好还原正序）
    for f in deferred.into_iter().rev() {
        c.pending.push_front(f);
    }
    match outcome {
        Ok(f) => f,
        // 本轮扫过的帧此刻已经在队列里，所以 `rr3_pending_dump` 就是"积压 + 本轮读到"的完整现场
        Err(msg) => panic!("{}；{}", msg, rr3_pending_dump(c)),
    }
}

/// 一直读到 EOF 或读超时，返回收到的全部帧（队列里的也算）。
/// 用于"这条连接该被关掉/不该出现某帧"的断言——它不会 panic，所以能把
/// "rrd 有没有回 accepted"这类否定式判据钉住。终止靠的是 `rr3_connect` 设的 5 s 读超时。
pub fn rr3_drain_until_closed(c: &mut RrdClient) -> Vec<Rr3Frame> {
    let mut got = Vec::new();
    while let Ok(f) = rr3_try_recv(c) {
        got.push(f);
    }
    got
}

/// 第一条 Response
pub fn rr3_read_rpc_frame(c: &mut RrdClient) -> Rr3Frame {
    rr3_read_where(c, " Response", |f| {
        rr3_body_split(&f.payload).map(|(ty, _)| ty == Rr3PayloadType::Response).unwrap_or(false)
    })
}

pub fn rr3_read_rpc(c: &mut RrdClient) -> Value {
    let f = rr3_read_rpc_frame(c);
    rr3_json_of(&f)
}

/// 第一条 `rr3-runtime/module/state` 广播且 module/phase 匹配，返回体
pub fn rr3_read_state(c: &mut RrdClient, module: &str, phase: &str) -> Value {
    let f = rr3_read_where(c, &format!("state 广播 {module}/{phase}"), |f| {
        match rr3_body_split(&f.payload) {
            Ok((Rr3PayloadType::Notification, body)) => {
                let Ok(v) = serde_json::from_slice::<Value>(&body) else { return false };
                v["topic"] == "rr3-runtime/module/state"
                    && v["module"] == module
                    && v["phase"] == phase
            }
            _ => false,
        }
    });
    rr3_json_of(&f)
}
