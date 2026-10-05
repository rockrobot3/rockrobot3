#![cfg(unix)]
//! rr3-power 真模块的低电兜底端到端（0.0.2 T4b；ADR-0007/0012；Q4 裁定）。
//!
//! 走 `--profile` 装配单正门（与 e2e_act 同一装机布局）起**真 rr3-power**，钉 Q4 的
//! 四件兜底动作面：模块发 `rr3-power/battery/low` → rrd 置全局低电旗（上升沿恰一次）
//! → 点名广播 `rr3-runtime/system/power` → supervisor 的 spawn 漏斗拒绝一切新 spawn。
//!
//! **时序前提（读测试前先读这段）**：sim 电量 5%（env 注入）→ 电压 3250mV ≤ 阈值
//! 3300mV → 模块**注册后的第一拍**就发 battery/low。旁观者此刻才在接入路上，注定
//! 错过首拍的 battery/low 与 system/power 帧——所以帧级证据分工：兜底触发与点名广播
//! 靠 rrd 日志（服务侧真相，`低电兜底触发` 行含透传值与广播结果），帧形状靠 daemon.rs
//! 的单测钉死；观察者能拿到的帧证据是**周期流**（battery/state、charger/state）与
//! **段内不再有 low**（模块侧边沿机"同段恰一次"的集成面）。"杀模块 → 重启被拒"
//! 靠日志断言；兜底帧永不重发靠用例末尾的破坏性收尾计数。

mod support;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use rr_ipc::{rr3_body_split, Rr3Frame, Rr3PayloadType};
use serde_json::Value;

use support::*;

/// 装配单：power.charger 绑 rr3-power。power.battery 是基础层恒在位格，绑它致命
/// （profile.rs 的 `base 层 binding 必须致命` 钉过），可绑格只有 charger。
const PROFILE: &str = r#"
schema_version = 1

[os]
route = "distro"

[bindings]
"power.charger" = "rr3-power"

[faces]
rr3-power = ["power.charger"]
"#;

/// config 夹具：字段名与仓内 `modules/rr3-power/config.toml` 同形状（main.rs 的
/// `config_parse_covers_repo_shape` 钉过同形状解析，漂移当场红）。周期压短让观察者
/// 在用例寿命内拿到多拍周期流；阈值 3300mV 与 env 注入的 5% → 3250mV 构成越阈算式。
const CONFIG: &str = r#"
[rr3-power]
log_level = "info"

[rr3-power.battery]
sample_period_ms = 200
low_voltage_mv = 3300
cutoff_voltage_mv = 3150

[rr3-power.charger]
state_period_ms = 300
"#;

/// 装机布局夹具（e2e_act 同款）：Drop 清整树——断言 panic 后 /tmp 不许留残迹。
struct Root {
    dir: PathBuf,
}

