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

pub fn run(cmd: &str, bus: &str) -> Result<String, String> {
    if !COMMANDS.contains(&cmd) {
        return Err(format!("未知命令 {cmd}，可用：{}", COMMANDS.join(" | ")));
    }
    let envelope = rr3_ctl_query(bus, &format!("rr3.ctl.{cmd}"))?;
    // 错误信封统一走 Err：ota 的恒拒（rrd 回 AIRY_EPERM，文案点 M6）与 reboot 的
    // 缺省恒拒（文案点双重门）都落在这一条上，main 会把它打到 stderr 并以非 0 退出
    if let Some(err) = envelope.get("error") {
        return Err(format!(
            "{cmd} 被拒（code {}）：{}",
            err.get("code")
                .map(Value::to_string)
                .unwrap_or_else(|| "?".into()),
            err.get("message").and_then(Value::as_str).unwrap_or("?")
        ));
    }
    match cmd {
        "status" => format_status(&envelope),
        "health" => format_health(&envelope),
        "logs" => format_logs(&envelope),
        "counters" => format_counters(&envelope),
        "reboot" => format_reboot(&envelope),
        // COMMANDS 白名单已把 cmd 限定在六者之内；其余到这里必有 result（不会发生）
        _ => Err(format!("{cmd}: 应答形状意外（无 result 也无 error）")),
    }
}

