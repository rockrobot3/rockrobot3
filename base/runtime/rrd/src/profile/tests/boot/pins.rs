//! 引脚装载判据：角色查册、per_instance、requires、direction、形状与供电轨。

use super::super::*;
use crate::profile::rr3_profile_parse;

/// 校验①-a：role 未在 roles.toml 在册 → 致命点名（真契约上跑）
#[test]
fn pin_unknown_role_is_fatal() {
    let root = Root::new("pin-role");
    let text = GOOD.replace("role = \"act.axis.pwm\"", "role = \"act.axis.magical\"");
    let p = root.write_profile("pin-role.toml", &text);
    let e = boot_inputs(&p).expect_err("自造角色必须致命");
    assert!(e.contains("角色 act.axis.magical 未在册"), "{e}");
}

/// 校验①-b per_instance 两侧：true 角色缺 instance / false 角色填 instance 都红
#[test]
fn pin_per_instance_rules_are_fatal_both_sides() {
    let root = Root::new("pin-perinst");
    // act.axis.pwm per_instance=true，删 instance → 必填缺
    let text = GOOD.replace("instance = \"pan\"\n", "");
    let p = root.write_profile("perinst-missing.toml", &text);
    let e = boot_inputs(&p).expect_err("per_instance=true 缺 instance 必须致命");
    assert!(e.contains("instance 必填"), "{e}");

    // act.led.gpio per_instance=false，填 instance → 禁填
    let text = format!(
        "{GOOD}\n[[pin]]\nrole = \"act.led.gpio\"\ninstance = \"x\"\n\
         controller = \"gpiochip0\"\nphysical = [4]\ndirection = \"out\"\nfailsafe = \"hold\"\n"
    );
    let p = root.write_profile("perinst-extra.toml", &text);
    let e = boot_inputs(&p).expect_err("per_instance=false 填 instance 必须致命");
    assert!(e.contains("instance 禁填"), "{e}");
}

/// 校验①-b 留空反例（§5 缺失语义）：requires 必填字段删键即红——channel 以
/// None = 缺、failsafe 以空串 = 缺，一次收集、逐条点名，一条不静默放行
#[test]
fn pin_requires_left_blank_is_fatal() {
    let root = Root::new("pin-blank");
    let text = GOOD
        .replace("channel = 0\n", "")
        .replace("failsafe = \"release\"\n", "");
    let p = root.write_profile("pin-blank.toml", &text);
    let e = boot_inputs(&p).expect_err("requires 字段留空必须致命");
    assert!(e.contains("缺 channel"), "{e}");
    assert!(e.contains("缺 failsafe"), "{e}");
}

/// 校验① direction 相等：pin.direction 与角色 direction 各说一套 → 致命点名
#[test]
fn pin_direction_mismatch_is_fatal() {
    let root = Root::new("pin-direction");
    let text = GOOD.replace("direction = \"out\"", "direction = \"in\"");
    let p = root.write_profile("pin-direction.toml", &text);
    let e = boot_inputs(&p).expect_err("direction 与角色不一致必须致命");
    assert!(
        e.contains("direction = in 与角色 act.axis.pwm direction = out 不一致"),
        "{e}"
    );
}

/// [[pin]] 形状：未知键/坏类型/缺身份键逐条点名，一次收集完
#[test]
fn pin_section_shape_errors_are_named() {
    let text =
        format!("{GOOD}\n[[pin]]\nrole = \"\"\ndirection = \"out\"\nwat = 1\nphysical = [\"x\"]\n");
    // GOOD 现役两条 pin，追加的坏 pin 序号 = #2
    let errs = rr3_profile_parse(&text).expect_err("形状不合必须报错");
    assert!(
        errs.iter().any(|e| e.contains("pin #2.role 缺失或为空")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("pin #2 未知键 wat")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.contains("pin #2.physical 的每一项都必须是整数")),
        "{errs:?}"
    );
    // 缺 role / 缺 direction（身份键缺席，解析期即红）
    let errs = rr3_profile_parse(format!("{GOOD}\n[[pin]]\ncontroller = \"pwm1\"\n").as_str())
        .expect_err("缺 role/direction 必须报错");
    assert!(
        errs.iter().any(|e| e.contains("pin #2 缺 role")),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("pin #2 缺 direction")),
        "{errs:?}"
    );
}

/// 校验⑥-a/⑥-b 反例（§18 F-5 的自造域名 + 功率面挂 none）：未在册 rail 与
/// kind=power 而 rail=none 都必须拒 boot 并点名
#[test]
fn pin_rail_domain_gate_is_fatal() {
    let root = Root::new("rail-gate");
    // ⑥-a：自造域名（F-5：rail24 不在任何一处在册集——文档示例值会被装机档抄）
    let text = GOOD.replace("rail = \"rail5v\"", "rail = \"rail24\"");
    let p = root.write_profile("rail-unknown.toml", &text);
    let e = boot_inputs(&p).expect_err("自造域名必须致命");
    assert!(
        e.contains("rail = rail24 未在 [[supplies.domain]] 在册"),
        "{e}"
    );

    // ⑥-b：kind=power 角色挂 none（act.axis.pwm 是 power，none 是 signal 侧显式值）
    let text = GOOD.replace("rail = \"rail5v\"", "rail = \"none\"");
    let p = root.write_profile("rail-none-power.toml", &text);
    let e = boot_inputs(&p).expect_err("功率面挂 none 必须致命");
    assert!(e.contains("kind = power"), "{e}");
}
