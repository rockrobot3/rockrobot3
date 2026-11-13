//! 子命令实现（F3 / ADR-0010；0.0.4 批B 起 CLI 六子命令与 ctl 白名单对齐）：
//! 六个 `rr3.ctl.*` 查询经 UDS 送达 rrd 的事务面。
//!
//! 每个子命令是一次**一次性控制连接**（ADR-0010 裁定 2）：连接 → 发一帧 `Request`
//! （dst=rrd、方法 `rr3.ctl.*`、src_task 按契约填 0）→ 收一帧 `Response` → 渲染成人读
//! 文本退出。rrd 侧不注册、不记账，一问一答即断。
//!
//! JSON 形状是 rrd ↔ rr3-ctl 的内部协议契约，**不**作为 CLI 的输出格式承诺
//! （ADR-0010 裁定 6）：`format_*` 系列把应答渲染成对齐的文本表，是人读的。
//!
//! `reboot` 是六方法里唯一的写侧：CLI **不加本地门**——rrd 执行面的双重门
//! （cfg(linux) 且 env `RR3_REBOOT_OK=1`）是唯一真值点，CLI 加门即第二真值；
//! 被拒时回显 rrd 文案并以非 0 退出（错误信封统一走 `Err`）。

use serde_json::{json, Value};

/// 命令面与 rrd 的 ctl 白名单六方法逐一同名对齐（`rr3.ctl.<cmd>`）
pub const COMMANDS: [&str; 6] = ["status", "health", "logs", "ota", "counters", "reboot"];

/// 一次查询的读超时：rrd 不回就报错退出，不挂死运维的 shell
const RR3_CTL_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// 客户端帧的 src_task 按契约填 0：未注册连接没有 rrd 分配的 task，与注册帧同口径
const RR3_CTL_SRC_TASK: u64 = 0;

/// 错误信封渲染（单点）：rrd 的拒应答统一成 `cmd 被拒（code N）：msg`。缺字段
/// 以 `?` 占位而不是丢信息——"被拒"这一事实比拒因字段的完整度更重要。
fn rr3_ctl_reject(cmd: &str, envelope: &Value) -> Option<String> {
    let err = envelope.get("error")?;
    Some(format!(
        "{cmd} 被拒（code {}）：{}",
        rr3_reject_code(err),
        rr3_reject_msg(err)
    ))
}

/// 拒因码（缺字段以 `?` 占位）。
fn rr3_reject_code(err: &Value) -> String {
    err.get("code")
        .map(Value::to_string)
        .unwrap_or_else(|| "?".into())
}

/// 拒因文案（缺字段以 `?` 占位）。
fn rr3_reject_msg(err: &Value) -> &str {
    err.get("message").and_then(Value::as_str).unwrap_or("?")
}

pub fn run(cmd: &str, bus: &str) -> Result<String, String> {
    rr3_cmd_known(cmd)?;
    let envelope = rr3_ctl_query(bus, &format!("rr3.ctl.{cmd}"))?;
    rr3_ctl_answer(cmd, &envelope)
}

/// 信封先行判拒、成功再分派渲染（判据次序：先 ensure 后 render）。
fn rr3_ctl_answer(cmd: &str, envelope: &Value) -> Result<String, String> {
    rr3_ctl_ensure(cmd, envelope)?;
    rr3_ctl_render(cmd, envelope)
}

/// 命令必须在白名单（未知命令点名可用项）。
fn rr3_cmd_known(cmd: &str) -> Result<(), String> {
    if !COMMANDS.contains(&cmd) {
        return Err(format!("未知命令 {cmd}，可用：{}", COMMANDS.join(" | ")));
    }
    Ok(())
}

/// 错误信封统一走 Err：ota 的恒拒（rrd 回 AIRY_EPERM，文案点 M6）与 reboot 的
/// 缺省恒拒（文案点双重门）都落在这一条上。
fn rr3_ctl_ensure(cmd: &str, envelope: &Value) -> Result<(), String> {
    rr3_ctl_reject(cmd, envelope).map_or(Ok(()), Err)
}

