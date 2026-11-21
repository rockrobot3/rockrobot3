//! e2e-act 阶段助手（自 `e2e_act.rs` 主用例按阶段抽出，判据不变）。
//!
//! 每个助手只做一段可独立判读的线上事实：夹具落盘、正门启动、三面 RPC、三面
//! intent 与反射环记账。主用例只保留顺序编排，失败点因此按阶段点名。

use std::path::{Path, PathBuf};
use std::time::Duration;

use rr_bus::RR3_TASK_RRD;
use rr_ipc::Rr3PayloadType;
use serde_json::json;

use crate::support::*;

use super::{AXIS_PAN, CONFIG, PROFILE};

/// 满配夹具落盘：module.toml / config.toml、servo 件装配清单、装配单与轴档案。
pub(super) fn stage_profile(root: &E2eRoot, bin: &Path) -> PathBuf {
    let mod_dir = root.dir.join("modules").join("rr3-act");
    std::fs::create_dir_all(&mod_dir).expect("建模块目录");
    std::fs::write(
        mod_dir.join("module.toml"),
        rr3_module_manifest_text("rr3-act", bin),
    )
    .expect("写模块 manifest");
    std::fs::write(mod_dir.join("config.toml"), CONFIG).expect("写模块 config");
    // 件装配清单单一真值（与 profile.rs stage_servo_module 同底稿），门 c 对
    // kind=power 角色执法覆盖判据——本夹具必须申报五键 demands。
    let asm = mod_dir.join("atoms/servo/assemble.toml");
    std::fs::create_dir_all(asm.parent().unwrap()).expect("建 atoms 目录");
    std::fs::write(&asm, include_str!("../../src/testdata/servo_assemble.toml"))
        .expect("stage servo assemble.toml");
    let profile = root.rr3_write_profile(PROFILE);
    // 同批 stage 轴档案：执行轴状态注入的正例基底（恰一份合法才投影）。
    let a = root
        .dir
        .join("hardware")
        .join("default")
        .join("axis")
        .join("pan.toml");
    std::fs::create_dir_all(a.parent().unwrap()).expect("建 axis 目录");
    std::fs::write(&a, AXIS_PAN).expect("写轴档案");
    profile
}

/// ① 正门 boot：spawn 行回显真 entry → 注册 → running；返回 act 的分配 task。
pub(super) fn boot_act(rrd: &mut RrdTest, bin: &Path) -> u64 {
    rrd.rr3_wait_log("rrd: 装配单已加载: ");
    let spawn = rrd.rr3_wait_log("spawn rr3-act pid=");
    assert!(
        spawn.contains(&format!("entry={}", bin.display())),
        "spawn 行必须回显渲染后的绝对 entry（bin 已 canonicalize，两侧同值）: {spawn}"
    );
    let reg = rrd.rr3_wait_log("注册 rr3-act task=");
    let act_task = rr3_field_from_log(&reg, "task=");
    rrd.rr3_wait_log("rr3-act → running");
    act_task
}

/// ② servo 点名：应答与限速流。先读 state 后读应答（反序会把先到的 state 丢掉）。
pub(super) fn servo_ok(obs: &mut RrdClient, obs_task: u64, act_task: u64) {
    rr3_send(
        obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            101,
            json!({"topic": "rr3-act/servo/command", "args": {"angle_deg": 90}}),
            obs_task,
        ),
    );
    let state = rr3_read_where(obs, " servo/state 流", |f| {
        rr3_json_of(f)["topic"] == "rr3-act/servo/state"
    });
    assert_eq!(
        state.header.src_task, act_task,
        "state 帧 src 必须是 rr3-act 的分配值"
    );
    let sv = rr3_json_of(&state);
    assert_eq!(sv["seq"], 1, "首条成功命令的流序从 1 起: {sv}");
    assert_eq!(sv["angle_deg"], 90);
    let v = rr3_read_call_reply(obs, 101);
    assert!(v.get("error").is_none(), "合法角度不该被拒: {v}");
    assert_eq!(v["result"]["angle_deg"], 90);
    assert_eq!(
        v["result"]["move_ms"], 800,
        "缺 move_ms 取 config 上限: {v}"
    );
}

