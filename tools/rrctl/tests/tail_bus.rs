//! `rr3-ctl tail` 的 UDS 端到端用例（计划册 §16 第 6 项）：真起 `rr3-ctl`
//! 子进程，对一台行为写死的假 rrd 走完 注册 → 事件过滤 → 收满退出 / 超时退出 /
//! 断链退出 / 注册失败 四条路径。
//!
//! 假 rrd 只实现 tail 用到的两种请求（`rr3.register` / `rr3.heartbeat`）与
//! 广播 Event，不做通用服务器——判据是"tail 在真实线路上怎么退"，不是 rrd 本身。

#![cfg(unix)]

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame, Rr3Header,
    Rr3PayloadType,
};
use serde_json::{json, Value};

const RR3_TAIL_SRC: u64 = 7;

/// 假 rrd 的回放剧本。
enum Mode {
    /// 先发一条别的主题，再发 `n` 条目标主题（过滤与收满判据）。
    Events { topic: String, n: usize },
    /// 注册后什么都不发（tail 必须按超时退出）。
    Timeout,
    /// 注册回执后立即断链（tail 必须报断链而不是等到超时）。
    Disconnect,
}

/// 临时总线路径：pid + 纳秒后缀，避免并发用例互踩。
fn tmp_bus(tag: &str) -> PathBuf {
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    std::env::temp_dir().join(format!("rr3-tail-{tag}-{}-{ns}.sock", std::process::id()))
}

/// 按当前时间造一帧 SEND（payload 类型由调用方给：Response / Event）。
fn frame_send(src: u64, dst: u64, ty: Rr3PayloadType, body: &str) -> Rr3Frame {
    let payload = rr3_body_encode(ty, body.as_bytes());
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
            src_task: src,
            dst_task: dst,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

fn reply(stream: &mut UnixStream, id: u64, result: Value) -> Result<(), rr_ipc::Rr3FrameError> {
    let body = json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string();
    rr3_frame_write(
        stream,
        &frame_send(1, RR3_TAIL_SRC, Rr3PayloadType::Response, &body),
    )
}

fn event(stream: &mut UnixStream, body: Value) -> Result<(), rr_ipc::Rr3FrameError> {
    let text = body.to_string();
    rr3_frame_write(
        stream,
        &frame_send(RR3_TAIL_SRC, 0, Rr3PayloadType::Event, &text),
    )
}

/// 假 rrd：注册回 `src_task=7`、心跳回 accepted，其余请求回空结果。
fn fake_rrd(listener: UnixListener, mode: Mode) {
    let Ok((mut stream, _)) = listener.accept() else {
        return;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut played = false;
    loop {
        let Ok(frame) = rr3_frame_read(&mut stream) else {
            return;
        };
        let Ok((ty, body)) = rr3_body_split(&frame.payload) else {
            return;
        };
        if ty != Rr3PayloadType::Request {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(&body) else {
            return;
        };
        let method = v["method"].as_str().unwrap_or("");
        let id = v["id"].as_u64().unwrap_or(0);
        let result = match method {
            "rr3.register" => json!({"src_task": RR3_TAIL_SRC}),
            "rr3.heartbeat" => json!({"beat": "accepted"}),
            _ => json!({}),
        };
        if reply(&mut stream, id, result).is_err() {
            return;
        }
        if method != "rr3.register" || played {
            continue;
        }
        played = true;
        match &mode {
            Mode::Events { topic, n } => {
                let other = "rr3-sense/audio/pcm";
                let _ = event(&mut stream, json!({"topic": other}));
                for _ in 0..*n {
                    let body = json!({"topic": topic, "doa": {"deg": 12.5}});
                    if event(&mut stream, body).is_err() {
                        return;
                    }
                }
            }
            Mode::Disconnect => return,
            Mode::Timeout => {}
        }
    }
}

/// 起假 rrd 并返回总线路径（socket 文件由调用方清理）。
fn fake_on(tag: &str, mode: Mode) -> PathBuf {
    let path = tmp_bus(tag);
    let listener = UnixListener::bind(&path).unwrap();
    std::thread::spawn(move || fake_rrd(listener, mode));
    path
}

/// 真起 rr3-ctl 子进程（`--bus-path` 指到假 rrd）。
fn run_tail(bus: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rr3-ctl"))
        .arg("--bus-path")
        .arg(bus)
        .arg("tail")
        .args(args)
        .output()
        .unwrap()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout_lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn tail_filters_topic_and_exits_after_count() {
    let topic = "rr3-sense/audio/vad_start";
    let bus = fake_on(
        "count",
        Mode::Events {
            topic: topic.into(),
            n: 2,
        },
    );
    let out = run_tail(&bus, &[topic, "--count", "2", "--secs", "10"]);
    std::fs::remove_file(&bus).ok();

    assert_eq!(out.status.code(), Some(0), "stderr={}", stderr_of(&out));
    let lines = stdout_lines(&out);
    assert_eq!(lines.len(), 2, "只输出目标主题的两条：{lines:?}");
    for line in &lines {
        let v: Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["body"]["topic"], topic);
        assert_eq!(v["src_task"], RR3_TAIL_SRC);
        assert!(v["ts_ms"].as_u64().is_some(), "行体必须带转发时刻");
    }
    assert!(
        stderr_of(&out).contains("收到 2 条"),
        "退出摘要：{}",
        stderr_of(&out)
    );
}

#[test]
fn tail_exits_nonzero_on_timeout() {
    let bus = fake_on("timeout", Mode::Timeout);
    let out = run_tail(
        &bus,
        &["rr3-sense/audio/vad_start", "--count", "1", "--secs", "1"],
    );
    std::fs::remove_file(&bus).ok();

    assert_eq!(out.status.code(), Some(1), "stderr={}", stderr_of(&out));
    assert!(stdout_lines(&out).is_empty(), "超时不得输出伪造事件");
    assert!(
        stderr_of(&out).contains("超时：已收 0/1 条"),
        "超时归因：{}",
        stderr_of(&out)
    );
}

#[test]
fn tail_exits_nonzero_on_disconnect() {
    let bus = fake_on("disc", Mode::Disconnect);
    let out = run_tail(
        &bus,
        &["rr3-sense/audio/vad_start", "--count", "5", "--secs", "10"],
    );
    std::fs::remove_file(&bus).ok();

    assert_eq!(out.status.code(), Some(1), "stderr={}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("总线已断开"),
        "断链归因：{}",
        stderr_of(&out)
    );
}

#[test]
fn tail_reports_registration_failure() {
    let bus = tmp_bus("no-listener");
    let out = run_tail(&bus, &["rr3-sense/audio/vad_start"]);
    std::fs::remove_file(&bus).ok();

    assert_eq!(out.status.code(), Some(1), "stderr={}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("tail 注册失败"),
        "注册失败归因：{}",
        stderr_of(&out)
    );
}
