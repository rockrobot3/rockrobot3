#![cfg(unix)]
//! rr3-connect 真模块的装配单正门端到端（0.0.2 T4c；ADR-0007/0012）。
//!
//! 与 e2e_act / e2e_power 同一装机布局：`--profile` 装配单正门起**真
//! rr3-connect**，钉 D7 验收面（方案 §3）：net/link 状态机周期流上总线 +
//! cloud/session 应答回路。两场景对偶：
//! - **在线**：sim 后端缺省 up → net/link 报 `up=true`（ssid/rssi 原样透传，
//!   seq 严格递增的周期流）+ `cloud/session` open 回 mock 会话应答；
//! - **断链**：env `RR3_HAL_SIM_LINK_UP=0` → net/link 报 `up=false` 且
//!   ssid/rssi 为 null（设备面抹掉的语义，rr-hal sim.rs 单测钉过）+
//!   session open 折 result.error 点名"链路"（业务性失败不断链）。
//!
//! sim 后端状态 env 定死、运行期不可翻转（跨进程），翻转分支只有 wifi.rs 的
//! 纯函数单测能钉——e2e 见首拍基线与周期流，与 e2e_power 的时序前提同款分工。

mod support;

use std::path::PathBuf;

use rr_ipc::Rr3PayloadType;
use serde_json::{json, Value};

use support::*;

/// 装配单：connect 两格位各绑 rr3-connect、faces 与 bindings 互认领。
/// 格位名取自真 `contracts/slots.toml`（net.online 是基础层 base 格，绑它致命，
/// e2e_profile 钉过——这里只绑身体层两格）。
const PROFILE: &str = r#"
schema_version = 1

[os]
route = "distro"

[bindings]
"net.wifi" = "rr3-connect"
"net.bt" = "rr3-connect"

[faces]
rr3-connect = ["net.wifi", "net.bt"]
"#;

/// config 夹具：与仓内 `modules/rr3-connect/config.toml` 同形状（main.rs 的
/// `config_parse_covers_repo_shape` 钉过同形状解析，漂移当场红）。周期压短让
/// 观察者在用例寿命内拿到多拍周期流（seq 严格递增的判据靠它）。
const CONFIG: &str = r#"
[rr3-connect]
log_level = "info"

[rr3-connect.wifi]
link_report_period_ms = 200
roam_probe_enabled = false

[rr3-connect.session]
tls_min_version = "1.3"
timeout_ms = 20000
"#;

/// 装机布局夹具（e2e_act/e2e_power 同款）：Drop 清整树——断言 panic 后 /tmp
/// 不许留残迹。tag 区分两场景：同一测试二进制并行跑，目录不得相撞。
struct Root {
    dir: PathBuf,
}