/// 渲染器签名（信封 → 人读文本）。
type Renderer = fn(&Value) -> Result<String, String>;

/// 命令 → 渲染器（表驱动，与 COMMANDS 白名单同名）。
const RENDERERS: [(&str, Renderer); 5] = [
    ("status", format_status),
    ("health", format_health),
    ("logs", format_logs),
    ("counters", format_counters),
    ("reboot", format_reboot),
];

/// 成功信封按命令分派到各自的渲染器。
fn rr3_ctl_render(cmd: &str, envelope: &Value) -> Result<String, String> {
    RENDERERS.iter().find(|(name, _)| *name == cmd).map_or_else(
        // COMMANDS 白名单已把 cmd 限定在六者之内；其余到这里必有 result（不会发生）
        || Err(format!("{cmd}: 应答形状意外（无 result 也无 error）")),
        |(_, render)| render(envelope),
    )
}

/// 一次完整往返：连接 → 发请求帧 → 收应答帧 → 解析 JSON 信封（result 或 error 原样带回）
#[cfg(unix)]
fn rr3_ctl_query(bus: &str, method: &str) -> Result<Value, String> {
    use std::io::Write as _;

    let mut stream = rr3_ctl_connect(bus)?;
    let frame = rr3_ctl_frame(method);
    // 整帧一次 write_all（rr3_frame_write 的口径），不按字段分次写
    rr_ipc::rr3_frame_write(&mut stream, &frame).map_err(|e| format!("写帧失败: {e:?}"))?;
    let _ = std::io::stdout().flush();
    rr3_ctl_reply(&mut stream)
}

/// 控制连接：连不上/设读超时失败都点名（rrd 在跑吗？--bus-path 对吗？）。
#[cfg(unix)]
fn rr3_ctl_connect(bus: &str) -> Result<std::os::unix::net::UnixStream, String> {
    use std::path::Path;

    let stream = rr_ipc::Rr3UdsListener::rr3_connect(Path::new(bus))
        .map_err(|e| format!("连接 {bus} 失败: {e}（rrd 在跑吗？--bus-path 对吗？）"))?;
    stream
        .set_read_timeout(Some(RR3_CTL_READ_TIMEOUT))
        .map_err(|e| format!("设读超时失败: {e}"))?;
    Ok(stream)
}

