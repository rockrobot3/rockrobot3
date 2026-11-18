//! 所有权表装载：`contracts/topics.toml` 全集（生产真相）+ 仅测试的叠加层。
//!
//! 叠加层不是生产放行口（spec §6 / R6）：默认不加载；加载时由 main 打印绝对路径
//! （`rr3_topics_abs` 一次解析，装载与横幅共用）与新增条目清单；
//! 与契约 topic 撞名 → 拒绝启动；声明内建命名空间 `rr3-runtime/*` 的 topic → 同样拒绝。
//! 两条路径（契约、叠加层）都在**本入口内**各自规范化一次（CF-4 / 遗留 ③），所以调用方
//! 传相对原词也改不了"致命文案回显绝对路径"这一条；main 那次解析服务的是横幅凭证。

use std::path::{Path, PathBuf};

use rr_registry::{rr3_topic_parse, Rr3TopicSet};

/// 内建命名空间：`rr3-runtime/*` 两个主题属 rrd 的下行广播，模块上行冒充必须被拒（spec §5.0）。
/// 强制点在装载处：`rr3_topics_load` 对叠加层任一名命中
/// `rr3_topics_reserved` 的条目即拒启动并点名（见
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
            .expect("编译期常量路径必有父目录")
            .to_path_buf()
    })
}

fn read_set(path: &Path, role: &str) -> Result<Rr3TopicSet, Vec<String>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| vec![format!("读不到{role} {}: {e}", path.display())])?;
    let set = rr3_topic_parse(&text);
    rr3_topics_gate(path, role, &set)?;
    Ok(set)
}

/// 装载后两道门：空表即契约事故；形状错误逐条包上 {role}+path（哪一册坏了一眼可见）。
fn rr3_topics_gate(path: &Path, role: &str, set: &Rr3TopicSet) -> Result<(), Vec<String>> {
    if set.rr3_topic_is_empty() {
        return Err(vec![format!(
            "{role} {} 解析后为空，拒绝启动",
            path.display()
        )]);
    }
    rr3_topic_shape_errs(path, role, set)
}

/// 形状错误逐条包上 {role}+path（哪一册坏了一眼可见）。
fn rr3_topic_shape_errs(path: &Path, role: &str, set: &Rr3TopicSet) -> Result<(), Vec<String>> {
    let errs = set.rr3_topic_check();
    if errs.is_empty() {
        return Ok(());
    }
    Err(errs
        .into_iter()
        .map(|e| format!("{role} {} 形状不合: {e}", path.display()))
        .collect())
}

/// 把命令行传入的路径解析成绝对路径（`canonicalize`：纯 `std::fs`，两平台同语义）。
/// 只做一次，装载与横幅共用——横幅那行是"叠加层已生效"的可见凭证（spec §6），
/// 按 CLI 原词回显会让凭证的内容依赖 CWD，等于没留。解析失败（路径不存在/不可读）
/// 如实报错，绝不静默回落到原词。
pub fn rr3_topics_abs(path: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path)
        .map_err(|e| format!("路径 {} 无法解析为绝对路径: {e}", path.display()))
}

