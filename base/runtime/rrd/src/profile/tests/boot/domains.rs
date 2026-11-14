//! 电力域与证据判据：武装/测点/绑定位/证据形制与容器核验。

use super::super::*;
use crate::profile::rr3_profile_parse;
use std::path::PathBuf;

/// 校验⑥-c 反例：armed=true 而 sense=none / max_age_ms=0——未标定即不武装，
/// 无测点/无龄期上界的域不得武装拒动（§3.1 降级语义 + §19 Y-4）。
/// GOOD 的 batt 域恰是 sense=none + max_age_ms=0 的最小反例，armed 一开即双红。
#[test]
fn domain_armed_without_sense_is_fatal() {
    let root = Root::new("armed-gate");
    let text = GOOD.replace(
        "switchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        "switchable = false\narmed = true\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
    );
    let p = root.write_profile("armed.toml", &text);
    let e = boot_inputs(&p).expect_err("武装而无测点/龄期必须致命");
    assert!(e.contains("校验⑥-c"), "{e}");
}

/// 校验⑥-d 反例：域绑定位写第二套语法（F-2 的 adc:gp34）——不在 pin_id 集
/// 即红，"一律写 pin_id，不再各造语法"
#[test]
fn domain_binding_uses_pin_id_not_adhoc_syntax() {
    let root = Root::new("pid-gate");
    let text = GOOD.replace(
        "sense = \"none\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        "sense = \"adc:gp34\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
    );
    let p = root.write_profile("pid.toml", &text);
    let e = boot_inputs(&p).expect_err("第二套语法必须致命");
    assert!(e.contains("不在 [pins] 的 pin_id 集"), "{e}");
    assert!(e.contains("sense = adc:gp34"), "{e}");
}

/// 校验⑥-e 形状反例一（批N21 件二）：armed=true 而 evidence = none——武装
/// 拒动必须引用一条真板实测证据（规则住 conventions §6.1），"none" 即未
/// 标定，须 armed = false。
#[test]
fn domain_armed_without_evidence_is_fatal() {
    let root = Root::new("evid-none-gate");
    let text = GOOD.replace(
        "switchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        "switchable = false\narmed = true\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
    );
    let p = root.write_profile("evid-none.toml", &text);
    let e = boot_inputs(&p).expect_err("武装而无证据引用必须致命");
    assert!(e.contains("校验⑥-e"), "{e}");
    assert!(e.contains("evidence = none"), "{e}");
}

/// 校验⑥-e 形状反例二（批N21 件二）：引用形制坏（三位编号）——
/// `<形态>/<四位编号>` 是唯一词形（机制真值 rr-ipc::evidence），装载即拒。
#[test]
fn domain_armed_with_malformed_evidence_ref_is_fatal() {
    let root = Root::new("evid-form-gate");
    let text = GOOD.replace(
        "switchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        "switchable = false\narmed = true\nevidence = \"实测/1\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
    );
    let p = root.write_profile("evid-form.toml", &text);
    let e = boot_inputs(&p).expect_err("形制坏的引用必须致命");
    assert!(e.contains("形制不合法"), "{e}");
}

/// 校验⑥-e 成对反例三（批N21 件二）：armed=false 却携引用——证据位与
/// 武装位成对（同 switch/switchable 范式），未武装显式写 "none"，不留过期引用。
#[test]
fn domain_disarmed_with_evidence_ref_is_fatal() {
    let root = Root::new("evid-pair-gate");
    let text = GOOD.replace(
        "switchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        "switchable = false\narmed = false\nevidence = \"实测/0001\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
    );
    let p = root.write_profile("evid-pair.toml", &text);
    let e = boot_inputs(&p).expect_err("未武装携证据必须致命");
    assert!(e.contains("未武装不携证据"), "{e}");
}

