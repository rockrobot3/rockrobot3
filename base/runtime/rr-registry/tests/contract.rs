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
    assert!(
        errs.is_empty(),
        "contracts/topics.toml 自身必须自洽: {errs:?}"
    );
}

#[test]
fn schema_embedded_has_expected_field_count() {
    let specs = rr_registry::rr3_registry_field_specs();
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
    let report = rr_registry::rr3_registry_validate_manifest(&p);
    assert!(report.ok(), "合法 manifest 必须通过: {:?}", report.errors);
}

#[test]
fn manifest_missing_required_field_fails() {
    let m = staged("miss", fixture_text().replace("type = \"module\"\n", ""));
    let report = rr_registry::rr3_registry_validate_manifest(&m);
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
    let report = rr_registry::rr3_registry_validate_manifest(&m);
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
    let report = rr_registry::rr3_registry_validate_manifest(&m);
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
    let report = rr_registry::rr3_registry_validate_manifest(&m);
    assert!(report.ok(), "单件层不再判可满足性: {:?}", report.errors);

    let facts =
        rr_registry::rr3_registry_depends_facts_from_text(&std::fs::read_to_string(&m).unwrap())
            .expect("事实提取不应失败");
    let errs =
        rr_registry::rr3_registry_resolve_boot_order(&[facts]).expect_err("未知能力必须被仲裁层拒");
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
    let report = rr_registry::rr3_registry_validate_manifest(&m);
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
    let report = rr_registry::rr3_registry_validate_manifest(&m);
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
    let report = rr_registry::rr3_registry_validate_manifest(&m);
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
    let report = rr_registry::rr3_registry_validate_manifest(&m);
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
    let report = rr_registry::rr3_registry_validate_manifest(&p);
    assert!(
        report.ok(),
        "含已注册 RPC 的合法 manifest 必须通过: {:?}",
        report.errors
    );
}

#[test]
fn topics_registry_zero_topics_is_rejected() {
    // F3：7 owner / 0 topic 的空注册表必须被拒——空表不是放行许可证
    let owners = rr_registry::rr3_registry_topic_owners(&topics_toml());
    assert_eq!(owners.len(), 7);
    let owners_only: String = topics_toml()
        .lines()
        .take_while(|l| !l.trim_start().starts_with("[[topic]]"))
        .collect::<Vec<_>>()
        .join("\n");
    let errs = rr_registry::rr3_registry_validate_topics(&owners_only, &owners);
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
    let report = rr_registry::rr3_registry_validate_manifest(&m);
    assert!(
        !report.ok(),
        "空注册表不得放行未注册主题: {:?}",
        report.errors
    );
}

// ---------- 最终整枝评审 C2：append-only 的形状守护 ----------

/// 注册表的全部 24 条 `(name, owner, kind)` 三元组。
/// 顺序即 `contracts/topics.toml` 的登记顺序。
const RR3_PINNED_TOPICS: [(&str, &str, &str); 24] = [
    ("rr3-sense/audio/pcm", "rr3-sense", "stream"),
    ("rr3-sense/audio/vad_start", "rr3-sense", "event"),
    ("rr3-sense/env/temperature", "rr3-sense", "event"),
    ("rr3-sense/imu/state", "rr3-sense", "stream"),
    ("rr3-act/servo/command", "rr3-act", "request"),
    ("rr3-act/servo/state", "rr3-act", "stream"),
    ("rr3-act/motor/reflex", "rr3-act", "notification"),
    ("rr3-act/led/effect", "rr3-act", "request"),
    ("rr3-interact/face/expression", "rr3-interact", "request"),
    ("rr3-interact/tts/text", "rr3-interact", "request"),
    ("rr3-interact/asr/text", "rr3-interact", "event"),
    ("rr3-interact/dialog/turn", "rr3-interact", "event"),
    ("rr3-power/battery/state", "rr3-power", "event"),
    ("rr3-power/charger/state", "rr3-power", "event"),
    ("rr3-connect/net/link", "rr3-connect", "event"),
    ("rr3-connect/cloud/session", "rr3-connect", "request"),
    ("rr3-brain/llm/completion", "rr3-brain", "request"),
    ("rr3-brain/intent/result", "rr3-brain", "event"),
    (
        "rr3-runtime/system/heartbeat",
        "rr3-runtime",
        "notification",
    ),
    ("rr3-runtime/module/state", "rr3-runtime", "notification"),
    ("rr3-sense/capture/dump", "rr3-sense", "request"),
    ("rr3-act/relay/set", "rr3-act", "request"),
    ("rr3-power/battery/low", "rr3-power", "event"),
    ("rr3-runtime/system/power", "rr3-runtime", "notification"),
];

