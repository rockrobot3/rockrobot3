//! staging 装配：骨架/模块/真值/配置/落位与自检。

use super::*;

/// staging 装配（纯 Rust）：把两份真值落成 deb 布局树，返回 staging 目录路径。
/// 路径 = `--out` 值加 `.stage` 后缀（与产物相邻，清掉即重来，不污染别处）。
pub fn rr3_stage(cfg: &Rr3PackConfig) -> Result<PathBuf, String> {
    let (names, profile_name, truth) = rr3_stage_check(cfg)?;
    rr3_stage_tree(cfg, &names, &profile_name, &truth)
}

/// 骨架 + 逐模块装配（预验已过，这里只落树）。
fn rr3_stage_tree(
    cfg: &Rr3PackConfig,
    names: &[String],
    profile_name: &str,
    truth: &rrd::profile::Rr3RuntimeTruthSet,
) -> Result<PathBuf, String> {
    let (stage, root) = rr3_stage_root(cfg)?;
    rr3_stage_skeleton(cfg, &stage, &root, profile_name, truth)?;
    rr3_stage_modules(cfg, &root, names)?;
    Ok(stage)
}

/// staging 目录（`--out` 加 `.stage` 后缀）与安装根；旧树先清空。
fn rr3_stage_root(cfg: &Rr3PackConfig) -> Result<(PathBuf, PathBuf), String> {
    let stage = cfg.out.with_file_name(format!(
        "{}.stage",
        cfg.out
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("out")
    ));
    rr3_stage_reset(&stage)?;
    let root = stage.join("usr/lib/rockrobot3");
    Ok((stage, root))
}

/// deb 骨架：目录/入口产物 + systemd 单元 + 数据真值。
fn rr3_stage_skeleton(
    cfg: &Rr3PackConfig,
    stage: &Path,
    root: &Path,
    profile_name: &str,
    truth: &rrd::profile::Rr3RuntimeTruthSet,
) -> Result<(), String> {
    rr3_stage_layout(cfg, stage)?;
    rr3_stage_unit(stage, profile_name)?;
    rr3_stage_truth(cfg, root, truth)
}

/// 逐模块：manifest（entry 改写）+ 产物 + config。
fn rr3_stage_modules(cfg: &Rr3PackConfig, root: &Path, names: &[String]) -> Result<(), String> {
    for name in names {
        rr3_stage_module(cfg, root, name)?;
    }
    Ok(())
}

/// staging 目录复用语义：**先清空再装配**。上一次失败或模块下线留下的文件不能
/// 被打进本次 deb——陈旧模块是"包里多一件"最贵的错法；清理失败直接报错，不静默
/// 续装。
fn rr3_stage_reset(stage: &Path) -> Result<(), String> {
    if stage.exists() {
        fs::remove_dir_all(stage)
            .map_err(|e| format!("清理旧 staging 树 {} 失败: {e}", stage.display()))?;
    }
    Ok(())
}

/// 注入产物完整性 + 模块名单 + 装载真值预验（缺席逐件点名，不静默跳过——
/// 装机半成品比不出包更危险）。
fn rr3_stage_check(
    cfg: &Rr3PackConfig,
) -> Result<(Vec<String>, String, rrd::profile::Rr3RuntimeTruthSet), String> {
    let names = rr3_stage_names(cfg)?;
    let (profile_name, truth) = rr3_truth_ready(cfg)?;
    Ok((names, profile_name, truth))
}

/// 注入产物两入口在场门 + 模块名单（缺件逐件点名）。
fn rr3_stage_names(cfg: &Rr3PackConfig) -> Result<Vec<String>, String> {
    rr3_payload_bins(cfg)?;
    rr3_module_names(&cfg.payload)
}

/// 两个入口产物在场门（缺席逐件点名）。
fn rr3_payload_bins(cfg: &Rr3PackConfig) -> Result<(), String> {
    for bin in ["rrd", "rr3-ctl"] {
        rr3_bin_ready(cfg, bin)?;
    }
    Ok(())
}

