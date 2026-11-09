//! 面板 HTTP 编解码面：**进程内唯一**读写 HTTP 字节的地方。
//!
//! 分层理由（解析 / 渲染两面自持）：路由（`server.rs`）只认
//! [`Rr3HttpRequest`](super::server::Rr3HttpRequest) 这个已解好的结构，执行面只认
//! 分派枚举——两者都不碰字节流。协议面是 HTTP/1.1 子集：请求行 + 头（只认
//! Content-Length）+ 定长 body；不做 keep-alive、不做 chunked、不做 TLS，
//! 一问一答 `Connection: close`。
//!
//! 全部读路径**边读边判上界**（`rr3_read_line_cap`）：慢速客户端不发 `\n` 也
//! 顶不穿面板线程内存——上界在读取过程中逐块判定，不做事后长度检查。
//!
//! 请求解析按三段拆开（请求行 / 头区 / body），每段独立可测：一段的判据坏了
//! 不会把另一段的错误文案淹没在同一函数里。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

use serde_json::{json, Value};

use super::server::{Rr3HttpRequest, Rr3PanelLimits};

/// 从缓冲读一行，**边读边判上界**：单行超 `cap` 立即 Err，不把整行读进内存。
///
/// `BufRead::read_line` 会一直分配到换行符为止——慢速客户端只要不发 `\n`，就能
/// 用一条超长请求行/头行把面板线程的内存顶穿（读超时只限时间，不限已分配量）。
/// 这里走 `fill_buf`/`consume`：每次只消费缓冲里的可见块，超界时 `out` 最多
/// `cap` 字节。返回追加字节数；0 = EOF。
pub(super) fn rr3_read_line_cap(
    reader: &mut impl BufRead,
    cap: usize,
    what: &str,
    out: &mut String,
) -> Result<usize, String> {
    let mut used = 0usize;
    loop {
        let buf = reader
            .fill_buf()
            .map_err(|e| format!("读{what}失败: {e}"))?;
        if buf.is_empty() {
            return Ok(used);
        }
        let (take, complete) = match buf.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (buf.len(), false),
        };
        if used + take > cap {
            return Err(format!("{what}超界（单行 > {cap}B）"));
        }
        let chunk =
            std::str::from_utf8(&buf[..take]).map_err(|e| format!("{what}不是合法 UTF-8: {e}"))?;
        out.push_str(chunk);
        reader.consume(take);
        used += take;
        if complete {
            return Ok(used);
        }
    }
}

/// 从连接上读一个请求：请求行 → 头（找 Content-Length）→ 读满 body。
/// 半包/慢客户端按 [`Rr3PanelLimits::read_timeout`] 折 Err，不 panic 不占线程；
/// 头区/body 上界同取 [`Rr3PanelLimits`]（I-18②：限额经 shared 贯穿）。
pub fn rr3_http_read_req(
    stream: &TcpStream,
    limits: &Rr3PanelLimits,
) -> Result<Rr3HttpRequest, String> {
    stream
        .set_read_timeout(Some(limits.read_timeout))
        .map_err(|e| format!("设读超时失败: {e}"))?;
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|e| format!("try_clone 失败: {e}"))?,
    );
    let (method, path, used) = rr3_req_line(&mut reader, limits.header_limit)?;
    // 请求行与头区共用同一总额预算：请求行不设界的话，单行无 \n 的慢发可让读侧
    // 无界增长（局域网 DoS 面，静态审计 0.0.8b 收口）。
    let content_length = rr3_req_heads(&mut reader, limits.header_limit, used)?;
    // body 上界先判、再分配：按声明长度 `vec![0; n]` 前必须确认 n 在限内
    if content_length > limits.body_limit {
        return Err(format!("body 超界（>{0}B）", limits.body_limit));
    }
    Ok(Rr3HttpRequest {
        method,
        path,
        body: rr3_req_body(&mut reader, content_length)?,
    })
}

/// 请求行："METHOD /path HTTP/1.x" →（方法, 路径, 已用头区字节数）。
fn rr3_req_line(reader: &mut impl BufRead, cap: usize) -> Result<(String, String, usize), String> {
    let mut line = String::new();
    let used = rr3_read_line_cap(reader, cap, "请求行", &mut line)?;
    if used == 0 {
        return Err("请求行不完整: 对端关闭".to_string());
    }
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| format!("请求行不完整: {line:?}"))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| format!("请求行缺路径: {line:?}"))?
        .to_string();
    Ok((method, path, used))
}

