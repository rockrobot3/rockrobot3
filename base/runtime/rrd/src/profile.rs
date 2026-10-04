//! 装配单装载（ADR-0012 裁定 3/5②，0.0.2 方案 D2 / Q3 裁定）。
//!
//! `profiles/<机型>.toml` 是**纯数据**装配单：`[bindings]` 格位→件、`[faces]` 模块→本形态
//! 启用的格位集合。本模块只做四件事，全部发生在 boot 之前（裁定 4：boot 前消费一次，
//! 运行期不重排，F⊗F 非法）：
//! 1. 解析装配单文本（fail-closed：形状不合逐条点名，一条不静默放行）；
//! 2. 查册门（裁定 5②）：bindings/faces 引用的每个格位必须命中
//!    [`contracts/slots.toml`](../../../contracts/slots.toml)——查册真值是
//!    `rr-registry::Rr3SlotTable`，与 CI 全集层共用同一份，不写第二套；
//! 3. 缺格判定：faces 声明的格位若非基础层、又没有 binding 指向本模块（或探测不到，
//!    见 [`crate::probe`]）→ (模块, 格位) 进缺格清单，由 daemon 点名 `拒绝启动`——
//!    绝不带病起，provides 仍留在可满足性宇宙（下游不连坐，ADR-0009 同款语义）；
//! 4. 把 face 模块名解析成 manifest 路径：`<modules-root>/profiles/<机>.toml` 的上两级即
//!    modules-root，manifest 在 `<modules-root>/modules/<名>/module.toml`——装机布局由
//!    打包步骤保证（modules/README.md 装机约定 + 0.0.2 T5 deb 打包同一布局）。
//!
//! 基础层格位（`net.online` / `power.battery`）**恒在位**（slots.toml 2026-10-03 注记）：
//! 不进 bindings、不进缺格判定，faces 里声明它们只是"本形态用到"的记账。
//! `[os].route` 运行期不读（ADR-0012 裁定 3 B3 注记），解析时无视该段。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rr_registry::{rr3_registry_slot_table_from_text, Rr3SlotTable};

/// 契约格位表的编译期定位（与 `topics_load::RR3_CONTRACT_TOPICS_PATH` 同深度：
/// base/runtime/<crate> → 仓库根）
pub const RR3_CONTRACT_SLOTS_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../../contracts/slots.toml");

/// 只认 schema_version = 1（与 slots.toml / profiles 同步演进；不匹配 = 契约事故，拒启动）
const RR3_PROFILE_SCHEMA_VERSION: i64 = 1;

/// 一份装配单（不可变快照）。BTreeMap 让"模块序/格位序"确定化为升序——
/// spawn 序由 `plan_boot` 仲裁裁，这里不携带任何顺序语义。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3Profile {
    /// [bindings]：格位 → 模块名（只允许 body/cognition 层格位）
    pub bindings: BTreeMap<String, String>,
    /// [faces]：模块名 → 本形态启用的格位集合
    pub faces: BTreeMap<String, Vec<String>>,
}

/// 装配链交给 daemon 的 boot 输入
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3ProfileBoot {
    /// face 模块的 manifest 路径（模块名升序，确定性）
    pub manifests: Vec<PathBuf>,
    /// 缺格 (模块, 格位)：faces 声明但无有效 binding（或探测不到）→ 该模块不 spawn
    pub missing: Vec<(String, String)>,
}

/// 装配单路径解析成绝对路径：装载与横幅凭证共用同一个解析结果。
/// 装配单**必须已存在**才可装载，所以用 `fs::canonicalize`（与主题契约/叠加层同工具、
/// 同一条"致命文案不漏 `..`"的理据，见 `topics_load::rr3_contract_resolved`）。
pub fn rr3_profile_absolute_path(path: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path)
        .map_err(|e| format!("路径 {} 无法解析为绝对路径: {e}", path.display()))
}