/// 单件注入产物在场门（缺席点名）。
fn rr3_bin_ready(cfg: &Rr3PackConfig, bin: &str) -> Result<(), String> {
    let p = cfg.payload.join(bin);
    if !p.is_file() {
        return Err(format!(
            "注入产物 {bin} 缺席: {}（约定 --payload 下有 rrd、rr3-ctl、rr3-modules/<name>/{{bin,manifest.toml}}）",
            p.display()
        ));
    }
    Ok(())
}

/// deb 骨架：目录 + 两个入口产物 + control/postinst。
fn rr3_stage_layout(cfg: &Rr3PackConfig, stage: &Path) -> Result<(), String> {
    rr3_stage_dirs(stage)?;
    rr3_stage_bins(cfg, stage)?;
    rr3_stage_debian(cfg, stage)
}

/// 两个入口产物落 usr/bin（错误文案带产物名）。
fn rr3_stage_bins(cfg: &Rr3PackConfig, stage: &Path) -> Result<(), String> {
    for bin in ["rrd", "rr3-ctl"] {
        fs::copy(cfg.payload.join(bin), stage.join("usr/bin").join(bin))
            .map_err(|e| format!("复制 {bin} 失败: {e}"))?;
    }
    Ok(())
}

/// 骨架目录三件。
fn rr3_stage_dirs(stage: &Path) -> Result<(), String> {
    for d in [
        stage.join("DEBIAN"),
        stage.join("usr/bin"),
        stage.join("usr/lib/systemd/system"),
    ] {
        fs::create_dir_all(&d).map_err(|e| format!("建目录 {} 失败: {e}", d.display()))?;
    }
    Ok(())
}

/// control + postinst（postinst 落位即置可执行位）。
fn rr3_stage_debian(cfg: &Rr3PackConfig, stage: &Path) -> Result<(), String> {
    fs::write(stage.join("DEBIAN/control"), rr3_control_text(&cfg.arch))
        .map_err(|e| format!("写 control 失败: {e}"))?;
    let postinst = stage.join("DEBIAN/postinst");
    fs::write(&postinst, rr3_postinst_text()).map_err(|e| format!("写 postinst 失败: {e}"))?;
    rr3_make_executable(&postinst);
    Ok(())
}

/// systemd 单元：--profile 装机位随 --profile 改写（内容改写②，见模块头注）。
/// 模板锚定缺省机型 distro.toml，非 distro 机型照抄则 systemd 拉起的 rrd 找不到
/// 装配单；锚点缺席先报错（对表 base/os/distro/units/rrd.service），不静默照抄。
fn rr3_stage_unit(stage: &Path, profile_name: &str) -> Result<(), String> {
    const SERVICE_PROFILE_ANCHOR: &str = "/usr/lib/rockrobot3/profiles/distro.toml";
    if !RR3_SERVICE_UNIT.contains(SERVICE_PROFILE_ANCHOR) {
        return Err(format!(
            "rrd.service 模板缺 --profile 装机位锚点 {SERVICE_PROFILE_ANCHOR}——对表 base/os/distro/units/rrd.service 后再出包"
        ));
    }
    fs::write(
        stage.join("usr/lib/systemd/system/rrd.service"),
        RR3_SERVICE_UNIT.replace(
            SERVICE_PROFILE_ANCHOR,
            &format!("{RR3_INSTALL_ROOT}/profiles/{profile_name}"),
        ),
    )
    .map_err(|e| format!("写 rrd.service 失败: {e}"))
}

