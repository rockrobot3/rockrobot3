//! TOML 键行手术面单测（B9 结构收口；批B13 按被试族切出，结构门与引脚面在
//! `surgery_gates.rs` / `surgery_pins.rs`）。
//!
//! 覆盖 TOML 键行手术的红绿判据：只动目标行、fail-closed 四拒、同名多义拒、
//! 值与证据更新后重解析等值。被测面 = [`crate::profile_edit`]（机制）经
//! [`crate::profile`] 重导出。

use crate::profile::{rr3_domain_apply, rr3_key_line_apply, rr3_profile_parse};

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

/// 单引号域名的档同样可手术（TOML 两形都合法、装载期都收）：旧判据只认双
/// 引号会把在册域误报"缺域"——写口功能丢失且拒因与事实不符。
#[test]
fn domain_keys_apply_accepts_single_quoted_name() {
    let text = SURGERY_TEXT.replace("name = \"batt\"", "name = 'batt'");
    let out = rr3_domain_apply(&text, "batt", &[("severe_mv", "3200")]).expect("单引号域应可手术");
    assert!(out.contains("severe_mv = 3200"), "{out}");
}

/// 域块头带行内注释/尾空白仍识头（TOML 允许；旧整行等值会漏收该域——
/// 轻则误报"缺域"，重则把其键行算进邻块）。
#[test]
fn domain_keys_apply_accepts_commented_block_head() {
    let text = SURGERY_TEXT.replace(
        "[[supplies.domain]]\nname = \"batt\"",
        "[[supplies.domain]]  # 电池域\nname = \"batt\"",
    );
    let out =
        rr3_domain_apply(&text, "batt", &[("severe_mv", "3200")]).expect("带注释的头行应识头");
    assert!(out.contains("severe_mv = 3200"), "{out}");
}

/// 块头括号内空白（TOML 规范：表头键名内空白忽略）：`[[ supplies.domain ]]`
/// 与紧写同义、装载期同收——手术面旧判据按紧写字面前缀匹配会误报"缺域"，
/// 在册域的写口功能整体丢失。
#[test]
fn domain_keys_apply_accepts_spaced_block_head() {
    let text = SURGERY_TEXT.replace(
        "[[supplies.domain]]\nname = \"batt\"",
        "[[ supplies.domain ]]\nname = \"batt\"",
    );
    let out = rr3_domain_apply(&text, "batt", &[("severe_mv", "3200")]).expect("括号内空白应识头");
    assert!(out.contains("severe_mv = 3200"), "{out}");
    assert!(out.contains("severe_mv = 4400"), "邻域不得波及: {out}");
}

/// 等价性对拍（B38）：装载器（TOML 解析）收的形状，手术面必须能定位——两处
/// 对"在册域"的判据不得劈叉，否则写口对合法档误报"缺域"。逐形各判一次，
/// 同时核"他域仍拒"（定界没被放宽成"见块就收"）。
#[test]
fn domain_locator_agrees_with_toml_loader() {
    for head in [
        "[[supplies.domain]]",
        "[[ supplies.domain ]]",
        "[[supplies.domain]]  # 带注释",
        "  [[supplies.domain]]   ",
    ] {
        let text = format!("{head}\nname = 'batt'\nnominal_mv = 3700\nsevere_mv = 3300\n");
        let v: toml::Value =
            toml::from_str(&text).unwrap_or_else(|e| panic!("{head}: 装载器应收: {e}"));
        assert_eq!(
            v["supplies"]["domain"].as_array().map(Vec::len),
            Some(1),
            "{head}"
        );
        rr3_domain_apply(&text, "batt", &[("severe_mv", "3200")])
            .unwrap_or_else(|e| panic!("{head}: 装载器收则手术必须收: {e}"));
        assert!(
            rr3_domain_apply(&text, "rail5v", &[("severe_mv", "1")]).is_err(),
            "{head}: 他域仍须拒"
        );
    }
}

/// 单层目录取档（对拍矩阵用：profiles/ 与 hardware/<形态>/axis/ 都是平铺）。
fn rr3_repo_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
        .collect();
    out.sort();
    out
}

