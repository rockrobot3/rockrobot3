//! daemon 面单测：并发槽、低电点名帧、socket 放行、启动摘要与降级原因。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rr_bus::{RR3_BROADCAST_DST_TASK, RR3_TASK_RRD};
use rr_ipc::{rr3_body_split, Rr3PayloadType, Rr3UdsListener};
use serde_json::json;

use super::boot::{rr3_backend_summary, rr3_boot_summary, rr3_degrade_json};
use super::ctl::rr3_socket_acl_grant;
use super::task::rr3_conn_acquire;
use super::tick::rr3_power_frame;

/// 并发连接槽（本轮审计）：到上限拒新、释放后可再申请、Drop 归还计数。
#[test]
fn conn_slots_reject_over_limit_and_recover_on_drop() {
    let live = Arc::new(AtomicUsize::new(0));
    let a = rr3_conn_acquire(&live, 2).expect("第 1 条");
    let b = rr3_conn_acquire(&live, 2).expect("第 2 条");
    assert!(rr3_conn_acquire(&live, 2).is_none(), "第 3 条必须拒");
    assert_eq!(live.load(Ordering::Relaxed), 2, "拒绝不得漏计数");
    drop(a);
    assert_eq!(live.load(Ordering::Relaxed), 1);
    let c = rr3_conn_acquire(&live, 2).expect("释放后可再申请");
    drop((b, c));
    assert_eq!(live.load(Ordering::Relaxed), 0);
}

/// 低电兜底点名帧的形状钉死（0.0.2 Q4）：Notification、src=rrd、dst=广播、
/// 体恰四字段（topic/reason/percent/voltage_mv），值从触发事件透传。e2e 的旁观者
/// 接入时首拍已过（兜底在模块注册后一拍内触发，e2e_power.rs 文件头的时序前提），
/// 帧级形状只有这里能钉——e2e 钉日志侧的触发与拦截，两半拼成完整证据链。
/// reason 两值各钉（批N5 件二笔3）：置旗 battery-low / 解除 battery-ok。
#[test]
fn system_power_frame_shape_is_pinned() {
    let f = rr3_power_frame("battery-low", 5, 3250);
    assert_eq!(f.header.src_task, RR3_TASK_RRD, "点名是 rrd 自己发的");
    assert_eq!(f.header.dst_task, RR3_BROADCAST_DST_TASK, "全量扇出");
    let (ty, body) = rr3_body_split(&f.payload).expect("前缀合法");
    assert_eq!(ty, Rr3PayloadType::Notification, "点名走通知不是应答");
    let v: serde_json::Value = serde_json::from_slice(&body).expect("体是 JSON");
    assert_eq!(v["topic"], "rr3-runtime/system/power");
    assert_eq!(v["reason"], "battery-low");
    assert_eq!(v["percent"], 5, "透传触发事件的 percent");
    assert_eq!(v["voltage_mv"], 3250, "透传触发事件的 voltage_mv");
    assert_eq!(
        v.as_object().map(|o| o.len()),
        Some(4),
        "体恰四字段，不得私加: {v}"
    );
    // 解除点名同形（批N5 件二笔3）：reason 换词、其余字段同账
    let f = rr3_power_frame("battery-ok", 80, 3900);
    let (_, body) = rr3_body_split(&f.payload).expect("前缀合法");
    let v: serde_json::Value = serde_json::from_slice(&body).expect("体是 JSON");
    assert_eq!(v["reason"], "battery-ok", "解除点名 reason=battery-ok");
    assert_eq!(v["percent"], 80);
    assert_eq!(v["voltage_mv"], 3900);
    assert_eq!(
        v.as_object().map(|o| o.len()),
        Some(4),
        "解除点名同恰四字段"
    );
}

/// socket 放行落位（批F5b R12）：temp 树真 bind 一个 uds → grant → 权限位
/// 0o660 断言。chgrp 面：开发机/CI 无 rockrobot3 组 → 走"组不在位留痕"路径
/// （属主不动），chmod 位仍成立——两环境测试都绿；板上组在位时 chown 生效，
/// 属主组断言归板上 cargo test 面，此处不断言 gid 以免开发机红。
#[test]
fn socket_acl_grant_sets_group_rw_mode() {
    let dir = std::env::temp_dir().join(format!("rr3-rrd-acl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("acl.sock");
    let _ = std::fs::remove_file(&sock);
    // 独占 bind：与生产同一条入口（B-9），测 acl 的同时顺带复测探测-清-bind 语义
    let listener = Rr3UdsListener::rr3_bind_exclusive(&sock).unwrap();
    rr3_socket_acl_grant(&sock);
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&sock).unwrap().permissions().mode();
    assert_eq!(
        mode & 0o777,
        0o660,
        "chmod 0o660 落位（组在位与否不影响 mode 位）"
    );
    drop(listener);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 启动摘要：后门路径空档/芯片必须显式 `-`（不造假值），规模数字来自装配面。
#[test]
fn boot_summary_marks_backdoor_and_counts() {
    let b = rr3_backend_summary("", None, None, None);
    let s = rr3_boot_summary("", "", "--manifest", 3, 1, 2, &b);
    assert!(s.contains("tier=-") && s.contains("chip=-"), "{s}");
    assert!(s.contains("modules=3") && s.contains("预降级=1"), "{s}");
    assert!(s.contains("缺格=2"), "缺格数必须回显: {s}");
    assert!(s.contains("hal=-"), "后门无后端须显式 -: {s}");
    let b = rr3_backend_summary("linux", Some("esp32"), Some("esp32"), Some("esp32"));
    let s = rr3_boot_summary(
        "rtos",
        "esp32",
        "/usr/lib/rockrobot3/profiles/esp32-mini.toml",
        1,
        0,
        0,
        &b,
    );
    assert!(s.contains("tier=rtos") && s.contains("chip=esp32"), "{s}");
    assert!(
        s.contains("hal=linux audio=esp32 servo=esp32 display=esp32"),
        "后端链必须逐面回显: {s}"
    );
}

/// 降级原因：空表照实 `[]`；两类原因逐条带模块/类别/文案，顺序即入表顺序。
#[test]
fn degrade_notes_carry_kind_and_detail() {
    assert_eq!(rr3_degrade_json(&[]), json!([]));
    let notes = vec![
        (
            "rr3-act".to_string(),
            "missing_slot".to_string(),
            "servo".to_string(),
        ),
        (
            "rr3-sense".to_string(),
            "assemble_broken".to_string(),
            "atoms/i2c 缺 assemble.toml".to_string(),
        ),
    ];
    let v = rr3_degrade_json(&notes);
    assert_eq!(v[0]["module"], "rr3-act", "{v}");
    assert_eq!(v[0]["kind"], "missing_slot", "{v}");
    assert_eq!(v[0]["detail"], "servo", "{v}");
    assert_eq!(v[1]["kind"], "assemble_broken", "{v}");
    assert_eq!(v[1]["detail"], "atoms/i2c 缺 assemble.toml", "{v}");
}