/// 校验⑥-e 文件半边（批N21 件二，boot 装载链持有容器根调用）：armed=true 的
/// 合法形制引用，证据文件缺席即红（fail-closed，容器缺不静默放行）；同一
/// 装配单落一份六键齐且 form 与目录一致的证据文件后 boot 过＝绿例——D11
/// 界面门的装载期半边全链打通。绿例顺带钉死 ⑥-c 的武装前提（有测点＋龄期
/// 上界 > 0），证据门不孤立于武装门。
#[test]
fn armed_domain_evidence_file_check_gate() {
    let root = Root::new("evid-file-gate");
    let text = GOOD.replace(
        "sense = \"none\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        "sense = \"power.charger.meter\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = true\nevidence = \"实测/0001\"\nmax_age_ms = 30000\n\n[[supplies.domain]]\nname = \"rail5v\"",
    );
    let p = root.write_profile("evid-file.toml", &text);

    // 红例：容器缺——armed 域的引用查无文件即拒
    let e = boot_inputs(&p).expect_err("证据文件缺席必须致命");
    assert!(e.contains("校验⑥-e"), "{e}");
    assert!(e.contains("读不到"), "{e}");

    // 绿例：容器里落一份形制合法的证据文件（frontmatter 六键齐，form 与
    // 形态目录名一致）→ boot 过
    let ev = root.dir.join("docs/evidence/实测/0001.md");
    std::fs::create_dir_all(ev.parent().expect("证据路径必有父目录")).expect("建证据容器");
    std::fs::write(
        &ev,
        concat!(
            "+++\n",
            "form = \"实测\"\n",
            "slot_or_role = \"board.firmware\"\n",
            "method = \"真板实测\"\n",
            "conclusion = \"域武装依据成立\"\n",
            "date = \"2026-10-23\"\n",
            "recorder = \"lei_huang\"\n",
            "+++\n\n正文。\n",
        ),
    )
    .expect("写证据文件");
    boot_inputs(&p).expect("证据文件四道核全过后 boot 必须过");
}

/// 仓内证据容器锁定（批N21 件二，首批真档形制回归）：rrd 装载面消费的容器
/// 根与仓内 `docs/evidence` 是同一处——首批两份证据必须四道核全过，真档
/// 形制漂移即刻在本仓测试红。
#[test]
fn repo_evidence_container_frontline_files_verify() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../docs/evidence");
    rr_ipc::evidence::rr3_evidence_verify(&root, "实测/0001").expect("0001 必须是合法证据");
    rr_ipc::evidence::rr3_evidence_verify(&root, "实测/0002").expect("0002 必须是合法证据");
}

/// [[supplies.domain]] 形状门（解析层）：缺键逐键点名、未知键点名、
/// 缺整个域数组点名——一次收集完，运维一次改对
#[test]
fn domain_section_shape_errors_are_named() {
    // 缺整个域数组（schema 3 起必填）：取 GOOD 的 [supplies] 段头做最小文本
    let minimal_head = GOOD.split("[[supplies.domain]]").next().unwrap_or(GOOD);
    let errs = rr3_profile_parse(minimal_head).expect_err("缺域数组必须报错");
    assert!(
        errs.iter().any(|e| e.contains("缺少 [[supplies.domain]]")),
        "{errs:?}"
    );

    // 域条目缺键 + 未知键逐条点名
    let errs = rr3_profile_parse(&format!(
        "{minimal_head}[[supplies.domain]]\nname = \"batt\"\nnominal_mv = 3700\nghost = 1\n"
    ))
    .expect_err("缺键/未知键必须报错");
    for want in [
        "supplies.domains[0].mild_mv",
        "supplies.domains[0].severe_mv",
        "supplies.domains[0].current_cap_ma",
        "supplies.domains[0].sense",
        "supplies.domains[0].current_sense",
        "supplies.domains[0].switch",
        "supplies.domains[0].switchable",
        "supplies.domains[0].armed",
        "supplies.domains[0].evidence",
        "supplies.domains[0].max_age_ms",
        "supplies.domains[0] 未知键 ghost",
    ] {
        assert!(errs.iter().any(|e| e.contains(want)), "缺 {want}: {errs:?}");
    }
}