/// 等价性对拍矩阵（B39）：仓内**真实**装配单与轴档案——装载器认得的域/键，
/// 手术面必须也能定位。B37（单引号/带注释块头）与 B38（括号内空白）是同一
/// 病根：文本启发式与解析器判据劈叉。逐档逐条判等后，新增合法书写形制不靠
/// 人工想用例也不会静默丢功能。不空转：至少各判一份。
#[test]
fn repo_files_locator_matches_loader() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut domains = 0usize;
    for f in rr3_repo_files(&root.join("profiles")) {
        let text = std::fs::read_to_string(&f).expect("读装配单");
        let p = rr3_profile_parse(&text).unwrap_or_else(|e| panic!("{}: {e:?}", f.display()));
        for d in &p.supplies.domains {
            rr3_domain_apply(&text, &d.name, &[("severe_mv", "1")]).unwrap_or_else(|e| {
                panic!(
                    "{} 域 {:?} 装载器在册但手术定位失败: {e}",
                    f.display(),
                    d.name
                )
            });
            domains += 1;
        }
    }
    assert!(domains > 0, "仓内装配单至少一份有域声明（对拍不得空转）");

    let mut axes = 0usize;
    let hw = root.join("hardware");
    for shape in std::fs::read_dir(&hw).into_iter().flatten().flatten() {
        for f in rr3_repo_files(&shape.path().join("axis")) {
            let text = std::fs::read_to_string(&f).expect("读轴档案");
            let v: toml::Value = toml::from_str(&text).expect("轴档案须是合法 TOML");
            assert!(
                v.get("current_ceil_ma").is_some(),
                "{} 十四键必填",
                f.display()
            );
            rr3_key_line_apply(&text, "current_ceil_ma", "1")
                .unwrap_or_else(|e| panic!("{} 装载器在册但手术定位失败: {e}", f.display()));
            axes += 1;
        }
    }
    assert!(axes > 0, "仓内至少一份轴档案（对拍不得空转）");
}

/// 装配单里的域表（解析器口径，变体矩阵的 oracle）
fn rr3_domains(v: &toml::Value) -> Vec<&toml::Value> {
    v.get("supplies")
        .and_then(|s| s.get("domain"))
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

/// 指名域的整型键（缺失即红——变体矩阵的判据不接受"读不出就当没变"）
fn rr3_dom_int(v: &toml::Value, name: &str, key: &str) -> i64 {
    rr3_domains(v)
        .into_iter()
        .find(|d| d.get("name").and_then(toml::Value::as_str) == Some(name))
        .and_then(|d| d.get(key).and_then(toml::Value::as_integer))
        .unwrap_or_else(|| panic!("域 {name:?} 缺整型键 {key}"))
}

/// 变体矩阵（B43）：对每份真实装配单的每个在册域，做三种合法书写变体
/// （括号内空白 / 块头行内注释 / 单引号名），逐变体断言三件事：变体仍是合法
/// TOML、手术能定位、手术后重解析**只有目标域变**（其余域逐键零漂移）。
/// B37/B38 的误报都是"合法变体被文本判据拒"，这里用解析器做 oracle，
/// 而不是只断言"没报错"。
#[test]
fn repo_domain_variants_survive_surgery() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut checked = 0usize;
    for f in rr3_repo_files(&root.join("profiles")) {
        let text = std::fs::read_to_string(&f).expect("读装配单");
        let base: toml::Value = toml::from_str(&text).expect("仓内装配单须合法 TOML");
        let names: Vec<String> = rr3_domains(&base)
            .into_iter()
            .filter_map(|d| {
                d.get("name")
                    .and_then(toml::Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        if names.is_empty() {
            continue;
        }
        let mut quoted = text.clone();
        for name in &names {
            quoted = quoted.replace(&format!("name = \"{name}\""), &format!("name = '{name}'"));
        }
        let variants = [
            text.replace("[[supplies.domain]]", "[[ supplies.domain ]]"),
            text.replace("[[supplies.domain]]", "[[supplies.domain]]  # 域"),
            quoted,
        ];
        for (vi, vtext) in variants.iter().enumerate() {
            let v: toml::Value = toml::from_str(vtext)
                .unwrap_or_else(|e| panic!("{} 变体 {vi} 不是合法 TOML: {e}", f.display()));
            for name in &names {
                let out = rr3_domain_apply(vtext, name, &[("severe_mv", "1")])
                    .unwrap_or_else(|e| panic!("{} 域 {name} 变体 {vi}: {e}", f.display()));
                let after: toml::Value = toml::from_str(&out).expect("手术产物须合法 TOML");
                for other in &names {
                    let want = if other == name {
                        1
                    } else {
                        rr3_dom_int(&v, other, "severe_mv")
                    };
                    assert_eq!(
                        rr3_dom_int(&after, other, "severe_mv"),
                        want,
                        "{} 域 {other} 在 {name} 手术、变体 {vi} 下漂移",
                        f.display()
                    );
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "变体矩阵不得空转（仓内装配单至少一份有域）");
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
