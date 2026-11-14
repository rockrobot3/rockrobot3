//! 执行轴档案投影：单轴/双轴/坏件/副本/轴错配。

use super::super::*;

/// 轴档案文本帮手（十四键，值面参照件二 §4.4 示例档）：pin/calibrated/hard_stop
/// 三键参数化——pin 走 GOOD 的 pin_id 语法（外键对账 F-4），状态两键即执行轴
/// 投影的输入面。
fn axis_text(pin: &str, calibrated: bool, hard_stop: &str) -> String {
    axis_text_range(pin, calibrated, hard_stop, 60, 120)
}

/// 行程可变的投影输入面（件五双轴用例：pan/tilt 各给一行程，注入文本可辨源）。
fn axis_text_range(pin: &str, calibrated: bool, hard_stop: &str, lo: u16, hi: u16) -> String {
    format!(
        "schema = 1\npin = \"{pin}\"\ncenter_deg = 90\nlo_deg = {lo}\nhi_deg = {hi}\ndir_sign = 1\nslew_deg_s = 100\nsettle_ms = 200\nhold = \"release\"\ncurrent_ceil_ma = 750\nwarmup_ms = 0\nhard_stop = \"{hard_stop}\"\ncalibrated = {calibrated}\nsource = \"manual\"\n"
    )
}

/// 双轴装配单（ESP32_GOOD + tilt pin）：rtos 档不派生 [hardware]，双 pin 可在册
/// （host 档派生键同值校验会把双轴拒在门外——host 模块实例是单轴单描述符，模型
/// 自洽；板侧双轴真形貌在 rtos 档，板侧描述符消费归 0.0.7）。channel/physical 与
/// pan 错开（③-a/③-b 双占各守一维）。件五消歧用例的输入面。
fn esp32_good_tilt() -> String {
    format!(
        "{ESP32_GOOD}\n[[pin]]\nrole = \"act.axis.pwm\"\ninstance = \"tilt\"\n\
         controller = \"ledc0\"\nchannel = 1\nphysical = [33]\ndirection = \"out\"\n\
         rail = \"rail5v\"\nfailsafe = \"release\"\nmux_group = []\n"
    )
}

/// 执行轴状态投影·恰一份（批N3 件三建，件五扩形）：合法档案装载成功 → 注入
/// 文本按 `calibrated=<bool>,hard_stop=<词>,lo=<n>,hi=<n>,center=<n>` 投影，
/// 点名清单干净——rr3-act 拒动判据与行程裁决的输入面就是这五键的档案真值。
#[test]
fn axis_state_projects_when_exactly_one_profile_loads() {
    let root = Root::new("axis-one");
    let p = root.write_profile("full.toml", GOOD);
    root.write_axis(&p, "pan", &axis_text("act.axis.pwm.pan", true, "yes"));
    let boot = boot_inputs(&p).expect("恰一份合法档案不改变 boot 绿面");
    assert_eq!(
        boot.axis_state, "calibrated=true,hard_stop=yes,lo=60,hi=120,center=90",
        "恰一份合法档案必须投影执行轴状态（rr3-act 拒动判据的输入面）"
    );
    assert!(boot.axis_notes.is_empty(), "{:?}", boot.axis_notes);
}

/// 执行轴状态投影·坏件（批N3 件三）：档案形状错 → 注入文本空串 = 不注入
/// （模块侧折保守缺省，§6 复位语义④），点名清单逐条入账且**不拒 boot**——
/// 档案装载是降级面，与缺格/缺角色同档。
#[test]
fn axis_state_stays_empty_and_notes_broken_profile() {
    let root = Root::new("axis-broken");
    let p = root.write_profile("full.toml", GOOD);
    root.write_axis(&p, "pan", "schema = 1\npin = \"act.axis.pwm.pan\"\n");
    let boot = boot_inputs(&p).expect("坏档案不拒 boot（降级面，与缺格同档）");
    assert!(
        boot.axis_state.is_empty(),
        "坏档案不得投影注入文本——缺真相按最窄面起跑"
    );
    assert!(
        boot.axis_notes
            .iter()
            .any(|n| n.contains("pan.toml") && n.contains("坏形")),
        "坏件必须点名到件: {:?}",
        boot.axis_notes
    );
}