/// 从 topics.toml 文本解析出的 (name, owner, kind) 三元组
fn registry_triples(text: &str) -> Vec<(String, String, String)> {
    fn topic_str(v: &toml::Value, k: &str) -> String {
        v.get(k)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    let root: toml::Value = text.parse().expect("contracts/topics.toml 必须是合法 TOML");
    let arr = root
        .get("topic")
        .and_then(toml::Value::as_array)
        .expect("contracts/topics.toml 缺 [[topic]] 数组");
    arr.iter()
        .map(|e| {
            (
                topic_str(e, "name"),
                topic_str(e, "owner"),
                topic_str(e, "kind"),
            )
        })
        .collect()
}

/// C2：把 `contracts/topics.toml` 的**形状**钉死——owner 数、topic 数、kind 分布，
/// 以及全部 24 条 `(name, owner, kind)` 三元组逐条在册（且不多不少）。
///
/// 计数做成硬 pin 是**故意**的，不是"禁止 append"：契约只增不改，新增主题合法，
/// 但必须是一次**有意识的编辑**——同一次改动里既要改 `contracts/topics.toml`，
/// 也要更新这里的 `RR3_PINNED_TOPICS` / 四个计数期望，以及把这些形状当事实来写的
/// 文档（`docs/architecture/architecture.md` §6.3、`docs/architecture/naming.md` §3、`contracts/README.md`）。
/// 少改任何一处，本用例即红。
///
/// 反过来说，删一条主题、改一条 kind、改一个 owner 数同样会红——评审证明这正是
/// 此前门禁的空洞（只有 owner 数与 fixture 引用的四条主题被钉住）。
#[test]
fn registry_shape_is_pinned() {
    use std::collections::HashSet;

    let text = topics_toml();

    // 1) owner 侧：7 个，且不重名
    let owners = rr_registry::rr3_registry_topic_owners(&text);
    assert_eq!(
        owners.len(),
        7,
        "v0.0.1 契约固定 7 个 owner（5+1 模块 + rrd）"
    );
    let unique_owners: HashSet<&String> = owners.iter().collect();
    assert_eq!(
        unique_owners.len(),
        owners.len(),
        "owner 名不得重复登记: {owners:?}"
    );

    // 2) topic 侧：24 条，且不重名（重名会让下面的子集论证失去意义）
    let topics = registry_triples(&text);
    assert_eq!(
        topics.len(),
        24,
        "契约固定 24 条主题（2026-10-04 Q1/Q4/D3 批次 21→24）"
    );
    let names: HashSet<&str> = topics.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(names.len(), topics.len(), "topic 名不得重复登记");

    // 3) kind 分布：8 request / 9 event / 3 stream / 4 notification
    let count = |want: &str| topics.iter().filter(|t| t.2 == want).count();
    assert_eq!(count("request"), 8, "kind=request 的分布漂移");
    assert_eq!(count("event"), 9, "kind=event 的分布漂移");
    assert_eq!(count("stream"), 3, "kind=stream 的分布漂移");
    assert_eq!(count("notification"), 4, "kind=notification 的分布漂移");
    assert_eq!(
        count("request") + count("event") + count("stream") + count("notification"),
        topics.len(),
        "存在四类之外的 kind: {topics:?}"
    );

    // 4) 24 条三元组逐条在册；配合上面的等长与唯一性，等价于"不多不少"
    for (name, owner, kind) in RR3_PINNED_TOPICS {
        assert!(
            topics
                .iter()
                .any(|t| t.0 == name && t.1 == owner && t.2 == kind),
            "主题三元组缺失或漂移: 期望 ({name}, {owner}, {kind})，实际注册表 {:?}",
            topics
                .iter()
                .filter(|t| t.0 == name)
                .map(|t| (&t.0, &t.1, &t.2))
                .collect::<Vec<_>>()
        );
    }
}

/// modules/ 下每个模块的 module.toml 都必须过同一份 manifest 契约。
/// 骨架可以没有实现，manifest 不可以不合契约——否则要等 rrd 上线才暴露拒注册。
#[test]
fn every_module_manifest_passes_contract() {
    let modules = repo_root().join("modules");
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(&modules)
        .expect("读不到 modules/ 目录")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join("module.toml").is_file())
        .collect();
    dirs.sort();
    let names: Vec<&str> = dirs
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "rr3-act",
            "rr3-brain",
            "rr3-connect",
            "rr3-interact",
            "rr3-power",
            "rr3-sense"
        ],
        "modules/ 的模块集合漂移：新增须走 rr3-mkmod，删除须走 ADR"
    );
    for name in names {
        let report =
            rr_registry::rr3_registry_validate_manifest(&modules.join(name).join("module.toml"));
        assert!(
            report.ok(),
            "{name} 的 module.toml 不合契约: {:?}",
            report.errors
        );
    }
}