/// 契约路径进装载前先规范化（Task 7 评审遗留 CF-4）。
///
/// **为什么这里用 `fs::canonicalize`，而套接字路径（`cli::rr3_socket_abs`）用的是
/// `std::path::absolute`**：契约主题表**必须已经存在**——它是 spec §6 的生产真相，
/// `missing_contract_is_fatal` 钉死了"文件丢了就拒启动"。"不存在"在这条路径上是错误，
/// 不是像待创建套接字那样的合法常态，所以 ENOENT 正是该如实报的那一条；反过来在套接字那边
/// 用 canonicalize 就会误拒一个 bind 前必然不存在的合法值（两个入口的分工由此而定，别互换）。
///
/// **为什么非规范化不可**：`RR3_CONTRACT_TOPICS_PATH` 由 `CARGO_MANIFEST_DIR` 拼出，字面就带
/// `..`（`…/rrd/../../../contracts/topics.toml`），而 `main.rs` 只把**叠加层**路径绝对化
/// （`rr3_topics_abs`）、契约这条没走那一步——于是装载失败的致命文案里漏出 `..`：
/// 运维照报错里的路径去查，那串 `..` 是按**他自己**的 CWD 展开的，指向的往往不是 rrd 刚读的那一份。
/// 放在装载入口而不是 main，是为了让**每个**调用方（含测试与后续 sim 侧）都在同一口径上。
/// canonicalize 失败与"读不到契约"是同一个前提、同一句文案，不静默回落原词。
fn rr3_contract_abs(contract: &Path) -> Result<PathBuf, Vec<String>> {
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
fn rr3_overlay_abs(overlay: &Path) -> Result<PathBuf, Vec<String>> {
    std::fs::canonicalize(overlay)
        .map_err(|e| vec![format!("读不到主题叠加层 {}: {e}", overlay.display())])
}

/// 叠加层里出现内建命名空间的名字 = 冒充 rrd 的下行广播（spec §5.0）。
/// 逐条点名；不命中的条目不受牵连。
fn rr3_reject_reserved(set: &Rr3TopicSet, path: &Path) -> Result<(), Vec<String>> {
    let errs: Vec<String> = set
        .rr3_topic_names()
        .iter()
        .filter(|t| rr3_topics_reserved(t))
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
/// `rr3_topic_merge` 只要退化（漏带一条生产 topic、把某条的 owner 或 kind 换成叠加层
/// 的值），装载就会把一张被改写过的所有权表交出去——那正是 spec §6
/// "叠加层不是生产放行口"最坏的实现方式。故合并后复检两件事并逐条点名：
/// ① merged 自身形状合法（`rr3_topic_check` 的范围：名字规则、owner 已登记、
///    首段 == owner、kind 在白名单内）；② 契约的**每一条 topic 名**都仍在 merged 里，
///    且它的 **owner 与 kind** 一字未改（叠加层只能新增）。
///
/// ② 里为什么要单独比 kind：`rr3_topic_check` 只问 kind 在不在
/// `[rules] topic_allowed_kinds` 白名单里，不问它是不是契约原值——契约某条 topic 的 kind
/// 被 merge 换成另一个白名单值时，①照样全过、owner 也比得出"没变"，只有拿契约原值逐条
/// 比对才抓得到（访问器 `Rr3TopicSet::rr3_topic_kind` 即为此而加）。
/// 复验范围到 kind 为止：主题集只存 name→owner、name→kind 与 owner 名单，`stable`、
/// `note` 那类注释性字段不在它的视图里，故本注释不称"全字段复验"。
/// 与 `rr3_reject_reserved` 一样是"装载即拒"，不留到运行期靠发布检查兜底。
/// 单条契约 topic 的合并复验：丢失 / owner 改写 / kind 改写，逐类点名。
fn rr3_merge_conflicts(
    base: &Rr3TopicSet,
    merged: &Rr3TopicSet,
    name: &str,
    errs: &mut Vec<String>,
) {
    if !merged.rr3_topic_contains(name) {
        errs.push(format!(
            "合并后契约 topic 丢失: {name}（叠加层不得覆盖生产条目）"
        ));
        return;
    }
    rr3_merge_rewrite(base, merged, name, errs);
}

/// owner / kind 两处改写复验（丢失已在调用方裁掉）：owner 优先点名。
fn rr3_merge_rewrite(base: &Rr3TopicSet, merged: &Rr3TopicSet, name: &str, errs: &mut Vec<String>) {
    let want = base.rr3_topic_owner(name);
    let want_kind = base.rr3_topic_kind(name);
    if merged.rr3_topic_owner(name) != want {
        errs.push(format!(
            "契约 topic {name} 的 owner 被叠加层改写: {:?} → {:?}",
            want,
            merged.rr3_topic_owner(name)
        ));
    } else if merged.rr3_topic_kind(name) != want_kind {
        errs.push(format!(
            "契约 topic {name} 的 kind 被叠加层改写: {:?} → {:?}",
            want_kind,
            merged.rr3_topic_kind(name)
        ));
    }
}

fn reject_bad_merge(base: &Rr3TopicSet, merged: &Rr3TopicSet) -> Result<(), Vec<String>> {
    let mut errs: Vec<String> = merged
        .rr3_topic_check()
        .into_iter()
        .map(|e| format!("合并后主题集形状不合: {e}"))
        .collect();
    for name in base.rr3_topic_names() {
        rr3_merge_conflicts(base, merged, &name, &mut errs);
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

/// 装载：契约 →（可选）叠加层合并。返回主题集与"叠加层新增的条目清单"。
/// 两条路径都在进装载入口时被规范化（CF-4 契约 / 遗留 ③ 叠加层），后续所有文案都取
/// 规范化后的那一个：`rr3_contract_abs` 与 `rr3_overlay_abs` 各管一条。
/// 叠加层两道拒绝：撞名 → 点名冲突条目；声明 `rr3-runtime/*` → 点名冒充条目。
/// 合并后还有一道：见 `reject_bad_merge`。
pub fn rr3_topics_load(
    contract: &Path,
    overlay: Option<&Path>,
) -> Result<(Rr3TopicSet, Vec<String>), Vec<String>> {
    let contract = rr3_contract_abs(contract)?;
    let base = read_set(&contract, "主题契约")?;
    let Some(ov) = overlay else {
        return Ok((base, Vec::new()));
    };
    let ov = rr3_overlay_abs(ov)?;
    let ov_set = read_set(&ov, "主题叠加层")?;
    rr3_topics_overlay(base, &ov_set, &ov)
}

/// 叠加层合并：冒充/撞名/合并复检三道门，回合并表与新增条目。
fn rr3_topics_overlay(
    base: Rr3TopicSet,
    ov: &Rr3TopicSet,
    ov_path: &Path,
) -> Result<(Rr3TopicSet, Vec<String>), Vec<String>> {
    rr3_reject_reserved(ov, ov_path)?;
    let added = ov.rr3_topic_names();
    let merged = base.rr3_topic_merge(ov)?; // 撞名 → Err(冲突条目)
    reject_bad_merge(&base, &merged)?;
    Ok((merged, added))
}

/// 首段是否落在内建命名空间（冒充面）
pub fn rr3_topics_reserved(topic: &str) -> bool {
    topic.split('/').next().unwrap_or("") == RR3_RESERVED_OWNER
}

#[cfg(test)]
mod tests;