/// 请求帧组包（dst=rrd、src_task=0、载荷 CRC 与体一致）。
#[cfg(unix)]
fn rr3_ctl_frame(method: &str) -> rr_ipc::Rr3Frame {
    use rr_ipc::{opcode, rr3_body_encode, Rr3Frame, Rr3Header, Rr3PayloadType};

    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": {}})
        .to_string()
        .into_bytes();
    let payload = rr3_body_encode(Rr3PayloadType::Request, &body);
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            // 契约规定 CLOCK_REALTIME 纳秒（envelope.schema.json offset 16）
            timestamp_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
            src_task: RR3_CTL_SRC_TASK,
            dst_task: rr_bus::RR3_TASK_RRD,
            payload_len: payload.len() as u32,
            // 载荷 CRC32 必须与体一致（线上完整性判据；发送路径另会复算，值同）
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// 收一帧应答：前缀分流 + 必须 Response + 体解析成 JSON。
#[cfg(unix)]
fn rr3_ctl_reply(stream: &mut std::os::unix::net::UnixStream) -> Result<Value, String> {
    use rr_ipc::Rr3PayloadType;

    let reply = rr_ipc::rr3_frame_read(stream)
        .map_err(|e| format!("读应答失败（{RR3_CTL_READ_TIMEOUT:?} 内无帧或对端断开）: {e:?}"))?;
    let (ty, body) = rr_ipc::rr3_body_split(&reply.payload)
        .map_err(|e| format!("应答 payload 前缀非法: {e:?}"))?;
    if ty != Rr3PayloadType::Response {
        return Err(format!("应答 payload 类型 {ty:?} 不是 Response"));
    }
    serde_json::from_slice(&body).map_err(|e| format!("应答体不是合法 JSON: {e}"))
}

/// 非 Unix 平台的编译面（④ 步只 `cargo check`）：UDS 是 Unix 专属
#[cfg(not(unix))]
fn rr3_ctl_query(_bus: &str, _method: &str) -> Result<Value, String> {
    Err("UDS 总线仅在 Unix 可用".to_string())
}

/// 渲染一张等宽对齐的文本表（列宽由数据决定，中文字符按 chars 计）
fn render_table(header: &[&str], rows: &[Vec<String>]) -> String {
    let widths = rr3_table_widths(header, rows);
    let line = |cells: &[String]| -> String {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c:<width$}", width = widths[i]))
            .collect::<Vec<_>>()
            .join("  ")
            // 末列对齐补出的尾随空格不进输出：对齐是列间的，行尾留着只有噪声
            .trim_end()
            .to_string()
    };
    let mut out = line(&header.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    out.push('\n');
    for r in rows {
        out.push_str(&line(r));
        out.push('\n');
    }
    out
}

/// 列宽（由表头与数据决定，中文字符按 chars 计）。
fn rr3_table_widths(header: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for r in rows {
        rr3_width_row(&mut widths, r);
    }
    widths
}

/// 一行的列宽推进（同列取最大）。
fn rr3_width_row(widths: &mut [usize], r: &[String]) {
    for (i, cell) in r.iter().enumerate() {
        widths[i] = widths[i].max(cell.chars().count());
    }
}

/// 单元格取值：字符串去引号、数字原样、缺字段显式 `?`（两种缺失不混用一种写法）
fn cell(v: &Value, key: &str) -> String {
    let v = &v[key];
    if let Some(s) = v.as_str() {
        s.to_string()
    } else {
        rr3_cell_num(v)
    }
}

/// 数字原样、缺字段显式 `?`。
fn rr3_cell_num(v: &Value) -> String {
    v.as_u64().map_or("?".to_string(), |n| n.to_string())
}

fn format_status(envelope: &Value) -> Result<String, String> {
    let (result, modules) = rr3_ctl_modules(envelope)?;
    let rows: Vec<Vec<String>> = modules
        .iter()
        .map(|m| vec![cell(m, "name"), cell(m, "task"), cell(m, "phase")])
        .collect();
    let mut out = render_table(&["NAME", "TASK", "PHASE"], &rows);
    out.push_str(&rr3_denied_note(result));
    Ok(out)
}

/// `result` 与其 `modules` 数组（缺件逐级点名）。
fn rr3_ctl_modules(envelope: &Value) -> Result<(&Value, &[Value]), String> {
    let result = rr3_ctl_result(envelope)?;
    Ok((result, rr3_modules_of(result)?))
}

/// `result.modules` 数组切片（缺件点名）。
fn rr3_modules_of(result: &Value) -> Result<&[Value], String> {
    result
        .get("modules")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| "应答缺 result.modules".to_string())
}

/// 应答 `result`（缺件点名）。
fn rr3_ctl_result(envelope: &Value) -> Result<&Value, String> {
    envelope
        .get("result")
        .ok_or_else(|| "应答缺 result".to_string())
}

/// `denied_publish` 附加行（缺字段不渲染）。
fn rr3_denied_note(result: &Value) -> String {
    result
        .get("denied_publish")
        .and_then(Value::as_u64)
        .map_or(String::new(), |d| format!("denied_publish = {d}\n"))
}

fn format_health(envelope: &Value) -> Result<String, String> {
    let (_result, modules) = rr3_ctl_modules(envelope)?;
    let rows: Vec<Vec<String>> = modules
        .iter()
        .map(|m| {
            vec![
                cell(m, "name"),
                cell(m, "task"),
                cell(m, "phase"),
                cell(m, "heartbeat_ms"),
                cell(m, "watchdog_ms"),
                cell(m, "since_beat_ms"),
            ]
        })
        .collect();
    Ok(render_table(
        &["NAME", "TASK", "PHASE", "HB_MS", "WD_MS", "SINCE_BEAT_MS"],
        &rows,
    ))
}

