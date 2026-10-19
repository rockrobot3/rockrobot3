//! 主题集：owner + topic 名 + kind + status 的集合视图。
//!
//! 生产真相只有 `contracts/topics.toml`。`rr3_topic_set_merge` 只服务**测试叠加层**
//! （spec §6 / R6）：叠加层不得覆盖生产条目，topic 名撞车即整册作废并点名冲突。
//! 空主题集是契约事故，不是放行许可证。
//!
//! `status`（0.0.5 批J C14 首例退役）：可省字段，缺省 = 未标记（语义"正常"）；
//! 显式 `retired` = 号位保留、义务解除——申报面（publishes/rpc.exposes/subscribes）
//! 引用即拒，门在 `validate.rs`（ADR-0019 修订 2 ②）。status 值域与 slots.toml 同一三态。

use std::collections::BTreeMap;
use toml::Value;

/// 合法主题状态（与 slots.toml 三态同轴；缺省未标记 = 正常语义）
const RR3_TOPIC_STATUS: [&str; 3] = ["normal", "placeholder", "retired"];

/// 一册主题集（不可变快照）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3TopicSet {
    topics: BTreeMap<String, String>,   // topic -> owner
    kinds: BTreeMap<String, String>,    // topic -> kind
    statuses: BTreeMap<String, String>, // topic -> status（只收显式标记）
    owners: Vec<String>,                // 升序去重
}

fn entries<'a>(root: &'a Value, key: &str) -> impl Iterator<Item = &'a toml::Table> {
    root.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_table)
}

/// 解析一份主题文本（`contracts/topics.toml` 与叠加层同构，共用同一解析器，不写第二套）
pub fn rr3_registry_topic_set_from_text(text: &str) -> Rr3TopicSet {
    let Ok(root) = toml::from_str::<Value>(text) else {
        return Rr3TopicSet::default();
    };
    let mut set = Rr3TopicSet::default();
    for o in entries(&root, "owner") {
        if let Some(n) = o.get("name").and_then(Value::as_str) {
            set.owners.push(n.to_string());
        }
    }
    for t in entries(&root, "topic") {
        let Some(name) = t.get("name").and_then(Value::as_str) else {
            continue;
        };
        set.topics.insert(
            name.to_string(),
            t.get("owner")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        );
        set.kinds.insert(
            name.to_string(),
            t.get("kind")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        );
        if let Some(st) = t.get("status").and_then(Value::as_str) {
            if !st.is_empty() {
                set.statuses.insert(name.to_string(), st.to_string());
            }
        }
    }
    set.owners.sort();
    set.owners.dedup();
    set
}

impl Rr3TopicSet {
    pub fn rr3_topic_set_is_empty(&self) -> bool {
        self.topics.is_empty()
    }

    pub fn rr3_topic_set_contains(&self, topic: &str) -> bool {
        self.topics.contains_key(topic)
    }

    /// 该 topic 的在册 owner（不在册 = None）
    pub fn rr3_topic_set_owner(&self, topic: &str) -> Option<String> {
        self.topics.get(topic).cloned()
    }

    /// 该 topic 的在册 kind（不在册 = None）。
    /// 存在的唯一理由是让装载处能拿**契约原值**复验 merge：`rr3_topic_set_check` 只问
    /// kind 在不在 `[rules] topic_allowed_kinds` 白名单里，不问它有没有被改写，
    /// 光靠形状自检抓不到"stream 被换成 event"这种退化（消费方见 rrd 的
    /// `reject_inconsistent_merge`）。
    pub fn rr3_topic_set_kind(&self, topic: &str) -> Option<String> {
        self.kinds.get(topic).cloned()
    }

    /// 该 topic 的显式状态标记（不在册或未标记 = None）。
    /// 消费方：validate.rs 的申报门（retired 主题不得被申报发布/订阅/暴露）。
    pub fn rr3_topic_set_status(&self, topic: &str) -> Option<String> {
        self.statuses.get(topic).cloned()
    }

    pub fn rr3_topic_set_names(&self) -> Vec<String> {
        self.topics.keys().cloned().collect()
    }

    /// 叠加合并：撞名即 Err(冲突条目列表)，不改 self
    pub fn rr3_topic_set_merge(&self, overlay: &Rr3TopicSet) -> Result<Rr3TopicSet, Vec<String>> {
        let mut conflicts: Vec<String> = self
            .topics
            .keys()
            .filter(|k| overlay.topics.contains_key(*k))
            .cloned()
            .collect();
        if !conflicts.is_empty() {
            conflicts.sort();
            return Err(conflicts);
        }
        let mut merged = self.clone();
        merged.topics.extend(overlay.topics.clone());
        merged.kinds.extend(overlay.kinds.clone());
        merged.statuses.extend(overlay.statuses.clone());
        merged.owners.extend(overlay.owners.iter().cloned());
        merged.owners.sort();
        merged.owners.dedup();
        Ok(merged)
    }

