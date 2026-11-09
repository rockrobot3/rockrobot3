//! TOML 键行手术面单测（B9 结构收口；批B13 按被试族切出，结构门与引脚面在
//! `surgery_gates.rs` / `surgery_pins.rs`）。
//!
//! 覆盖 TOML 键行手术的红绿判据：只动目标行、fail-closed 四拒、同名多义拒、
//! 值与证据更新后重解析等值。被测面 = [`crate::profile_edit`]（机制）经
//! [`crate::profile`] 重导出。

use crate::profile::{rr3_domain_apply, rr3_key_line_apply};

// ── TOML 键行手术面单测（批N22 件一）：红绿判据 = 只动目标行、fail-closed 四拒 ──

/// 手术夹具：三域块（带注释与 [supplies] 段头，形状 = default.toml 缩减）。
/// 手术函数是文本级纯函数，不吃完整档——注释保留与邻块原封是它的判据域。
const SURGERY_TEXT: &str = r#"# 电力网络声明
[supplies]
tick_period_ms = 200

[[supplies.domain]]
name = "batt" # 电池域
nominal_mv = 3700
severe_mv = 3300
armed = false
evidence = "none"

[[supplies.domain]]
name = "rail5v"
nominal_mv = 5000
severe_mv = 4400
armed = false
evidence = "none"

[[supplies.domain]]
name = "rail3v3"
nominal_mv = 3300
severe_mv = 3000
armed = false
evidence = "none"
"#;

/// 正例：只动目标域的目标键行——注释保留、邻域同名键行原封、块序不变
#[test]
fn domain_keys_apply_touches_target_block_only() {
    let out =
        rr3_domain_apply(SURGERY_TEXT, "batt", &[("severe_mv", "3200")]).expect("合法手术应过");
    // 目标行已改
    assert!(out.contains("severe_mv = 3200"), "{out}");
    // 邻域同名键行原封
    assert!(out.contains("severe_mv = 4400"), "rail5v 不得波及: {out}");
    assert!(out.contains("severe_mv = 3000"), "rail3v3 不得波及: {out}");
    // 注释与块序保留
    assert!(out.contains('#'), "注释须保留: {out}");
    assert!(out.contains("# 电池域"), "{out}");
    let batt = out.split("rail5v").next().unwrap_or_default();
    assert!(
        batt.contains("name = \"batt\"") && batt.contains("severe_mv = 3200"),
        "batt 块内自洽: {batt}"
    );
}

/// 正例：armed + evidence 两键行一次手术（写口联动写形状）
#[test]
fn domain_keys_apply_pairs_armed_and_evidence() {
    let out = rr3_domain_apply(
        SURGERY_TEXT,
        "rail5v",
        &[("armed", "true"), ("evidence", "\"实测/0002\"")],
    )
    .expect("合法联动手术应过");
    let rail5v = out.split("rail3v3").next().unwrap_or_default();
    assert!(rail5v.contains("armed = true"), "{rail5v}");
    assert!(rail5v.contains("evidence = \"实测/0002\""), "{rail5v}");
    // batt 块的 armed 原封
    let batt = out.split("rail5v").next().unwrap_or_default();
    assert!(batt.contains("armed = false"), "batt 不得波及: {batt}");
}

/// 缺域拒：域名不在册不带病落盘
#[test]
fn domain_keys_apply_rejects_missing_domain() {
    let e =
        rr3_domain_apply(SURGERY_TEXT, "rail9v", &[("severe_mv", "1")]).expect_err("缺域必须拒");
    assert!(e.contains("rail9v") && e.contains("缺域拒"), "{e}");
}

/// 多义拒：同名两块 = 装载期域名唯一判据破，手术面不替解析器裁决
#[test]
fn domain_keys_apply_rejects_ambiguous_domain() {
    let dup = format!("{SURGERY_TEXT}\n[[supplies.domain]]\nname = \"batt\"\nnominal_mv = 1\nsevere_mv = 2\narmed = false\nevidence = \"none\"\n");
    let e = rr3_domain_apply(&dup, "batt", &[("severe_mv", "1")]).expect_err("同名多义必须拒");
    assert!(e.contains("命中 2 个"), "{e}");
}

/// 键行缺拒：块内缺目标键（档被改病）不插入不猜
#[test]
fn domain_keys_apply_rejects_missing_key_line() {
    let broken = SURGERY_TEXT.replace("severe_mv = 3300\n", "");
    let e =
        rr3_domain_apply(&broken, "batt", &[("severe_mv", "3200")]).expect_err("键行缺失必须拒");
    assert!(e.contains("severe_mv 缺失"), "{e}");
}

/// 平铺键行手术（轴档案用）：替换正例 + 词边界（`hi_deg` 不得命中他行）+
/// 缺行拒
#[test]
fn toml_key_line_apply_flat_and_word_boundary() {
    let axis = "schema = 1\npin = \"act.axis.pwm.pan\"\nhi_deg = 120\nslew_deg_s = 100\n";
    let out = rr3_key_line_apply(axis, "hi_deg", "135").expect("平铺手术应过");
    assert!(out.contains("hi_deg = 135"), "{out}");
    assert!(out.contains("slew_deg_s = 100"), "{out}");
    // 词边界：后随非 '=' 不命中——"deg" 无命中即拒而非误伤
    let e = rr3_key_line_apply(axis, "deg", "1").expect_err("无命中必须拒");
    assert!(e.contains("deg 缺失"), "{e}");
    // 尾行替换保尾换行
    let out = rr3_key_line_apply(axis, "slew_deg_s", "90").expect("应过");
    assert!(out.ends_with('\n'), "尾换行须保留: {out:?}");
}
