//! 所有权表装载：`contracts/topics.toml` 全集（生产真相）+ 仅测试的叠加层。
//!
//! 叠加层不是生产放行口（spec §6 / R6）：默认不加载；加载时由 main 打印绝对路径
//! （`rr3_topics_absolute_path` 一次解析，装载与横幅共用）与新增条目清单；
//! 与契约 topic 撞名 → 拒绝启动；声明内建命名空间 `rr3-runtime/*` 的 topic → 同样拒绝。
//! 两条路径（契约、叠加层）都在**本入口内**各自规范化一次（CF-4 / 遗留 ③），所以调用方
//! 传相对原词也改不了"致命文案回显绝对路径"这一条；main 那次解析服务的是横幅凭证。

use std::path::{Path, PathBuf};

use rr_registry::{rr3_registry_topic_set_from_text, Rr3TopicSet};

/// 内建命名空间：`rr3-runtime/*` 两个主题属 rrd 的下行广播，模块上行冒充必须被拒（spec §5.0）。
/// 强制点在装载处：`rr3_topics_load` 对叠加层任一名命中
/// `rr3_topics_is_reserved_topic` 的条目即拒启动并点名（见
/// `overlay_declaring_reserved_namespace_is_rejected`）；运行时注册路径上的同一判定随 Task 5/6 落地。
pub const RR3_RESERVED_OWNER: &str = "rr3-runtime";

/// 契约主题的编译期定位（与 rr-ipc 测试同深度：base/runtime/<crate> → 仓库根）
pub const RR3_CONTRACT_TOPICS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../contracts/topics.toml"
);

/// 契约目录的运行期落点（0.0.2 T5）：`--contracts` 显式给值时原样用之（topics.toml /
/// slots.toml 由装载入口各自 join），否则回落编译期仓内路径——deb 装机后编译期路径
/// 不存在，payload 必须显式指向包内 `/usr/lib/rockrobot3/contracts`。
pub fn rr3_contracts_dir(contracts: Option<&Path>) -> PathBuf {
    contracts.map(Path::to_path_buf).unwrap_or_else(|| {
        Path::new(RR3_CONTRACT_TOPICS_PATH)
            .parent()
            .unwrap()
            .to_path_buf()
    })
}

