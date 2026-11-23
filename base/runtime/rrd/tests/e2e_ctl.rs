#![cfg(unix)]
// SPDX-License-Identifier: Apache-2.0
//! F3（ADR-0010）：`rr3.ctl.*` 控制查询面的线级用例。
//!
//! 判据形状：
//! - ctl 连接**不注册**也能得到应答（握手旁支），且一问一答即断（应答后连接被收掉）；
//! - `status` / `health` 的数据来自在册表快照，列出的必须只有真注册的模块——
//!   ctl 连接自己**不得**出现在快照里（裁定 2：运维探针不进账本）；
//! - `logs` 读 rrd 日志环，启动横幅（`监听 …`）必须在环里；
//! - `ota` 恒拒（AIRY_EPERM + 文案点 M6）；
//! - 已注册模块连接上行 `rr3.ctl.*` 被拒（Permission 错误帧）但**连接不断**
//!   （与 src_task 伪造同一口径：一次笔误不掉线）。
//!
//! 计数聚合/持久化/关停冲刷闭环独立在 `e2e_counters.rs`（同一 ctl 读口，场景不同）。

mod support;

use support::{
    register_sima_task, rr3_ask_ctl, rr3_assert_ctl_closed, rr3_json_of, rr3_manifest,
    rr3_read_rpc, rr3_read_where, rr3_register_frame, rr3_rpc_frame, rr3_send, RrdClient, RrdTest,
};

use rr_ipc::{rr3_body_split, Rr3PayloadType, Rr3StatusCode};

fn ctl_status_setup() -> (RrdTest, RrdClient, u64) {
    let t = RrdTest::rr3_start_no_overlay("ctl-status");
    let mut a = t.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    let reg = rr3_read_rpc(&mut a);
    let task = reg["result"]["src_task"]
        .as_u64()
        .expect("注册应答带 src_task");
    (t, a, task)
}

fn assert_status_module(modules: &[serde_json::Value], task: u64) {
    assert_eq!(modules[0]["name"], "rr3-sima");
    assert_eq!(modules[0]["task"], task);
    assert_eq!(modules[0]["phase"], "registered");
}

fn assert_status_cols(v: &serde_json::Value) {
    assert!(
        v["result"]["denied_publish"].is_u64(),
        "denied_publish 计数必须在场: {v}"
    );
    // 稳态判据依赖 fan_dropped 行（续轮四十三起 sim-chain 收尾必读）：读口缺列
    // 会让 60s 稳态段在最后一刻才报"缺 fan_dropped 行"——在线级用例里先钉住。
    assert!(
        v["result"]["fan_dropped"].is_u64(),
        "fan_dropped 丢帧计数必须在场: {v}"
    );
    assert!(
        matches!(
            v["result"]["log_sink"].as_str(),
            Some("on" | "off" | "dead")
        ),
        "log_sink 落盘状态必须可读（on/off/dead）: {v}"
    );
    assert!(
        matches!(
            v["result"]["ctr_store"].as_str(),
            Some("on" | "off" | "dead")
        ),
        "ctr_store 计数落盘执行面状态必须可读（on/off/dead）: {v}"
    );
    assert!(
        v["result"]["log_sink_max_us"].is_u64() && v["result"]["log_sink_slow"].is_u64(),
        "落盘写阻塞窗（峰值 us / 慢写计数）必须在场: {v}"
    );
    assert!(
        v["result"]["fan_write_timeout"].is_u64(),
        "fanout 写超时累计必须在场（慢消费者点名）: {v}"
    );
    assert!(
        v["result"]["boot"]
            .as_str()
            .is_some_and(|s| s.contains("启动摘要")),
        "boot 装配摘要必须在场（远程诊断不翻日志）: {v}"
    );
}

#[test]
fn ctl_status_lists_registered_modules_and_does_not_register_itself() {
    let (t, _a, task) = ctl_status_setup();
    let mut ctl = t.rr3_connect();
    let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.status", 1);
    let modules = v["result"]["modules"]
        .as_array()
        .expect("status 应带 result.modules");
    assert_eq!(
        modules.len(),
        1,
        "只有 rr3-sima 在册——ctl 连接自己不得进快照: {v}"
    );
    assert_status_module(modules, task);
    assert_status_cols(&v);
    rr3_assert_ctl_closed(&mut ctl);
}

#[test]
fn ctl_health_echoes_heartbeat_and_watchdog_numbers() {
    let t = RrdTest::rr3_start_no_overlay("ctl-health");
    let mut a = t.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 200, 900, "always"), "rr3-sima"),
    );
    rr3_read_rpc(&mut a);

    let mut ctl = t.rr3_connect();
    let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.health", 1);
    let rows = v["result"]["modules"]
        .as_array()
        .expect("health 应带 result.modules");
    assert_eq!(rows.len(), 1, "{v}");
    let r = &rows[0];
    // 配置回显条目**自己**的三个数（Rr3Expiry 同一条理据：判定用的数必须回显）
    assert_eq!(r["heartbeat_ms"], 200, "{v}");
    assert_eq!(r["watchdog_ms"], 900, "{v}");
    assert!(r["since_beat_ms"].is_u64(), "距上次心跳必须以数回显: {v}");
    assert_eq!(
        r["beat_peak_ms"], 0,
        "未发过续命拍时峰值必须为 0（首拍不计，不造假值）: {v}"
    );
    assert_eq!(r["crash_streak"], 0, "未崩溃时连击必须为 0: {v}");
    assert_eq!(r["restart_exhausted"], false, "未达上限: {v}");
    assert!(
        v["result"]["restart_exhausted"]
            .as_array()
            .expect("health 应带 result.restart_exhausted")
            .is_empty(),
        "无模块达上限时终态列表必须为空: {v}"
    );
    assert_eq!(
        v["result"]["watchdog_overruns"], 0,
        "新起守护的拍超时累计必须为 0: {v}"
    );
    assert_eq!(r["phase"], "registered");
    rr3_assert_ctl_closed(&mut ctl);
}

