//! 逐模块装配面：manifest 事实解析、装机改写、config/包/可执行件/opt 落位。
//!
//! 判据要点：module.toml 的 name 必须与目录名一致（facts 交叉核验）；entry
//! 装机绝对化后重解析复核；缺 bin/config 按警告+整模块跳过（不带病装）。
//!
//! 拆分动因：`pack/stage.rs` 513 行，模块装配与骨架/真值落树分属两簇。

use super::*;

/// 单模块落 staging：产物按 lang 分型（rust = 注入 bin 按 entry 落位；python =
/// pkg 树落位 + 打包器生成 shim——PYTHONPATH 须装机绝对，只有打包器知道
/// RR3_INSTALL_ROOT）；config 与可选原子件/编排/形态档按"在位即纯副本"随装。
pub(super) fn rr3_stage_module(cfg: &Rr3PackConfig, root: &Path, name: &str) -> Result<(), String> {
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

/// 三件可选单文件（config-esp32 / config-pi4b / composite）随装，缺席合法跳过。
fn rr3_stage_opt_files(repo_mod: &Path, staged: &Path, name: &str) -> Result<(), String> {
    rr3_stage_opt_file(
        &repo_mod.join("config-esp32.toml"),
        &staged.join("config-esp32.toml"),
        &format!("复制 {name} config-esp32.toml 失败"),
    )?;
    rr3_stage_opt_file(
        &repo_mod.join("config-pi4b.toml"),
        &staged.join("config-pi4b.toml"),
        &format!("复制 {name} config-pi4b.toml 失败"),
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
