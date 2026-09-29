//! 所有权表装载：`contracts/topics.toml` 全集（生产真相）+ 仅测试的叠加层。
//!
//! 叠加层不是生产放行口（spec §6 / R6）：默认不加载；加载时由 main 打印绝对路径
//! （`rr3_topics_absolute_path` 一次解析，装载与横幅共用）与新增条目清单；
//! 与契约 topic 撞名 → 拒绝启动；声明内建命名空间 `rr3-runtime/*` 的 topic → 同样拒绝。

use std::path::{Path, PathBuf};

use rr_registry::{rr3_registry_topic_set_from_text, Rr3TopicSet};

/// 内建命名空间：`rr3-runtime/*` 两个主题属 rrd 的下行广播，模块上行冒充必须被拒（spec §5.0）。
/// 强制点在装载处：`rr3_topics_load` 对叠加层任一名命中
/// `rr3_topics_is_reserved_topic` 的条目即拒启动并点名（见
/// `overlay_declaring_reserved_namespace_is_rejected`）；运行时注册路径上的同一判定随 Task 5/6 落地。
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
        // 形状错误本身不带来源，这里逐条包上 {role}+path：契约的错和叠加层的错
        // 不能打出一样的字，否则"哪一册坏了"要人二次猜
        return Err(errs
            .into_iter()
            .map(|e| format!("{role} {} 形状不合: {e}", path.display()))
            .collect());
    }
    Ok(set)
}

/// 把命令行传入的路径解析成绝对路径（`canonicalize`：纯 `std::fs`，两平台同语义）。
/// 只做一次，装载与横幅共用——横幅那行是"叠加层已生效"的可见凭证（spec §6），
/// 按 CLI 原词回显会让凭证的内容依赖 CWD，等于没留。解析失败（路径不存在/不可读）
/// 如实报错，绝不静默回落到原词。
pub fn rr3_topics_absolute_path(path: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path)
        .map_err(|e| format!("路径 {} 无法解析为绝对路径: {e}", path.display()))
}