/// 数据真值随包：按清单逐条落位（files 直落、dirs 在则整目录随装）。装机布局
/// 与仓根同名同布（清单相对路径两侧通用）；轴档案目录缺席合法跳过——与装载链
/// rr3_axis_files_load 的"无 axis 目录 = 无档案"同语义（基-25 清单 dirs 成员）。
fn rr3_stage_truth(
    cfg: &Rr3PackConfig,
    root: &Path,
    truth: &rrd::profile::Rr3RuntimeTruthSet,
) -> Result<(), String> {
    for rel in &truth.files {
        rr3_stage_one_file(cfg, root, rel)?;
    }
    rr3_stage_truth_dirs(cfg, root, truth)
}

/// 目录真值随装（在则整目录随装，缺席合法跳过）。
fn rr3_stage_truth_dirs(
    cfg: &Rr3PackConfig,
    root: &Path,
    truth: &rrd::profile::Rr3RuntimeTruthSet,
) -> Result<(), String> {
    for rel in &truth.dirs {
        rr3_stage_one_dir(cfg, root, rel)?;
    }
    Ok(())
}

/// 单册落位：父目录就绪后按清单相对路径同名复制。
fn rr3_stage_one_file(cfg: &Rr3PackConfig, root: &Path, rel: &str) -> Result<(), String> {
    let dst = root.join(rel);
    rr3_stage_parent(&dst, rel)?;
    fs::copy(cfg.repo.join(rel), &dst).map_err(|e| format!("复制 {rel} 失败: {e}"))?;
    Ok(())
}

/// 落位父目录就绪（清单相对路径无父目录即拒）。
fn rr3_stage_parent(dst: &Path, rel: &str) -> Result<(), String> {
    let parent = dst
        .parent()
        .ok_or_else(|| format!("清单相对路径异常（无父目录）: {rel}"))?;
    fs::create_dir_all(parent).map_err(|e| format!("建目录 {} 失败: {e}", parent.display()))
}

/// 单目录落位：在则整目录随装（轴档案目录缺席合法跳过）。
fn rr3_stage_one_dir(cfg: &Rr3PackConfig, root: &Path, rel: &str) -> Result<(), String> {
    let src = cfg.repo.join(rel);
    if src.is_dir() {
        rr3_copy_dir(&src, &root.join(rel))?;
    }
    Ok(())
}

/// 装载消费真值集预验（基-25／实-12）：打包与装载同一判据，清单真值只有 rrd
/// `rr3_truth_set` 一份；坏单/缺册都拒出包，不在打包面自带第二份文件清单。
fn rr3_truth_ready(
    cfg: &Rr3PackConfig,
) -> Result<(String, rrd::profile::Rr3RuntimeTruthSet), String> {
    let profile_name = format!("{}.toml", cfg.profile);
    let profile_src = cfg.repo.join("profiles").join(&profile_name);
    // 打包先按装载同判据预验装配单（rr3_profile_parse，与 boot 链同一函数）：坏单
    // 拒出包——出包时红比装机 boot 红早一个环节（块-15：装载必需件缺席而装机验证全绿
    // 才是最贵的洞）；预验同时给出 [platform].chip，平台事实表按它选册。
    let profile = rr3_profile_precheck(&profile_src)?;
    let truth = rr3_truth_collect(cfg, &profile)?;
    Ok((profile_name, truth))
}

/// 真值集构造 + 缺册门（清单真值只有 rrd `rr3_truth_set` 一份）。
fn rr3_truth_collect(
    cfg: &Rr3PackConfig,
    profile: &rrd::profile::Rr3Profile,
) -> Result<rrd::profile::Rr3RuntimeTruthSet, String> {
    let truth = rrd::profile::rr3_truth_set(profile, &cfg.profile);
    rr3_truth_missing(cfg, &truth)?;
    Ok(truth)
}