/// 执行轴状态投影·同 pin 副本（批N3 件三建，件五收口）：两份合法档案同一 pin
/// → 注入文本空串并点名副本——同 pin 副本不是消歧对象是病灶（恰一份合法才投影
/// 的 boot 链防线；写侧同款拒面在 axis_write 件四，读侧点名指清理）。
#[test]
fn axis_state_stays_empty_and_notes_duplicate_pin_profiles() {
    let root = Root::new("axis-two");
    let p = root.write_profile("full.toml", GOOD);
    root.write_axis(&p, "pan", &axis_text("act.axis.pwm.pan", true, "yes"));
    root.write_axis(&p, "pan-copy", &axis_text("act.axis.pwm.pan", true, "yes"));
    let boot = boot_inputs(&p).expect("两份合法档案不改变 boot 绿面");
    assert!(
        boot.axis_state.is_empty(),
        "同 pin 副本不得投影注入文本——恰一份合法才投影"
    );
    assert!(
        boot.axis_notes
            .iter()
            .any(|n| n.contains("2 份合法档案（副本）") && n.contains("先清理副本")),
        "同 pin 副本必须点名病灶: {:?}",
        boot.axis_notes
    );
}

/// 执行轴状态投影·双轴消歧（批N3 件五，收"多轴未消歧"挂账）：pan + tilt 两份
/// 合法档案，轴读数 = pan（env 缺省同源）→ 只投影 pan 的行程真值（五键文本
/// 可辨源：pan [60,120] ≠ tilt [80,100]）；点名清单干净——注入的行程必须属于
/// 被驱动的那根轴。
#[test]
fn axis_state_projects_driven_axis_when_two_axes_present() {
    let root = Root::new("axis-dual");
    let p = root.write_profile("dual.toml", &esp32_good_tilt());
    root.write_axis(&p, "pan", &axis_text("act.axis.pwm.pan", true, "yes"));
    root.write_axis(
        &p,
        "tilt",
        &axis_text_range("act.axis.pwm.tilt", true, "yes", 80, 100),
    );
    let boot = boot_inputs(&p).expect("双轴两份合法档案不改变 boot 绿面");
    assert_eq!(
        boot.axis_state, "calibrated=true,hard_stop=yes,lo=60,hi=120,center=90",
        "双轴在场只投影被驱动轴（env 缺省 pan）的行程真值"
    );
    assert!(boot.axis_notes.is_empty(), "{:?}", boot.axis_notes);
}

/// 执行轴状态投影·轴读数与档案 instance 错配（批N3 件五）：装配单只挂 tilt
/// pin、档案只有 tilt，轴读数 = 缺省 pan → 单份也对轴，"注入 tilt 行程、驱动
/// pan 轴"是错配，宁可空串折保守缺省并点名。
#[test]
fn axis_state_stays_empty_when_axis_env_misses_every_instance() {
    let text = GOOD.replace("instance = \"pan\"", "instance = \"tilt\"");
    let root = Root::new("axis-mismatch");
    let p = root.write_profile("tilt-only.toml", &text);
    root.write_axis(
        &p,
        "tilt",
        &axis_text_range("act.axis.pwm.tilt", true, "yes", 80, 100),
    );
    let boot = boot_inputs(&p).expect("轴错配不改变 boot 绿面（降级面）");
    assert!(
        boot.axis_state.is_empty(),
        "轴读数对不上任何 instance 不得投影——错配行程比无行程更危险"
    );
    assert!(
        boot.axis_notes
            .iter()
            .any(|n| n.contains("无一 instance = \"pan\"")),
        "错配必须点名轴读数与 instance 集: {:?}",
        boot.axis_notes
    );
}