/// ③ 业务性失败折 result.error 不断链（越界、缺参各一；失败不触发 state 发布）。
pub(super) fn servo_reject(obs: &mut RrdClient, obs_task: u64) {
    rr3_send(
        obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            102,
            json!({"topic": "rr3-act/servo/command", "args": {"angle_deg": 200}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(obs, 102);
    let err = v["result"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("越界必须回 result.error 字符串: {v}"));
    assert!(err.contains("行程"), "越界要点名行程区间: {err}");

    rr3_send(
        obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            103,
            json!({"topic": "rr3-act/servo/command", "args": {}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(obs, 103);
    let err = v["result"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("缺参必须回 result.error 字符串: {v}"));
    assert!(err.contains("angle_deg"), "缺参要点名参数: {err}");
}

/// ④a led 面：白名单效果与初始 off 回显。
pub(super) fn led_effect(obs: &mut RrdClient, obs_task: u64) {
    rr3_send(
        obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            104,
            json!({"topic": "rr3-act/led/effect", "args": {"effect": "blink"}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(obs, 104);
    assert_eq!(v["result"]["effect"], "blink", "{v}");
    assert_eq!(v["result"]["previous"], "off", "初始效果是 off: {v}");
}

/// ④b relay 点名：吸合的状态回读、应答与真布尔门（先 state 后应答，同 ② 纪律）。
pub(super) fn relay_rpc(obs: &mut RrdClient, obs_task: u64, act_task: u64) {
    rr3_send(
        obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            105,
            json!({"topic": "rr3-act/relay/set", "args": {"on": true}}),
            obs_task,
        ),
    );
    let st = rr3_read_where(obs, " relay/state 流（RPC 路）", |f| {
        rr3_json_of(f)["topic"] == "rr3-act/relay/state"
    });
    assert_eq!(
        st.header.src_task, act_task,
        "state 帧 src 必须是 rr3-act 的分配值"
    );
    let sv = rr3_json_of(&st);
    assert_eq!(sv["seq"], 1, "RPC 路首条 relay/state 从 1 起: {sv}");
    assert_eq!(sv["on"], true, "RPC 吸合的状态回读: {sv}");
    let v = rr3_read_call_reply(obs, 105);
    assert_eq!(v["result"]["on"], true, "吸合应答回显同一真值: {v}");

    rr3_send(
        obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            106,
            json!({"topic": "rr3-act/relay/set", "args": {"on": 1}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(obs, 106);
    let err = v["result"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("非布尔必须回 result.error 字符串: {v}"));
    assert!(err.contains("布尔"), "类型不符要点名: {err}");
}

/// 有界窗口内收集 relay/state（破坏性读法；窗口内其他帧不在断言域）。
fn relay_stray(obs: &mut RrdClient) -> Vec<String> {
    rr3_collect_within(obs, Duration::from_millis(250))
        .iter()
        .filter(|f| rr3_json_of(f)["topic"] == "rr3-act/relay/state")
        .map(|f| rr3_json_of(f).to_string())
        .collect()
}

/// ④c RPC 路同态抑制：已吸合再发吸合，不得产生新的 relay/state。
pub(super) fn relay_same_state(obs: &mut RrdClient, obs_task: u64) {
    rr3_send(
        obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            107,
            json!({"topic": "rr3-act/relay/set", "args": {"on": true}}),
            obs_task,
        ),
    );
    let stray = relay_stray(obs);
    assert!(
        stray.is_empty(),
        "RPC 同态 relay 命令不得产生 relay/state: {stray:?}"
    );
}

/// ⑤ brain 身份连接：按在册名注册（发布归属门按在册名判），返回连接与 task。
pub(super) fn brain_connect(rrd: &mut RrdTest) -> (RrdClient, u64) {
    let mut brain = rrd.rr3_connect();
    rr3_send(
        &mut brain,
        &rr3_register_frame(
            &rr3_manifest("rr3-brain", 3_600_000, 3_600_001, "never"),
            "rr3-brain",
        ),
    );
    let v = rr3_read_rpc(&mut brain);
    let brain_task = v["result"]["src_task"]
        .as_u64()
        .unwrap_or_else(|| panic!("brain 注册应答缺 src_task: {v}"));
    (brain, brain_task)
}

/// ⑤ intent 合法动作：relay 断开经两条驱动路共用的状态流回读（seq 延续 ④ 的 1）。
pub(super) fn intent_relay(
    brain: &mut RrdClient,
    obs: &mut RrdClient,
    brain_task: u64,
    act_task: u64,
) {
    rr3_send(
        brain,
        &rr3_json_frame(
            Rr3PayloadType::Event,
            &json!({
                "topic": "rr3-brain/intent/result",
                "trace": 72,
                "user_text": "开灯",
                "actions": [{"topic": "rr3-act/relay/set", "args": {"on": false}}],
            }),
            brain_task,
            RR3_TASK_RRD,
        ),
    );
    let st = rr3_read_where(obs, " relay/state 流（intent 路）", |f| {
        rr3_json_of(f)["topic"] == "rr3-act/relay/state"
    });
    assert_eq!(
        st.header.src_task, act_task,
        "intent 路的 state 帧同样出自 rr3-act"
    );
    let sv = rr3_json_of(&st);
    assert_eq!(sv["seq"], 2, "两条驱动路共用一条状态流，流序连贯: {sv}");
    assert_eq!(sv["on"], false, "intent 动作断开继电器: {sv}");
}

/// ⑤b intent 路同态抑制：已断再发断开，不得产生新的 relay/state。
pub(super) fn intent_relay_same(brain: &mut RrdClient, obs: &mut RrdClient, brain_task: u64) {
    rr3_send(
        brain,
        &rr3_json_frame(
            Rr3PayloadType::Event,
            &json!({
                "topic": "rr3-brain/intent/result",
                "trace": 73,
                "user_text": "再关一次灯",
                "actions": [{"topic": "rr3-act/relay/set", "args": {"on": false}}],
            }),
            brain_task,
            RR3_TASK_RRD,
        ),
    );
    let stray = relay_stray(obs);
    assert!(
        stray.is_empty(),
        "同态 relay 命令不得产生 relay/state: {stray:?}"
    );
}

/// ⑥ intent 转向动作：servo/command 过同两道门落 servo/state（seq 延续 ② 的 1）。
/// 限速窗必须在广播前等满（act 收帧即裁决，测试侧发完再睡拦不住 gate）。
pub(super) fn intent_servo(
    brain: &mut RrdClient,
    obs: &mut RrdClient,
    brain_task: u64,
    act_task: u64,
) {
    std::thread::sleep(Duration::from_millis(100));
    rr3_send(
        brain,
        &rr3_json_frame(
            Rr3PayloadType::Event,
            &json!({
                "topic": "rr3-brain/intent/result",
                "trace": 73,
                "user_text": "向左看",
                "actions": [{"topic": "rr3-act/servo/command", "args": {"angle_deg": 70}}],
            }),
            brain_task,
            RR3_TASK_RRD,
        ),
    );
    let st = rr3_read_where(obs, " servo/state 流（intent 路）", |f| {
        rr3_json_of(f)["topic"] == "rr3-act/servo/state"
    });
    assert_eq!(
        st.header.src_task, act_task,
        "intent 路的 state 帧同样出自 rr3-act"
    );
    let sv = rr3_json_of(&st);
    assert_eq!(
        sv["seq"], 2,
        "两条驱动路共用一条 servo 状态流，流序连贯: {sv}"
    );
    assert_eq!(sv["angle_deg"], 70, "左档转向落角: {sv}");
}

/// ⑦ 越权动作：归属门拒，有界窗口内不得再有 relay/state。
pub(super) fn intent_denied(brain: &mut RrdClient, obs: &mut RrdClient, brain_task: u64) {
    rr3_send(
        brain,
        &rr3_json_frame(
            Rr3PayloadType::Event,
            &json!({
                "topic": "rr3-brain/intent/result",
                "trace": 74,
                "user_text": "把别人家的电通上",
                "actions": [{"topic": "rr3-power/battery/state", "args": {"on": true}}],
            }),
            brain_task,
            RR3_TASK_RRD,
        ),
    );
    let stray = relay_stray(obs);
    assert!(
        stray.is_empty(),
        "越权动作被归属门拒后不得产生 relay/state: {stray:?}"
    );
}

/// ⑧ 反射环 tick 记账：ctl.counters 有界轮询（12 拍 × 250ms = 3s 上界）。
pub(super) fn reflex_ticks(rrd: &RrdTest) {
    let mut ticks = 0u64;
    for attempt in 0..12u64 {
        std::thread::sleep(Duration::from_millis(250));
        let mut ctl = rrd.rr3_connect();
        let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.counters", 500 + attempt);
        ticks = v["result"]["counters"]["rr3-act/reflex_ticks"]
            .as_u64()
            .unwrap_or(0);
        if ticks >= 1 {
            break;
        }
    }
    assert!(
        ticks >= 1,
        "反射环跑满数秒后 rr3-act/reflex_ticks 必须非零（1kHz 闭环宿主位的记账线）: {ticks}"
    );
}
