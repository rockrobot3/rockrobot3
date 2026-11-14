//! 本地假端点（对位 test_cloud.py FakeSiliconFlow：记录请求、回罐头；std
//! TcpListener 手写微型 HTTP/1.1——仓内首个 HTTP 服务端测试夹具，仅测试编译）。
//! asr/llm/tts 三家原子件 mech 测试与 cloud.rs 本域测试共用。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::RR3_TEST_ENV_LOCK;

/// env 动作的测试互斥（全 crate 一柄，见 [`super::RR3_TEST_ENV_LOCK`]）
pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    RR3_TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// 测试 HTTP 服务收帧记账：(path, authorization, body)
pub(crate) type FakeReqs = Arc<Mutex<Vec<(String, String, Vec<u8>)>>>;
/// 可选路径开关（一次性 5xx / 坏 JSON / 缺字段夹具共用）
pub(crate) type FakePath = Arc<Mutex<Option<String>>>;

pub(crate) struct FakeCloud {
    pub(crate) base: String,
    pub(crate) requests: FakeReqs,
    pub(crate) fail_path: FakePath,
    /// 一次性 5xx（首请求 500、随即复位）——ASR 有界重试的正门夹具
    pub(crate) fail_once_path: FakePath,
    pub(crate) bad_json_path: FakePath,
    pub(crate) missing_field_path: FakePath,
    /// 累计 accept 次数（连接复用断言：N 请求 = 1 accept 即证明复用）
    pub(crate) conns: Arc<AtomicU32>,
}

impl FakeCloud {
    /// 默认档：应答带 `Connection: close`——每请求一条连接（对位既有断言）
    pub(crate) fn start() -> Self {
        Self::start_with(false)
    }

    /// keep-alive 档：同连接连续服务多请求（连接复用回归的观测面）
    pub(crate) fn start_keep_alive() -> Self {
        Self::start_with(true)
    }

    fn start_with(keep_alive: bool) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let fail_path = Arc::new(Mutex::new(None));
        let fail_once_path = Arc::new(Mutex::new(None));
        let bad_json_path = Arc::new(Mutex::new(None));
        let missing_field_path = Arc::new(Mutex::new(None));
        let conns = Arc::new(AtomicU32::new(0));
        let (r, f, fo, b, m, c) = (
            requests.clone(),
            fail_path.clone(),
            fail_once_path.clone(),
            bad_json_path.clone(),
            missing_field_path.clone(),
            conns.clone(),
        );
        // 线程随测试进程退出回收（cargo test 主线程等所有 #[test] 完成，本
        // detached 线程不阻塞收尾；每测试独立端口，线程间零干扰）
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                c.fetch_add(1, Ordering::Relaxed);
                // keep-alive 档在同一条流上连续读数个请求；读到 EOF 即收
                while let Some((path, headers, body)) = read_request(&mut stream) {
                    r.lock().unwrap().push((
                        path.clone(),
                        headers.get("authorization").cloned().unwrap_or_default(),
                        body,
                    ));
                    let fail = f.lock().unwrap().clone();
                    let fail_once = fo.lock().unwrap().clone();
                    let bad = b.lock().unwrap().clone();
                    let missing = m.lock().unwrap().clone();
                    let (status, payload): (&str, Vec<u8>) = if Some(&path) == fail_once.as_ref() {
                        *fo.lock().unwrap() = None; // 一次性：下一请求恢复罐头
                        ("500 Internal Server Error", br#"{"error":"boom"}"#.to_vec())
                    } else if Some(&path) == fail.as_ref() {
                        ("500 Internal Server Error", br#"{"error":"boom"}"#.to_vec())
                    } else if Some(&path) == bad.as_ref() {
                        // 200 + 非 JSON 体（B-16②：云 200 坏体的静默假成功口）
                        ("200 OK", b"not-json-at-all".to_vec())
                    } else if Some(&path) == missing.as_ref() {
                        ("200 OK", b"{}".to_vec())
                    } else if path == "/v1/audio/transcriptions" {
                        ("200 OK", "{\"text\": \"你好\"}".as_bytes().to_vec())
                    } else if path == "/v1/chat/completions" {
                        (
                            "200 OK",
                            json!({"choices": [{"message": {"role": "assistant", "content": "好的"}}]})
                                .to_string()
                                .into_bytes(),
                        )
                    } else {
                        // /v1/audio/speech → 原始 PCM
                        ("200 OK", [1u8, 0].repeat(1600))
                    };
                    respond(&mut stream, status, &payload, keep_alive);
                    if !keep_alive {
                        break;
                    }
                }
            }
        });
        Self {
            base: format!("http://127.0.0.1:{port}"),
            requests,
            fail_path,
            fail_once_path,
            bad_json_path,
            missing_field_path,
            conns,
        }
    }

    pub(crate) fn last(&self) -> (String, String, Vec<u8>) {
        self.requests.lock().unwrap().last().unwrap().clone()
    }
}

/// 读一个 POST 请求（循环读至 Content-Length 满足——单次 read 不可靠）
fn read_request(stream: &mut TcpStream) -> Option<(String, HashMap<String, String>, Vec<u8>)> {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&buf[..n]);
        if let Some(p) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
    };
    let text = String::from_utf8_lossy(&data[..header_end]).to_string();
    let mut lines = text.lines();
    let request_line = lines.next()?.to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }
    let clen: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    while data.len() < header_end + clen {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }
    let path = request_line.split_whitespace().nth(1)?.to_string();
    Some((path, headers, data[header_end..header_end + clen].to_vec()))
}

fn respond(stream: &mut TcpStream, status: &str, body: &[u8], keep_alive: bool) {
    let conn = if keep_alive { "keep-alive" } else { "close" };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: {conn}\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}
