#![cfg(unix)]
// SPDX-License-Identifier: Apache-2.0
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
//!
//! 阶段体在 [`phases`]（自本文件按阶段抽出，判据不变）：主用例只保留顺序编排，
//! 失败点按阶段点名。

mod support;

#[path = "e2e_act/phases.rs"]
mod phases;

use support::*;

/// 满配装配单：act 三格位各绑 rr3-act、enabled_slots 与 bindings 互认领。
/// 格位名取自真 `contracts/slots.toml`（Q1 裁定立格 act.relay 后三格齐备）。
/// schema_version=2 起三新段必填（[platform]/[supplies]/[slots]，ADR-0018 修订 1）；
/// present 只声明 bindings 用到的非 base 格位（base 格在册恒在，声明即致命）。
/// schema_version=3 起 [[pin]] 进场（增补轮六批N1）——本夹具只声明舵机轴一条
/// （批N3 件三：执行轴状态注入的正例基底），led/relay 的角色照旧缺绑定，校验②
/// 记 role_missing 非致命，boot 照常。schema_version=4 起 [supplies.budget]
/// 必填（批N2 校验⑩ 分母）——host 档合法不适用显式写，boot 照常。
/// schema_version=5 起 [runtime] 必填（批N5 件二 F-30）——log_ring_cap 进场。
const PROFILE: &str = concat!(
    include_str!("../src/testdata/profile_host_prefix.toml"),
    r#"
[slots]
present = ["act.servo", "act.led", "act.relay"]

[bindings]
"act.servo" = "rr3-act"
"act.led" = "rr3-act"
"act.relay" = "rr3-act"

[enabled_slots]
rr3-act = ["act.servo", "act.led", "act.relay"]

"#,
    include_str!("../src/testdata/pin_servo_pan.toml"),
);

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
/// `config_parse_covers_repo_shape` 钉过同形状解析，漂移当场红）。行程键不在
/// config（批N3 件五门 g 收敛：行程真值 = 轴档案经 RR3_ACT_AXIS_STATE 注入）。
const CONFIG: &str = r#"
[rr3-act]
log_level = "info"

[rr3-act.servo]
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

// 装机布局夹具用 support 的共享 [`support::E2eRoot`]（e2e_power/e2e_connect 同款）；
// 轴档案的 stage 在 `phases::stage_profile` 内紧跟 `rr3_write_profile` 落盘。

#[test]
fn act_module_full_face_end_to_end() {
    // 夹具：真二进制 + 真 manifest（entry 绝对化）+ config 同树
    let bin = rr3_build_module_bin("rr3-act");
    let root = E2eRoot::rr3_new("act", "act");
    let profile = phases::stage_profile(&root, &bin);
    // ① 装配单正门 boot；env 显式钉 sim 探针（act 三格位归 PWM/GPIO 硬件面，
    // 开发机与 CI runner 无 sysfs 设备，不显式选 sim 会被 fail-closed 判缺格）。
    // tick 必传小值：supervisor 扫尸节拍复用同一旋钮，安静 tick 会让收尸永不发生。
    let mut rrd =
        RrdTest::rr3_start_profile_env("e2e-act", &profile, E2E_TICK_MS, &[("RR3_PROBE", "sim")]);
    let act_task = phases::boot_act(&mut rrd, &bin);
    // ②–④ 三面 RPC（servo / led / relay）与两条同态抑制
    let (mut obs, obs_task) = rr3_connect_observer(&mut rrd);
    phases::servo_ok(&mut obs, obs_task, act_task);
    phases::servo_reject(&mut obs, obs_task);
    phases::led_effect(&mut obs, obs_task);
    phases::relay_rpc(&mut obs, obs_task, act_task);
    phases::relay_same_state(&mut obs, obs_task);
    // ⑤–⑦ intent 路：另一身份广播，状态流回读；越权动作不产生状态
    let (mut brain, brain_task) = phases::brain_connect(&mut rrd);
    phases::intent_relay(&mut brain, &mut obs, brain_task, act_task);
    phases::intent_relay_same(&mut brain, &mut obs, brain_task);
    phases::intent_servo(&mut brain, &mut obs, brain_task, act_task);
    phases::intent_denied(&mut brain, &mut obs, brain_task);
    // ⑧ 反射环 1kHz 记账线
    phases::reflex_ticks(&rrd);
}