fn read_set(path: &Path, role: &str) -> Result<Rr3TopicSet, Vec<String>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| vec![format!("读不到{role} {}: {e}", path.display())])?;
    let set = rr3_registry_topic_set_from_text(&text);
    if set.rr3_topic_set_is_empty() {
        return Err(vec![format!(
            "{role} {} 解析后为空，拒绝启动",
            path.display()
        )]);
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

/// 契约路径进装载前先规范化（Task 7 评审遗留 CF-4）。
///
/// **为什么这里用 `fs::canonicalize`，而套接字路径（`cli::rr3_socket_absolute_path`）用的是
/// `std::path::absolute`**：契约主题表**必须已经存在**——它是 spec §6 的生产真相，
/// `missing_contract_is_fatal` 钉死了"文件丢了就拒启动"。"不存在"在这条路径上是错误，
/// 不是像待创建套接字那样的合法常态，所以 ENOENT 正是该如实报的那一条；反过来在套接字那边
/// 用 canonicalize 就会误拒一个 bind 前必然不存在的合法值（两个入口的分工由此而定，别互换）。
///
/// **为什么非规范化不可**：`RR3_CONTRACT_TOPICS_PATH` 由 `CARGO_MANIFEST_DIR` 拼出，字面就带
/// `..`（`…/rrd/../../../contracts/topics.toml`），而 `main.rs` 只把**叠加层**路径绝对化
/// （`rr3_topics_absolute_path`）、契约这条没走那一步——于是装载失败的致命文案里漏出 `..`：
/// 运维照报错里的路径去查，那串 `..` 是按**他自己**的 CWD 展开的，指向的往往不是 rrd 刚读的那一份。
/// 放在装载入口而不是 main，是为了让**每个**调用方（含测试与后续 sim 侧）都在同一口径上。
/// canonicalize 失败与"读不到契约"是同一个前提、同一句文案，不静默回落原词。
fn rr3_contract_resolved(contract: &Path) -> Result<PathBuf, Vec<String>> {
    std::fs::canonicalize(contract)
        .map_err(|e| vec![format!("读不到主题契约 {}: {e}", contract.display())])
}

/// 叠加层路径与契约同口径进装载入口即规范化（Task 7 评审遗留 ③，与 CF-4 同一修法）。
///
/// 原先只有 `main.rs` 绝对化叠加层（为了给横幅留一张与 CWD 无关的凭证），装载入口拿到的
/// 却仍是调用方给什么就用什么 —— 于是**叠加层**的撞名/冒充内建命名空间这类致命文案里
/// 可能漏出相对原词（带 `..` 或裸文件名）：运维照报错里的路径去查，展开的是他自己的 CWD。
/// 与契约那一条同理，"不存在"在叠加层这条路径上也是错误（它必须读得出来），所以
/// `canonicalize` 的 ENOENT 正是该如实报的那一条，文案措辞与 `read_set` 的读失败保持一致。
fn rr3_overlay_resolved(overlay: &Path) -> Result<PathBuf, Vec<String>> {
    std::fs::canonicalize(overlay)
        .map_err(|e| vec![format!("读不到主题叠加层 {}: {e}", overlay.display())])
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

/// 合并后对**整张表**再自检一次（Task 3 评审 DEFER→Task 5 的欠账；kind 一支随 Task 6 批次补齐）。
///
/// `read_set` 只对契约、叠加层各自自检，合并这一步本身从未被验证过：
/// `rr3_topic_set_merge` 只要退化（漏带一条生产 topic、把某条的 owner 或 kind 换成叠加层
/// 的值），装载就会把一张被改写过的所有权表交出去——那正是 spec §6
/// "叠加层不是生产放行口"最坏的实现方式。故合并后复检两件事并逐条点名：
/// ① merged 自身形状合法（`rr3_topic_set_check` 的范围：名字规则、owner 已登记、
///    首段 == owner、kind 在白名单内）；② 契约的**每一条 topic 名**都仍在 merged 里，
///    且它的 **owner 与 kind** 一字未改（叠加层只能新增）。
///
/// ② 里为什么要单独比 kind：`rr3_topic_set_check` 只问 kind 在不在
/// `[rules] topic_allowed_kinds` 白名单里，不问它是不是契约原值——契约某条 topic 的 kind
/// 被 merge 换成另一个白名单值时，①照样全过、owner 也比得出"没变"，只有拿契约原值逐条
/// 比对才抓得到（访问器 `Rr3TopicSet::rr3_topic_set_kind` 即为此而加）。
/// 复验范围到 kind 为止：主题集只存 name→owner、name→kind 与 owner 名单，`stable`、
/// `note` 那类注释性字段不在它的视图里，故本注释不称"全字段复验"。
/// 与 `reject_reserved_namespace` 一样是"装载即拒"，不留到运行期靠发布检查兜底。
fn reject_inconsistent_merge(base: &Rr3TopicSet, merged: &Rr3TopicSet) -> Result<(), Vec<String>> {
    let mut errs: Vec<String> = merged
        .rr3_topic_set_check()
        .into_iter()
        .map(|e| format!("合并后主题集形状不合: {e}"))
        .collect();
    for name in base.rr3_topic_set_names() {
        let want = base.rr3_topic_set_owner(&name);
        let want_kind = base.rr3_topic_set_kind(&name);
        if !merged.rr3_topic_set_contains(&name) {
            errs.push(format!(
                "合并后契约 topic 丢失: {name}（叠加层不得覆盖生产条目）"
            ));
        } else if merged.rr3_topic_set_owner(&name) != want {
            errs.push(format!(
                "契约 topic {name} 的 owner 被叠加层改写: {:?} → {:?}",
                want,
                merged.rr3_topic_set_owner(&name)
            ));
        } else if merged.rr3_topic_set_kind(&name) != want_kind {
            errs.push(format!(
                "契约 topic {name} 的 kind 被叠加层改写: {:?} → {:?}",
                want_kind,
                merged.rr3_topic_set_kind(&name)
            ));
        }
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

/// 装载：契约 →（可选）叠加层合并。返回主题集与"叠加层新增的条目清单"。
/// 两条路径都在进装载入口时被规范化（CF-4 契约 / 遗留 ③ 叠加层），后续所有文案都取
/// 规范化后的那一个：`rr3_contract_resolved` 与 `rr3_overlay_resolved` 各管一条。
/// 叠加层两道拒绝：撞名 → 点名冲突条目；声明 `rr3-runtime/*` → 点名冒充条目。
/// 合并后还有一道：见 `reject_inconsistent_merge`。
pub fn rr3_topics_load(
    contract: &Path,
    overlay: Option<&Path>,
) -> Result<(Rr3TopicSet, Vec<String>), Vec<String>> {
    let contract = rr3_contract_resolved(contract)?;
    let base = read_set(&contract, "主题契约")?;
    let Some(ov) = overlay else {
        return Ok((base, Vec::new()));
    };
    let ov = rr3_overlay_resolved(ov)?;
    let ov_set = read_set(&ov, "主题叠加层")?;
    reject_reserved_namespace(&ov_set, &ov)?;
    let added = ov_set.rr3_topic_set_names();
    let merged = base.rr3_topic_set_merge(&ov_set)?; // 撞名 → Err(冲突条目)
    reject_inconsistent_merge(&base, &merged)?;
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

    /// 不带叠加层 = 生产面，31 条主题一个不多
    #[test]
    fn contract_only_load_is_the_production_surface() {
        let (set, added) = rr3_topics_load(&contract(), None).expect("契约装载应成功");
        assert_eq!(set.rr3_topic_set_names().len(), 31);
        assert!(added.is_empty(), "没传叠加层就不该有新增条目");
        assert!(set.rr3_topic_set_contains("rr3-runtime/system/heartbeat"));
    }

    #[test]
    fn overlay_adds_fixture_topics_and_lists_them() {
        let (set, added) =
            rr3_topics_load(&contract(), Some(&fixture_overlay())).expect("夹具应能合并");
        assert_eq!(
            set.rr3_topic_set_names().len(),
            34,
            "31 契约 + 3 夹具（0.0.5 第 IV 轮 MCP-0 契约 29→30：append brain/audio/tts_pcm；增补轮六 批N2 件二 30→31：append rr3-power/rail/state）"
        );
        assert_eq!(added.len(), 3);
        assert!(added.contains(&"rr3-sima/ping/state".to_string()));
        assert!(set.rr3_topic_set_contains("rr3-sima/ping/state"));
    }

    /// 合并后必须**逐条复验 topic 名、owner 与 kind**（Task 3 评审 DEFER→Task 5；kind 一支随
    /// Task 6 批次补，`Rr3TopicSet::rr3_topic_set_kind` 就是为它加的访问器）：装载交出去的那张表，
    /// 除了"两边各自合法"，还得保证生产条目一条不少、owner 与 kind 一字未改。复验范围到 kind
    /// 为止（`stable`、`note` 不在主题集的视图里），别把这条读成全字段复验。
    /// 断言取自 `rr3_topics_load` 的真实返回，而不是再拼一遍期望集合——去掉装载处的
    /// 复验步骤本身不会让本测试变红（正确的 merge 造不出不自洽的表），它挡的是
    /// merge 将来退化；装载处的复验是同一件事的 fail-closed 版本，kind 那一支的**反向**实证
    /// 见 `contract_topic_kind_rewritten_by_the_merge_is_rejected`。
    #[test]
    fn merged_set_is_rechecked_against_the_contract() {
        let (base_set, _) = rr3_topics_load(&contract(), None).expect("契约装载应成功");
        let (merged, _) =
            rr3_topics_load(&contract(), Some(&fixture_overlay())).expect("夹具应能合并");
        assert!(
            merged.rr3_topic_set_check().is_empty(),
            "{:?}",
            merged.rr3_topic_set_check()
        );
        for name in base_set.rr3_topic_set_names() {
            assert!(
                merged.rr3_topic_set_contains(&name),
                "合并后契约条目丢失: {name}"
            );
            assert_eq!(
                merged.rr3_topic_set_owner(&name),
                base_set.rr3_topic_set_owner(&name),
                "契约 topic {name} 的 owner 被叠加层改写"
            );
            assert_eq!(
                merged.rr3_topic_set_kind(&name),
                base_set.rr3_topic_set_kind(&name),
                "契约 topic {name} 的 kind 被叠加层改写"
            );
        }
        // 叠加层新增的 owner 也必须进了这张表，否则"复验"会连合法新增一起否掉
        assert_eq!(
            merged.rr3_topic_set_owner("rr3-sima/ping/state").as_deref(),
            Some("rr3-sima")
        );
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
        let e1 = rr3_topics_load(
            &contract(),
            Some(&tmp_named("empty", "schema_version = 1\n")),
        )
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

    /// CF-4：装载入口必须先把契约路径规范化再进文案。带 `..` 的原词漏进报错时，运维照着查，
    /// 那串 `..` 是按**他自己**的 CWD 展开的，指向的不是 rrd 刚读的那一份。
    /// 前提两条各自钉住，免得这条修法对着假想敌：① 生产常量 `RR3_CONTRACT_TOPICS_PATH`
    /// 字面就带 `..`（它由 `CARGO_MANIFEST_DIR` 拼出）；② 喂进装载入口的路径确实绕了 `..`。
    /// 与 CF-3（套接字用 `std::path::absolute`）的分工见 `rr3_contract_resolved` 的注释：
    /// 契约**必须已存在**，所以 canonicalize 在这里恰是对的工具，而 bind 前的套接字用它会误拒。
    #[test]
    fn contract_path_is_normalised_before_it_reaches_the_error_text() {
        assert!(
            RR3_CONTRACT_TOPICS_PATH.contains(".."),
            "用例前提：生产常量本身就是带 .. 的形状，否则这条修法没有对象（收到 {RR3_CONTRACT_TOPICS_PATH}）"
        );
        let dir = std::env::temp_dir().join(format!("rr3-cf4-{}", std::process::id()));
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).expect("建用例临时目录");
        let target = dir.join("cf4-contract.toml");
        std::fs::write(
            &target,
            "\n[[topic]]\nname = \"rr3-sima/ping/state\"\nkind = \"telepathy\"\nowner = \"rr3-sima\"\n",
        )
        .expect("写坏契约");
        let ladden = nested.join("..").join("cf4-contract.toml");
        assert!(
            ladden
                .components()
                .any(|c| c == std::path::Component::ParentDir),
            "用例前提：喂进去的必须真带 .. 分量，收到 {ladden:?}"
        );

        let e = rr3_topics_load(&ladden, None).expect_err("坏契约必须拒");
        assert!(e.iter().all(|m| m.starts_with("主题契约 ")), "{e:?}");
        assert!(
            e.iter().all(|m| !m.contains("..")),
            "报错文案不得漏出 `..`，收到 {e:?}"
        );
        let want = std::fs::canonicalize(&target).expect("用例夹具必须能规范化");
        assert!(
            e.iter().all(|m| m.contains(&want.display().to_string())),
            "文案里的路径必须是规范化后的那一份 {want:?}，收到 {e:?}"
        );
        // M-10：自己收自己的临时件（Global Constraints 8）——这条用例按 pid 建目录，
        // 不清就得在 /tmp 里每个测试进程留一份。
        let _ = std::fs::remove_dir_all(&dir);
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
            e.iter()
                .any(|m| m.contains("不在 owner rr3-sima 命名空间内")),
            "必须点名越权的 topic，收到 {e:?}"
        );
    }

    /// `rr3_topic_set_names()` 承诺升序：契约文本是"按 owner 分组手写"的顺序，
    /// 若实现退化成插入序，本测试的第一/最后一条断言就会红。
    #[test]
    fn names_are_ascending_not_contract_file_order() {
        let (set, _) = rr3_topics_load(&contract(), None).expect("契约装载应成功");
        let names = set.rr3_topic_set_names();
        assert!(
            names.windows(2).all(|w| w[0] < w[1]),
            "主题名必须严格升序: {names:?}"
        );
        assert_eq!(
            names.first().map(String::as_str),
            Some("rr3-act/led/effect")
        );
        assert_eq!(
            names.last().map(String::as_str),
            Some("rr3-sentinel/diag/probe"),
            "升序末位随 0.0.7 批E append（rr3-sentinel > rr3-sense）演进"
        );
    }

    /// 不在册 topic 的 owner 是 None（rrd 靠这个返回值拒注册，不能给空串或 panic）
    #[test]
    fn unknown_topic_has_no_owner() {
        let (set, _) = rr3_topics_load(&contract(), None).expect("契约装载应成功");
        assert_eq!(set.rr3_topic_set_owner("rr3-nobody/not/registered"), None);
        assert_eq!(
            set.rr3_topic_set_owner("rr3-sense/audio/pcm").as_deref(),
            Some("rr3-sense")
        );
    }

    /// 复验必须连 **kind** 一起比（Task 5 评审 M-15）：`rr3_topic_set_check` 只问 kind
    /// 在不在白名单里，不问它是不是契约原值——把契约某条 topic 的 kind 从 `stream` 改成
    /// 另一个合法值 `event`，形状自检照样过，owner 也比得出"没变"，两条分支都抓不到。
    /// 本用例刻意不走 `rr3_topics_load`（正确的 merge 造不出 kind 被改写的表），而是手写
    /// 一对 base/merged 代表"merge 退化后的产物"，直接打复验函数；同一对输入里的合法新增
    /// 对照组证明这条规则不会把"只加新 topic"一起否掉。
    #[test]
    fn contract_topic_kind_rewritten_by_the_merge_is_rejected() {
        let base = rr3_registry_topic_set_from_text(
            r#"schema_version = 1
[[owner]]
name = "rr3-sense"
[[topic]]
name = "rr3-sense/audio/pcm"
kind = "stream"
owner = "rr3-sense"
"#,
        );
        // 退化 merge 的产物：名字与 owner 都没动，只把 kind 换成另一个白名单值
        let tampered = rr3_registry_topic_set_from_text(
            r#"schema_version = 1
[[owner]]
name = "rr3-sense"
[[topic]]
name = "rr3-sense/audio/pcm"
kind = "event"
owner = "rr3-sense"
"#,
        );
        assert!(
            tampered.rr3_topic_set_check().is_empty(),
            "用例前提：kind 换成 event 仍是合法值，形状自检抓不到（{:?}），只能靠复验抓",
            tampered.rr3_topic_set_check()
        );
        assert_eq!(
            reject_inconsistent_merge(&base, &tampered).expect_err("契约 topic 的 kind 被改写必须拒"),
            vec![
                "契约 topic rr3-sense/audio/pcm 的 kind 被叠加层改写: Some(\"stream\") → Some(\"event\")"
                    .to_string()
            ],
            "必须点名 topic 与改写前后的两个 kind 值：写法与上面那条 owner 改写错误一致\
            （Debug 打在 Option 上，所以是 Some(...) 而不是裸引号串）"
        );
        // 对照组：只新增 topic、契约条目一字未动 —— 合法叠加层不得被误拒
        let legit = rr3_registry_topic_set_from_text(
            r#"schema_version = 1
[[owner]]
name = "rr3-sense"
[[owner]]
name = "rr3-sima"
[[topic]]
name = "rr3-sense/audio/pcm"
kind = "stream"
owner = "rr3-sense"
[[topic]]
name = "rr3-sima/ping/state"
kind = "event"
owner = "rr3-sima"
"#,
        );
        assert!(
            reject_inconsistent_merge(&base, &legit).is_ok(),
            "合法新增不得被 kind 复验误拒: {:?}",
            reject_inconsistent_merge(&base, &legit)
        );
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
        assert!(
            rel.is_relative(),
            "用例前提：喂进去的必须真是相对路径，收到 {rel:?}"
        );

        let got = rr3_topics_absolute_path(&rel).expect("相对叠加层路径必须能解析");
        assert!(
            got.is_absolute(),
            "横幅凭证必须是绝对路径，否则内容依赖 CWD，收到 {got:?}"
        );
        assert_eq!(
            got,
            std::fs::canonicalize(&abs).unwrap(),
            "解析结果必须指向夹具本身"
        );

        let (set, added) = rr3_topics_load(&contract(), Some(&rel)).expect("相对路径必须能装载");
        assert_eq!(
            set.rr3_topic_set_names().len(),
            34,
            "31 契约 + 3 夹具（0.0.5 第 IV 轮 MCP-0 契约 29→30：append brain/audio/tts_pcm；增补轮六 批N2 件二 30→31：append rr3-power/rail/state）"
        );
        assert_eq!(added.len(), 3);
    }

    /// 解析不了的路径要如实报错，不许悄悄回显原词（否则路径绝对化形同虚设）
    #[test]
    fn unresolvable_path_reports_instead_of_echoing() {
        let e = rr3_topics_absolute_path(Path::new("no-such-dir/rr3-nope.toml"))
            .expect_err("不存在的路径必须报错");
        assert!(
            e.contains("无法解析为绝对路径") && e.contains("rr3-nope.toml"),
            "{e}"
        );
    }

    /// 叠加层声明内建命名空间 `rr3-runtime/*` 的新条目必须被装载处拒掉并点名。
    /// owner 写成 `rr3-runtime` 让这份叠加层**形状完全合法**（首段与 owner 一致、owner 已登记），
    /// 故拦下它的只可能是保留命名空间规则，不是形状规则——两条规则各测各的。
    ///
    /// 遗留 ③ 的另一半实证：喂进去的是**带 `..` 的相对原词**，而致命文案里的路径必须是
    /// 规范化后的那一份（`rr3_overlay_resolved`）。原先只有 `main.rs` 绝对化叠加层，装载入口
    /// 直接拿调用方给的原词做文案 → 运维照报错里的路径去查，展开的是他自己的 CWD。
    /// 与 `contract_path_is_normalised_before_it_reaches_the_error_text`（CF-4）同口径、同工具。
    #[test]
    fn overlay_declaring_reserved_namespace_is_rejected() {
        let dir = std::env::temp_dir().join(format!("rr3-ov-cf3-{}", std::process::id()));
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).expect("建用例临时目录");
        let target = dir.join("rr3-reserved-overlay.toml");
        std::fs::write(
            &target,
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
        )
        .expect("写坏叠加层");
        let ov = nested.join("..").join("rr3-reserved-overlay.toml");
        assert!(
            ov.components()
                .any(|c| c == std::path::Component::ParentDir),
            "用例前提：喂进去的必须真带 .. 分量，收到 {ov:?}"
        );
        let e = rr3_topics_load(&contract(), Some(&ov)).expect_err("冒充内建命名空间必须拒");
        let want = std::fs::canonicalize(&target).expect("用例夹具必须能规范化");
        assert_eq!(
            e,
            vec![format!(
                "主题叠加层 {} 冒充内建命名空间: rr3-runtime/evil/cmd（rr3-runtime/* 属 rrd 的下行广播，叠加层不得声明）",
                want.display()
            )],
            "必须只点名冒充的那条，合法条目不受牵连，且路径是规范化后的 {want:?}，收到 {e:?}"
        );
        assert!(
            !e[0].contains(".."),
            "报错文案不得漏出 `..`（那串按运维自己的 CWD 展开），收到 {e:?}"
        );
        // M-10：自己收自己的临时件（Global Constraints 8）
        let _ = std::fs::remove_dir_all(&dir);
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
            e.iter().all(|m| m.starts_with("主题叠加层 "))
                && e.iter().all(|m| m.contains("形状不合")),
            "叠加层的形状错误必须逐条标注来源，收到 {e:?}"
        );
        assert!(
            e.iter().any(|m| m.contains("不在 owner x 命名空间内")),
            "{e:?}"
        );

        // 契约自己坏掉时，同一位置的错误必须写"主题契约"而不是"主题叠加层"
        let contract_bad = tmp_named(
            "shape-contract",
            "\n[[topic]]\nname = \"rr3-sima/ping/state\"\nkind = \"telepathy\"\nowner = \"rr3-sima\"\n",
        );
        let e2 = rr3_topics_load(&contract_bad, None).expect_err("契约形状不合必须拒");
        assert!(
            e2.iter().all(|m| m.starts_with("主题契约 "))
                && e2.iter().any(|m| m.contains("kind 非法")),
            "契约的形状错误必须标成契约来源，收到 {e2:?}"
        );
    }
}