/// contracts/topics.toml 每条 owner 的 `path` 必须真的存在。
/// 曾经 6 个 owner 指向不存在的 modules/rr3-*：契约在说谎，且五步检查无人拦截。
#[test]
fn every_owner_path_exists() {
    let root = repo_root();
    let v: toml::Value = toml::from_str(&topics_toml()).expect("topics.toml 必须可解析");
    let owners = v
        .get("owner")
        .and_then(toml::Value::as_array)
        .expect("缺 [[owner]] 数组");
    assert_eq!(
        owners.len(),
        7,
        "owner 计数漂移（注册表形状 pin 之外的交叉检查）"
    );
    for o in owners {
        let name = o
            .get("name")
            .and_then(toml::Value::as_str)
            .expect("owner 缺 name");
        let path = o
            .get("path")
            .and_then(toml::Value::as_str)
            .expect("owner 缺 path");
        assert!(
            !path.starts_with('/') && !path.split('/').any(|s| s == ".."),
            "owner {name} 的 path 必须是仓库内相对路径: {path}"
        );
        assert!(
            root.join(path).is_dir(),
            "owner {name} 登记的 path 不存在: {path}"
        );
    }
}

// ---------- 依赖仲裁（ADR-0009）：跨 manifest 全集层的契约用例 ----------

/// 仲裁只吃 name / provides / depends.required / start_priority 四样事实，
/// 夹具因此只写这四样——其余字段的语法归单件校验，不在本节重复
fn depends_manifest(name: &str, provides: &[&str], requires: &[&str], prio: i64) -> String {
    let list = |v: &[&str]| {
        v.iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "[manifest]\nname = \"{name}\"\n\
         [provides]\ncapabilities = [{}]\n\
         [depends]\nrequired = [{}]\n\
         [resources]\nstart_priority = {prio}\n",
        list(provides),
        list(requires)
    )
}

fn facts_of(texts: &[String]) -> Vec<rr_registry::Rr3DependsFacts> {
    texts
        .iter()
        .map(|t| {
            rr_registry::rr3_registry_depends_facts_from_text(t).expect("夹具事实提取不该失败")
        })
        .collect()
}

#[test]
fn depends_chain_boots_providers_first_with_priority_tie_break() {
    // 链式：c 依赖 b 提供的能力、b 依赖 a 提供的能力 → a、b 必须排在 c 前；
    // 就绪集合里 priority 小者先行（d=40 先于 a=50），同层排序与装载顺序无关
    let order = rr_registry::rr3_registry_resolve_boot_order(&facts_of(&[
        depends_manifest("rr3-cc", &[], &["cap.mid"], 10),
        depends_manifest("rr3-bb", &["cap.mid"], &["cap.low"], 60),
        depends_manifest("rr3-aa", &["cap.low"], &[], 50),
        depends_manifest("rr3-dd", &[], &[], 40),
    ]))
    .expect("链式必须成序");
    assert_eq!(order, vec!["rr3-dd", "rr3-aa", "rr3-bb", "rr3-cc"]);
}