/// 装载消费真值集缺册门（缺任一必需册即拒出包）。
fn rr3_truth_missing(
    cfg: &Rr3PackConfig,
    truth: &rrd::profile::Rr3RuntimeTruthSet,
) -> Result<(), String> {
    let missing: Vec<String> = truth
        .files
        .iter()
        .filter(|rel| !cfg.repo.join(rel).is_file())
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "装载消费真值集缺册: {}（--repo 须指向仓根 {}；清单真值 = rrd rr3_truth_set——缺任一必需册 rrd 装载即拒 boot，打包拒出包，基-25/实-12）",
            missing.join(", "),
            cfg.repo.display()
        ));
    }
    Ok(())
}

/// 装配单读取 + 预验（打包与装载同一判据，坏单点名拒收）。
fn rr3_profile_precheck(profile_src: &Path) -> Result<rrd::profile::Rr3Profile, String> {
    let text = fs::read_to_string(profile_src)
        .map_err(|e| format!("读不到装配单 {}: {e}", profile_src.display()))?;
    rrd::profile::rr3_profile_parse(&text).map_err(|errs| {
        format!(
            "装配单 {} 预验失败（打包与装载同一判据，坏单不得出包）: {}",
            profile_src.display(),
            errs.join("; ")
        )
    })
}

/// 单模块落 staging：产物按 lang 分型（rust = 注入 bin 按 entry 落位；python =
/// pkg 树落位 + 打包器生成 shim——PYTHONPATH 须装机绝对，只有打包器知道
/// RR3_INSTALL_ROOT）；config 与可选原子件/编排/形态档按"在位即纯副本"随装。
fn rr3_stage_module(cfg: &Rr3PackConfig, root: &Path, name: &str) -> Result<(), String> {
    let mod_src = cfg.payload.join("rr3-modules").join(name);
    let staged = root.join("modules").join(name);
    let (facts, entry_path) = rr3_stage_manifest(&mod_src, &staged, name)?;
    rr3_stage_lang(&mod_src, &staged, &entry_path, name, &facts.lang)?;
    rr3_stage_cfg(cfg, &staged, name)?;
    rr3_stage_opt(cfg, &staged, name)
}

/// 产物按 lang 分型落位（python = pkg 树 + shim；其余 = 注入 bin 按 entry 落位）。
fn rr3_stage_lang(
    mod_src: &Path,
    staged: &Path,
    entry_path: &Path,
    name: &str,
    lang: &str,
) -> Result<(), String> {
    if lang == "python" {
        return rr3_stage_py(mod_src, staged, entry_path, name);
    }
    rr3_stage_bin(mod_src, entry_path, name)
}

/// manifest 改写落位 + entry 父目录就绪；返回 facts 与 entry 装机路径。
fn rr3_stage_manifest(
    mod_src: &Path,
    staged: &Path,
    name: &str,
) -> Result<(rrd::manifest::Rr3ManifestFacts, PathBuf), String> {
    let (text, facts) = rr3_mod_facts(mod_src, name)?;
    rr3_stage_mod_write(staged, name, &text, &facts)?;
    let entry_path = staged.join(&facts.entry);
    rr3_stage_entry_dir(&entry_path)?;
    Ok((facts, entry_path))
}

/// manifest 改写落位（entry 父目录由调用方按装机路径再就绪）。
fn rr3_stage_mod_write(
    staged: &Path,
    name: &str,
    text: &str,
    facts: &rrd::manifest::Rr3ManifestFacts,
) -> Result<(), String> {
    fs::create_dir_all(staged).map_err(|e| format!("建目录 {} 失败: {e}", staged.display()))?;
    rr3_stage_toml_write(staged, name, text, facts)
}

/// manifest 改写（entry 装机绝对化）后落位。
fn rr3_stage_toml_write(
    staged: &Path,
    name: &str,
    text: &str,
    facts: &rrd::manifest::Rr3ManifestFacts,
) -> Result<(), String> {
    let installed_entry = format!("{RR3_INSTALL_ROOT}/modules/{name}/{}", facts.entry);
    let rewritten = rr3_manifest_rewrite(text, &installed_entry, &facts.entry, name)?;
    fs::write(staged.join("module.toml"), rewritten)
        .map_err(|e| format!("写 {}/module.toml 失败: {e}", staged.display()))
}

