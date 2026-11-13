//! 主题前缀规则：owner 命名空间、未注册 owner、契约自洽。

use super::*;

#[test]
fn topics_prefix_accepts_canonical_topic() {
    let owners = rr_registry::rr3_topics_owners(&topics_toml());
    let errs = rr_registry::rr3_topics_chk(
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
    let owners = rr_registry::rr3_topics_owners(&topics_toml());
    let errs = rr_registry::rr3_topics_chk(
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
    let owners = rr_registry::rr3_topics_owners(&topics_toml());
    let errs = rr_registry::rr3_topics_chk(
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
    let owners = rr_registry::rr3_topics_owners(&topics_toml());
    let errs = rr_registry::rr3_topics_chk(
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
    let owners = rr_registry::rr3_topics_owners(&topics_toml());
    assert_eq!(
        owners.len(),
        8,
        "契约 8 个 owner（5+1 模块 + rrd + rr3-sentinel，0.0.7 批E append）"
    );
    let errs = rr_registry::rr3_topics_chk(&topics_toml(), &owners);
    assert!(
        errs.is_empty(),
        "contracts/topics.toml 自身必须自洽: {errs:?}"
    );
}
