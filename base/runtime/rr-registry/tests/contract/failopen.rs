//! Fix round 1 的 fail-open 封口：非字符串项、rpc_exposes 查册、空注册表。

use super::*;

// ---------- Fix round 1：封死三处 fail-open 漏洞 ----------

#[test]
fn manifest_non_string_publish_item_is_rejected() {
    // F1：publishes 混入非字符串项必须报告，而非被 filter_map 静默丢弃
    let m = staged(
        "f1pub",
        fixture_text().replace(
            "publishes = [\"rr3-sense/audio/vad_start\", \"rr3-sense/env/temperature\"]",
            "publishes = [\"rr3-sense/audio/vad_start\", 42]",
        ),
    );
    let report = rr_registry::rr3_manifest_gate(&m);
    assert!(
        report.errors.iter().any(|e| e.contains("events.publishes")
            && e.contains("应为字符串")
            && e.contains("42")),
        "非字符串列表项必须被抓: {:?}",
        report.errors
    );
    assert!(!report.ok(), "{:?}", report.errors);
}

#[test]
fn manifest_non_string_dependency_item_is_rejected() {
    // F1：depends.required 混入非字符串项同样必须报告
    let m = staged(
        "f1dep",
        fixture_text().replace("required = [\"bus.subscribe\"]", "required = [42]"),
    );
    let report = rr_registry::rr3_manifest_gate(&m);
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("depends.required") && e.contains("应为字符串")),
        "非字符串依赖项必须被抓: {:?}",
        report.errors
    );
}

#[test]
fn manifest_rpc_exposes_must_be_registered() {
    // F2：rpc.exposes 命中 contracts/topics.toml（CI 检查③），未注册的 RPC 名必须被拒
    let m = staged(
        "f2rpc",
        fixture_text().replace(
            "exposes = [\"rr3-sense/capture/dump\"]",
            "exposes = [\"rr3-sense/capture/typooo\"]",
        ),
    );
    let report = rr_registry::rr3_manifest_gate(&m);
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("未注册主题 rr3-sense/capture/typooo")),
        "未注册 RPC 名必须被抓: {:?}",
        report.errors
    );
}

#[test]
fn manifest_fixture_rpc_exposes_still_accepted() {
    // F2：新增 rr3-sense/capture/dump 后，合法 fixture 的 rpc.exposes 仍须通过
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rr3-sense.toml");
    let report = rr_registry::rr3_manifest_gate(&p);
    assert!(
        report.ok(),
        "含已注册 RPC 的合法 manifest 必须通过: {:?}",
        report.errors
    );
}

#[test]
fn topics_registry_zero_topics_is_rejected() {
    // F3：8 owner / 0 topic 的空注册表必须被拒——空表不是放行许可证
    let owners = rr_registry::rr3_topics_owners(&topics_toml());
    assert_eq!(owners.len(), 8);
    let owners_only: String = topics_toml()
        .lines()
        .take_while(|l| !l.trim_start().starts_with("[[topic]]"))
        .collect::<Vec<_>>()
        .join("\n");
    let errs = rr_registry::rr3_topics_chk(&owners_only, &owners);
    assert!(
        errs.iter().any(|e| e.contains("主题注册表为空")),
        "空注册表必须被拒: {errs:?}"
    );

    // 下游成员检查不得被空注册表空洞满足：manifest 仍须因未注册主题被拒
    let dir = std::env::temp_dir().join("rr3-mf-emptyreg");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("modules/rr3-sense")).unwrap();
    std::fs::create_dir_all(dir.join("contracts")).unwrap();
    std::fs::write(dir.join("contracts/topics.toml"), &owners_only).unwrap();
    let m = dir.join("modules/rr3-sense/module.toml");
    std::fs::write(
        &m,
        fixture_text()
            .replace(
                "publishes = [\"rr3-sense/audio/vad_start\", \"rr3-sense/env/temperature\"]",
                "publishes = [\"rr3-sense/audio/ghost\"]",
            )
            .replace(
                "subscribes = [\"rr3-runtime/module/state\"]",
                "subscribes = [\"rr3-runtime/no/such\"]",
            ),
    )
    .unwrap();
    let report = rr_registry::rr3_manifest_gate(&m);
    assert!(
        !report.ok(),
        "空注册表不得放行未注册主题: {:?}",
        report.errors
    );
}
