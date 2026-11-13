//! 命令面单测：命令表可派发与各应答格式化的形状判据。

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

/// 拒应答渲染（B44 拆件后的单点）：code/message 都在文案里；字段缺失也要保住
/// "被拒"这一事实；成功信封不干预（走各自 format_*）。
#[test]
fn rejection_envelope_names_code_and_message() {
    let env = json!({"error": {"code": 7, "message": "ota 在 M6 前恒拒"}});
    let msg = rr3_ctl_reject("ota", &env).expect("有 error 必渲染");
    assert!(msg.contains("ota 被拒"), "{msg}");
    assert!(msg.contains("code 7"), "{msg}");
    assert!(msg.contains("M6"), "{msg}");
    let bare = json!({"error": {}});
    let msg = rr3_ctl_reject("reboot", &bare).expect("有 error 必渲染");
    assert!(msg.contains("code ?") && msg.ends_with('?'), "{msg}");
    assert!(rr3_ctl_reject("status", &json!({"result": {}})).is_none());
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
    let env = json!({"result": {"lines": ["rrd: 监听 /tmp/a.sock", "rrd: 注册 rr3-sima task=2"]}});
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
            let payload =
                rr_ipc::rr3_body_encode(rr_ipc::Rr3PayloadType::Response, reply_body.as_bytes());
            let frame = rr_ipc::Rr3Frame {
                header: rr_ipc::Rr3Header {
                    opcode: rr_ipc::opcode::SEND,
                    flags: 0,
                    trace_id: 0,
                    timestamp_ns: 0,
                    src_task: rr_bus::RR3_TASK_RRD,
                    dst_task: 0,
                    payload_len: payload.len() as u32,
                    payload_crc32: rr_ipc::rr3_crc32(&payload),
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
