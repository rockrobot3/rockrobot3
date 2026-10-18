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
use rr_ipc::{rr3_body_split, Rr3PayloadType};
use serde_json::json;

use support::*;

/// 发一帧 ctl 请求（src 按契约填 0），收回第一条 Response 体——e2e_ctl 同款帮手
/// 的本地副本（ctl 一问一答即断，用例内逐拍新开裸连接，搬 support 反增耦合面）
fn rr3_ask_ctl(c: &mut RrdClient, method: &str, id: u64) -> serde_json::Value {
    rr3_send(c, &rr3_rpc_frame(method, id, json!({}), 0));
    let f = rr3_read_where(c, &format!("{method} 的应答"), |f| {
        rr3_body_split(&f.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    rr3_json_of(&f)
}

/// 满配装配单：act 三格位各绑 rr3-act、enabled_slots 与 bindings 互认领。
/// 格位名取自真 `contracts/slots.toml`（Q1 裁定立格 act.relay 后三格齐备）。
/// schema_version=2 起三新段必填（[platform]/[supplies]/[slots]，ADR-0018 修订 1）；
/// present 只声明 bindings 用到的非 base 格位（base 格在册恒在，声明即致命）。
/// schema_version=3 起 [[pin]] 进场（增补轮六批N1）——本夹具只声明舵机轴一条
/// （批N3 件三：执行轴状态注入的正例基底），led/relay 的角色照旧缺绑定，校验②
/// 记 role_missing 非致命，boot 照常。schema_version=4 起 [supplies.budget]
/// 必填（批N2 校验⑩ 分母）——host 档合法不适用显式写，boot 照常。
const PROFILE: &str = r#"
schema_version = 4

[os]
route = "distro"

[platform]
tier = "host"
chip = "rk3566"

[supplies]
watchdog_present = false
watchdog_timeout_ms = 15000
watchdog_feeder = "rrd"
tick_period_ms = 200
tick_jitter_ms = 100
safety_core_interval_ms = 1000

[[supplies.domain]]
name = "rail5v"
nominal_mv = 5000
mild_mv = 4600
severe_mv = 4400
current_cap_ma = 2000
sense = "none"
current_sense = "none"
switch = "none"
switchable = false
armed = false
max_age_ms = 0

[supplies.budget]
not_applicable = true

[slots]
present = ["act.servo", "act.led", "act.relay"]

[bindings]
"act.servo" = "rr3-act"
"act.led" = "rr3-act"
"act.relay" = "rr3-act"

[enabled_slots]
rr3-act = ["act.servo", "act.led", "act.relay"]

[[pin]]
role = "act.axis.pwm"
instance = "pan"
controller = "pwm0"
channel = 0
physical = [12]
direction = "out"
rail = "rail5v"
failsafe = "release"
mux_group = []
"#;

/// 轴档案夹具（hardware/<形态>/axis/pan.toml，件二装载器落位；批N3 件三）：
/// 已标定 + 挡块在册——执行轴状态投影 `calibrated=true,hard_stop=yes`，Auto 放行，
/// 既有 servo 点名断言不红；拒动语义由模块单测钉（e2e 只钉注入链路通）。
/// 值面参照件二 §4.4 示例档（(center,lo,hi) = (90,60,120)）。
const AXIS_PAN: &str = r#"
schema = 1
pin = "act.axis.pwm.pan"
center_deg = 90
lo_deg = 60
hi_deg = 120
dir_sign = 1
slew_deg_s = 100
settle_ms = 200
hold = "release"
current_ceil_ma = 750
warmup_ms = 0
hard_stop = "yes"
calibrated = true
source = "manual"
"#;

/// config 夹具：与仓内 `modules/rr3-act/config.toml` 同形状（main.rs 的
/// `config_parse_covers_repo_shape` 钉过同形状解析，漂移当场红）。
const CONFIG: &str = r#"
[rr3-act]
log_level = "info"

[rr3-act.servo]
angle_limit_deg = [70, 110]
move_timeout_ms = 800
state_min_interval_ms = 50

[rr3-act.led]
blink_period_ms = 500
effects = ["on", "off", "blink"]

[rr3-act.intent]
open_topics = ["rr3-act/relay/set", "rr3-act/servo/command"]

[rr3-act.reflex]
loop_hz = 1000
saturate_notice_ms = 500
"#;

/// 接线档底稿（增补轮六 §4.3）：装载期校验⑦要求装机档必在——vcc/gnd-only
/// 单线组（net=rail5v 在装配单域在册），对 [[pin]] 增删免疫。
const WIRING: &str = r#"
[[wire]]
connector = "J-T"
hdr_pin = [1, 2]
signal = ["vcc", "gnd"]
net = "rail5v"
feeds = []
mate = "none"
keyed = false
rated_a = 1.0
gnd_common = "star-1"
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
        // 同批 stage 接线档（hardware/<形态>/wiring.toml，rr3_wiring_path_for_profile
        // 推导的同一布局）：装载期校验⑦要求装机档必在
        let w = self
            .dir
            .join("hardware")
            .join("default")
            .join("wiring.toml");
        std::fs::create_dir_all(w.parent().unwrap()).expect("建 hardware 目录");
        std::fs::write(&w, WIRING).expect("写接线档");
        // 同批 stage 轴档案（hardware/<形态>/axis/pan.toml，件二装载器落位；批N3 件三
        // 执行轴状态注入的正例基底——恰一份合法才投影，见 profile.rs rr3_axis_state_text）
        let a = self
            .dir
            .join("hardware")
            .join("default")
            .join("axis")
            .join("pan.toml");
        std::fs::create_dir_all(a.parent().unwrap()).expect("建 axis 目录");
        std::fs::write(&a, AXIS_PAN).expect("写轴档案");
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
    // 同批 stage servo 件装配清单（与 profile.rs 单测 stage_servo_module 同形状）：
    // 门 c（批N2 ⑨-c）对 kind=power 角色执法覆盖判据——act.axis.pwm（kind=power）
    // 挂轨 rail5v 绑 rr3-act，件申报面无 rail5v 需求即拒 boot，故夹具件必须申报
    // （五键 demands，与仓内 servo 件同形状）
    let asm = mod_dir.join("atoms/servo/assemble.toml");
    std::fs::create_dir_all(asm.parent().unwrap()).expect("建 atoms 目录");
    std::fs::write(
        &asm,
        concat!(
            "schema_version = 2\n\n[assemble]\n",
            "handles = [\n",
            "  { trait = \"Rr3PwmServo\", desc_key = \"pwm_chip\" },\n",
            "  { trait = \"Rr3PwmServo\", desc_key = \"pwm_channel\" },\n",
            "]\n",
            "ops = []\n",
            "demands = [\n",
            "  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = [] },\n",
            "]\n",
        ),
    )
    .expect("stage servo assemble.toml");
    let profile = root.write_profile(PROFILE);

    // ---------- ① 装配单正门 boot：spawn 行回显真 entry → 注册 → running ----------
    // tick 必传小值（E2E_TICK_MS）：supervisor 扫尸节拍复用同一旋钮，安静 tick 会让
    // running 的收尸/广播永不发生（e2e_profile/e2e_spawn_restart 同一条纪律）。
    // env 显式钉 sim 探针（MC-4：非 test 面不再默认 sim）：act 三格位归 PWM/GPIO
    // 硬件面，开发机（macOS）与 CI runner 无这些 linux sysfs 设备，不显式选 sim
    // 会被 fail-closed 判缺格、模块不 spawn（被测行为之外的夹具前提）。
    let mut rrd =
        RrdTest::rr3_start_profile_env("e2e-act", &profile, E2E_TICK_MS, &[("RR3_PROBE", "sim")]);
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

    // ---------- ④b RPC 路同态抑制（批M M-β L-6；⑤b 只盖 intent 路，索引 §13.4 L-6 缺口）----------
    // 继电器已吸合（④ seq=1 on=true）再发同态吸合：两条驱动路共用同一抑制判定
    // （composite.rs RPC 路与 intent 路都调 rr3_relay_state_due，act 册 §9 不变量
    // "已实装行为有护栏"）。有界窗口收集（破坏性读法，⑤b/⑦ 同款纪律），窗口内
    // 混入的心跳等帧不在断言域；id=107 的应答不单独断言（同形状 result.on 已被
    // 105 钉过），被本窗口读走即弃。
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            107,
            json!({"topic": "rr3-act/relay/set", "args": {"on": true}}),
            obs_task,
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
        "RPC 同态 relay 命令不得产生 relay/state: {stray:?}"
    );

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

    // ---------- ⑤b 同态抑制（0.0.5 批I A-7 修好后进 e2e，act 册 §9 不变量）----------
    // 继电器已断再发断开：同态命令不得产生新 relay/state。有界窗口收集
    // （破坏性读法，⑦ 同款纪律），窗口内混入的 servo/心跳等帧不在断言域。
    rr3_send(
        &mut brain,
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
    let drained = rr3_collect_within(&mut obs, Duration::from_millis(250));
    let stray: Vec<String> = drained
        .iter()
        .filter(|f| rr3_json_of(f)["topic"] == "rr3-act/relay/state")
        .map(|f| rr3_json_of(f).to_string())
        .collect();
    assert!(
        stray.is_empty(),
        "同态 relay 命令不得产生 relay/state: {stray:?}"
    );

    // ---------- ⑥ intent 转向动作（批F2）：servo/command 过同两道门落 servo/state ----------
    // "向左看"类词表意图由 brain 广播 actions 落 rr3-act/servo/command（70°=左档，
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
                "actions": [{"topic": "rr3-act/servo/command", "args": {"angle_deg": 70}}],
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
    assert_eq!(sv["angle_deg"], 70, "左档转向落角: {sv}");

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

    // ---------- ⑧ 反射环 tick 记账（批M M-β A-6）：ctl.counters 有界轮询 ----------
    // reflex 环按 loop_hz 节拍空转、每速率窗（250ms，rr-core 节拍方案）结算一次，
    // 窗内 tick 数经 rr3_counter_bump 进模块自报面、心跳带上 rrd 归并——
    // "1kHz 闭环的宿主位真实存在"从日志宣言升级为**可点验的记账**（MC-11 判据：
    // 卡死与静默停摆看得见）。ctl 只对未注册连接开放（ADR-0010），逐拍新开裸
    // 连接一问一答；心跳拍上报存在延迟，有界轮询（12 拍 × 250ms = 3s 上界）。
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