/// 读头区直到空行（或 EOF，与 `read_line` 的 0 字节语义一致）：只认
/// Content-Length，其余头一律忽略。返回声明的 body 长度。
fn rr3_req_heads(reader: &mut impl BufRead, cap: usize, mut used: usize) -> Result<usize, String> {
    let mut content_length = 0usize;
    loop {
        let remaining = cap.saturating_sub(used);
        if remaining == 0 {
            return Err(format!("头区超界（>{cap}B）"));
        }
        let mut head = String::new();
        let n = rr3_read_line_cap(reader, remaining, "头区", &mut head)?;
        if n == 0 {
            break;
        }
        used += n;
        let head = head.trim_end();
        if head.is_empty() {
            break;
        }
        if let Some(v) = head
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .map(str::trim)
        {
            content_length = v
                .parse()
                .map_err(|_| format!("Content-Length 不是数字: {v:?}"))?;
        }
    }
    Ok(content_length)
}

/// 按声明长度读满 body——半包在这里等满或超时。
fn rr3_req_body(reader: &mut impl Read, len: usize) -> Result<Vec<u8>, String> {
    let mut body = vec![0u8; len];
    reader
        .read_exact(&mut body)
        .map_err(|e| format!("读 body 失败（半包/超时）: {e}"))?;
    Ok(body)
}

/// 写一个 HTTP 应答并关连接（Connection: close：一问一答，与 ctl 短连同口径）
pub fn rr3_http_respond(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(), String> {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(body))
        .and_then(|_| stream.flush())
        .map_err(|e| format!("写应答失败: {e}"))
}

/// 请求体 JSON 解析：面板的 POST body 必须是 JSON 对象（空 body 视为 `{}`——
/// beep/reboot 这类无参动作允许裸 POST）
pub(super) fn rr3_panel_body_json(body: &[u8]) -> Result<Value, String> {
    if body.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(json!({}));
    }
    let text = std::str::from_utf8(body).map_err(|e| format!("body 非 UTF-8: {e}"))?;
    serde_json::from_str(text).map_err(|e| format!("body 不是合法 JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B-10（本轮审计）：超长且无换行的请求行/头行必须在读取过程中被上界截停，
    /// 不允许先把整行分配进内存再判长度（慢速客户端可借此顶穿面板线程内存）。
    #[test]
    fn read_line_cap_stops_before_unbounded_growth() {
        let mut c = std::io::Cursor::new(vec![b'a'; 4096]);
        let mut out = String::new();
        let e = rr3_read_line_cap(&mut c, 64, "请求行", &mut out).unwrap_err();
        assert!(e.contains("超界"), "{e}");
        assert!(out.len() <= 64, "已读内容不得超过上界: {}", out.len());
    }

    /// 正常行/EOF 语义：按行截断、返回追加字节数，EOF 返回 0
    #[test]
    fn read_line_cap_returns_line_and_eof() {
        let mut c = std::io::Cursor::new(b"GET /x HTTP/1.1\r\nHost: a\r\n".to_vec());
        let mut out = String::new();
        assert_eq!(
            rr3_read_line_cap(&mut c, 64, "请求行", &mut out).unwrap(),
            17
        );
        assert_eq!(out, "GET /x HTTP/1.1\r\n");
        let mut out = String::new();
        assert_eq!(rr3_read_line_cap(&mut c, 64, "头区", &mut out).unwrap(), 9);
        assert_eq!(out, "Host: a\r\n");
        let mut out = String::new();
        assert_eq!(
            rr3_read_line_cap(&mut c, 64, "头区", &mut out).unwrap(),
            0,
            "EOF 必须返回 0"
        );
    }

    /// 三段解析各自的坏形判据：缺路径、非法 Content-Length、头区超界
    #[test]
    fn req_stages_reject_bad_shapes() {
        let mut c = std::io::Cursor::new(b"GET\r\n".to_vec());
        assert!(rr3_req_line(&mut c, 64).unwrap_err().contains("缺路径"));

        let mut c = std::io::Cursor::new(b"Content-Length: x\r\n\r\n".to_vec());
        assert!(rr3_req_heads(&mut c, 64, 0)
            .unwrap_err()
            .contains("不是数字"));

        let mut c = std::io::Cursor::new(b"Host: a\r\n\r\n".to_vec());
        assert!(rr3_req_heads(&mut c, 4, 0)
            .unwrap_err()
            .contains("头区超界"));
    }
}