/// 叠加层里出现内建命名空间的名字 = 冒充 rrd 的下行广播（spec §5.0）。
/// 逐条点名；不命中的条目不受牵连。
fn reject_reserved_namespace(set: &Rr3TopicSet, path: &Path) -> Result<(), Vec<String>> {
    let errs: Vec<String> = set
        .rr3_topic_set_names()
        .iter()
        .filter(|t| rr3_topics_is_reserved_topic(t))
        .map(|t| {
            format!(
                "主题叠加层 {} 冒充内建命名空间: {t}（{RR3_RESERVED_OWNER}/* 属 rrd 的下行广播，叠加层不得声明）",
                path.display()
            )
        })
        .collect();
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

/// 装载：契约 →（可选）叠加层合并。返回主题集与"叠加层新增的条目清单"。
/// 叠加层两道拒绝：撞名 → 点名冲突条目；声明 `rr3-runtime/*` → 点名冒充条目。
pub fn rr3_topics_load(
    contract: &Path,
    overlay: Option<&Path>,
) -> Result<(Rr3TopicSet, Vec<String>), Vec<String>> {
    let base = read_set(contract, "主题契约")?;
    let Some(ov) = overlay else { return Ok((base, Vec::new())) };
    let ov_set = read_set(ov, "主题叠加层")?;
    reject_reserved_namespace(&ov_set, ov)?;
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

    /// 把一个绝对路径改写成"从 cwd 出发"的相对路径（纯分量运算，不碰文件系统）。
    /// 只服务"相对路径也能绝对化"那条实证：不把测试建立在"cargo 把测试 CWD 定在包根"这个实现细节上。
    fn relativize(abs: &Path, cwd: &Path) -> Option<PathBuf> {
        let a: Vec<_> = abs.components().collect();
        let c: Vec<_> = cwd.components().collect();
        let mut common = 0;
        while common < a.len() && common < c.len() && a[common] == c[common] {
            common += 1;
        }
        let mut out = PathBuf::new();
        for _ in common..c.len() {
            out.push("..");
        }
        for comp in &a[common..] {
            out.push(comp.as_os_str());
        }
        (!out.as_os_str().is_empty()).then_some(out)
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

    /// 横幅凭证必须绝对：用**相对**叠加层路径调用解析入口，断言拿回的是绝对路径
    /// 且指向同一份夹具；顺带钉住"装载入口本身不要求调用方先绝对化"。
    /// （Windows 上 canonicalize 会带 `\\?\` verbatim 前缀，那仍是绝对路径，
    /// `is_absolute()` 与等值比较都成立；Linux 生产侧就是干净的 `/abs/...`。）
    #[test]
    fn relative_overlay_path_resolves_to_absolute() {
        let abs = fixture_overlay();
        let cwd = std::env::current_dir().expect("测试进程必须有可查的 CWD");
        let rel = relativize(&abs, &cwd).expect("夹具路径必须能改写成相对形式");
        assert!(rel.is_relative(), "用例前提：喂进去的必须真是相对路径，收到 {rel:?}");

        let got = rr3_topics_absolute_path(&rel).expect("相对叠加层路径必须能解析");
        assert!(got.is_absolute(), "横幅凭证必须是绝对路径，否则内容依赖 CWD，收到 {got:?}");
        assert_eq!(got, std::fs::canonicalize(&abs).unwrap(), "解析结果必须指向夹具本身");

        let (set, added) = rr3_topics_load(&contract(), Some(&rel)).expect("相对路径必须能装载");
        assert_eq!(set.rr3_topic_set_names().len(), 24, "21 契约 + 3 夹具");
        assert_eq!(added.len(), 3);
    }

    /// 解析不了的路径要如实报错，不许悄悄回显原词（否则路径绝对化形同虚设）
    #[test]
    fn unresolvable_path_reports_instead_of_echoing() {
        let e = rr3_topics_absolute_path(Path::new("no-such-dir/rr3-nope.toml"))
            .expect_err("不存在的路径必须报错");
        assert!(e.contains("无法解析为绝对路径") && e.contains("rr3-nope.toml"), "{e}");
    }

    /// 叠加层声明内建命名空间 `rr3-runtime/*` 的新条目必须被装载处拒掉并点名。
    /// owner 写成 `rr3-runtime` 让这份叠加层**形状完全合法**（首段与 owner 一致、owner 已登记），
    /// 故拦下它的只可能是保留命名空间规则，不是形状规则——两条规则各测各的。
    #[test]
    fn overlay_declaring_reserved_namespace_is_rejected() {
        let ov = tmp_named(
            "reserved",
            r#"
[[owner]]
name = "rr3-runtime"
[[owner]]
name = "rr3-sima"
[[topic]]
name = "rr3-runtime/evil/cmd"
kind = "event"
owner = "rr3-runtime"
[[topic]]
name = "rr3-sima/extra/state"
kind = "event"
owner = "rr3-sima"
"#,
        );
        let e = rr3_topics_load(&contract(), Some(&ov)).expect_err("冒充内建命名空间必须拒");
        assert_eq!(
            e,
            vec![format!(
                "主题叠加层 {} 冒充内建命名空间: rr3-runtime/evil/cmd（rr3-runtime/* 属 rrd 的下行广播，叠加层不得声明）",
                ov.display()
            )],
            "必须只点名冒充的那条，合法条目不受牵连"
        );
    }

    /// 形状错误必须自带来源——契约的错与叠加层的错打出来不能一样
    #[test]
    fn shape_errors_name_their_source() {
        let overlay_bad = tmp_named(
            "shape-overlay",
            "\n[[owner]]\nname = \"x\"\n[[topic]]\nname = \"rr3-sense/audio/pcm\"\nkind = \"event\"\nowner = \"x\"\n",
        );
        let e = rr3_topics_load(&contract(), Some(&overlay_bad)).expect_err("形状不合必须拒");
        assert!(
            e.iter().all(|m| m.starts_with("主题叠加层 ")) && e.iter().all(|m| m.contains("形状不合")),
            "叠加层的形状错误必须逐条标注来源，收到 {e:?}"
        );
        assert!(e.iter().any(|m| m.contains("不在 owner x 命名空间内")), "{e:?}");

        // 契约自己坏掉时，同一位置的错误必须写"主题契约"而不是"主题叠加层"
        let contract_bad = tmp_named(
            "shape-contract",
            "\n[[topic]]\nname = \"rr3-sima/ping/state\"\nkind = \"telepathy\"\nowner = \"rr3-sima\"\n",
        );
        let e2 = rr3_topics_load(&contract_bad, None).expect_err("契约形状不合必须拒");
        assert!(
            e2.iter().all(|m| m.starts_with("主题契约 ")) && e2.iter().any(|m| m.contains("kind 非法")),
            "契约的形状错误必须标成契约来源，收到 {e2:?}"
        );
    }
}
