use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR 已是 base/runtime/rr-registry -> runtime -> base -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn topics_toml() -> String {
    std::fs::read_to_string(repo_root().join("contracts/topics.toml"))
        .expect("contracts/topics.toml 必须存在（Task 4 产物）")
}

#[test]
fn topics_prefix_accepts_canonical_topic() {
    let owners = rr_registry::rr3_registry_topic_owners(&topics_toml());
    let errs = rr_registry::rr3_registry_validate_topics(
        r#"
[[topic]]
name = "rr3-sense/audio/vad_start"
owner = "rr3-sense"
kind = "event"
"#,
        &owners,
    );
    assert!(errs.is_empty(), "合法主题被误拒: {errs:?}");
}

#[test]
fn topics_prefix_rejects_foreign_owner() {
    let owners = rr_registry::rr3_registry_topic_owners(&topics_toml());
    let errs = rr_registry::rr3_registry_validate_topics(
        r#"
[[topic]]
name = "rr3-power/audio/vad"
owner = "rr3-sense"
kind = "event"
"#,
        &owners,
    );
    assert_eq!(errs.len(), 1, "越权发布必须被抓: {errs:?}");
}

#[test]
fn topics_prefix_rejects_topic_outside_owner_namespace() {
    let owners = rr_registry::rr3_registry_topic_owners(&topics_toml());
    let errs = rr_registry::rr3_registry_validate_topics(
        r#"
[[topic]]
name = "rr3-sense/audio/vad_start/extra"
owner = "rr3-sense"
kind = "event"
"#,
        &owners,
    );
    assert_eq!(errs.len(), 1, "四段主题必须被拒: {errs:?}");
    assert!(errs[0].contains("topic 非法"), "{errs:?}");
}

#[test]
fn topics_prefix_rejects_unregistered_owner() {
    let owners = rr_registry::rr3_registry_topic_owners(&topics_toml());
    let errs = rr_registry::rr3_registry_validate_topics(
        r#"
[[topic]]
name = "rr3-ghost/x/y"
owner = "rr3-ghost"
kind = "event"
"#,
        &owners,
    );
    assert_eq!(errs.len(), 1);
}

#[test]
fn topics_prefix_contract_self_consistency() {
    let owners = rr_registry::rr3_registry_topic_owners(&topics_toml());
    assert_eq!(owners.len(), 7, "v0.0.1 固定 7 个 owner（5+1 模块 + rrd）");
    let errs = rr_registry::rr3_registry_validate_topics(&topics_toml(), &owners);
    assert!(errs.is_empty(), "contracts/topics.toml 自身必须自洽: {errs:?}");
}

#[test]
fn schema_embedded_has_expected_field_count() {
    let specs = rr_registry::rr3_registry_field_specs();
    assert_eq!(specs.len(), 19, "manifest 契约固定 19 字段（方案 §4 表格逐行落地）");
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
    std::fs::copy(repo_root().join("contracts/topics.toml"), dir.join("contracts/topics.toml"))
        .unwrap();
    let m = dir.join("modules/rr3-sense/module.toml");
    std::fs::write(&m, text).unwrap();
    m
}

#[test]
fn manifest_fixture_passes_validation() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rr3-sense.toml");
    let report = rr_registry::rr3_registry_validate_manifest(&p);
    assert!(report.ok(), "合法 manifest 必须通过: {:?}", report.errors);
}

#[test]
fn manifest_missing_required_field_fails() {
    let m = staged("miss", fixture_text().replace("type = \"module\"\n", ""));
    let report = rr_registry::rr3_registry_validate_manifest(&m);
    assert!(
        report.errors.iter().any(|e| e.contains("缺少必填字段 manifest.type")),
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
            .replace("restart_policy = \"on-failure\"", "restart_policy = \"always!\""),
    );
    let report = rr_registry::rr3_registry_validate_manifest(&m);
    // 三个原始违规；name 非法还会连带让 publishes 的前缀归属失败（≥1 条），故只断言包含关系
    for want in ["manifest.name 非法", "semver 非法", "restart_policy 越界"] {
        assert!(report.errors.iter().any(|e| e.contains(want)), "缺 {want}: {:?}", report.errors);
    }
}

#[test]
fn manifest_heartbeat_must_be_below_watchdog() {
    let m = staged(
        "health",
        fixture_text().replace("heartbeat_ms = 2000", "heartbeat_ms = 20000"),
    );
    let report = rr_registry::rr3_registry_validate_manifest(&m);
    assert!(
        report.errors.iter().any(|e| e.contains("必须小于 watchdog")),
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
    let report = rr_registry::rr3_registry_validate_manifest(&m);
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
    let report = rr_registry::rr3_registry_validate_manifest(&m);
    assert!(
        report.errors.iter().any(|e| e.contains("未注册主题")),
        "{:?}",
        report.errors
    );
}

#[test]
fn manifest_unknown_dependency_rejected() {
    let m = staged(
        "dep",
        fixture_text().replace("required = [\"bus.subscribe\"]", "required = [\"motor.foc\"]"),
    );
    let report = rr_registry::rr3_registry_validate_manifest(&m);
    assert!(
        report.errors.iter().any(|e| e.contains("未知能力 motor.foc")),
        "{:?}",
        report.errors
    );
}

#[test]
fn manifest_capability_shape_and_type_and_bool_enforced() {
    // provides 段形状非法 + type/lang 越界 + upgrades.staging 类型错
    let m = staged(
        "shape",
        fixture_text()
            .replace("capabilities = [\"audio.capture\", \"env.temperature\"]", "capabilities = [\"root\"]")
            .replace("lang = \"rust\"", "lang = \"cpp\"")
            .replace("staging = true", "staging = \"true\""),
    );
    let report = rr_registry::rr3_registry_validate_manifest(&m);
    assert_eq!(report.errors.len(), 3, "{:?}", report.errors);
}

#[test]
fn check_rules_cover_spec_section4_shapes() {
    assert!(rr_registry::rr3_registry_check("module_name", "rr3-sense").is_empty());
    assert_eq!(rr_registry::rr3_registry_check("module_name", "rr3-sense2").len(), 1);
    assert_eq!(rr_registry::rr3_registry_check("module_name", "rr3-a-b").len(), 1);
    assert!(rr_registry::rr3_registry_check("capability", "audio.capture").is_empty());
    assert_eq!(rr_registry::rr3_registry_check("capability", "audio").len(), 1);
    assert!(rr_registry::rr3_registry_check("topic", "rr3-sense/audio/vad_start").is_empty());
    assert_eq!(rr_registry::rr3_registry_check("topic", "rr3-sense/audio").len(), 1);
    assert!(rr_registry::rr3_registry_check("none", "任意").is_empty());
    assert_eq!(rr_registry::rr3_registry_check("no_such_rule", "x").len(), 1);
}

#[test]
fn check_rule_semver() {
    assert!(rr_registry::rr3_registry_check("semver", "0.1.2").is_empty());
    for bad in ["1.0.0-rc1", "v1", "1", ".1.2", "1.2."] {
        assert_eq!(rr_registry::rr3_registry_check("semver", bad).len(), 1, "{bad} 必须被拒");
    }
}
