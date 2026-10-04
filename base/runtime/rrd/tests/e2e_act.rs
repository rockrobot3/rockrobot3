#![cfg(unix)]
//! rr3-act 真模块的装配单正门端到端（0.0.2 T4a；ADR-0007/0012；Q1 裁定）。
//!
//! 与 `e2e_audio_chain.rs` 的分工：那条走 `--manifest` 直驱 boot 钉音频链；这条走
//! `--profile` 装配单正门（装机布局，与 T5 deb 同树）起**真 rr3-act**，钉 T4a 的
//! 验收面（方案 §3）：装配单驱动 e2e 中 rr3-act 在册 running、servo 点名应答——
//! 三事务面（servo/led/relay）的 rr3.call 回路穿过真进程，业务性失败折
//! result.error 不断链，servo/state 限速流上总线被旁观连接真实收到（rr-bus
//! 广播全量扇出、不按订阅过滤，conn.rs 的既证事实）。
//!
//! 关键夹具差异（相对 e2e_profile 的 sim_peer 模板）：module.toml 用**仓内真契约
//! 工件**渲染 entry（`rr3_module_manifest_text`），且必须同树写 config.toml——
//! rr3_config_load 缺省取 manifest 同目录的 config.toml，缺它模块开机即 exit(1)
//! （探活失败不是被测行为，是夹具烂）。

mod support;

use std::path::PathBuf;

use serde_json::json;

use support::*;

/// 满配装配单：act 三格位各绑 rr3-act、faces 与 bindings 互认领。
/// 格位名取自真 `contracts/slots.toml`（Q1 裁定立格 act.relay 后三格齐备）。
const PROFILE: &str = r#"
schema_version = 1

[os]
route = "distro"

[bindings]
"act.servo" = "rr3-act"
"act.led" = "rr3-act"
"act.relay" = "rr3-act"

[faces]
rr3-act = ["act.servo", "act.led", "act.relay"]
"#;

/// config 夹具：与仓内 `modules/rr3-act/config.toml` 同形状（main.rs 的
/// `config_parse_covers_repo_shape` 钉过同形状解析，漂移当场红）。
const CONFIG: &str = r#"
[rr3-act]
log_level = "info"

[rr3-act.servo]
angle_limit_deg = [0, 180]
move_timeout_ms = 800
state_min_interval_ms = 50

[rr3-act.led]
blink_period_ms = 500

[rr3-act.reflex]
loop_hz = 1000
saturate_notice_ms = 500
"#;

/// 装机布局夹具（e2e_profile 同款）：Drop 清整树——断言 panic 后 /tmp 不许留残迹。
struct Root {
    dir: PathBuf,
}