#[test]
fn depends_cycle_rejects_whole_batch() {
    // 环（loop1↔loop2）+ 环下游（down）→ 整批拒且逐个点名；无辜者不得被点名
    let errs = rr_registry::rr3_registry_resolve_boot_order(&facts_of(&[
        depends_manifest("rr3-loop1", &["cap.p"], &["cap.q"], 100),
        depends_manifest("rr3-loop2", &["cap.q"], &["cap.p"], 100),
        depends_manifest("rr3-down", &[], &["cap.p"], 100),
        depends_manifest("rr3-ok", &[], &[], 100),
    ]))
    .expect_err("环必须拒整批");
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("依赖环涉及"), "{errs:?}");
    for name in ["rr3-loop1", "rr3-loop2", "rr3-down"] {
        assert!(errs[0].contains(name), "缺 {name}: {errs:?}");
    }
    assert!(!errs[0].contains("rr3-ok"), "无辜模块被点名: {errs:?}");
}

#[test]
fn depends_base_capability_needs_no_module_provider() {
    // base_capabilities 永远在位（基座不在 boot 序里），不建边、不需要提供者
    let order = rr_registry::rr3_registry_resolve_boot_order(&facts_of(&[depends_manifest(
        "rr3-solo",
        &[],
        &["bus.publish", "net.online"],
        100,
    )]))
    .expect("只依赖 base 能力必须成序");
    assert_eq!(order, vec!["rr3-solo"]);
}

#[test]
fn depends_multiple_providers_all_boot_before_consumer() {
    // 两个提供者并存：启动期全部排前（裁定 2 的保守全序），层内按 priority
    let order = rr_registry::rr3_registry_resolve_boot_order(&facts_of(&[
        depends_manifest("rr3-use", &[], &["cap.x"], 10),
        depends_manifest("rr3-p2", &["cap.x"], &[], 60),
        depends_manifest("rr3-p1", &["cap.x"], &[], 50),
    ]))
    .expect("多提供者必须成序");
    assert_eq!(order, vec!["rr3-p1", "rr3-p2", "rr3-use"]);
}

#[test]
fn every_module_dependency_set_is_satisfiable_and_acyclic() {
    // CI 全集补位：单件校验不再判"未知能力"（ADR-0009 裁定 1），modules/ 六模块
    // 的整体依赖面必须在仲裁层过——否则要等 rrd boot 才暴露
    let modules = repo_root().join("modules");
    let mut facts = Vec::new();
    for entry in std::fs::read_dir(&modules)
        .expect("读不到 modules/ 目录")
        .filter_map(|e| e.ok())
    {
        let m = entry.path().join("module.toml");
        if m.is_file() {
            facts.push(
                rr_registry::rr3_registry_depends_facts_from_text(
                    &std::fs::read_to_string(&m).unwrap(),
                )
                .expect("模块 manifest 事实提取不该失败"),
            );
        }
    }
    assert_eq!(facts.len(), 6, "modules/ 模块集合漂移");
    let order =
        rr_registry::rr3_registry_resolve_boot_order(&facts).expect("六模块依赖面必须可满足且无环");
    assert_eq!(order.len(), 6, "{order:?}");
}

// ---------- 能力格查册门（ADR-0012 裁定 5②）：manifest 与 profile 双侧对 slots.toml 查册 ----------

fn slots_toml() -> String {
    std::fs::read_to_string(repo_root().join("contracts/slots.toml"))
        .expect("contracts/slots.toml 必须存在（ADR-0012 裁定 1 产物）")
}

