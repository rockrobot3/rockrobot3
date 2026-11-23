// SPDX-License-Identifier: Apache-2.0
//! axis 单测（文件头：本文件只放测试，生产面在 `axis.rs`）。
//!
//! 拆分动因：`axis.rs` 591 行、距 600 行门只剩个位数余量，先把测试面搬出。

use super::*;

fn tmpdir(tag: &str) -> PathBuf {
    crate::test_fixtures::tmpdir_ns("rrd-axis-t-", tag)
}

/// §4.4 示例档（pan 轴）：全部判据走绿的正例基底
fn sample_text() -> String {
    crate::test_fixtures::axis_sample_text(90, "unknown", false) + "\n"
}

fn sample_pin_ids() -> BTreeSet<String> {
    ["act.axis.pwm.pan", "act.axis.pwm.tilt"]
        .into_iter()
        .map(String::from)
        .collect()
}

/// 解析 → 逐键值面断言；序列化键序 = 词表序；再装载 round-trip 逐键相等
#[test]
fn parse_trip_order() {
    let f =
        rr3_act_prof_parse(&sample_text(), "axis.toml", &sample_pin_ids()).expect("示例档应全绿");
    assert_eq!(f.pin, "act.axis.pwm.pan");
    let p = &f.profile;
    assert_eq!((p.center_deg, p.lo_deg, p.hi_deg), (90, 60, 120));
    assert_eq!(p.dir_sign, 1);
    assert_eq!(p.slew_deg_s, 100);
    assert_eq!(p.settle_ms, 200);
    assert_eq!(p.hold, Rr3ActHold::Release);
    assert_eq!(p.current_ceil_ma, 750);
    assert_eq!(p.warmup_ms, 0);
    assert_eq!(p.hard_stop, Rr3ActHardStop::Unknown);
    assert!(!p.calibrated);
    assert_eq!(p.source, Rr3ActSource::Manual);

    let text = rr3_act_prof_toml(&f);
    let keys: Vec<&str> = text
        .lines()
        .filter_map(|l| l.split('=').next())
        .map(|k| k.trim())
        .collect();
    let vocab: Vec<&str> = RR3_ACT_PROFILE_FIELDS.iter().map(|f| f.key).collect();
    assert_eq!(keys, vocab, "发射键序必须与十四键词表同序");
    let f2 = rr3_act_prof_parse(&text, "axis.toml", &sample_pin_ids()).expect("回读应全绿");
    assert_eq!(f2, f, "round-trip 逐键一致");
}

/// 未知键 + 缺键同批全数点名（错误全收集，不挤牙膏；F-6 留空即红）
#[test]
fn unknown_collected() {
    let mut lines: Vec<String> = sample_text().lines().map(String::from).collect();
    lines.push("extra_key = 1".to_string());
    let idx = lines
        .iter()
        .position(|l| l.starts_with("calibrated"))
        .unwrap();
    lines.remove(idx);
    let errs = rr3_act_prof_parse(&lines.join("\n"), "axis.toml", &sample_pin_ids())
        .expect_err("未知键 + 缺键应全数点名");
    assert_eq!(errs.len(), 2, "错误全收集：{errs:?}");
    assert!(errs.iter().any(|e| e.contains("未知键 extra_key")));
    assert!(errs.iter().any(|e| e.contains("缺键 calibrated")));
}

/// pin 外键未命中 [pins] 的 pin_id 集（F-4：档案与绑定之间必须有外键）
#[test]
fn pin_key_miss_red() {
    let text = sample_text().replace("act.axis.pwm.pan", "act.axis.pwm.unknown");
    let errs =
        rr3_act_prof_parse(&text, "axis.toml", &sample_pin_ids()).expect_err("外键未命中应红");
    assert_eq!(errs.len(), 1);
    assert!(
        errs[0].contains("pin = \"act.axis.pwm.unknown\"") && errs[0].contains("F-4"),
        "{errs:?}"
    );
}

/// schema 版本对账：只认 1，升版走契约级评审
#[test]
fn schema_mismatch_red() {
    let text = sample_text().replace("schema = 1", "schema = 2");
    let errs =
        rr3_act_prof_parse(&text, "axis.toml", &sample_pin_ids()).expect_err("schema 版本不对应红");
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("schema"), "{errs:?}");
}

/// 行程窗口自洽判据：须严格 lo < center < hi（lo == center 也破）
#[test]
fn window_self_red() {
    let text = sample_text().replace("lo_deg = 60", "lo_deg = 90");
    let errs =
        rr3_act_prof_parse(&text, "axis.toml", &sample_pin_ids()).expect_err("行程窗口自洽破应红");
    assert!(errs[0].contains("行程窗口自洽破"), "{errs:?}");
}

/// dir_sign = 0 是物理矛盾（单独点名）；dir_sign = 2 越域（±1 之外）
#[test]
fn dir_zero_out_red() {
    let text = sample_text().replace("dir_sign = 1", "dir_sign = 0");
    let errs =
        rr3_act_prof_parse(&text, "axis.toml", &sample_pin_ids()).expect_err("dir_sign = 0 应红");
    assert!(errs[0].contains("dir_sign = 0"), "{errs:?}");
    let text = sample_text().replace("dir_sign = 1", "dir_sign = 2");
    let errs =
        rr3_act_prof_parse(&text, "axis.toml", &sample_pin_ids()).expect_err("dir_sign = 2 应红");
    assert!(errs[0].contains("越域"), "{errs:?}");
}

/// 词键词表外即红（hold 词形真值在 rr-hal-api，装载侧不认第二份）
#[test]
fn hold_word_out_red() {
    let text = sample_text().replace("hold = \"release\"", "hold = \"auto\"");
    let errs = rr3_act_prof_parse(&text, "axis.toml", &sample_pin_ids()).expect_err("词表外应红");
    assert!(errs[0].contains("hold = \"auto\" 不在册"), "{errs:?}");
}

/// 落盘 → 读回 round-trip；原子写序走完不得留 tmp 残留
#[cfg(unix)]
#[test]
fn store_tmp_leftover() {
    let dir = tmpdir("round");
    let path = dir.join("pan.toml");
    let f = rr3_act_prof_parse(&sample_text(), "axis.toml", &sample_pin_ids()).unwrap();
    rr3_act_prof_store(&path, &f).expect("落盘应成功");
    assert!(
        !dir.join("pan.toml.tmp").exists(),
        "原子写序走完不得留 tmp 残留"
    );
    let loaded = rr3_act_profile_load(&path, &sample_pin_ids())
        .expect("读回应全绿")
        .expect("刚写完的档案必在场");
    assert_eq!(loaded, f);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 无档案 = None（机制不代言缺省）；坏件 = Err 且字节不动（不许覆盖）
#[test]
fn load_miss_overwrite() {
    let dir = tmpdir("broken");
    let path = dir.join("pan.toml");
    assert_eq!(
        rr3_act_profile_load(&path, &sample_pin_ids()).unwrap(),
        None,
        "无档案 = None"
    );
    std::fs::write(&path, "schema = 1\npin = \"x\"\n").unwrap();
    let before = std::fs::read(&path).unwrap();
    let errs = rr3_act_profile_load(&path, &sample_pin_ids()).expect_err("坏件应红");
    assert!(errs.iter().any(|e| e.contains("缺键")), "{errs:?}");
    assert_eq!(std::fs::read(&path).unwrap(), before, "装载失败不得动文件");
    let _ = std::fs::remove_dir_all(&dir);
}
