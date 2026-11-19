#![cfg(unix)]
//! 运维计数闭环（0.0.4 批B Q1/Q2 裁定、0.0.5 增补轮四 批W3）：心跳 `counters`
//! 字段 → rrd 按 `模块名/键` 归并 → `rr3.ctl.counters` 读回；`RR3_DATA_DIR` 注入
//! 后的落盘、重启加载与关停冲刷。
//!
//! 判据形状：
//! - 聚合：同键跨拍累加、异键并存；不带 counters 的旧形状心跳照常 accepted；
//!   rrd **先归并后应答**——应答收到时本拍计数必已进表（顺序契约）；
//! - 持久化：boots 自增写盘 → 心跳计数落盘 → 杀进程 → 重启加载 + boots 再自增；
//! - 关停：窗口节流不提前落盘，SIGTERM 后强制 flush 与内存一致且退 0。
//!
//! ctl 连接不注册（与 `e2e_ctl.rs` 同口径），一问一答助手在 `support/ctl.rs`。

mod support;

use support::{
    register_sima_task, rr3_ask_ctl, rr3_assert_ctl_closed, rr3_manifest, rr3_read_rpc,
    rr3_register_frame, rr3_rpc_frame, rr3_send, RrdTest,
};

use serde_json::Value;

/// 运维计数聚合（0.0.4 批B，Q1 裁定）：心跳可选 `counters` 字段 → rrd 按
/// `模块名/键` 归并 → `rr3.ctl.counters` 读回。兼容面同批钉住：不带 counters
/// 的心跳照常 accepted（旧模块形状 `{}`）。
/// 顺序契约（2026-10-07 倒置修复）：rrd **先归并后应答**——心跳应答返回时，
/// 本拍 counters 必已进聚合表；本用例"应答一收到就读 ctl"正是该契约的观测面
/// （旧序在机器繁忙时会读到上一拍的账，曾以"期望 5 实得 2"实证）。
#[test]
fn ctl_counters_aggregates_heartbeat_counters() {
    let t = RrdTest::rr3_start_no_overlay("ctl-counters");
    let mut a = t.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    let task = rr3_read_rpc(&mut a)["result"]["src_task"]
        .as_u64()
        .expect("注册应答带 src_task");

    // 不带 counters 的心跳：accepted（可选字段缺省 = 旧线上形状）
    rr3_send(
        &mut a,
        &rr3_rpc_frame("rr3.heartbeat", 2, serde_json::json!({}), task),
    );
    let v = rr3_read_rpc(&mut a);
    assert!(
        v["result"].is_object(),
        "无 counters 的心跳照常 accepted: {v}"
    );

    // 带 counters 的两拍心跳：同键累加、异键并存
    for (id, counters) in [
        (3, serde_json::json!({"asr_ok": 2, "relay_on": 1})),
        (4, serde_json::json!({"asr_ok": 3})),
    ] {
        rr3_send(
            &mut a,
            &rr3_rpc_frame(
                "rr3.heartbeat",
                id,
                serde_json::json!({"counters": counters}),
                task,
            ),
        );
        let v = rr3_read_rpc(&mut a);
        assert!(v["result"].is_object(), "带 counters 的心跳 accepted: {v}");
    }

    let mut ctl = t.rr3_connect();
    let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.counters", 1);
    let counters = v["result"]["counters"]
        .as_object()
        .expect("counters 应带 result.counters 对象");
    assert_eq!(
        counters.get("rr3-sima/asr_ok").and_then(Value::as_u64),
        Some(5),
        "同键跨拍累加（模块前缀归并）: {counters:?}"
    );
    assert_eq!(
        counters.get("rr3-sima/relay_on").and_then(Value::as_u64),
        Some(1)
    );
    rr3_assert_ctl_closed(&mut ctl);
}

