// SPDX-License-Identifier: Apache-2.0
//! 云端 HTTP 传输：进程级共享 Agent（连接池）、POST 与应答体带帽读尽。

use std::io::Read;
use std::time::Duration;

/// 云端应答体策略帽（32 MiB）：ASR/LLM 应答 KB 级，TTS PCM 30s@48k 双声道约
/// 5.7MB；32 MiB 给足长语音余量，同时任何单次调用都不允许把板子内存顶穿。
const RR3_CLOUD_BODY_CAP: usize = 32 * 1024 * 1024;

/// 空闲连接池上界（连接复用机制的策略量）：三家端点同宿主不同路径，对话轮内
/// 同步串行，1 条即可复用；2 条给面板测连与对话轮并发的两路各留一条。总 4 条
/// 是"进程内至多两路并发 × 两条宿主"的上界，内存占用与连接数都可判定。
const RR3_CLOUD_IDLE_MAX: usize = 4;
const RR3_CLOUD_IDLE_PER_HOST: usize = 2;

/// 进程级共享 HTTP 代理（连接复用机制）：`ureq::post()` 顶层函数每次新建 Agent，
/// 即每次调用重做 TCP＋TLS 握手——一轮对话 ASR/LLM/TTS 三次握手，移动网络下多付
/// 数百 ms，且握手失败面按调用次数放大。共享 Agent 自带空闲连接池：同宿主请求
/// 串行复用同一条连接（keep-alive），池上界见 [`RR3_CLOUD_IDLE_MAX`]。
///
/// 失效连接语义如实登记：服务端在空闲期关闭连接后，客户端复用才发现的写失败由
/// ureq 自动换新连接重放（`send_prelude` 失败路径）；写成功而读响应前被关的窄
/// 竞态 ureq 不重放非幂等 POST 体，由调用方的有界重试（`rr3_http_try`）兜住。
fn rr3_http_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        eprintln!(
            "rr3-brain: 云端连接池就绪（空闲≤{RR3_CLOUD_IDLE_MAX}，每宿主≤{RR3_CLOUD_IDLE_PER_HOST}）"
        );
        ureq::AgentBuilder::new()
            .max_idle_connections(RR3_CLOUD_IDLE_MAX)
            .max_idle_connections_per_host(RR3_CLOUD_IDLE_PER_HOST)
            .build()
    })
}

/// 一次 POST；返回 (status, body)。4xx/5xx 也带回状态码与错误体（调用方折业务失败）。
///
/// key 空串 → 不带 Authorization 头（local 免鉴权路径）；real 档缺 key 由调用方
/// 先报错，不在此静默放行。传输异常（连接拒绝/超时/读断——Python OSError 族对位）
/// 与读体失败统一折「云端不可达 {url}: …」（URL 不含密钥——Authorization 只进
/// header，文案可安全带出）。
///
/// `fresh = true` 强制新 Agent（空连接池）：重试专用——池里可能躺着对端已关的
/// 连接，"写成功、读 EOF"在 macOS/Linux 调度下都可复现，复用同一池重试会二次踩雷。
pub(crate) fn rr3_http_post(
    url: &str,
    key: &str,
    body: &[u8],
    content_type: &str,
    timeout_s: f64,
    fresh: bool,
) -> Result<(u16, Vec<u8>), String> {
    let agent_req = if fresh {
        ureq::post(url)
    } else {
        rr3_http_agent().post(url)
    };
    let req = agent_req
        .set("Content-Type", content_type)
        .timeout(Duration::from_secs_f64(timeout_s));
    let req = if key.is_empty() {
        req
    } else {
        req.set("Authorization", &format!("Bearer {key}"))
    };
    let (status, resp) = match req.send_bytes(body) {
        Ok(resp) => (resp.status(), resp),
        // 4xx/5xx：urllib HTTPError 对位——状态码与错误体照回（调用方折业务失败）
        Err(ureq::Error::Status(code, resp)) => (code, resp),
        Err(e) => return Err(format!("云端不可达 {url}: {e}")),
    };
    Ok((status, rr3_read_body(url, resp)?))
}

/// 应答体读尽（read-until-close 语义对 urllib resp.read() 同形状——TTS 响应无
/// Content-Length、服务端发完即断，按流读尽即得）。读侧强制策略帽：云端/中间人
/// 返回超大响应时不能把板子的内存顶穿；读侧走 `take(cap+1)`，多读 1 字节
/// 专用于判"是否还有余量"，分配上界仍是 cap+1。
fn rr3_read_body(url: &str, resp: ureq::Response) -> Result<Vec<u8>, String> {
    rr3_read_capped_body(url, resp.into_reader(), RR3_CLOUD_BODY_CAP)
}

/// 带帽读体（帽参数显式化，单测用小帽钉边界）。
pub(crate) fn rr3_read_capped_body(
    url: &str,
    reader: impl Read,
    cap: usize,
) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    rr3_read_take(url, reader, cap, &mut buf)?;
    if buf.len() > cap {
        return Err(format!(
            "云端应答体超界（>{cap}B）: {url}（流式应答读尽策略帽，防内存耗尽）"
        ));
    }
    Ok(buf)
}

/// 带帽读尽：`take(cap+1)` 多读 1 字节专门用于判"是否还有余量"。
fn rr3_read_take(
    url: &str,
    reader: impl Read,
    cap: usize,
    buf: &mut Vec<u8>,
) -> Result<(), String> {
    reader
        .take(cap as u64 + 1)
        .read_to_end(buf)
        .map_err(|e| format!("云端不可达 {url}: {e}"))?;
    Ok(())
}