/// modules/ 六模块的 (模块名, manifest 根 Value)，按名字升序
fn module_manifest_values() -> Vec<(String, toml::Value)> {
    let modules = repo_root().join("modules");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&modules)
        .expect("读不到 modules/ 目录")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
    {
        let m = entry.join("module.toml");
        if m.is_file() {
            let v: toml::Value = toml::from_str(&std::fs::read_to_string(&m).unwrap())
                .expect("module.toml 必须可解析");
            let name = v
                .get("manifest")
                .and_then(|m| m.get("name"))
                .and_then(toml::Value::as_str)
                .expect("manifest 缺 name")
                .to_string();
            out.push((name, v));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn str_list(v: &toml::Value, path: &str) -> Vec<String> {
    let Some(node) = path.split('.').try_fold(v, |acc, seg| acc.get(seg)) else {
        return vec![];
    };
    node.as_array()
        .map(|a| {
            a.iter()
                .filter_map(toml::Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// 基础层 = base_capabilities 的表内化（ADR-0012 裁定 1）：
/// 两份名单逐条钉死等价——第二份白名单不存在，仲裁与查册共用同一 base 真值。
#[test]
fn slots_base_layer_is_pinned_to_base_capabilities() {
    let mut base =
        rr_registry::rr3_registry_slot_table_from_text(&slots_toml()).rr3_slot_table_base();
    base.sort();
    let mut caps = rr_registry::rr3_registry_rule_list("base_capabilities")
        .expect("module.schema.toml 必须登记 base_capabilities 规则");
    caps.sort();
    assert_eq!(base, caps, "slots 基础层与 base_capabilities 漂移");
}

/// 格位表的全部 21 条 `(name, layer)`，顺序即 `contracts/slots.toml` 登记顺序。
/// 与 `registry_shape_is_pinned` 同一钉法：append 合法但必须是有意识的编辑——
/// 同一次改动里同步更新 `contracts/slots.toml`、本常量与层分布计数、
/// `profiles/default.toml`（如涉及绑定面），以及把这些数字当事实抄进正文的文档。
const RR3_PINNED_SLOTS: [(&str, &str); 21] = [
    ("bus.publish", "base"),
    ("bus.subscribe", "base"),
    ("bus.rpc", "base"),
    ("net.online", "base"),
    ("power.battery", "base"),
    ("act.servo", "body"),
    ("act.led", "body"),
    ("act.relay", "body"),
    ("net.wifi", "body"),
    ("net.bt", "body"),
    ("ui.face", "body"),
    ("audio.playback", "body"),
    ("audio.capture", "body"),
    ("audio.vad", "body"),
    ("sensor.env", "body"),
    ("sensor.imu", "body"),
    ("power.charger", "body"),
    ("cognition.dialog", "cognition"),
    ("cognition.asr", "cognition"),
    ("cognition.llm", "cognition"),
    ("cognition.tts", "cognition"),
];

#[test]
fn slots_registry_shape_is_pinned() {
    use std::collections::BTreeMap;

    let table = rr_registry::rr3_registry_slot_table_from_text(&slots_toml());
    assert!(
        table.rr3_slot_table_check().is_empty(),
        "{:?}",
        table.rr3_slot_table_check()
    );

    // 1) 21 格、逐条 (name, layer) 在册且不多不少（配合唯一性与等长断言）
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (name, layer) in RR3_PINNED_SLOTS {
        assert_eq!(
            table.rr3_slot_table_layer(name).as_deref(),
            Some(layer),
            "格位缺失或层漂移: {name}"
        );
        *counts.entry(layer).or_insert(0) += 1;
    }
    assert_eq!(
        table.rr3_slot_table_names().len(),
        RR3_PINNED_SLOTS.len(),
        "格位总数漂移"
    );

    // 2) 层分布 5/12/4
    assert_eq!(counts.get("base"), Some(&5));
    assert_eq!(counts.get("body"), Some(&12));
    assert_eq!(counts.get("cognition"), Some(&4));
}

/// 门禁②（manifest 侧）：provides 与 depends.* 引用的每个能力必须命中格位表。
/// 单件校验只保语法（ADR-0009 裁定 1），"是否在册"只在全集层判——本用例就是那道全集门。
#[test]
fn every_manifest_capability_is_a_registered_slot() {
    let table = rr_registry::rr3_registry_slot_table_from_text(&slots_toml());
    let manifests = module_manifest_values();
    assert_eq!(manifests.len(), 6, "modules/ 模块集合漂移");
    for (name, v) in &manifests {
        let mut caps = str_list(v, "provides.capabilities");
        caps.extend(str_list(v, "depends.required"));
        caps.extend(str_list(v, "depends.optional"));
        assert!(
            !caps.is_empty(),
            "{name} 的能力面为空（provides 或 depends 至少登记一项）"
        );
        for cap in caps {
            assert!(
                table.rr3_slot_table_contains(&cap),
                "{name} 引用了表外能力 {cap}（新能力先立格再用，ADR-0012 裁定 1）"
            );
        }
    }
}

/// 门禁②（profile 侧）+ 裁定 3 的装配自洽：default 装配单的 bindings 逐格对表查册、
/// 绑到真实模块、与该模块的 manifest provides 对账；faces 与 provides 全集相等；
/// 全部非 base 格位恰好被绑定一次（default 机不留缺格）。
#[test]
fn default_profile_binds_module_slots_to_real_modules() {
    let root = repo_root();
    let table = rr_registry::rr3_registry_slot_table_from_text(&slots_toml());
    let profiles = root.join("profiles");
    let mut profile_files: Vec<String> = std::fs::read_dir(&profiles)
        .expect("profiles/ 目录必须存在（ADR-0012 裁定 3 产物）")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_str().unwrap().to_string())
        .filter(|n| n.ends_with(".toml"))
        .collect();
    profile_files.sort();
    assert_eq!(
        profile_files,
        vec!["default.toml".to_string(), "distro.toml".to_string()],
        "profiles/ 机型集合漂移"
    );

    let text = std::fs::read_to_string(profiles.join("default.toml")).unwrap();
    let v: toml::Value = toml::from_str(&text).expect("default.toml 必须可解析");

    // [os] route 双轨取值（ADR-0013）
    let route = v
        .get("os")
        .and_then(|o| o.get("route"))
        .and_then(toml::Value::as_str)
        .expect("profile 缺 [os] route");
    assert!(
        route == "distro" || route == "buildroot",
        "route 越界: {route}"
    );

    let manifests: std::collections::BTreeMap<String, toml::Value> =
        module_manifest_values().into_iter().collect();

    // [bindings]：格位 → 件
    let bindings = v
        .get("bindings")
        .and_then(toml::Value::as_table)
        .expect("缺 [bindings]");
    let mut bound: Vec<String> = Vec::new();
    for (slot, provider) in bindings {
        let provider = provider
            .as_str()
            .unwrap_or_else(|| panic!("bindings.{slot} 非字符串"));
        assert!(
            table.rr3_slot_table_contains(slot),
            "bindings 引用表外格位 {slot}"
        );
        let layer = table.rr3_slot_table_layer(slot).unwrap();
        assert_ne!(
            layer, "base",
            "基础层格位 {slot} 由基座内建承载，不得绑定模块"
        );
        let m = manifests
            .get(provider)
            .unwrap_or_else(|| panic!("bindings.{slot} 绑到不在册模块 {provider}"));
        assert!(
            str_list(m, "provides.capabilities")
                .iter()
                .any(|c| c == slot),
            "{provider} 的 manifest 未 provides 被绑格位 {slot}"
        );
        bound.push(slot.clone());
    }

    // 全部非 base 格位恰好绑定一次（配合 BTreeMap 键唯一即"不多不少"）
    bound.sort();
    let mut unbound: Vec<String> = table
        .rr3_slot_table_names()
        .into_iter()
        .filter(|n| table.rr3_slot_table_layer(n).as_deref() != Some("base"))
        .filter(|n| !bound.iter().any(|b| b == n))
        .collect();
    assert!(
        unbound.is_empty(),
        "default 机存在缺格（fail-closed，缺格即 Degraded）: {unbound:?}"
    );
    unbound.clear();

    // [faces]：模块 → 格位集合 == manifest provides 全集
    let faces = v
        .get("faces")
        .and_then(toml::Value::as_table)
        .expect("缺 [faces]");
    let mut face_modules: Vec<&String> = faces.keys().collect();
    face_modules.sort();
    let mut manifest_modules: Vec<&String> = manifests.keys().collect();
    manifest_modules.sort();
    assert_eq!(
        face_modules, manifest_modules,
        "[faces] 覆盖的模块集合与 modules/ 漂移"
    );
    for (module, face) in faces {
        let mut slots: Vec<String> = face
            .as_array()
            .expect("faces 值必须是数组")
            .iter()
            .filter_map(toml::Value::as_str)
            .map(String::from)
            .collect();
        slots.sort();
        let mut caps = str_list(&manifests[module], "provides.capabilities");
        caps.sort();
        assert_eq!(slots, caps, "{module} 的 [faces] 与 manifest provides 漂移");
        for s in &slots {
            assert!(table.rr3_slot_table_contains(s), "[faces] 引用表外格位 {s}");
        }
    }
}

/// 门禁②（profile 侧）+ 0.0.2 T5 的 distro 装配单对账：快路机型的绑定面**恰好是
/// 身体层**（非 base 未绑格位 == cognition.\* 四格，即 rr3-brain 的 provides 全集），
/// faces 仍覆盖全部在册模块且与 provides 全集相等——"faces 保留声明、格位不绑定"
/// 正是快路 payload 缺 Python brain 的诚实表达（P2：缺格点名降级，不拒 boot；
/// CI distro-smoke 的断言锚点与这份装配单同源）。
#[test]
fn distro_profile_binds_exactly_the_body_layer() {
    let root = repo_root();
    let table = rr_registry::rr3_registry_slot_table_from_text(&slots_toml());
    let manifests: std::collections::BTreeMap<String, toml::Value> =
        module_manifest_values().into_iter().collect();
    let text = std::fs::read_to_string(root.join("profiles/distro.toml")).unwrap();
    let v: toml::Value = toml::from_str(&text).expect("distro.toml 必须可解析");

    let route = v
        .get("os")
        .and_then(|o| o.get("route"))
        .and_then(toml::Value::as_str)
        .expect("profile 缺 [os] route");
    assert_eq!(route, "distro", "distro 机 route 必须是快路");

    let bindings = v
        .get("bindings")
        .and_then(toml::Value::as_table)
        .expect("缺 [bindings]");
    for (slot, provider) in bindings {
        let provider = provider
            .as_str()
            .unwrap_or_else(|| panic!("bindings.{slot} 非字符串"));
        assert!(
            table.rr3_slot_table_contains(slot),
            "bindings 引用表外格位 {slot}"
        );
        assert_ne!(
            table.rr3_slot_table_layer(slot).unwrap(),
            "base",
            "基础层格位 {slot} 由基座内建承载，不得绑定模块"
        );
        let m = manifests
            .get(provider)
            .unwrap_or_else(|| panic!("bindings.{slot} 绑到不在册模块 {provider}"));
        assert!(
            str_list(m, "provides.capabilities")
                .iter()
                .any(|c| c == slot),
            "{provider} 的 manifest 未 provides 被绑格位 {slot}"
        );
    }

    // 未绑定的非 base 格位恰好 = rr3-brain 的 provides 全集（快路缺格的精确边界：
    // 一格不多——身体层全在位；一格不少——brain 四格全缺）
    let mut unbound: Vec<String> = table
        .rr3_slot_table_names()
        .into_iter()
        .filter(|n| table.rr3_slot_table_layer(n).as_deref() != Some("base"))
        .filter(|n| !bindings.contains_key(n))
        .collect();
    unbound.sort();
    let mut brain_caps = str_list(&manifests["rr3-brain"], "provides.capabilities");
    brain_caps.sort();
    assert_eq!(
        unbound, brain_caps,
        "distro 机缺格边界漂移：未绑格位必须恰为 rr3-brain 的 provides 全集"
    );

    // faces 仍覆盖全部在册模块（含 brain——缺格声明要可点名），且与 provides 全集相等
    let faces = v
        .get("faces")
        .and_then(toml::Value::as_table)
        .expect("缺 [faces]");
    let mut face_modules: Vec<&String> = faces.keys().collect();
    face_modules.sort();
    let mut manifest_modules: Vec<&String> = manifests.keys().collect();
    manifest_modules.sort();
    assert_eq!(
        face_modules, manifest_modules,
        "[faces] 覆盖的模块集合与 modules/ 漂移"
    );
    for (module, face) in faces {
        let mut slots: Vec<String> = face
            .as_array()
            .expect("faces 值必须是数组")
            .iter()
            .filter_map(toml::Value::as_str)
            .map(String::from)
            .collect();
        slots.sort();
        let mut caps = str_list(&manifests[module], "provides.capabilities");
        caps.sort();
        assert_eq!(slots, caps, "{module} 的 [faces] 与 manifest provides 漂移");
        for s in &slots {
            assert!(table.rr3_slot_table_contains(s), "[faces] 引用表外格位 {s}");
        }
    }
}