impl Root {
    fn new(tag: &str) -> Root {
        let dir = std::env::temp_dir().join(format!("rr3-act-e2e-{}-{tag}", std::process::id()));
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

#[test]
fn act_module_full_face_end_to_end() {
    // ---------- 夹具：真二进制 + 真 manifest（entry 绝对化）+ config 同树 ----------
    let bin = rr3_build_module_bin("rr3-act");
    let root = Root::new("act");
    let mod_dir = root.dir.join("modules").join("rr3-act");
    std::fs::create_dir_all(&mod_dir).expect("建模块目录");
    std::fs::write(
        mod_dir.join("module.toml"),
        rr3_module_manifest_text("rr3-act", &bin),
    )
    .expect("写模块 manifest");
    std::fs::write(mod_dir.join("config.toml"), CONFIG).expect("写模块 config");
    let profile = root.write_profile(PROFILE);

    // ---------- ① 装配单正门 boot：spawn 行回显真 entry → 注册 → running ----------
    // tick 必传小值（E2E_TICK_MS）：supervisor 扫尸节拍复用同一旋钮，安静 tick 会让
    // running 的收尸/广播永不发生（e2e_profile/e2e_spawn_restart 同一条纪律）。
    let mut rrd = RrdTest::rr3_start_profile("e2e-act", &profile, E2E_TICK_MS);
    rrd.rr3_wait_log("rrd: 装配单已加载: ");
    let spawn = rrd.rr3_wait_log("spawn rr3-act pid=");
    assert!(
        spawn.contains(&format!("entry={}", bin.display())),
        "spawn 行必须回显渲染后的绝对 entry（bin 已 canonicalize，两侧同值）: {spawn}"
    );
    let reg = rrd.rr3_wait_log("注册 rr3-act task=");
    let act_task = rr3_field_from_log(&reg, "task=");
    rrd.rr3_wait_log("rr3-act → running");

    // ---------- ② 旁观连接 + servo 点名：应答与限速流 ----------
    let (mut obs, obs_task) = rr3_connect_observer(&mut rrd);
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            101,
            json!({"topic": "rr3-act/servo/command", "args": {"angle_deg": 90}}),
            obs_task,
        ),
    );
    // 先读 state 后读应答：两条帧的相对到达序不保证（同一 socket 上应答与广播
    // 不保序，conn.rs 的既证事实）。read_where 把不匹配的帧存回队列，应答由
    // rr3_read_call_reply 从队列兜底找回——两种时序都覆盖。反过来先读应答会把
    // state 丢掉（read_call_reply 不回存），而限速门 50ms 内没有第二条成功命令
    // 可以再触发发布，state 一丢断言就只能靠赌时序。
    let state = rr3_read_where(&mut obs, " servo/state 流", |f| {
        rr3_json_of(f)["topic"] == "rr3-act/servo/state"
    });
    assert_eq!(
        state.header.src_task, act_task,
        "state 帧 src 必须是 rr3-act 的分配值"
    );
    let sv = rr3_json_of(&state);
    assert_eq!(sv["seq"], 1, "首条成功命令的流序从 1 起: {sv}");
    assert_eq!(sv["angle_deg"], 90);
    let v = rr3_read_call_reply(&mut obs, 101);
    assert!(v.get("error").is_none(), "合法角度不该被拒: {v}");
    assert_eq!(v["result"]["angle_deg"], 90);
    assert_eq!(
        v["result"]["move_ms"], 800,
        "缺 move_ms 取 config 上限: {v}"
    );

    // ---------- ③ 业务性失败折 result.error 不断链 ----------
    // rr3_reply_call 走 Response.result 槽；JSON-RPC error 槽留给 rrd 的路由拒绝
    // （e2e_audio_chain 同一条线上分界）。失败命令不触发 state 发布（Err 分支无
    // state_angle），限速门的记账不受影响。
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            102,
            json!({"topic": "rr3-act/servo/command", "args": {"angle_deg": 200}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(&mut obs, 102);
    let err = v["result"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("越界必须回 result.error 字符串: {v}"));
    assert!(err.contains("行程"), "越界要点名行程区间: {err}");

    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            103,
            json!({"topic": "rr3-act/servo/command", "args": {}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(&mut obs, 103);
    let err = v["result"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("缺参必须回 result.error 字符串: {v}"));
    assert!(err.contains("angle_deg"), "缺参要点名参数: {err}");

    // ---------- ④ led / relay 两面（同一回路，白名单与真布尔语义） ----------
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            104,
            json!({"topic": "rr3-act/led/effect", "args": {"effect": "blink"}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(&mut obs, 104);
    assert_eq!(v["result"]["effect"], "blink", "{v}");
    assert_eq!(v["result"]["previous"], "off", "初始效果是 off: {v}");

    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            105,
            json!({"topic": "rr3-act/relay/set", "args": {"on": true}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(&mut obs, 105);
    assert_eq!(v["result"]["on"], true, "吸合应答回显同一真值: {v}");

    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            106,
            json!({"topic": "rr3-act/relay/set", "args": {"on": 1}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(&mut obs, 106);
    let err = v["result"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("非布尔必须回 result.error 字符串: {v}"));
    assert!(err.contains("布尔"), "类型不符要点名: {err}");
}
