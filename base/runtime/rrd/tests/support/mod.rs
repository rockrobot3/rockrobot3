//! 集成测试助手：起**真 rrd 子进程** + 裸 UDS 客户端，断言线级行为。
//!
//! 只在 Unix 编译（每个测试文件顶层 `#![cfg(unix)]`），Windows 侧这个模块整体不参与编译，
//! 所以 `cargo check --all-targets` 在 Windows 上是绿的——那不是证据，是盲区。
//!
//! 顺序约定（照 Task 7 的并发形状）：应答由 conn 线程写、广播由 writer 线程写，同一 socket
//! 上**只保证整帧原子，不保证相对顺序**。所以本文件只提供"按类型/字段挑帧"的读法，
//! 没有任何"读第 N 帧"的助手——想这么写说明测试在赌时序。

#![allow(dead_code)] // 各测试文件按需取用，没用到不算死码

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame, Rr3Header,
    Rr3PayloadType,
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
        let _ = std::fs::remove_file(&socket);
        let mut child = Command::new(env!("CARGO_BIN_EXE_rrd"))
            .arg("--socket")
            .arg(&socket)
            .arg("--topics-overlay")
            .arg(RR3_OVERLAY_PATH)
            .args(["--restart-base-ms", "20"])
            .args(["--pre-register-timeout-ms", "300"])
            .stdout(Stdio::piped())
            // stderr 直接继承：出错就在 cargo 输出里，不必再开一条管道读（也避免缓冲区写满把 rrd 卡死）
            .stderr(Stdio::inherit())
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

        let mut me = RrdTest { socket: socket.clone(), child, lines: rx, seen: Vec::new() };
        // 就绪判据用日志而非 connect：connect 探测会留下一条"首帧读取失败"噪声日志
        me.rr3_wait_log(&format!("监听 {}", socket.display()));
        me
    }

    /// 裸客户端：5s 读超时，卡住即失败而不是挂住整个测试目标
    pub fn rr3_connect(&self) -> UnixStream {
        let s = UnixStream::connect(&self.socket).unwrap_or_else(|e| {
            panic!("连接 {} 失败: {e}；rrd 已打印:\n{}", self.socket.display(), self.seen.join("\n"))
        });
        s.set_read_timeout(Some(Duration::from_secs(5))).expect("set_read_timeout");
        s
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

pub fn rr3_send(stream: &mut UnixStream, frame: &Rr3Frame) {
    rr3_frame_write(stream, frame).unwrap_or_else(|e| panic!("写帧失败: {e:?}"));
}

pub fn rr3_recv(stream: &mut UnixStream) -> Rr3Frame {
    match rr3_frame_read(&mut *stream) {
        Ok(f) => f,
        Err(e) => panic!("读帧失败: {e:?}"),
    }
}

/// 拆出类型 + JSON 体（本套件的体一律 JSON）
pub fn rr3_json_of(frame: &Rr3Frame) -> Value {
    let (_, body) = rr3_body_split(&frame.payload).unwrap_or_else(|e| panic!("前缀非法: {e:?}"));
    serde_json::from_slice(&body).expect("体必须是 JSON")
}

/// 按谓词挑帧（最多 16 帧）。为什么必须挑：见本文件头注释的"顺序约定"。
pub fn rr3_read_where<F: FnMut(&Rr3Frame) -> bool>(
    stream: &mut UnixStream,
    what: &str,
    mut pred: F,
) -> Rr3Frame {
    let mut skipped = Vec::new();
    for _ in 0..16 {
        let frame = rr3_recv(stream);
        if pred(&frame) {
            return frame;
        }
        skipped.push(format!("{:?} {}", frame.header, String::from_utf8_lossy(&frame.payload)));
    }
    panic!("16 帧内没等到{what}；跳过的帧:\n{}", skipped.join("\n---\n"));
}

/// 第一条 Response
pub fn rr3_read_rpc_frame(stream: &mut UnixStream) -> Rr3Frame {
    rr3_read_where(stream, " Response", |f| {
        rr3_body_split(&f.payload).map(|(ty, _)| ty == Rr3PayloadType::Response).unwrap_or(false)
    })
}

pub fn rr3_read_rpc(stream: &mut UnixStream) -> Value {
    let f = rr3_read_rpc_frame(stream);
    rr3_json_of(&f)
}

/// 第一条 `rr3-runtime/module/state` 广播且 module/phase 匹配，返回体
pub fn rr3_read_state(stream: &mut UnixStream, module: &str, phase: &str) -> Value {
    let f = rr3_read_where(stream, &format!("state 广播 {module}/{phase}"), |f| {
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