    /// 自身形状检查：复用 `rr3_registry_check("topic", …)` 与 `[rules] topic_allowed_kinds`；
    /// status 显式标记时值域必须合法（与 slots.toml 同一三态）
    pub fn rr3_topic_set_check(&self) -> Vec<String> {
        if self.rr3_topic_set_is_empty() {
            return vec!["主题集为空：必须至少登记一个 [[topic]]（ADR-0006）".to_string()];
        }
        let allowed =
            crate::schema::rr3_registry_rule_list("topic_allowed_kinds").unwrap_or_default();
        let mut errs = Vec::new();
        for (name, owner) in &self.topics {
            errs.extend(crate::validate::rr3_registry_check("topic", name));
            let kind = self.kinds.get(name).cloned().unwrap_or_default();
            if !self.owners.iter().any(|o| o == owner) {
                errs.push(format!("topic {name} 的 owner 未登记: {owner}"));
            } else if name.split('/').next().unwrap_or("") != owner.as_str() {
                errs.push(format!("topic {name} 不在 owner {owner} 命名空间内"));
            }
            if !allowed.iter().any(|k| k == &kind) {
                errs.push(format!("topic {name} 的 kind 非法: {kind}"));
            }
        }
        for (name, status) in &self.statuses {
            if !RR3_TOPIC_STATUS.contains(&status.as_str()) {
                errs.push(format!(
                    "topic {name} 的 status 非法: {status}（只允许 {RR3_TOPIC_STATUS:?}）"
                ));
            }
        }
        errs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // include_str! 以本文件所在目录为基准：src → rr-registry → runtime → base → 仓库根
    const CONTRACT: &str = include_str!("../../../../contracts/topics.toml");
    // 叠加层夹具落在 workspace 根的 tests/fixtures/（刻意不进 modules/、不进 contracts/）
    const OVERLAY: &str = include_str!("../../tests/fixtures/sim-topics.toml");

    /// 契约必须解析成 8 owner / 32 topic（与 contract.rs 的形状 pin 同源、彼此独立）
    #[test]
    fn contract_text_parses_into_expected_shape() {
        let set = rr3_registry_topic_set_from_text(CONTRACT);
        assert_eq!(set.topics.len(), 32, "契约 topic 数（批N4 件六二 31→32：append rr3-brain/ops/list，B-12）");
        assert_eq!(set.owners.len(), 8, "契约 owner 数");
        assert!(!set.rr3_topic_set_is_empty());
        assert!(set.rr3_topic_set_contains("rr3-runtime/module/state"));
        assert!(!set.rr3_topic_set_contains("rr3-sima/ping/state"));
        assert_eq!(
            set.rr3_topic_set_owner("rr3-sense/audio/pcm").as_deref(),
            Some("rr3-sense")
        );
        assert!(
            set.rr3_topic_set_check().is_empty(),
            "{:?}",
            set.rr3_topic_set_check()
        );
    }

    /// 坏文本给空集，空集必须报错（fail-open 护栏）
    #[test]
    fn unparsable_text_is_not_a_pass_port() {
        let set = rr3_registry_topic_set_from_text("this = is not = valid = toml");
        assert!(set.rr3_topic_set_is_empty());
        assert!(!set.rr3_topic_set_check().is_empty());
    }

    /// 合法叠加层只增不改，生产条目的 owner 仍以契约为准
    #[test]
    fn merge_appends_without_touching_contract_entries() {
        let base = rr3_registry_topic_set_from_text(CONTRACT);
        let merged = base
            .rr3_topic_set_merge(&rr3_registry_topic_set_from_text(OVERLAY))
            .expect("合并");
        assert_eq!(
            merged.topics.len(),
            35,
            "32 契约 + 3 夹具（0.0.5 第 IV 轮 MCP-0 契约 29→30：append brain/audio/tts_pcm；增补轮六 批N2 件二 30→31：append rr3-power/rail/state；批N4 件六二 31→32：append rr3-brain/ops/list，B-12）"
        );
        assert_eq!(merged.owners.len(), 10);
        assert_eq!(
            merged.rr3_topic_set_owner("rr3-sense/audio/pcm").as_deref(),
            Some("rr3-sense")
        );
        // kind 走同一条"只增不改"承诺：契约条目的 kind 必须以契约原值回、叠加层的 kind 必须
        // 带得过来、不在册的必须回 None——rrd 装载处的 kind 复验就建在这三个事实上。
        assert_eq!(
            merged.rr3_topic_set_kind("rr3-sense/audio/pcm").as_deref(),
            Some("stream")
        );
        assert_eq!(
            merged.rr3_topic_set_kind("rr3-sima/ping/state").as_deref(),
            Some("event")
        );
        assert_eq!(
            merged.rr3_topic_set_kind("rr3-nobody/x/y"),
            None,
            "不在册 topic 的 kind 是 None 而非空串"
        );
        assert!(merged.rr3_topic_set_contains("rr3-sima/ping/state"));
        // self 未被改动
        assert_eq!(base.topics.len(), 32, "契约计数（批N4 件六二 31→32）");
        assert_eq!(
            base.rr3_topic_set_kind("rr3-sima/ping/state"),
            None,
            "merge 不得把叠加层的 kind 写回 self"
        );
    }

    /// 撞名必须 Err 并点名条目（测试注入不得改写生产契约的 owner/kind）
    #[test]
    fn merge_rejects_and_names_topic_conflict() {
        let evil = rr3_registry_topic_set_from_text(
            r#"
[[owner]]
name = "rr3-sima"
[[topic]]
name = "rr3-sense/audio/pcm"
kind = "event"
owner = "rr3-sima"
"#,
        );
        let errs = rr3_registry_topic_set_from_text(CONTRACT)
            .rr3_topic_set_merge(&evil)
            .expect_err("撞名必须 Err");
        assert_eq!(errs, vec!["rr3-sense/audio/pcm".to_string()]);
    }

    /// 叠加层自身形状：未登记 owner / 非法 kind 都被同一条规则函数抓出
    #[test]
    fn overlay_shape_uses_the_same_rules() {
        let bad = rr3_registry_topic_set_from_text(
            r#"
[[owner]]
name = "rr3-sima"
[[topic]]
name = "rr3-simb/ping/state"
kind = "telepathy"
owner = "rr3-simb"
"#,
        );
        let errs = bad.rr3_topic_set_check();
        assert!(errs.iter().any(|e| e.contains("owner 未登记")), "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("kind 非法")), "{errs:?}");
    }

    /// C14 首例退役（0.0.5 批J）：asr/text 与 dialog/turn 显式 status="retired"，
    /// 号位保留、义务解除；其余 26 条无显式标记。status 随 merge 带得过来。
    #[test]
    fn contract_pins_two_retired_topics() {
        let set = rr3_registry_topic_set_from_text(CONTRACT);
        assert_eq!(
            set.rr3_topic_set_status("rr3-interact/asr/text").as_deref(),
            Some("retired"),
            "asr/text 必须显式 retired（号位保留、义务解除）"
        );
        assert_eq!(
            set.rr3_topic_set_status("rr3-interact/dialog/turn")
                .as_deref(),
            Some("retired")
        );
        assert_eq!(
            set.rr3_topic_set_status("rr3-sense/audio/pcm"),
            None,
            "未标记主题 = None（正常语义），不是空串"
        );
        assert!(
            set.rr3_topic_set_check().is_empty(),
            "{:?}",
            set.rr3_topic_set_check()
        );

        let merged = set
            .rr3_topic_set_merge(&rr3_registry_topic_set_from_text(OVERLAY))
            .expect("合并");
        assert_eq!(
            merged
                .rr3_topic_set_status("rr3-interact/asr/text")
                .as_deref(),
            Some("retired"),
            "retired 标记必须随 merge 带过来（申报门在合并后的表上执法）"
        );
    }

    /// 非法 status 值必须被形状检查点名
    #[test]
    fn bad_topic_status_is_named() {
        let bad = rr3_registry_topic_set_from_text(
            r#"
[[owner]]
name = "rr3-sima"
[[topic]]
name = "rr3-sima/ping/state"
kind = "event"
owner = "rr3-sima"
status = "archived"
"#,
        );
        let errs = bad.rr3_topic_set_check();
        assert!(
            errs.iter().any(|e| e.contains("status 非法: archived")),
            "{errs:?}"
        );
    }

    /// 随仓库提交的夹具叠加层必须自检通过且与契约零撞名
    #[test]
    fn committed_overlay_fixture_is_self_consistent() {
        let set = rr3_registry_topic_set_from_text(OVERLAY);
        assert!(
            set.rr3_topic_set_check().is_empty(),
            "{:?}",
            set.rr3_topic_set_check()
        );
        assert_eq!(set.topics.len(), 3);
        rr3_registry_topic_set_from_text(CONTRACT)
            .rr3_topic_set_merge(&set)
            .expect("夹具叠加层不得与契约撞名");
    }
}
