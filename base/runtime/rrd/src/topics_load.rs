//! 所有权表装载：`contracts/topics.toml` 全集（生产真相）+ 仅测试的叠加层。
//!
//! 叠加层不是生产放行口（spec §6 / R6）：默认不加载；加载时由 main 打印绝对路径与
//! 新增条目清单；与契约 topic 撞名 → 拒绝启动。

use std::path::Path;

use rr_registry::{rr3_registry_topic_set_from_text, Rr3TopicSet};

/// 内建命名空间：`rr3-runtime/*` 两个主题属 rrd 的下行广播，模块上行冒充必须被拒（spec §5.0）
pub const RR3_RESERVED_OWNER: &str = "rr3-runtime";

/// 契约主题的编译期定位（与 rr-ipc 测试同深度：base/runtime/<crate> → 仓库根）
pub const RR3_CONTRACT_TOPICS_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../../contracts/topics.toml");

fn read_set(path: &Path, role: &str) -> Result<Rr3TopicSet, Vec<String>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| vec![format!("读不到{role} {}: {e}", path.display())])?;
    let set = rr3_registry_topic_set_from_text(&text);
    if set.rr3_topic_set_is_empty() {
        return Err(vec![format!("{role} {} 解析后为空，拒绝启动", path.display())]);
    }
    let errs = set.rr3_topic_set_check();
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(set)
}

/// 装载：契约 →（可选）叠加层合并。返回主题集与"叠加层新增的条目清单"。
pub fn rr3_topics_load(
    contract: &Path,
    overlay: Option<&Path>,
) -> Result<(Rr3TopicSet, Vec<String>), Vec<String>> {
    let base = read_set(contract, "主题契约")?;
    let Some(ov) = overlay else { return Ok((base, Vec::new())) };
    let ov_set = read_set(ov, "主题叠加层")?;
    let added = ov_set.rr3_topic_set_names();
    let merged = base.rr3_topic_set_merge(&ov_set)?; // 撞名 → Err(冲突条目)
    Ok((merged, added))
}