/// 计数持久化（0.0.4 批B，Q2 裁定）：RR3_DATA_DIR 注入 → boots 自增写盘 →
/// 心跳计数落盘 → 断电（杀进程）→ 重启加载 + boots 再自增。原子写序的
/// tmp 残留断言在 counters.rs 单测里（load/store 的单元面），这里钉线上闭环。
#[test]
fn counters_persist_across_restart_and_boots_accumulate() {
    let dir = std::env::temp_dir().join(format!(
        "rrd-counters-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("counters.json");
    let envs = [("RR3_DATA_DIR", dir.to_str().unwrap())];

    // 第一世：启动自增 boots（=1）并回写；模块一拍带 counters 的心跳落盘
    {
        let mut t = RrdTest::rr3_start_env("counters-gen1", &envs);
        // 启用与回写在同一行日志里（daemon 单点落环），一次等掉全语义
        let line = t.rr3_wait_log("counters 持久化已启用");
        assert!(line.contains("boots 自增已回写"), "{line}");
        let mut a = t.rr3_connect();
        rr3_send(
            &mut a,
            &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
        );
        let task = rr3_read_rpc(&mut a)["result"]["src_task"]
            .as_u64()
            .expect("注册应答带 src_task");
        rr3_send(
            &mut a,
            &rr3_rpc_frame(
                "rr3.heartbeat",
                2,
                serde_json::json!({"counters": {"chat_rounds": 4}}),
                task,
            ),
        );
        rr3_read_rpc(&mut a);
        // 首拍即刷（R26 闸门 last=None）：读到盘上出现模块键为止，不猜时刻
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains("rr3-sima/chat_rounds") {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "心跳计数应 write-through 落盘，盘上始终没有: {path:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
                .expect("counters.json 必须是合法 JSON");
        assert_eq!(stored["boots"], 1, "第一世 boots 自增到 1: {stored}");
        assert_eq!(stored["rr3-sima/chat_rounds"], 4, "{stored}");
        assert!(
            !dir.join("counters.json.tmp").exists(),
            "原子写序走完不得留 tmp 残留"
        );
        // 显式退场不动计数账；进程随 drop 收尾（counters.json 留在盘上 = "断电"）
    }

    // 第二世：重启加载第一世的账 → boots 自增到 2 → 模块计数从 4 起继续累加
    {
        let mut t = RrdTest::rr3_start_env("counters-gen2", &envs);
        let line = t.rr3_wait_log("counters 持久化已启用");
        assert!(line.contains("boots 自增已回写"), "{line}");
        let (mut a, task) = register_sima_task(&t);
        rr3_send(
            &mut a,
            &rr3_rpc_frame(
                "rr3.heartbeat",
                2,
                serde_json::json!({"counters": {"chat_rounds": 2}}),
                task,
            ),
        );
        rr3_read_rpc(&mut a);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let ok = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .map(|v| v["rr3-sima/chat_rounds"] == 6 && v["boots"] == 2)
                .unwrap_or(false);
            if ok {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "重启加载 + boots=2 + 4+2=6 的闭环没走通"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// R26 心跳合并写 + 关停强制 flush（0.0.5 增补轮四 批W3）。两个判据一条用例钉死：
/// ①**节流本体**——首拍即刷（闸门 last=None）之后，窗口 M 内的后续变化**不得**
/// 提前落盘（M 拉到 10 分钟，测试时长内窗口判据不可能成立，盘上若出现新值即节流失效）；
/// ②**关停完整性**——SIGTERM 后重读计数与内存一致（窗口内的尾部变化由关停路径
/// 一次性写全），且进程以 0 退出（处理器已装、flush 完自己退，信号死=没装上）。
#[test]
fn sigterm_forces_flush_of_pending_counters() {
    let dir = std::env::temp_dir().join(format!(
        "rrd-counters-term-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("counters.json");
    let envs = [
        ("RR3_DATA_DIR", dir.to_str().unwrap()),
        ("RR3_COUNTERS_FLUSH_MS", "600000"),
    ];

    let mut t = RrdTest::rr3_start_env("counters-term", &envs);
    let line = t.rr3_wait_log("counters 持久化已启用");
    assert!(line.contains("boots 自增已回写"), "{line}");

    let mut a = t.rr3_connect();
    rr3_send(
        &mut a,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    let task = rr3_read_rpc(&mut a)["result"]["src_task"]
        .as_u64()
        .expect("注册应答带 src_task");

    // 第一拍（首拍即刷）：盘上出现 term_flush=9
    rr3_send(
        &mut a,
        &rr3_rpc_frame(
            "rr3.heartbeat",
            2,
            serde_json::json!({"counters": {"term_flush": 9}}),
            task,
        ),
    );
    rr3_read_rpc(&mut a);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let ok = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .map(|v| v["rr3-sima/term_flush"] == 9)
            .unwrap_or(false);
        if ok {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "首拍即刷（last=None）落盘没发生: {path:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    // 第二拍（窗口内）：应答回来 = 账已进内存（顺序契约），但盘必须仍是 9
    rr3_send(
        &mut a,
        &rr3_rpc_frame(
            "rr3.heartbeat",
            3,
            serde_json::json!({"counters": {"term_flush": 1}}),
            task,
        ),
    );
    rr3_read_rpc(&mut a);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let stored: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
        .expect("counters.json 必须仍是合法 JSON");
    assert_eq!(
        stored["rr3-sima/term_flush"], 9,
        "窗口 M=600s 内第二拍变化不得提前落盘（节流失效）: {stored}"
    );

    // SIGTERM：关停强制 flush 补上尾部 → 盘 == 内存（10），进程退 0
    let status = t.rr3_sigterm_and_wait();
    assert!(
        status.success(),
        "SIGTERM 后必须以 0 退出（flush 完自己退）；信号死 = 处理器没装上: {status}"
    );
    let stored: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
        .expect("关停 flush 后 counters.json 必须可解析");
    assert_eq!(stored["boots"], 1, "{stored}");
    assert_eq!(
        stored["rr3-sima/term_flush"], 10,
        "关停 flush 必须把窗口内尾部变化（9+1）写全: {stored}"
    );
    assert!(
        !dir.join("counters.json.tmp").exists(),
        "原子写序走完不得留 tmp 残留"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
