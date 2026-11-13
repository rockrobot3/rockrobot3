//! manifest 校验面：必填字段、心跳/看门狗关系、发布前缀、能力形状、semver。

use super::*;

#[test]
fn schema_embedded_has_expected_field_count() {
    let specs = rr_registry::rr3_field_specs();
    assert_eq!(
        specs.len(),
        19,
        "manifest 契约固定 19 字段（方案 §4 表格逐行落地）"
    );
    assert_eq!(
        specs.iter().filter(|f| f.required).count(),
        11,
        "11 个必填：manifest 6 + events 2 + resources 1 + health 2"
    );
}

fn fixture_text() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rr3-sense.toml"),
    )
    .unwrap()
}

/// 把改写后的 fixture 放进带契约副本的临时根，返回 manifest 路径
fn staged(tag: &str, text: String) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rr3-mf-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("modules/rr3-sense")).unwrap();
    std::fs::create_dir_all(dir.join("contracts")).unwrap();
    std::fs::copy(
        repo_root().join("contracts/topics.toml"),
        dir.join("contracts/topics.toml"),
    )
    .unwrap();
    let m = dir.join("modules/rr3-sense/module.toml");
    std::fs::write(&m, text).unwrap();
    m
}

#[test]
fn manifest_fixture_passes_validation() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rr3-sense.toml");
    let report = rr_registry::rr3_manifest_gate(&p);
    assert!(report.ok(), "合法 manifest 必须通过: {:?}", report.errors);
}

#[test]
fn manifest_missing_required_field_fails() {
    let m = staged("miss", fixture_text().replace("type = \"module\"\n", ""));
    let report = rr_registry::rr3_manifest_gate(&m);
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("缺少必填字段 manifest.type")),
        "{:?}",
        report.errors
    );
}

#[test]
fn manifest_bad_name_version_policy_fails() {
    let m = staged(
        "bad3",
        fixture_text()
            .replace("name = \"rr3-sense\"", "name = \"Sense_Module\"")
            .replace("version = \"0.0.1\"", "version = \"v1\"")
            .replace(
                "restart_policy = \"on-failure\"",
                "restart_policy = \"always!\"",
            ),
    );
    let report = rr_registry::rr3_manifest_gate(&m);
    // 三个原始违规；name 非法还会连带让 publishes 的前缀归属失败（≥1 条），故只断言包含关系
    for want in ["manifest.name 非法", "semver 非法", "restart_policy 越界"] {
        assert!(
            report.errors.iter().any(|e| e.contains(want)),
            "缺 {want}: {:?}",
            report.errors
        );
    }
}

#[test]
fn manifest_heartbeat_must_be_below_watchdog() {
    let m = staged(
        "health",
        fixture_text().replace("heartbeat_ms = 2000", "heartbeat_ms = 20000"),
    );
    let report = rr_registry::rr3_manifest_gate(&m);
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("必须小于 watchdog")),
        "{:?}",
        report.errors
    );
}

#[test]
fn manifest_publish_prefix_ownership_enforced() {
    // D7③ 的 manifest 侧：发布主题前缀必须等于 manifest.name
    let m = staged(
        "prefix",
        fixture_text().replace(
            "publishes = [\"rr3-sense/audio/vad_start\", \"rr3-sense/env/temperature\"]",
            "publishes = [\"rr3-power/battery/state\"]",
        ),
    );
    let report = rr_registry::rr3_manifest_gate(&m);
    assert!(
        report.errors.iter().any(|e| e.contains("越权主题")),
        "{:?}",
        report.errors
    );
}

#[test]
fn manifest_unregistered_topic_rejected() {
    let m = staged(
        "unreg",
        fixture_text().replace(
            "publishes = [\"rr3-sense/audio/vad_start\", \"rr3-sense/env/temperature\"]",
            "publishes = [\"rr3-sense/audio/ghost\"]",
        ),
    );
    let report = rr_registry::rr3_manifest_gate(&m);
    assert!(
        report.errors.iter().any(|e| e.contains("未注册主题")),
        "{:?}",
        report.errors
    );
}

#[test]
fn manifest_unknown_dependency_moved_to_resolution_layer() {
    // ADR-0009 裁定 1：单件校验只保 capability 形状，"未知能力"的判定迁到
    // 跨 manifest 仲裁层——形状合法但谁也不提供的能力（拼错的 motor.foc）
    // 在单件层放行、在全集层被点名拒掉
    let m = staged(
        "dep",
        fixture_text().replace(
            "required = [\"bus.subscribe\"]",
            "required = [\"motor.foc\"]",
        ),
    );
    let report = rr_registry::rr3_manifest_gate(&m);
    assert!(report.ok(), "单件层不再判可满足性: {:?}", report.errors);

    let facts = rr_registry::rr3_depends_parse(&std::fs::read_to_string(&m).unwrap())
        .expect("事实提取不应失败");
    let errs = rr_registry::rr3_boot_order(&[facts]).expect_err("未知能力必须被仲裁层拒");
    assert!(
        errs.iter().any(|e| e.contains("未知能力 motor.foc")),
        "{errs:?}"
    );
}

#[test]
fn manifest_capability_shape_and_type_and_bool_enforced() {
    // provides 段形状非法 + type/lang 越界 + upgrades.staging 类型错
    let m = staged(
        "shape",
        fixture_text()
            .replace(
                "capabilities = [\"audio.capture\", \"env.temperature\"]",
                "capabilities = [\"root\"]",
            )
            .replace("lang = \"rust\"", "lang = \"cpp\"")
            .replace("staging = true", "staging = \"true\""),
    );
    let report = rr_registry::rr3_manifest_gate(&m);
    assert_eq!(report.errors.len(), 3, "{:?}", report.errors);
}

#[test]
fn check_rules_cover_spec_section4_shapes() {
    assert!(rr_registry::rr3_registry_check("module_name", "rr3-sense").is_empty());
    assert_eq!(
        rr_registry::rr3_registry_check("module_name", "rr3-sense2").len(),
        1
    );
    assert_eq!(
        rr_registry::rr3_registry_check("module_name", "rr3-a-b").len(),
        1
    );
    assert!(rr_registry::rr3_registry_check("capability", "audio.capture").is_empty());
    assert_eq!(
        rr_registry::rr3_registry_check("capability", "audio").len(),
        1
    );
    assert!(rr_registry::rr3_registry_check("topic", "rr3-sense/audio/vad_start").is_empty());
    assert_eq!(
        rr_registry::rr3_registry_check("topic", "rr3-sense/audio").len(),
        1
    );
    assert!(rr_registry::rr3_registry_check("none", "任意").is_empty());
    assert_eq!(
        rr_registry::rr3_registry_check("no_such_rule", "x").len(),
        1
    );
}

#[test]
fn check_rule_semver() {
    assert!(rr_registry::rr3_registry_check("semver", "0.1.2").is_empty());
    for bad in ["1.0.0-rc1", "v1", "1", ".1.2", "1.2."] {
        assert_eq!(
            rr_registry::rr3_registry_check("semver", bad).len(),
            1,
            "{bad} 必须被拒"
        );
    }
}