/// 首段是否落在内建命名空间（冒充面）
pub fn rr3_topics_is_reserved_topic(topic: &str) -> bool {
    topic.split('/').next().unwrap_or("") == RR3_RESERVED_OWNER
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn contract() -> std::path::PathBuf {
        Path::new(RR3_CONTRACT_TOPICS_PATH).to_path_buf()
    }

    /// 真·夹具叠加层（随仓库提交的那份）：rrd 的上一级即 base/runtime/tests/fixtures
    fn fixture_overlay() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/sim-topics.toml")
    }

    fn tmp_named(tag: &str, text: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("rr3-topics-{}-{tag}.toml", std::process::id()));
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(text.as_bytes()).unwrap();
        p
    }

    /// 不带叠加层 = 生产面，21 条主题一个不多
    #[test]
    fn contract_only_load_is_the_production_surface() {
        let (set, added) = rr3_topics_load(&contract(), None).expect("契约装载应成功");
        assert_eq!(set.rr3_topic_set_names().len(), 21);
        assert!(added.is_empty(), "没传叠加层就不该有新增条目");
        assert!(set.rr3_topic_set_contains("rr3-runtime/system/heartbeat"));
    }

    #[test]
    fn overlay_adds_fixture_topics_and_lists_them() {
        let (set, added) = rr3_topics_load(&contract(), Some(&fixture_overlay())).expect("夹具应能合并");
        assert_eq!(set.rr3_topic_set_names().len(), 24, "21 契约 + 3 夹具");
        assert_eq!(added.len(), 3);
        assert!(added.contains(&"rr3-sima/ping/state".to_string()));
        assert!(set.rr3_topic_set_contains("rr3-sima/ping/state"));
    }

    /// 撞名必须拒启动并点名条目。
    /// 叠加层里的 topic 写成别人的命名空间（如 rr3-sima 声称 rr3-sense/audio/pcm）在
    /// `read_set` 的形状自检那一步就会被"不在 owner 命名空间内"拦下；只有**自身形状合法**
    /// 的叠加层（owner 与首段一致）才走得到 merge，进而被撞名规则点名——故这里用 rr3-sense。
    #[test]
    fn overlay_conflict_refuses_startup_and_names_the_entry() {
        let ov = tmp_named(
            "conflict",
            r#"
[[owner]]
name = "rr3-sense"
[[topic]]
name = "rr3-sense/audio/pcm"
kind = "event"
owner = "rr3-sense"
"#,
        );
        assert_eq!(
            rr3_topics_load(&contract(), Some(&ov)).expect_err("撞名必须拒"),
            vec!["rr3-sense/audio/pcm".to_string()]
        );
    }

    /// 空叠加层 / owner 未登记的叠加层都必须拒，不得静默放行
    #[test]
    fn bad_overlay_is_rejected_not_ignored() {
        let e1 = rr3_topics_load(&contract(), Some(&tmp_named("empty", "schema_version = 1\n")))
            .expect_err("空叠加层必须拒");
        assert!(e1[0].contains("为空"), "{e1:?}");

        let e2 = rr3_topics_load(
            &contract(),
            Some(&tmp_named(
                "evil",
                "\n[[topic]]\nname = \"rr3-sima/ping/state\"\nkind = \"event\"\nowner = \"rr3-sima\"\n",
            )),
        )
        .expect_err("owner 未登记必须拒");
        assert!(e2.iter().any(|e| e.contains("owner 未登记")), "{e2:?}");
    }

    /// 契约文件本身丢了也要拒启动（不能"零主题继续跑"）
    #[test]
    fn missing_contract_is_fatal() {
        let e = rr3_topics_load(Path::new("/nonexistent/rr3-topics.toml"), None)
            .expect_err("契约缺失必须拒");
        assert!(e[0].contains("读不到主题契约"), "{e:?}");
    }

    #[test]
    fn reserved_namespace_detection() {
        assert!(rr3_topics_is_reserved_topic("rr3-runtime/system/heartbeat"));
        assert!(rr3_topics_is_reserved_topic("rr3-runtime/module/state"));
        assert!(!rr3_topics_is_reserved_topic("rr3-sima/ping/state"));
        assert!(!rr3_topics_is_reserved_topic(""));
    }

    /// 叠加层里 owner 已登记、但 topic 首段不是该 owner —— 冒充别人的命名空间必须拒
    /// （钉住 `rr3_topic_set_check` 的"不在 owner 命名空间内"分支）
    #[test]
    fn overlay_topic_outside_its_owner_namespace_is_rejected() {
        let ov = tmp_named(
            "crossns",
            "\n[[owner]]\nname = \"rr3-sima\"\n[[topic]]\nname = \"rr3-simb/hijack/state\"\nkind = \"event\"\nowner = \"rr3-sima\"\n",
        );
        let e = rr3_topics_load(&contract(), Some(&ov)).expect_err("冒充他人命名空间必须拒");
        assert!(
            e.iter().any(|m| m.contains("不在 owner rr3-sima 命名空间内")),
            "必须点名越权的 topic，收到 {e:?}"
        );
    }

    /// `rr3_topic_set_names()` 承诺升序：契约文本是"按 owner 分组手写"的顺序，
    /// 若实现退化成插入序，本测试的第一/最后一条断言就会红。
    #[test]
    fn names_are_ascending_not_contract_file_order() {
        let (set, _) = rr3_topics_load(&contract(), None).expect("契约装载应成功");
        let names = set.rr3_topic_set_names();
        assert!(names.windows(2).all(|w| w[0] < w[1]), "主题名必须严格升序: {names:?}");
        assert_eq!(names.first().map(String::as_str), Some("rr3-act/led/effect"));
        assert_eq!(names.last().map(String::as_str), Some("rr3-sense/imu/state"));
    }

    /// 不在册 topic 的 owner 是 None（rrd 靠这个返回值拒注册，不能给空串或 panic）
    #[test]
    fn unknown_topic_has_no_owner() {
        let (set, _) = rr3_topics_load(&contract(), None).expect("契约装载应成功");
        assert_eq!(set.rr3_topic_set_owner("rr3-nobody/not/registered"), None);
        assert_eq!(set.rr3_topic_set_owner("rr3-sense/audio/pcm").as_deref(), Some("rr3-sense"));
    }
}