impl Root {
    fn new(tag: &str) -> Root {
        let dir = std::env::temp_dir().join(format!("rr3-power-e2e-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir); // pid 复用留下的上次残根不许干扰本次
        std::fs::create_dir_all(dir.join("profiles")).expect("建 profiles 目录");
        Root { dir }
    }

    fn write_profile(&self, text: &str) -> PathBuf {
        let p = self.dir.join("profiles").join("default.toml");
        std::fs::write(&p, text).expect("写装配单");
        p
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 帧的 (载荷类型, topic) 摘要：谓词与计数共用的纯函数底座。非 JSON / 坏前缀 /
/// 非 object 体一律 None——谓词里不许 panic（SCAN_BUDGET 的硬前提），`get` 而非
/// 下标就是为了非 object 体不炸。
fn frame_topic(f: &Rr3Frame) -> Option<(Rr3PayloadType, String)> {
    let (ty, body) = rr3_body_split(&f.payload).ok()?;
    let v: Value = serde_json::from_slice(&body).ok()?;
    Some((ty, v.get("topic")?.as_str()?.to_string()))
}

#[test]
fn power_module_low_battery_fallback_end_to_end() {
    // ---------- 夹具：真二进制 + 真 manifest + config 同树 ----------
    let bin = rr3_build_module_bin("rr3-power");
    let root = Root::new("power");
    let mod_dir = root.dir.join("modules").join("rr3-power");
    std::fs::create_dir_all(&mod_dir).expect("建模块目录");
    std::fs::write(
        mod_dir.join("module.toml"),
        rr3_module_manifest_text("rr3-power", &bin),
    )
    .expect("写模块 manifest");
    std::fs::write(mod_dir.join("config.toml"), CONFIG).expect("写模块 config");
    let profile = root.write_profile(PROFILE);

    // ---------- ① 正门 boot：spawn → 注册 → running → 兜底触发 ----------
    // env 显式钉 sim 后端（不赌平台默认）+ 5% 起表。tick 必传小值（E2E_TICK_MS）：
    // supervisor 扫尸节拍复用同一旋钮，安静 tick 会让 ③ 的拒绝永不发生。
    let mut rrd = RrdTest::rr3_start_profile_env(
        "e2e-power",
        &profile,
        E2E_TICK_MS,
        &[
            ("RR3_HAL_BACKEND", "sim"),
            ("RR3_HAL_SIM_BATTERY_PERCENT", "5"),
        ],
    );
    rrd.rr3_wait_log("rrd: 装配单已加载: ");
    let spawn = rrd.rr3_wait_log("spawn rr3-power pid=");
    assert!(
        spawn.contains(&format!("entry={}", bin.display())),
        "spawn 行必须回显渲染后的绝对 entry（bin 已 canonicalize，两侧同值）: {spawn}"
    );
    let reg = rrd.rr3_wait_log("注册 rr3-power task=");
    let power_task = rr3_field_from_log(&reg, "task=");
    rrd.rr3_wait_log("rr3-power → running");

    // 兜底触发的服务侧真相：透传值钉在日志里（percent=5 voltage_mv=3250），且这一行
    // 在 system/power 扇出**之后**打出（conn.rs 钩子序：先置旗扇出、后 rr3_log），
    // 它出现即"广播已发"的可靠界标——观察者从这一刻起才接入。
    let trigger = rrd.rr3_wait_log("低电兜底触发：battery/low percent=5 voltage_mv=3250");
    assert!(
        trigger.contains("广播 rr3-runtime/system/power"),
        "触发行必须点名广播主题: {trigger}"
    );

    // ---------- ② 旁观者：周期流（首拍 low 错过，正是时序前提的分工） ----------
    let (mut obs, _obs_task) = rr3_connect_observer(&mut rrd);
    let st1 = rr3_read_where(&mut obs, " battery/state 周期流", |f| {
        matches!(
            frame_topic(f),
            Some((Rr3PayloadType::Event, t)) if t == "rr3-power/battery/state"
        )
    });
    assert_eq!(
        st1.header.src_task, power_task,
        "state 帧 src 必须是 rr3-power 的分配值"
    );
    let v = rr3_json_of(&st1);
    assert_eq!(v["percent"], 5, "env 注入的起表电量必须原样上总线: {v}");
    assert_eq!(
        v["voltage_mv"], 3250,
        "电压曲线 3200mV + 10mV/1% 的派生值: {v}"
    );
    let seq1 = v["seq"]
        .as_u64()
        .unwrap_or_else(|| panic!("state 带 seq: {v}"));
    let st2 = rr3_read_where(&mut obs, " battery/state 序列推进", |f| {
        matches!(
            frame_topic(f),
            Some((Rr3PayloadType::Event, t)) if t == "rr3-power/battery/state"
        ) && rr3_json_of(f)["seq"].as_u64().is_some_and(|s| s > seq1)
    });
    assert!(
        rr3_json_of(&st2)["seq"].as_u64().unwrap() > seq1,
        "周期流必须推进（seq 严格递增）"
    );

    let ch = rr3_read_where(&mut obs, " charger/state", |f| {
        matches!(
            frame_topic(f),
            Some((Rr3PayloadType::Event, t)) if t == "rr3-power/charger/state"
        )
    });
    assert_eq!(ch.header.src_task, power_task);
    assert_eq!(
        rr3_json_of(&ch)["charging"],
        false,
        "未注入充电键，默认未充电（backend.rs 的缺省语义）"
    );

    // ---------- ③ 杀模块 → spawn 漏斗必须拒绝重启（Q4 的"停止新 spawn"） ----------
    // pkill 按二进制绝对路径点名（路径无正则元字符）；SIGTERM 后 supervisor 扫尸 →
    // plan 重启 → rr3_boot_spawn 撞上低电旗 → 拒绝日志。
    let status = Command::new("pkill")
        .args(["-f", &bin.display().to_string()])
        .status()
        .expect("起 pkill");
    assert!(
        status.success(),
        "pkill 没匹配到模块进程（{bin:?}）——夹具错"
    );
    rrd.rr3_wait_log("低电兜底：拒绝 spawn rr3-power");
    // 否定断言窗口：不得出现**真的** spawn 行。拒绝行是"拒绝 spawn rr3-power（全局…"，
    // 不含 " pid="，不会误触；真重启了必在这里现形。
    rrd.rr3_assert_no_log("spawn rr3-power pid=", Duration::from_millis(1500));

    // ---------- ④ 末尾破坏性收尾：段内 battery/low 恰一次、system/power 不重发 ----------
    // 模块死后帧流安静，collect 快速终止。收尾窗口不得再有 battery/low（模块侧边沿机
    // 同段恰一次；若真重启了，新实例首拍还会再发——与 ③ 双保险），也不得再有
    // system/power（rrd 侧旗只升不降、上升沿恰一次）。
    let tail = rr3_collect_within(&mut obs, Duration::from_millis(500));
    let lows = tail
        .iter()
        .filter(|f| matches!(frame_topic(f), Some((_, ref t)) if t == "rr3-power/battery/low"))
        .count();
    assert_eq!(
        lows, 0,
        "同一次低电段内 battery/low 不得重发；收尾帧: {tail:?}"
    );
    let powers = tail
        .iter()
        .filter(|f| matches!(frame_topic(f), Some((_, ref t)) if t == "rr3-runtime/system/power"))
        .count();
    assert_eq!(powers, 0, "rrd 侧上升沿恰一次，system/power 不得重复点名");
}