/// entry 父目录就绪（入口可能带子目录）。
fn rr3_stage_entry_dir(entry_path: &Path) -> Result<(), String> {
    if let Some(parent) = entry_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("建目录 {} 失败: {e}", parent.display()))?;
    }
    Ok(())
}

/// 读取并校验注入产物 manifest：文件在否 → 解析 → 目录名与 manifest.name 一致
/// → entry 安全（绝对路径/上级目录逃逸在此拒）。
fn rr3_mod_facts(
    mod_src: &Path,
    name: &str,
) -> Result<(String, rrd::manifest::Rr3ManifestFacts), String> {
    let manifest_src = mod_src.join("manifest.toml");
    let (text, facts) = rr3_facts_load(&manifest_src, name)?;
    rr3_name_agree(&facts, name)?;
    rr3_entry_ok(&facts.entry, name)?;
    Ok((text, facts))
}

/// 注入产物 manifest 读入并解析（在场门与两条错误文案逐字保留）。
fn rr3_facts_load(
    manifest_src: &Path,
    name: &str,
) -> Result<(String, rrd::manifest::Rr3ManifestFacts), String> {
    let text = rr3_facts_text(manifest_src, name)?;
    let facts = rr3_facts_parse(&text, name)?;
    Ok((text, facts))
}

/// manifest 文本读入（缺席先点名，读失败带路径）。
fn rr3_facts_text(manifest_src: &Path, name: &str) -> Result<String, String> {
    if !manifest_src.is_file() {
        return Err(format!(
            "模块 {name} 注入产物不完整: {} 缺席（约定 rr3-modules/<name>/manifest.toml）",
            manifest_src.display()
        ));
    }
    fs::read_to_string(manifest_src).map_err(|e| format!("读 {} 失败: {e}", manifest_src.display()))
}

/// manifest 文本解析（坏形点名注入产物来源）。
fn rr3_facts_parse(text: &str, name: &str) -> Result<rrd::manifest::Rr3ManifestFacts, String> {
    rr3_manifest_parse(text).map_err(|errs| {
        format!(
            "模块 {name} 的 manifest 解析失败: {}（注入产物须是仓内 module.toml 的副本）",
            errs.join("; ")
        )
    })
}

/// 目录名与 manifest.name 必须一致（注入产物自相矛盾即拒）。
fn rr3_name_agree(facts: &rrd::manifest::Rr3ManifestFacts, name: &str) -> Result<(), String> {
    if facts.name != name {
        return Err(format!(
            "模块目录名 {name} 与 manifest.name {} 不一致（注入产物自相矛盾，打包拒绝）",
            facts.name
        ));
    }
    Ok(())
}

/// 仓内 config.toml 随包：模块开机即读 manifest 同目录 config.toml，缺件必重启环。
fn rr3_stage_cfg(cfg: &Rr3PackConfig, staged: &Path, name: &str) -> Result<(), String> {
    let config_src = cfg.repo.join("modules").join(name).join("config.toml");
    if !config_src.is_file() {
        return Err(format!(
            "仓内文件缺席: {}（模块开机即读 manifest 同目录 config.toml，缺件必重启环——随包必带）",
            config_src.display()
        ));
    }
    fs::copy(&config_src, staged.join("config.toml"))
        .map_err(|e| format!("复制 {name} config.toml 失败: {e}"))?;
    Ok(())
}

/// python 模块：pkg 树 + 装机绝对路径 shim。
fn rr3_stage_py(
    mod_src: &Path,
    staged: &Path,
    entry_path: &Path,
    name: &str,
) -> Result<(), String> {
    let pkg_src = mod_src.join("pkg");
    rr3_stage_pkg(&pkg_src, staged, name)?;
    rr3_stage_py_shim(entry_path, name)?;
    rr3_make_executable(entry_path);
    Ok(())
}