/// 解析装配单文本。形状错误全部收集完才返回（运维一次改对），逐条点名、不静默放行。
pub fn rr3_profile_from_text(text: &str) -> Result<Rr3Profile, Vec<String>> {
    let root: toml::Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("装配单 TOML 解析失败: {e}")]),
    };
    let mut errs = Vec::new();
    match root.get("schema_version").and_then(toml::Value::as_integer) {
        Some(v) if v == RR3_PROFILE_SCHEMA_VERSION => {}
        other => errs.push(format!(
            "schema_version 必须为 {RR3_PROFILE_SCHEMA_VERSION}，收到 {other:?}"
        )),
    }
    // [bindings]：非空表、每格非空字符串值。[os] 段运行期不读，解析时无视（见文件头）。
    let mut bindings = BTreeMap::new();
    match root.get("bindings").and_then(toml::Value::as_table) {
        None => errs
            .push("缺少 [bindings]（body/cognition 格位 → 模块；可为空表但段必须在）".to_string()),
        Some(t) => {
            for (slot, v) in t {
                match v.as_str() {
                    Some(m) if !m.is_empty() => {
                        bindings.insert(slot.clone(), m.to_string());
                    }
                    _ => errs.push(format!("bindings.{slot} 应为非空字符串（模块名）")),
                }
            }
        }
    }
    // [faces]：非空表、每值非空字符串数组。faces 决定起谁（Q3 裁定），空表 = 无可起模块。
    let mut faces = BTreeMap::new();
    match root.get("faces").and_then(toml::Value::as_table) {
        None => errs.push("缺少 [faces]（模块 → 本形态启用的格位集合）".to_string()),
        Some(t) => {
            if t.is_empty() {
                errs.push("[faces] 为空：faces 决定起谁，空装配单没有可起模块".to_string());
            }
            for (module, v) in t {
                match v.as_array() {
                    Some(items) => {
                        let mut slots = Vec::new();
                        for item in items {
                            match item.as_str() {
                                Some(s) if !s.is_empty() => slots.push(s.to_string()),
                                _ => errs.push(format!(
                                    "faces.{module} 的每一项都必须是非空字符串（格位名）"
                                )),
                            }
                        }
                        faces.insert(module.clone(), slots);
                    }
                    None => errs.push(format!("faces.{module} 应为字符串数组")),
                }
            }
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(Rr3Profile { bindings, faces })
}

/// 装配链 boot 输入装配（daemon 在 bind 之前调用，fatal 即非 0 退出、套接字不留残迹）。
/// 致命错误（读不到 / 形状不合 / 查册未命中 / 结构不自洽）打包成一条串返回；
/// 缺格**不是**致命项（P2：缺格只降点名模块，缺格≠拒 boot），进 [`Rr3ProfileBoot::missing`]。
pub fn rr3_profile_boot_inputs(profile_path: &Path) -> Result<Rr3ProfileBoot, String> {
    let abs = rr3_profile_absolute_path(profile_path)?;
    let text = std::fs::read_to_string(&abs)
        .map_err(|e| format!("读不到装配单 {}: {e}", abs.display()))?;
    let profile = rr3_profile_from_text(&text).map_err(|errs| {
        errs.iter()
            .map(|e| format!("装配单 {} 形状不合: {e}", abs.display()))
            .collect::<Vec<_>>()
            .join("; ")
    })?;

    // 查册门（裁定 5②）：格位真值只有 rr-registry 那一册，CI 全集层与本入口共用。
    let slots_abs = std::fs::canonicalize(Path::new(RR3_CONTRACT_SLOTS_PATH))
        .map_err(|e| format!("读不到格位契约 {}: {e}", RR3_CONTRACT_SLOTS_PATH))?;
    let slots_text = std::fs::read_to_string(&slots_abs)
        .map_err(|e| format!("读不到格位契约 {}: {e}", slots_abs.display()))?;
    let table: Rr3SlotTable = rr3_registry_slot_table_from_text(&slots_text);
    if table.rr3_slot_table_is_empty() {
        return Err(format!(
            "格位契约 {} 解析后为空，拒绝启动（空格位表是契约事故）",
            slots_abs.display()
        ));
    }

    let mut errs: Vec<String> = Vec::new();
    // bindings 的格位必须查册命中，且不得是基础层（基础层由基座内建承载，slots.toml 注记）
    for slot in profile.bindings.keys() {
        if !table.rr3_slot_table_contains(slot) {
            errs.push(format!(
                "bindings 格位 {slot} 未在册（contracts/slots.toml 无此格，装配单不得自造格位）"
            ));
        } else if table.rr3_slot_table_layer(slot).as_deref() == Some("base") {
            errs.push(format!(
                "bindings 不得绑定基础层格位 {slot}（基础层由基座内建承载，恒在位）"
            ));
        }
    }
    // faces 的格位必须查册命中（基础层允许——恒在位，只记账不绑定）
    for (module, slots) in &profile.faces {
        for slot in slots {
            if !table.rr3_slot_table_contains(slot) {
                errs.push(format!(
                    "faces 模块 {module} 的格位 {slot} 未在册（contracts/slots.toml 无此格）"
                ));
            }
        }
    }
    // 结构自洽：每条 binding 都必须被 faces 认领（faces[M] 含该格位）——
    // 否则它是死数据：绑了没人用，装配单与 faces 各说一套。
    for (slot, module) in &profile.bindings {
        let claimed = profile
            .faces
            .get(module)
            .is_some_and(|slots| slots.contains(slot));
        if !claimed {
            errs.push(format!(
                "bindings 条目 {slot}={module} 未被 faces 认领（模块 {module} 不在 faces，或其 faces 不含 {slot}）"
            ));
        }
    }
    if !errs.is_empty() {
        return Err(errs.join("; "));
    }

    // 缺格判定：faces 声明的非基础层格位没有指向本模块的有效 binding（或探测不到）。
    // 基础层恒在位，跳过（slots.toml 注记：net.online / power.battery boot 即在位、恒不降级）。
    let probe = crate::probe::rr3_probe_default();
    let mut missing: Vec<(String, String)> = Vec::new();
    // 探测先行：bindings 声明的格位逐格探测（sim 后端全在位；探测自身故障按"探测不到"处理，
    // 方案 §4）。0.0.2 缺格只由装配单缺 binding 表达，这条环是板上探测批次的接缝。
    for (slot, module) in &profile.bindings {
        if !probe.rr3_probe_slot(slot) {
            missing.push((module.clone(), slot.clone()));
        }
    }
    for (module, slots) in &profile.faces {
        for slot in slots {
            let base = table.rr3_slot_table_layer(slot).as_deref() == Some("base");
            if base {
                continue;
            }
            if profile.bindings.get(slot) != Some(module)
                && !missing.contains(&(module.clone(), slot.clone()))
            {
                missing.push((module.clone(), slot.clone()));
            }
        }
    }
    missing.sort();

    // manifest 解析：modules-root = 装配单路径的上两级（<root>/profiles/<机>.toml）。
    let root = abs.parent().and_then(Path::parent).ok_or_else(|| {
        format!(
            "装配单 {} 无法定位 modules-root（须在 <root>/profiles/ 下）",
            abs.display()
        )
    })?;
    let manifests: Vec<PathBuf> = profile
        .faces
        .keys()
        .map(|m| root.join("modules").join(m).join("module.toml"))
        .collect();
    Ok(Rr3ProfileBoot { manifests, missing })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全部规则走真的 contracts/slots.toml（查册真值同源），夹具落 pid 唯一临时目录
    struct Root {
        dir: PathBuf,
    }

    const GOOD: &str = r#"
schema_version = 1

[os]
route = "distro"

[bindings]
"act.servo" = "rr3-pa"
"sensor.env" = "rr3-pb"
"cognition.llm" = "rr3-pc"

[faces]
rr3-pa = ["act.servo"]
rr3-pb = ["sensor.env"]
rr3-pc = ["cognition.llm"]
"#;

    impl Root {
        fn new(tag: &str) -> Root {
            let dir =
                std::env::temp_dir().join(format!("rr3-profile-{}-{tag}", std::process::id()));
            std::fs::create_dir_all(dir.join("profiles")).expect("建 profiles 目录");
            Root { dir }
        }

        fn write_profile(&self, name: &str, text: &str) -> PathBuf {
            let p = self.dir.join("profiles").join(name);
            std::fs::write(&p, text).expect("写装配单");
            p
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn good_profile_parses() {
        let p = rr3_profile_from_text(GOOD).expect("应能解析");
        assert_eq!(p.bindings.len(), 3);
        assert_eq!(p.faces.len(), 3);
        assert_eq!(p.faces["rr3-pa"], vec!["act.servo".to_string()]);
        // [os].route 被无视（运行期不读），但它的存在不构成错误
    }

    #[test]
    fn parse_errors_are_all_collected_and_named() {
        let text = GOOD
            .replace("schema_version = 1", "schema_version = 2")
            .replace("\"act.servo\" = \"rr3-pa\"", "\"act.servo\" = \"\"")
            .replace("rr3-pb = [\"sensor.env\"]", "rr3-pb = [\"\"]")
            .replace("rr3-pc = [\"cognition.llm\"]", "rr3-pc = 3");
        let errs = rr3_profile_from_text(&text).expect_err("形状不合必须报错");
        assert_eq!(errs.len(), 4, "{errs:?}");
        assert!(
            errs.iter().any(|e| e.contains("schema_version")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("bindings.act.servo")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("faces.rr3-pb 的每一项")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("faces.rr3-pc 应为字符串数组")),
            "{errs:?}"
        );
    }

    #[test]
    fn missing_sections_and_empty_faces_are_errors() {
        let errs = rr3_profile_from_text("schema_version = 1\n").expect_err("缺段必须报错");
        assert!(
            errs.iter().any(|e| e.contains("缺少 [bindings]")),
            "{errs:?}"
        );
        assert!(errs.iter().any(|e| e.contains("缺少 [faces]")), "{errs:?}");

        let errs = rr3_profile_from_text("schema_version = 1\n[bindings]\n\n[faces]\n")
            .expect_err("空 faces 必须报错");
        assert!(errs.iter().any(|e| e.contains("[faces] 为空")), "{errs:?}");
    }

    #[test]
    fn unparsable_toml_is_single_error() {
        let errs = rr3_profile_from_text("[bindings\n").expect_err("必须报错");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].starts_with("装配单 TOML 解析失败"), "{errs:?}");
    }

    /// 满配：manifest 路径按 <root>/modules/<名>/module.toml 解析，缺格为空
    #[test]
    fn full_profile_resolves_manifests_and_finds_no_missing() {
        let root = Root::new("full");
        let p = root.write_profile("full.toml", GOOD);
        let boot = rr3_profile_boot_inputs(&p).expect("满配装配单应通过");
        assert!(boot.missing.is_empty(), "{:?}", boot.missing);
        assert_eq!(boot.manifests.len(), 3);
        // 比对基准必须是**规范化后**的根目录：boot_inputs 拿到的 manifest 路径派生自
        // canonicalize 过的装配单路径（macOS 的 /tmp 是 /private/tmp 的符号链接），
        // 拿夹具的原始路径比对就会红在系统布局上，不是红在被测行为。
        let modules_root = std::fs::canonicalize(&root.dir)
            .expect("夹具根目录必须可规范化")
            .display()
            .to_string();
        for (i, name) in ["rr3-pa", "rr3-pb", "rr3-pc"].iter().enumerate() {
            assert_eq!(
                boot.manifests[i],
                Path::new(&modules_root)
                    .join("modules")
                    .join(name)
                    .join("module.toml"),
                "manifest 路径必须按 modules-root 解析且升序"
            );
        }
    }

    /// 缺格：bindings 移除任一格 → (模块, 格位) 进缺格清单，其余不受牵连
    #[test]
    fn removed_binding_is_missing_for_its_module_only() {
        let root = Root::new("missing");
        let p = root.write_profile(
            "missing.toml",
            &GOOD.replace("\"sensor.env\" = \"rr3-pb\"\n", ""),
        );
        let boot = rr3_profile_boot_inputs(&p).expect("缺格不是致命项");
        assert_eq!(
            boot.missing,
            vec![("rr3-pb".to_string(), "sensor.env".to_string())],
            "只有点名模块缺格"
        );
        assert_eq!(
            boot.manifests.len(),
            3,
            "缺格模块的 manifest 仍在解析结果里（供日志点名）"
        );
    }

    /// faces 声明了格位但 binding 指向别的模块：对点名模块算缺格，同时构成"binding 未被认领"
    /// 的结构不自洽 → 致命（fail-closed，装配单各说一套必须当场听见）
    #[test]
    fn binding_claimed_by_another_module_is_fatal() {
        let root = Root::new("cross");
        let text = GOOD.replace("\"sensor.env\" = \"rr3-pb\"", "\"sensor.env\" = \"rr3-pc\"");
        let p = root.write_profile("cross.toml", &text);
        let e = rr3_profile_boot_inputs(&p).expect_err("binding 被他人认领必须致命");
        assert!(e.contains("未被 faces 认领"), "{e}");
    }

    /// 基础层格位进 bindings = 违反"基础层由基座内建承载"（slots.toml 注记）→ 致命
    #[test]
    fn base_layer_binding_is_fatal() {
        let root = Root::new("base");
        let text = GOOD.replace("[bindings]\n", "[bindings]\n\"bus.publish\" = \"rr3-pa\"\n");
        let p = root.write_profile("base.toml", &text);
        let e = rr3_profile_boot_inputs(&p).expect_err("基础层 binding 必须致命");
        assert!(e.contains("bindings 不得绑定基础层格位 bus.publish"), "{e}");
    }

    /// 查册门：faces/bindings 引用未在册格位 → 致命并点名（真契约上跑）
    #[test]
    fn unknown_slot_is_fatal_and_named() {
        let root = Root::new("unknown");
        let text = GOOD.replace("rr3-pc = [\"cognition.llm\"]", "rr3-pc = [\"sensor.wifi\"]");
        let p = root.write_profile("unknown.toml", &text);
        let e = rr3_profile_boot_inputs(&p).expect_err("未在册格位必须致命");
        assert!(
            e.contains("faces 模块 rr3-pc 的格位 sensor.wifi 未在册"),
            "{e}"
        );
    }

    /// 基础层格位进 faces 只记账、不算缺格（power.battery 恒在位，default 机同款形状）
    #[test]
    fn base_layer_face_slot_is_never_missing() {
        let root = Root::new("baseface");
        let text = GOOD.replace(
            "rr3-pc = [\"cognition.llm\"]",
            "rr3-pc = [\"cognition.llm\", \"power.battery\"]",
        );
        let p = root.write_profile("baseface.toml", &text);
        let boot = rr3_profile_boot_inputs(&p).expect("基础层 face 格位不缺格");
        assert!(boot.missing.is_empty(), "{:?}", boot.missing);
    }

    /// 装配单文件不存在 = 致命，文案是绝对化口径（canonicalize 的 ENOENT）
    #[test]
    fn missing_profile_file_is_fatal() {
        let e = rr3_profile_boot_inputs(Path::new("/nonexistent/rr3-nope.toml"))
            .expect_err("读不到装配单必须致命");
        assert!(e.contains("无法解析为绝对路径"), "{e}");
    }
}