/// 一次完整往返：连接 → 发请求帧 → 收应答帧 → 解析 JSON 信封（result 或 error 原样带回）
#[cfg(unix)]
fn rr3_ctl_query(bus: &str, method: &str) -> Result<Value, String> {
    use std::io::Write as _;
    use std::path::Path;

    use rr_ipc::{
        opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame,
        Rr3Header, Rr3PayloadType,
    };

    let mut stream = rr_ipc::Rr3UdsListener::rr3_connect(Path::new(bus))
        .map_err(|e| format!("连接 {bus} 失败: {e}（rrd 在跑吗？--bus-path 对吗？）"))?;
    stream
        .set_read_timeout(Some(RR3_CTL_READ_TIMEOUT))
        .map_err(|e| format!("设读超时失败: {e}"))?;
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": {}})
        .to_string()
        .into_bytes();
    let payload = rr3_body_encode(Rr3PayloadType::Request, &body);
    let frame = Rr3Frame {
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
        },
        payload,
    };
    // 整帧一次 write_all（rr3_frame_write 的口径），不按字段分次写
    rr3_frame_write(&mut stream, &frame).map_err(|e| format!("写帧失败: {e:?}"))?;
    let _ = std::io::stdout().flush();
    let reply = rr3_frame_read(&mut stream)
        .map_err(|e| format!("读应答失败（{RR3_CTL_READ_TIMEOUT:?} 内无帧或对端断开）: {e:?}"))?;
    let (ty, body) =
        rr3_body_split(&reply.payload).map_err(|e| format!("应答 payload 前缀非法: {e:?}"))?;
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
    let mut widths: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for r in rows {
        for (i, cell) in r.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
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

/// 单元格取值：字符串去引号、数字原样、缺字段显式 `?`（两种缺失不混用一种写法）
fn cell(v: &Value, key: &str) -> String {
    let v = &v[key];
    if let Some(s) = v.as_str() {
        s.to_string()
    } else if let Some(n) = v.as_u64() {
        n.to_string()
    } else {
        "?".to_string()
    }
}

fn format_status(envelope: &Value) -> Result<String, String> {
    let result = envelope.get("result").ok_or("应答缺 result")?;
    let modules = result
        .get("modules")
        .and_then(Value::as_array)
        .ok_or("应答缺 result.modules")?;
    let rows: Vec<Vec<String>> = modules
        .iter()
        .map(|m| vec![cell(m, "name"), cell(m, "task"), cell(m, "phase")])
        .collect();
    let mut out = render_table(&["NAME", "TASK", "PHASE"], &rows);
    if let Some(d) = result.get("denied_publish").and_then(Value::as_u64) {
        out.push_str(&format!("denied_publish = {d}\n"));
    }
    Ok(out)
}

fn format_health(envelope: &Value) -> Result<String, String> {
    let result = envelope.get("result").ok_or("应答缺 result")?;
    let modules = result
        .get("modules")
        .and_then(Value::as_array)
        .ok_or("应答缺 result.modules")?;
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
    if lines.is_empty() {
        return Ok("（rrd 日志环为空）".to_string());
    }
    let mut out = String::new();
    for l in lines {
        out.push_str(l.as_str().unwrap_or("?"));
        out.push('\n');
    }
    Ok(out)
}

/// `counters` 运维计数读面（0.0.4 批B append，与 health 分开）：health 是心跳
/// 健康语义，counters 是业务运维账。全表键 = `模块名/键`（模块自报归并）或
/// rrd 自有键（如 `boots`，无前缀）——渲染按模块名前缀分组成人读表，
/// 自有键归 `rrd` 组；空表打"无计数"。
fn format_counters(envelope: &Value) -> Result<String, String> {
    let counters = envelope
        .get("result")
        .and_then(|r| r.get("counters"))
        .and_then(Value::as_object)
        .ok_or("应答缺 result.counters")?;
    if counters.is_empty() {
        return Ok("（无计数）".to_string());
    }
    let mut rows: Vec<(String, String, u64)> = Vec::with_capacity(counters.len());
    for (k, v) in counters {
        let n = v
            .as_u64()
            .ok_or_else(|| format!("计数 {k} 的值不是 u64: {v}"))?;
        let (module, key) = match k.split_once('/') {
            Some((m, rest)) => (m.to_string(), rest.to_string()),
            None => ("rrd".to_string(), k.clone()),
        };
        rows.push((module, key, n));
    }
    // 不依赖 JSON 对象的迭代序（serde_json feature 可换底层 Map）：显式排序定渲染
    rows.sort();
    let rows = rows
        .into_iter()
        .map(|(m, k, n)| vec![m, k, n.to_string()])
        .collect::<Vec<_>>();
    Ok(render_table(&["MODULE", "KEY", "VALUE"], &rows))
}

/// `reboot` 系统重启（0.0.4 批C append）：CLI 不加本地门（见模块头注释），
/// rrd 执行面已解锁时回执 `{"reboot": true}`，这里只渲染一行确认。
fn format_reboot(envelope: &Value) -> Result<String, String> {
    let result = envelope.get("result").ok_or("应答缺 result")?;
    match result.get("reboot").and_then(Value::as_bool) {
        Some(true) => Ok("系统重启已发起（rrd 侧 systemctl reboot 已执行）".to_string()),
        _ => Err("reboot 应答缺 result.reboot 或非 true".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_declared_command_is_dispatchable() {
        for cmd in COMMANDS {
            // 连不上假套接字也应该得到"连接失败"而非"未知命令"——命令面在查询之前已放行
            let err = run(cmd, "/tmp/rr3-ctl-no-such.sock").unwrap_err();
            assert!(!err.contains("未知命令"), "{cmd} 被 COMMANDS 白名单挡住了");
        }
    }

    #[test]
    fn unknown_command_names_the_alternatives() {
        let err = run("nope", "/tmp/t.sock").unwrap_err();
        assert!(
            err.contains("status") && err.contains("reboot"),
            "错误文案没给出可用命令: {err}"
        );
    }

    #[test]
    fn command_list_matches_the_rrd_ctl_whitelist() {
        // 六方法同名对齐是本批的裁定（ADR-0010 裁定 2/4/6）：方法名漂移在此变红
        assert_eq!(
            COMMANDS,
            ["status", "health", "logs", "ota", "counters", "reboot"]
        );
    }

    #[test]
    fn status_table_is_aligned_and_counts_denied_publish() {
        let env = json!({"result": {"modules": [
            {"name": "rr3-long-name", "task": 2, "phase": "running"},
            {"name": "rr3-a", "task": 3, "phase": "registered"}
        ], "denied_publish": 7}});
        let out = format_status(&env).unwrap();
        // 列宽由最长名字决定：短行必须被补齐到同一列位（NAME 列宽 13 = "rr3-long-name"）
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "NAME           TASK  PHASE");
        assert_eq!(lines[1], "rr3-long-name  2     running");
        assert_eq!(lines[2], "rr3-a          3     registered");
        assert_eq!(lines[3], "denied_publish = 7");
    }

    #[test]
    fn status_table_says_empty_when_no_module_is_registered() {
        let env = json!({"result": {"modules": [], "denied_publish": 0}});
        let out = format_status(&env).unwrap();
        assert_eq!(out, "NAME  TASK  PHASE\ndenied_publish = 0\n");
    }

    #[test]
    fn health_table_carries_the_three_heartbeat_numbers() {
        let env = json!({"result": {"modules": [
            {"name": "rr3-sima", "task": 2, "phase": "running",
             "heartbeat_ms": 200, "watchdog_ms": 1000, "since_beat_ms": 501}
        ]}});
        let out = format_health(&env).unwrap();
        let line = out.lines().nth(1).unwrap();
        for needle in ["rr3-sima", "running", "200", "1000", "501"] {
            assert!(line.contains(needle), "健康行缺 {needle}: {line}");
        }
    }

    #[test]
    fn logs_are_printed_verbatim_without_json_plumbing() {
        let env =
            json!({"result": {"lines": ["rrd: 监听 /tmp/a.sock", "rrd: 注册 rr3-sima task=2"]}});
        let out = format_logs(&env).unwrap();
        assert!(out.contains("rrd: 监听 /tmp/a.sock"), "{out}");
        assert!(out.contains("rrd: 注册 rr3-sima task=2"), "{out}");
        assert!(
            !out.contains("\"lines\""),
            "日志输出不得漏出 JSON 结构: {out}"
        );
    }

    #[test]
    fn missing_fields_render_as_question_mark_not_null() {
        let env = json!({"result": {"modules": [{"name": "rr3-x"}]}});
        let out = format_status(&env).unwrap();
        assert!(out.contains("?"), "缺失字段应渲染 ?：{out}");
        assert!(!out.contains("null"), "缺失字段不得渲染成 null：{out}");
    }

    #[test]
    fn malformed_envelope_is_an_error_not_a_panic() {
        assert!(format_status(&json!({})).is_err());
        assert!(format_health(&json!({"result": {}})).is_err());
        assert!(format_logs(&json!({"result": {"lines": 3}})).is_err());
        assert!(format_counters(&json!({})).is_err());
        assert!(format_counters(&json!({"result": {"counters": 3}})).is_err());
        assert!(format_reboot(&json!({"result": {}})).is_err());
        assert!(format_reboot(&json!({"result": {"reboot": false}})).is_err());
    }

    #[test]
    fn counters_group_by_module_prefix_and_render_sorted() {
        let env = json!({"result": {"counters": {
            "rr3-brain/asr_fail": 1, "rr3-brain/asr_ok": 8,
            "rr3-act/relay_on": 1, "boots": 2
        }}});
        let out = format_counters(&env).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 5, "{out}");
        assert!(lines[0].starts_with("MODULE"), "{out}");
        // 字节序：rr3-* 组在前，rrd 自有键（boots）的 rrd 组在后；组内按键名序
        assert!(
            lines[1].starts_with("rr3-act") && lines[1].contains("relay_on"),
            "{out}"
        );
        assert!(
            lines[2].starts_with("rr3-brain") && lines[2].contains("asr_fail"),
            "{out}"
        );
        assert!(
            lines[3].starts_with("rr3-brain") && lines[3].contains("asr_ok"),
            "{out}"
        );
        assert!(
            lines[4].starts_with("rrd") && lines[4].contains("boots"),
            "{out}"
        );
        assert!(out.contains(" 8"), "计数 8 应渲染: {out}");
    }

    #[test]
    fn counters_empty_table_says_no_counters() {
        let env = json!({"result": {"counters": {}}});
        assert_eq!(format_counters(&env).unwrap(), "（无计数）");
    }

    #[test]
    fn counters_non_u64_value_is_an_error_not_a_question_mark() {
        // 与 status 的 cell 不同：计数是 u64 全表，形状坏了报错而不是渲染 ?
        assert!(format_counters(&json!({"result": {"counters": {"boots": "x"}}})).is_err());
    }

    #[test]
    fn reboot_success_renders_acknowledgement() {
        let env = json!({"result": {"reboot": true}});
        let out = format_reboot(&env).unwrap();
        assert!(out.contains("重启"), "{out}");
    }

    /// 真往返：起一个假的"最小 rrd"（accept → 读一帧 → 回 canned 应答），
    /// run() 走完整 UDS 链路。只验协议面（帧往返、信封分流），渲染另有纯函数用例。
    #[cfg(unix)]
    mod unix_tests {
        use super::*;
        use std::io::Read;
        use std::os::unix::net::UnixListener;
        use std::thread;

        /// 进程号 + 自增计数，免引外部依赖（与 rr-ipc uds 测试同一套命名法）
        fn tmp_sock(tag: &str) -> std::path::PathBuf {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            std::env::temp_dir().join(format!(
                "rr3-ctl-{}-{tag}-{}.sock",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ))
        }

        /// 起 fake rrd：accept 一条连接、读一帧、回 `reply_body`，然后退出线程。
        fn spawn_fake_rrd(reply_body: String) -> String {
            let path = tmp_sock("fake");
            let _ = std::fs::remove_file(&path);
            let listener = UnixListener::bind(&path).expect("bind 失败");
            thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("accept 失败");
                let mut buf = [0u8; 4096];
                // 读到一帧即回（帧长 128 头 + payload；测试体远小于 4096）
                let n = stream.read(&mut buf).expect("读请求失败");
                assert!(n > 128, "首帧应含 128B 头: {n}");
                let payload = rr_ipc::rr3_body_encode(
                    rr_ipc::Rr3PayloadType::Response,
                    reply_body.as_bytes(),
                );
                let frame = rr_ipc::Rr3Frame {
                    header: rr_ipc::Rr3Header {
                        opcode: rr_ipc::opcode::SEND,
                        flags: 0,
                        trace_id: 0,
                        timestamp_ns: 0,
                        src_task: rr_bus::RR3_TASK_RRD,
                        dst_task: 0,
                        payload_len: payload.len() as u32,
                    },
                    payload,
                };
                rr_ipc::rr3_frame_write(&mut stream, &frame).expect("写应答失败");
            });
            path.to_string_lossy().into_owned()
        }

        #[test]
        fn status_roundtrip_renders_the_fake_rrds_reply() {
            let bus = spawn_fake_rrd(
                json!({"result": {"modules": [{"name": "rr3-sima", "task": 2, "phase": "running"}],
                      "denied_publish": 0}})
                .to_string(),
            );
            let out = run("status", &bus).expect("往返应成功");
            assert!(out.contains("rr3-sima") && out.contains("running"), "{out}");
        }

        #[test]
        fn error_envelope_from_rrd_becomes_a_cli_error() {
            let bus = spawn_fake_rrd(
                json!({"error": {"code": -22, "message": "OTA（含模块级 staging 更新）随 M6 落地，当前拒绝执行"}})
                    .to_string(),
            );
            let err = run("ota", &bus).unwrap_err();
            assert!(
                err.contains("M6") && err.contains("-22"),
                "错误应带码与文案: {err}"
            );
        }

        #[test]
        fn counters_roundtrip_renders_the_fake_rrds_reply() {
            let bus = spawn_fake_rrd(
                json!({"result": {"counters": {"rr3-sima/beat_ok": 3, "boots": 1}}}).to_string(),
            );
            let out = run("counters", &bus).expect("往返应成功");
            assert!(
                out.contains("rr3-sima") && out.contains("beat_ok") && out.contains("3"),
                "{out}"
            );
            assert!(out.contains("boots"), "rrd 自有键应入表: {out}");
        }

        #[test]
        fn reboot_rejection_from_rrd_becomes_a_cli_error() {
            let bus = spawn_fake_rrd(
                json!({"error": {"code": -22, "message": "reboot 执行面未解锁（需 cfg(linux) 且 env RR3_REBOOT_OK=1，缺省恒拒）"}})
                    .to_string(),
            );
            let err = run("reboot", &bus).unwrap_err();
            assert!(
                err.contains("-22") && err.contains("RR3_REBOOT_OK"),
                "拒绝应带码与文案: {err}"
            );
        }
    }
}
