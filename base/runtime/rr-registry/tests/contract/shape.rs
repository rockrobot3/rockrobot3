//! append-only 形状守护：42 条主题三元组 pin、退役主题、全量 manifest 过契约。

use super::*;

// ---------- 最终整枝评审 C2：append-only 的形状守护 ----------

/// 注册表的全部 42 条 `(name, owner, kind)` 三元组。
/// 顺序即 `contracts/topics.toml` 的登记顺序。
const RR3_PINNED_TOPICS: [(&str, &str, &str); 42] = [
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
    ("rr3-brain/audio/tts_pcm", "rr3-brain", "stream"),
    (
        "rr3-runtime/system/heartbeat",
        "rr3-runtime",
        "notification",
    ),
    ("rr3-runtime/module/state", "rr3-runtime", "notification"),
    ("rr3-sense/capture/dump", "rr3-sense", "request"),
    ("rr3-act/relay/set", "rr3-act", "request"),
    ("rr3-power/battery/low", "rr3-power", "event"),
    ("rr3-power/battery/ok", "rr3-power", "event"),
    ("rr3-runtime/system/power", "rr3-runtime", "notification"),
    ("rr3-act/relay/state", "rr3-act", "event"),
    ("rr3-connect/wifi/scan", "rr3-connect", "request"),
    ("rr3-connect/wifi/connect", "rr3-connect", "request"),
    ("rr3-connect/wifi/forget", "rr3-connect", "request"),
    ("rr3-sentinel/diag/probe", "rr3-sentinel", "request"),
    ("rr3-power/rail/state", "rr3-power", "event"),
    // 批N4 件六二/九二/九三 append 的七条（当批只改了计数 pin、漏了本数组——
    // 批N5 件二笔3 随 battery/ok 入表同批补齐，恢复"逐条在册且不多不少"的等价）
    ("rr3-brain/ops/list", "rr3-brain", "request"),
    ("rr3-act/ops/list", "rr3-act", "request"),
    ("rr3-connect/ops/list", "rr3-connect", "request"),
    ("rr3-interact/ops/list", "rr3-interact", "request"),
    ("rr3-power/ops/list", "rr3-power", "request"),
    ("rr3-brain/ops/set", "rr3-brain", "request"),
    // ADR-0023 模型行面三件（列表/CRUD/连通性测试）
    ("rr3-brain/model/list", "rr3-brain", "request"),
    ("rr3-brain/model/set", "rr3-brain", "request"),
    ("rr3-brain/model/test", "rr3-brain", "request"),
    ("rr3-sense/ops/list", "rr3-sense", "request"),
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
/// 以及全部 39 条 `(name, owner, kind)` 三元组逐条在册（且不多不少）。
///
/// 计数做成硬 pin 是**故意**的，不是"禁止 append"：契约只增不改，新增主题合法，
/// 但必须是一次**有意识的编辑**——同一次改动里既要改 `contracts/topics.toml`，
/// 也要更新这里的 `RR3_PINNED_TOPICS` / 四个计数期望，以及把这些形状当事实来写的
/// 文档（`docs/architecture/architecture.md` §6.3、`docs/architecture/naming.md` §3、`contracts/README.md`）。
/// 少改任何一处，本用例即红。
///（注：本数组在批N4 件六二/九二/九三只改了计数 pin、漏了随批补条——批N5 件二笔3
/// 补齐七条后重新与 topics.toml 等长，"不多不少"的等价自此恢复。）
///
/// 反过来说，删一条主题、改一条 kind、改一个 owner 数同样会红——评审证明这正是
/// 此前门禁的空洞（只有 owner 数与 fixture 引用的四条主题被钉住）。
#[test]
fn registry_shape_is_pinned() {
    use std::collections::HashSet;

    let text = topics_toml();

    // 1) owner 侧：8 个，且不重名
    let owners = rr_registry::rr3_topics_owners(&text);
    assert_eq!(
        owners.len(),
        8,
        "契约 8 个 owner（5+1 模块 + rrd + rr3-sentinel，2026-10-11 0.0.7 批E append）"
    );
    let unique_owners: HashSet<&String> = owners.iter().collect();
    assert_eq!(
        unique_owners.len(),
        owners.len(),
        "owner 名不得重复登记: {owners:?}"
    );

    // 2) topic 侧：与钉数单点一致，且不重名（重名会让下面的子集论证失去意义）
    let topics = registry_triples(&text);
    assert_eq!(
        topics.len(),
        rr_registry::RR3_CONTRACT_TOPIC_COUNT,
        "契约主题数与钉数单点不一致（改 contracts/topics.toml 须同批改 RR3_CONTRACT_TOPIC_COUNT）"
    );
    let names: HashSet<&str> = topics.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(names.len(), topics.len(), "topic 名不得重复登记");

    // 3) kind 分布：钉数 request，其余三类按契约现状
    let count = |want: &str| topics.iter().filter(|t| t.2 == want).count();
    assert_eq!(
        count("request"),
        rr_registry::RR3_CONTRACT_REQUEST_COUNT,
        "kind=request 分布与钉数单点不一致（改契约须同批改 RR3_CONTRACT_REQUEST_COUNT）"
    );
    assert_eq!(
        count("event"),
        12,
        "kind=event 的分布漂移（批N5 件二笔3 11→12：append rr3-power/battery/ok）"
    );
    assert_eq!(count("stream"), 4, "kind=stream 的分布漂移");
    assert_eq!(count("notification"), 4, "kind=notification 的分布漂移");
    assert_eq!(
        count("request") + count("event") + count("stream") + count("notification"),
        topics.len(),
        "存在四类之外的 kind: {topics:?}"
    );

    // 4) 39 条三元组逐条在册；配合上面的等长与唯一性，等价于"不多不少"
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

/// C14（ADR-0019 修订 2 ②，2026-10-10 评审裁定）：retired 主题**号位保留、义务解除**——
/// asr/text 与 dialog/turn 仍在册（pin 住号位），但申报面（publishes/rpc.exposes/subscribes）
/// 引用它们必被拒；未标记状态的主题 = None（正常语义）。
#[test]
fn retired_topics_are_pinned_and_undeclarable() {
    // ① 号位保留：两条 retired 主题仍在册；普通主题未标记 = None
    let set = rr_registry::rr3_topic_parse(&topics_toml());
    assert_eq!(
        set.rr3_topic_status("rr3-interact/asr/text").as_deref(),
        Some("retired"),
        "asr/text 的 retired 标记漂移"
    );
    assert_eq!(
        set.rr3_topic_status("rr3-interact/dialog/turn").as_deref(),
        Some("retired"),
        "dialog/turn 的 retired 标记漂移"
    );
    assert_eq!(
        set.rr3_topic_status("rr3-sense/audio/pcm"),
        None,
        "未标记主题必须读出 None（正常语义）"
    );

    // ② 申报门 CI 层反例：rr3-interact 发布 asr/text（前缀归属合法）但主题已
    // retired → 逐条点名拒。申报撤除（批J C14③④）后任何人不得再挂回。
    let m = staged(
        "retired",
        fixture_text()
            .replace("name = \"rr3-sense\"", "name = \"rr3-interact\"")
            .replace(
                "publishes = [\"rr3-sense/audio/vad_start\", \"rr3-sense/env/temperature\"]",
                "publishes = [\"rr3-interact/asr/text\"]",
            ),
    );
    let report = rr_registry::rr3_manifest_gate(&m);
    assert!(
        report.errors.iter().any(|e| e.contains("retired 主题")),
        "发布 retired 主题必须被拒: {:?}",
        report.errors
    );
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
        let report = rr_registry::rr3_manifest_gate(&modules.join(name).join("module.toml"));
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
        8,
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
