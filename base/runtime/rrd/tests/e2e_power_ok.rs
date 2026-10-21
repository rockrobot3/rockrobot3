#![cfg(unix)]
//! 低电兜底**解除路径**的端到端（批N5 件二笔3，全案 §17 运维留痕 F-30 第三项）。
//!
//! 兄弟文件 `e2e_power.rs` 走真模块钉"置旗"半边（sim 电量 env 启动定格，恢复沿出不来）；
//! 本文件用**测试连接注册 rr3-power**伪造沿事件钉"清旗"半边：归属三层检查对注册名
//! rr3-power 放行 `rr3-power/battery/low|ok`（前缀=自己、主题在册、owner 匹配——
//! 三方一致即发布合法），conn 臂的四个可观察面各钉一条：
//! 置旗日志 → system/power(reason=battery-low) 帧 → 解除日志 →
//! system/power(reason=battery-ok) 帧 → 重复 ok 不重复广播（旗未置位无从解除）。
//!
//! 与 e2e_power.rs 的分工：那边钉"真模块发的 low"，这边钉"rrd 对 low/ok 的**反应**"——
//! 发布者是谁不重要（生产里就是 rr3-power 本尊），重要的是 rrd 收到沿事件后的记账与点名。

mod support;

use rr_bus::{RR3_BROADCAST_DST_TASK, RR3_TASK_RRD};
use rr_ipc::{rr3_body_split, Rr3PayloadType};
use serde_json::{json, Value};
use std::time::Duration;

use support::*;

/// rr3-power 化身连接的 manifest：publishes 申报 low|ok 两主题（归属检查不看申报面，
/// 申报是登记纪律——与真 module.toml 同源的字段面）
fn power_manifest() -> String {
    rr3_manifest("rr3-power", 3_600_000, 3_600_001, "never").replace(
        "publishes = []",
        "publishes = [\"rr3-power/battery/low\", \"rr3-power/battery/ok\"]",
    )
}

/// 按 (reason, percent, voltage_mv) 挑 system/power 点名帧：谓词纯函数（字段比对，
/// 无内部状态——SCAN_BUDGET 的硬前提）
fn is_power_frame(f: &rr_ipc::Rr3Frame, reason: &str) -> bool {
    matches!(
        rr3_body_split(&f.payload),
        Ok((Rr3PayloadType::Notification, _))
    ) && rr3_json_of(f)["topic"] == "rr3-runtime/system/power"
        && rr3_json_of(f)["reason"] == reason
}

#[test]
fn battery_ok_clears_low_power_flag_and_names_the_recovery_once() {
    let mut rrd = RrdTest::rr3_start("power-ok");

    // ---------- 化身连接 + 旁观者 ----------
    let mut obs = rrd.rr3_connect();
    rr3_send(
        &mut obs,
        &rr3_register_frame(
            &rr3_manifest("rr3-obs", 3_600_000, 3_600_001, "never"),
            "rr3-obs",
        ),
    );
    let obs_task = rr3_read_rpc(&mut obs)["result"]["src_task"]
        .as_u64()
        .expect("obs 注册应答");

    let mut p = rrd.rr3_connect();
    rr3_send(&mut p, &rr3_register_frame(&power_manifest(), "rr3-power"));
    let power_task = rr3_read_rpc(&mut p)["result"]["src_task"]
        .as_u64()
        .expect("rr3-power 化身注册应答");
    assert_ne!(power_task, obs_task);

    // ---------- ① low：置旗 + reason=battery-low 点名 ----------
    rr3_send(
        &mut p,
        &rr3_json_frame(
            Rr3PayloadType::Event,
            &json!({"topic": "rr3-power/battery/low", "seq": 1, "percent": 5, "voltage_mv": 3250}),
            power_task,
            RR3_TASK_RRD,
        ),
    );
    let trigger = rrd.rr3_wait_log("低电兜底触发：battery/low percent=5 voltage_mv=3250");
    assert!(
        trigger.contains("置全局低电旗"),
        "触发行必须点名置旗动作: {trigger}"
    );
    let low_frame = rr3_read_where(&mut obs, " system/power 置旗点名", |f| {
        is_power_frame(f, "battery-low")
    });
    assert_power_frame_shape(&low_frame, "battery-low", 5, 3250);

    // ---------- ② ok：清旗 + reason=battery-ok 解除点名 ----------
    rr3_send(
        &mut p,
        &rr3_json_frame(
            Rr3PayloadType::Event,
            &json!({"topic": "rr3-power/battery/ok", "seq": 1, "percent": 80, "voltage_mv": 3900}),
            power_task,
            RR3_TASK_RRD,
        ),
    );
    let clear = rrd.rr3_wait_log("低电兜底解除：battery/ok percent=80 voltage_mv=3900");
    assert!(
        clear.contains("清全局低电旗") && clear.contains("恢复新 spawn"),
        "解除行必须点名清旗与放行两个动作: {clear}"
    );
    let ok_frame = rr3_read_where(&mut obs, " system/power 解除点名", |f| {
        is_power_frame(f, "battery-ok")
    });
    assert_power_frame_shape(&ok_frame, "battery-ok", 80, 3900);

    // ---------- ③ 重复 ok：旗未置位，不重复广播 ----------
    rr3_send(
        &mut p,
        &rr3_json_frame(
            Rr3PayloadType::Event,
            &json!({"topic": "rr3-power/battery/ok", "seq": 2, "percent": 81, "voltage_mv": 3901}),
            power_task,
            RR3_TASK_RRD,
        ),
    );
    rrd.rr3_wait_log("低电兜底本未触发：重复 battery/ok 不重复广播");
    // 转发面照常（battery/ok seq=2 会到旁观者），点名面必须安静——collect 窗口内
    // 不得再出现任何 system/power 帧（前后两条已各被 ①② 读走，队列里不会攒着它）
    let tail = rr3_collect_within(&mut obs, Duration::from_millis(400));
    let powers: Vec<Value> = tail
        .iter()
        .filter(|f| rr3_json_of(f)["topic"] == "rr3-runtime/system/power")
        .map(|f| rr3_json_of(f))
        .collect();
    assert!(
        powers.is_empty(),
        "重复 ok 不得再次点名广播（下降沿恰一次）: {powers:?}"
    );
}

/// 点名帧的头与体形状（daemon.rs 单测钉帧级形状，这里钉线上的同一帧）
fn assert_power_frame_shape(f: &rr_ipc::Rr3Frame, reason: &str, percent: u64, voltage_mv: u64) {
    assert_eq!(f.header.src_task, RR3_TASK_RRD, "点名由 rrd 署名");
    assert_eq!(f.header.dst_task, RR3_BROADCAST_DST_TASK, "点名是广播语义");
    let v: Value = rr3_json_of(f);
    assert_eq!(v["topic"], "rr3-runtime/system/power");
    assert_eq!(v["reason"], reason);
    assert_eq!(v["percent"], percent, "透传触发事件的 percent: {v}");
    assert_eq!(
        v["voltage_mv"], voltage_mv,
        "透传触发事件的 voltage_mv: {v}"
    );
    assert_eq!(v.as_object().map(|o| o.len()), Some(4), "体恰四字段: {v}");
}