/// pkg 树落位（缺席先点名，复制失败带模块名）。
fn rr3_stage_pkg(pkg_src: &Path, staged: &Path, name: &str) -> Result<(), String> {
    if !pkg_src.is_dir() {
        return Err(format!(
            "模块 {name}（lang=python）注入产物不完整: {} 缺席（python 模块约定 rr3-modules/<name>/{{pkg/,manifest.toml}}）",
            pkg_src.display()
        ));
    }
    rr3_copy_dir(pkg_src, &staged.join("pkg")).map_err(|e| format!("复制 {name} pkg 树失败: {e}"))
}

/// python 入口 shim：装机绝对路径的 pkg/config 写进 entry。
fn rr3_stage_py_shim(entry_path: &Path, name: &str) -> Result<(), String> {
    let pkg_installed = format!("{RR3_INSTALL_ROOT}/modules/{name}/pkg");
    let config_installed = format!("{RR3_INSTALL_ROOT}/modules/{name}/config.toml");
    fs::write(
        entry_path,
        rr3_python_shim_text(
            Path::new(&pkg_installed),
            &name.replace('-', "_"),
            Path::new(&config_installed),
        ),
    )
    .map_err(|e| format!("写 {name} shim 失败: {e}"))
}

/// rust 模块：注入 bin 按 entry 落位。
fn rr3_stage_bin(mod_src: &Path, entry_path: &Path, name: &str) -> Result<(), String> {
    let bin_src = mod_src.join("bin");
    if !bin_src.is_file() {
        return Err(format!(
            "模块 {name} 注入产物不完整: {} 缺席（rust 模块约定 rr3-modules/<name>/{{bin,manifest.toml}}）",
            bin_src.display()
        ));
    }
    fs::copy(&bin_src, entry_path).map_err(|e| format!("复制 {name} bin 失败: {e}"))?;
    Ok(())
}

/// 可选随装件（G4/C1：组合件编排与原子件声明树；形态档 config-esp32 同款纪律，
/// 在位即纯副本——缺省不是错误，模块形态允许无原子件/无编排）。
fn rr3_stage_opt(cfg: &Rr3PackConfig, staged: &Path, name: &str) -> Result<(), String> {
    let repo_mod = cfg.repo.join("modules").join(name);
    rr3_stage_opt_files(&repo_mod, staged, name)?;
    rr3_stage_opt_tree(
        &repo_mod.join("atoms"),
        &staged.join("atoms"),
        &format!("复制 {name} atoms 树失败"),
    )
}

/// 两件可选单文件（config-esp32 / composite）随装，缺席合法跳过。
fn rr3_stage_opt_files(repo_mod: &Path, staged: &Path, name: &str) -> Result<(), String> {
    rr3_stage_opt_file(
        &repo_mod.join("config-esp32.toml"),
        &staged.join("config-esp32.toml"),
        &format!("复制 {name} config-esp32.toml 失败"),
    )?;
    rr3_stage_opt_file(
        &repo_mod.join("composite.toml"),
        &staged.join("composite.toml"),
        &format!("复制 {name} composite.toml 失败"),
    )
}

/// 单件可选随装：来源在位即原样复制，缺席合法跳过。
fn rr3_stage_opt_file(src: &Path, dst: &Path, ctx: &str) -> Result<(), String> {
    if !src.is_file() {
        return Ok(());
    }
    fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| format!("{ctx}: {e}"))
}

/// 单目录可选随装：来源在位即整树复制，缺席合法跳过。
fn rr3_stage_opt_tree(src: &Path, dst: &Path, ctx: &str) -> Result<(), String> {
    if !src.is_dir() {
        return Ok(());
    }
    rr3_copy_dir(src, dst).map_err(|e| format!("{ctx}: {e}"))
}