impl Root {
    fn new(tag: &str) -> Root {
        let dir =
            std::env::temp_dir().join(format!("rr3-connect-e2e-{}-{tag}", std::process::id()));
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

/// 帧的 (载荷类型, topic) 摘要：谓词共用的纯函数底座。非 JSON / 坏前缀 /
/// 非 object 体一律 None——谓词里不许 panic（read_where 预算的硬前提）。
fn frame_topic(f: &rr_ipc::Rr3Frame) -> Option<(Rr3PayloadType, String)> {
    let (ty, body) = rr_ipc::rr3_body_split(&f.payload).ok()?;
    let v: Value = serde_json::from_slice(&body).ok()?;
    Some((ty, v.get("topic")?.as_str()?.to_string()))
}

/// 场景共用骨架：夹具 → 装配单正门 boot → 旁观连接。返回 (rrd, 旁观者, task, Root)。
/// Root 随返回值活在调用方测试函数尾：drop 清树的时点必须在 rrd 死后（Drop 顺序
/// 按元组序，rrd 在前）——e2e_act/power 的同一纪律，两场景目录靠 tag 区分。
#[allow(clippy::type_complexity)]
fn boot_connect(tag: &str, env: &[(&str, &str)]) -> (RrdTest, RrdClient, u64, Root) {
    let bin = rr3_build_module_bin("rr3-connect");
    let root = Root::new(tag);
    let mod_dir = root.dir.join("modules").join("rr3-connect");
    std::fs::create_dir_all(&mod_dir).expect("建模块目录");
    std::fs::write(
        mod_dir.join("module.toml"),
        rr3_module_manifest_text("rr3-connect", &bin),
    )
    .expect("写模块 manifest");
    std::fs::write(mod_dir.join("config.toml"), CONFIG).expect("写模块 config");
    let profile = root.write_profile(PROFILE);

    // tick 必传小值（E2E_TICK_MS）：supervisor 扫尸节拍复用同一旋钮（e2e_act 同条纪律）
    let mut rrd = RrdTest::rr3_start_profile_env(tag, &profile, E2E_TICK_MS, env);
    rrd.rr3_wait_log("rrd: 装配单已加载: ");
    rrd.rr3_wait_log("spawn rr3-connect pid=");
    rrd.rr3_wait_log("rr3-connect → running");
    let (obs, obs_task) = rr3_connect_observer(&mut rrd);
    (rrd, obs, obs_task, root)
}

#[test]
fn connect_module_link_up_and_session_open_end_to_end() {
    // ---------- 夹具 + boot：sim 缺省在线（不注入 UP 键，钉缺省语义） ----------
    let (mut rrd, mut obs, obs_task, _root) =
        boot_connect("conn-up", &[("RR3_HAL_BACKEND", "sim")]);
    let reg = rrd.rr3_wait_log("注册 rr3-connect task=");
    let conn_task = rr3_field_from_log(&reg, "task=");

    // ---------- ① net/link 周期流：首拍基线 + seq 严格递增 ----------
    let l1 = rr3_read_where(&mut obs, " net/link 首拍", |f| {
        matches!(
            frame_topic(f),
            Some((Rr3PayloadType::Event, ref t)) if t == "rr3-connect/net/link"
        )
    });
    assert_eq!(l1.header.src_task, conn_task, "link 帧 src 必须是分配值");
    let v1 = rr3_json_of(&l1);
    assert_eq!(v1["up"], true, "sim 缺省在线（backend.rs 缺省语义）: {v1}");
    assert_eq!(v1["ssid"], "rock-sim", "SSID 缺省值透传: {v1}");
    assert_eq!(v1["rssi_dbm"], -50, "RSSI 缺省值透传: {v1}");
    let seq1 = v1["seq"]
        .as_u64()
        .unwrap_or_else(|| panic!("link 带 seq: {v1}"));
    let l2 = rr3_read_where(&mut obs, " net/link 序列推进", |f| {
        matches!(
            frame_topic(f),
            Some((Rr3PayloadType::Event, ref t)) if t == "rr3-connect/net/link"
        ) && rr3_json_of(f)["seq"].as_u64().map_or(false, |s| s > seq1)
    });
    assert!(
        rr3_json_of(&l2)["seq"].as_u64().unwrap() > seq1,
        "周期流必须推进（seq 严格递增）"
    );

    // ---------- ② cloud/session open：mock 会话应答 ----------
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            201,
            json!({"topic": "rr3-connect/cloud/session", "args": {"action": "open"}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(&mut obs, 201);
    assert!(v.get("error").is_none(), "链路在线时 open 不该被拒: {v}");
    assert_eq!(v["result"]["session"], "opened", "{v}");
    assert_eq!(
        v["result"]["endpoint"], "mock",
        "云侧联调范围外，sim 只回 mock: {v}"
    );
    assert_eq!(
        v["result"]["ssid"], "rock-sim",
        "会话应答回显当前链路 ssid: {v}"
    );

    // ---------- ③ 业务性失败折 result.error 不断链 ----------
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            202,
            json!({"topic": "rr3-connect/cloud/session", "args": {"action": "close"}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(&mut obs, 202);
    let err = v["result"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("未知 action 必须回 result.error 字符串: {v}"));
    assert!(err.contains("close"), "未知值要点名: {err}");
}

#[test]
fn connect_module_link_down_session_refused_end_to_end() {
    // ---------- 夹具 + boot：env 注入断链场景 ----------
    let (mut rrd, mut obs, obs_task, _root) = boot_connect(
        "conn-down",
        &[("RR3_HAL_BACKEND", "sim"), ("RR3_HAL_SIM_LINK_UP", "0")],
    );
    let reg = rrd.rr3_wait_log("注册 rr3-connect task=");
    let conn_task = rr3_field_from_log(&reg, "task=");

    // ---------- ① net/link 报断链基线：up=false，ssid/rssi 必须是 null ----------
    let l1 = rr3_read_where(&mut obs, " net/link 断链基线", |f| {
        matches!(
            frame_topic(f),
            Some((Rr3PayloadType::Event, ref t)) if t == "rr3-connect/net/link"
        )
    });
    assert_eq!(l1.header.src_task, conn_task);
    let v1 = rr3_json_of(&l1);
    assert_eq!(v1["up"], false, "env 注入的断链必须原样上总线: {v1}");
    assert!(
        v1["ssid"].is_null() && v1["rssi_dbm"].is_null(),
        "断链时设备面抹掉 ssid/rssi，模块侧如实透传 null: {v1}"
    );

    // ---------- ② session open 折 result.error 点名"链路" ----------
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            211,
            json!({"topic": "rr3-connect/cloud/session", "args": {"action": "open"}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(&mut obs, 211);
    let err = v["result"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("断链时 open 必须折 result.error 字符串: {v}"));
    assert!(err.contains("链路"), "断链要点名判据词: {err}");
}