#[test]
fn ctl_logs_returns_ring_tail_containing_banner() {
    let t = RrdTest::rr3_start_no_overlay("ctl-logs");
    let mut ctl = t.rr3_connect();
    let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.logs", 1);
    let lines = v["result"]["lines"]
        .as_array()
        .expect("logs 应带 result.lines");
    let joined: Vec<&str> = lines.iter().filter_map(|l| l.as_str()).collect();
    assert!(
        joined.iter().any(|l| l.contains("监听")),
        "启动横幅必须已在日志环里（rr3_log 单点落环）: {joined:?}"
    );
    rr3_assert_ctl_closed(&mut ctl);
}

#[test]
fn ctl_ota_is_always_refused_with_permission() {
    let t = RrdTest::rr3_start_no_overlay("ctl-ota");
    let mut ctl = t.rr3_connect();
    let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.ota", 7);
    let err = &v["error"];
    assert!(
        err.is_object(),
        "ota 必须以错误信封应答（协议面定型、语义随 M6）: {v}"
    );
    // AIRY_EPERM = 3（契约冻结集；不新增 NotSupported 码）
    assert_eq!(err["code"], Rr3StatusCode::Permission as i64, "{v}");
    assert!(
        err["message"].as_str().unwrap_or_default().contains("M6"),
        "恒拒文案必须点 M6: {v}"
    );
    rr3_assert_ctl_closed(&mut ctl);
}

/// 系统重启（0.0.4 批C，Q3 裁定）：方法面在白名单（协议面定型），执行面
/// 双重门（cfg(linux) + RR3_REBOOT_OK=1）缺省关闭——不注入 env 的 rrd 恒拒，
/// 与 ota 的"方法面定型、执行面恒拒"同构。解锁路径不测（真重启），其存在性
/// 由 conn.rs 的 `rr3_reboot_unlocked` 单点承载。
#[test]
fn ctl_reboot_is_refused_by_default_double_gate() {
    let t = RrdTest::rr3_start_no_overlay("ctl-reboot");
    let mut ctl = t.rr3_connect();
    let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.reboot", 7);
    let err = &v["error"];
    assert!(err.is_object(), "缺省必须以错误信封拒: {v}");
    assert_eq!(err["code"], Rr3StatusCode::Permission as i64, "{v}");
    let msg = err["message"].as_str().unwrap_or_default();
    assert!(
        msg.contains("RR3_REBOOT_OK"),
        "拒因必须点名解锁旋钮，运维一眼可辨: {v}"
    );
    rr3_assert_ctl_closed(&mut ctl);
}

/// 装配快照读口（批N4 件九笔4）：--manifest 调试路径无装配快照 → 如实回
/// `available:false` + 点名解锁路径（不造假数据）。快照在册面（--profile 启动）
/// 的渲染完整性由 assembly.rs 单测用门 d 同源词表逐键钉死；两端到端形状在此合拢。
#[test]
fn ctl_wiring_reports_unavailable_without_profile_boot() {
    // 基-23：本用例要钉的是"--manifest 调试后门路径无装配快照"，起法显式走后门
    // （rr3_start_manifest_stub）——监听面主口径（rr3_start 一族）已改走缺格跳过
    // 装配单正门，那里的 rr3.ctl.wiring 是 available:true，不再是这里的被测形状。
    let t = RrdTest::rr3_start_manifest_stub("ctl-wiring");
    let mut ctl = t.rr3_connect();
    let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.wiring", 1);
    let result = &v["result"];
    assert!(
        result.is_object(),
        "wiring 必须以成功信封应答（不可得不是协议错误）: {v}"
    );
    assert_eq!(result["available"], false, "{v}");
    let msg = result["reason"].as_str().unwrap_or_default();
    assert!(
        msg.contains("--profile"),
        "不可得理由必须点名 --profile 解锁路径: {v}"
    );
    rr3_assert_ctl_closed(&mut ctl);
}

#[test]
fn registered_module_uplink_ctl_is_refused_but_connection_survives() {
    let t = RrdTest::rr3_start_no_overlay("ctl-uplink-deny");
    let (mut a, task) = register_sima_task(&t);

    // 注册后上行帧的 src_task 必须等于分配值（AOS-STD-IPC-024），ctl 上行也不例外
    rr3_send(
        &mut a,
        &rr3_rpc_frame("rr3.ctl.status", 9, serde_json::json!({}), task),
    );
    let f = rr3_read_where(&mut a, "ctl 上行的拒绝应答", |f| {
        rr3_body_split(&f.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    let v = rr3_json_of(&f);
    assert_eq!(v["error"]["code"], Rr3StatusCode::Permission as i64, "{v}");

    // 同一连接仍活着：喂一次心跳，应答照常（一次笔误不该让整机掉线）
    rr3_send(
        &mut a,
        &rr3_rpc_frame("rr3.heartbeat", 10, serde_json::json!({}), task),
    );
    let f = rr3_read_where(&mut a, "拒绝后的心跳应答", |f| {
        rr3_body_split(&f.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    let v = rr3_json_of(&f);
    assert!(v["result"].is_object(), "心跳应答必须是成功信封: {v}");
}
