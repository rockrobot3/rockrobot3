#![cfg(unix)]
//! rr3-act 真模块的装配单正门端到端（0.0.2 T4a；0.0.3 T3 意图执行面；
//! ADR-0007/0012；Q1 裁定）。
//!
//! 与 `e2e_audio_chain.rs` 的分工：那条走 `--manifest` 直驱 boot 钉音频链；这条走
//! `--profile` 装配单正门（装机布局，与 T5 deb 同树）起**真 rr3-act**，钉 T4a 的
//! 验收面（方案 §3）：装配单驱动 e2e 中 rr3-act 在册 running、servo 点名应答——
//! 三事务面（servo/led/relay）的 rr3.call 回路穿过真进程，业务性失败折
//! result.error 不断链，servo/state 限速流上总线被旁观连接真实收到（rr-bus
//! 广播全量扇出、不按订阅过滤，conn.rs 的既证事实）。0.0.3 T3 起再加意图执行面：
//! brain 身份连接广播 intent 事件，actions 经两道归属门（exposes 归属 + 意图面
//! 开放）驱动 relay，断电平的效果以 relay/state 广播回读——RPC 与 intent 两条驱动路
//! 共用同一条状态流（流序连贯）。0.0.6 批F2 起意图面再开放转向：
//! `rr3-act/servo/command` 过同样的两道门落角，servo/state 同流序回读——
//! 判据 8"注入向左/向右类意图后舵机转到对应档位"的 sim 侧证据。越权动作被拒后不产生状态。
//!
//! 关键夹具差异（相对 e2e_profile 的 sim_peer 模板）：module.toml 用**仓内真契约
//! 工件**渲染 entry（`rr3_module_manifest_text`），且必须同树写 config.toml——
//! rr3_config_load 缺省取 manifest 同目录的 config.toml，缺它模块开机即 exit(1)
//! （探活失败不是被测行为，是夹具烂）。

mod support;

use std::path::PathBuf;
use std::time::Duration;

use rr_bus::RR3_TASK_RRD;
use rr_ipc::Rr3PayloadType;
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
    // 先读 state 后读应答（② 同一条纪律：read_call_reply 不回存，反序会把先到的
    // state 帧丢掉）。RPC 驱动路同样出 relay/state（0.0.3 T3：变化即发，RPC 与
    // intent 两条驱动路共用同一条状态流）。
    let st = rr3_read_where(&mut obs, " relay/state 流（RPC 路）", |f| {
        rr3_json_of(f)["topic"] == "rr3-act/relay/state"
    });
    assert_eq!(
        st.header.src_task, act_task,
        "state 帧 src 必须是 rr3-act 的分配值"
    );
    let sv = rr3_json_of(&st);
    assert_eq!(sv["seq"], 1, "RPC 路首条 relay/state 从 1 起: {sv}");
    assert_eq!(sv["on"], true, "RPC 吸合的状态回读: {sv}");
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

    // ---------- ⑤ intent actions 过真进程：brain 身份连接 + 合法动作落电平 ----------
    // 以 rr3-brain 在册身份另开连接广播 intent 事件（发布归属门按在册名判，
    // manifest 的 publishes 列表不参与）。act 订阅面经两道归属门执行 relay，
    // 断电平的效果以 relay/state 广播回读——e2e 证据链需要状态回读（补裁定的
    // 由来：模块 stdout 接 Stdio::null()，无状态流则意图执行效果不可观测）。
    // seq=2 延续 ④ 的 RPC 路（seq=1），钉"两条驱动路共用一条状态流"。
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

    rr3_send(
        &mut brain,
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
    let st = rr3_read_where(&mut obs, " relay/state 流（intent 路）", |f| {
        rr3_json_of(f)["topic"] == "rr3-act/relay/state"
    });
    assert_eq!(
        st.header.src_task, act_task,
        "intent 路的 state 帧同样出自 rr3-act"
    );
    let sv = rr3_json_of(&st);
    assert_eq!(sv["seq"], 2, "两条驱动路共用一条状态流，流序连贯: {sv}");
    assert_eq!(sv["on"], false, "intent 动作断开继电器: {sv}");

    // ---------- ⑥ intent 转向动作（批F2）：servo/command 过同两道门落 servo/state ----------
    // "向左看"类词表意图由 brain 广播 actions 落 rr3-act/servo/command（45°=左档，
    // brain 侧词表→角度映射由 python 单测覆盖，e2e 在 act 边界钉总线契约——判据 8
    // 的 sim 侧证据）。seq=2 延续 ② 的 RPC 路（seq=1）：②的 RPC servo 已在限速门
    // 记账，窗内（50ms）连发被拒且不补发（流是摘要不是逐拍镜像，设计行为）——UDS
    // 本地链路快到 act 处理 ⑥ 距 ② 的记账只有十几毫秒，所以等窗必须放在**广播前**
    // （act 收帧即裁决，测试侧发完再睡拦不住 act 的 gate 判定）。取 100ms 留裕度。
    std::thread::sleep(Duration::from_millis(100));
    rr3_send(
        &mut brain,
        &rr3_json_frame(
            Rr3PayloadType::Event,
            &json!({
                "topic": "rr3-brain/intent/result",
                "trace": 73,
                "user_text": "向左看",
                "actions": [{"topic": "rr3-act/servo/command", "args": {"angle_deg": 45}}],
            }),
            brain_task,
            RR3_TASK_RRD,
        ),
    );
    let st = rr3_read_where(&mut obs, " servo/state 流（intent 路）", |f| {
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
    assert_eq!(sv["angle_deg"], 45, "左档转向落角: {sv}");

    // ---------- ⑦ 越权动作：归属门拒，有界窗口内不得再有 relay/state ----------
    // 拒执行是 act 进程内的日志面（模块 stdout 接 Stdio::null()），e2e 侧唯一
    // 可观测面是"没有状态发布"。collect 是破坏性读法，按 support 纪律只放用例末尾。
    rr3_send(
        &mut brain,
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
    let drained = rr3_collect_within(&mut obs, Duration::from_millis(250));
    let stray: Vec<String> = drained
        .iter()
        .filter(|f| rr3_json_of(f)["topic"] == "rr3-act/relay/state")
        .map(|f| rr3_json_of(f).to_string())
        .collect();
    assert!(
        stray.is_empty(),
        "越权动作被归属门拒后不得产生 relay/state: {stray:?}"
    );
}