fn format_logs(envelope: &Value) -> Result<String, String> {
    let lines = envelope
        .get("result")
        .and_then(|r| r.get("lines"))
        .and_then(Value::as_array)
        .ok_or("应答缺 result.lines")?;
    Ok(rr3_logs_body(lines))
}

/// 日志行体：空环给固定文案，否则逐行拼接。
fn rr3_logs_body(lines: &[Value]) -> String {
    if lines.is_empty() {
        return "（rrd 日志环为空）".to_string();
    }
    rr3_logs_text(lines)
}

/// 日志行逐行拼接（非字符串项以 `?` 占位）。
fn rr3_logs_text(lines: &[Value]) -> String {
    let mut out = String::new();
    for l in lines {
        out.push_str(l.as_str().unwrap_or("?"));
        out.push('\n');
    }
    out
}

/// `counters` 运维计数读面（0.0.4 批B append，与 health 分开）：health 是心跳
/// 健康语义，counters 是业务运维账。全表键 = `模块名/键`（模块自报归并）或
/// rrd 自有键（如 `boots`，无前缀）——渲染按模块名前缀分组成人读表，
/// 自有键归 `rrd` 组；空表打"无计数"。
fn format_counters(envelope: &Value) -> Result<String, String> {
    let counters = rr3_counters_of(envelope)?;
    if counters.is_empty() {
        return Ok("（无计数）".to_string());
    }
    let rows = rr3_counter_rows(counters)?;
    Ok(render_table(&["MODULE", "KEY", "VALUE"], &rows))
}

/// `result.counters` 对象（缺件点名）。
fn rr3_counters_of(envelope: &Value) -> Result<&serde_json::Map<String, Value>, String> {
    envelope
        .get("result")
        .and_then(|r| r.get("counters"))
        .and_then(Value::as_object)
        .ok_or_else(|| "应答缺 result.counters".to_string())
}

/// 全表键拆成 `(模块, 键, 计数)` 并按模块/键序定渲染序。
fn rr3_counter_rows(counters: &serde_json::Map<String, Value>) -> Result<Vec<Vec<String>>, String> {
    let mut rows: Vec<(String, String, u64)> = Vec::with_capacity(counters.len());
    for (k, v) in counters {
        rows.push(rr3_counter_row(k, v)?);
    }
    // 不依赖 JSON 对象的迭代序（serde_json feature 可换底层 Map）：显式排序定渲染
    rows.sort();
    Ok(rows
        .into_iter()
        .map(|(m, k, n)| vec![m, k, n.to_string()])
        .collect())
}

/// 单条计数：值必须 u64；键按首个 `/` 拆模块前缀，无前缀归 `rrd` 组。
fn rr3_counter_row(k: &str, v: &Value) -> Result<(String, String, u64), String> {
    let n = v
        .as_u64()
        .ok_or_else(|| format!("计数 {k} 的值不是 u64: {v}"))?;
    let (module, key) = match k.split_once('/') {
        Some((m, rest)) => (m.to_string(), rest.to_string()),
        None => ("rrd".to_string(), k.to_string()),
    };
    Ok((module, key, n))
}

/// `reboot` 系统重启（0.0.4 批C append）：CLI 不加本地门（见模块头注释），
/// rrd 执行面已解锁时回执 `{"reboot": true}`，这里只渲染一行确认。
fn format_reboot(envelope: &Value) -> Result<String, String> {
    let result = rr3_ctl_result(envelope)?;
    if result.get("reboot").and_then(Value::as_bool) == Some(true) {
        return Ok("系统重启已发起（rrd 侧重启命令已执行）".to_string());
    }
    Err("reboot 应答缺 result.reboot 或非 true".to_string())
}

#[cfg(test)]
mod tests;
