//! 装配单文本与改写：sim 变换、entry 绝对化、装机布局与旁件拷贝。

use std::path::{Path, PathBuf};

use rrd::manifest::rr3_manifest_parse;
use rrd::profile::{rr3_profile_parse, Rr3Profile};

use crate::drill::rr3_bin_dir;

/// 仓源 manifest 的 entry 装机改写（B45 起委托 rrd 单点 [`rr3_manifest_rewrite`]：
/// 演练布局与 rr3-pack 打包共用同一实现，四道门与文案不再各写一份）。
pub(crate) fn rr3_entry_rewrite(
    text: &str,
    installed: &Path,
    facts_entry: &str,
    module: &str,
) -> Result<String, String> {
    rrd::manifest::rr3_manifest_rewrite(text, &installed.to_string_lossy(), facts_entry, module)
}

/// 装配布局的一条产物：模块名、entry 绝对路径
pub type Rr3AssembleEntry = (String, PathBuf);

/// 演练夹具：把仓源真值装进 workdir 的装机布局（modules-root），返回
/// (夹具装配单路径, 模块产物清单)。
/// 装配单经 [`rr3_asm_sim_text`] sim 变换后落位（仓源原文零改）；接线档
/// 同批原文拷贝零改（`hardware/<形态>/wiring.toml`，校验⑦ 要求装机档必在，缺件
/// fail-fast）；manifest 仅 entry 绝对化；config 同目录就位。
pub fn rr3_assemble_layout(
    repo: &Path,
    workdir: &Path,
    profile_src: &Path,
) -> Result<(PathBuf, Vec<Rr3AssembleEntry>), String> {
    let (profile_dst, profile) = rr3_asm_profile_read(workdir, profile_src)?;
    let entries = rr3_asm_layout_rest(repo, workdir, profile_src, &profile)?;
    Ok((profile_dst, entries))
}

/// 接线档随装 + 逐模块装机清单（装配单已读定）。
fn rr3_asm_layout_rest(
    repo: &Path,
    workdir: &Path,
    profile_src: &Path,
    profile: &Rr3Profile,
) -> Result<Vec<Rr3AssembleEntry>, String> {
    rr3_asm_wiring(repo, workdir, profile_src)?;
    rr3_layout_entries(repo, workdir, profile)
}

/// 装配单：读源 → sim 变换 → 复核解析 → 落位，返回 (落位路径, 解析结果)。
fn rr3_asm_profile_read(
    workdir: &Path,
    profile_src: &Path,
) -> Result<(PathBuf, Rr3Profile), String> {
    let profile_text = rr3_asm_profile_text(profile_src)?;
    rr3_asm_layout(workdir, profile_src, &profile_text)
}

/// 源装配单读入（读不到点名源路径）。
fn rr3_asm_profile_text(profile_src: &Path) -> Result<String, String> {
    std::fs::read_to_string(profile_src)
        .map_err(|e| format!("读不到装配单 {}: {e}", profile_src.display()))
}

/// sim 变换 → 复核解析 → 落位。
fn rr3_asm_layout(
    workdir: &Path,
    profile_src: &Path,
    profile_text: &str,
) -> Result<(PathBuf, Rr3Profile), String> {
    let sim_text = rr3_asm_sim_text(profile_text)?;
    let profile = rr3_asm_parse(&sim_text, profile_src)?;
    let profile_dst = rr3_layout_profile(workdir, profile_src, &sim_text)?;
    Ok((profile_dst, profile))
}

/// sim 变换后复核解析（坏单点名源装配单）。
fn rr3_asm_parse(sim_text: &str, profile_src: &Path) -> Result<Rr3Profile, String> {
    rr3_profile_parse(sim_text).map_err(|errs| {
        format!(
            "装配单 {} sim 变换后不可解析: {errs:?}",
            profile_src.display()
        )
    })
}

/// 接线档同批装机（文件名主干定 `hardware/<形态>/wiring.toml`）。
fn rr3_asm_wiring(repo: &Path, workdir: &Path, profile_src: &Path) -> Result<(), String> {
    let stem = rr3_layout_stem(profile_src)?;
    rr3_layout_wiring(repo, workdir, stem)
}

/// 逐模块装机清单（manifest entry 绝对化 + 旁件），顺序随装配单。
fn rr3_layout_entries(
    repo: &Path,
    workdir: &Path,
    profile: &Rr3Profile,
) -> Result<Vec<Rr3AssembleEntry>, String> {
    let mut entries = Vec::with_capacity(profile.enabled_slots.len());
    for module in profile.enabled_slots.keys() {
        entries.push(rr3_layout_module(repo, workdir, module)?);
    }
    Ok(entries)
}

/// 装配单键名摘取（行首 `key =` 形；非键行 None）。
fn rr3_profile_key_of(line: &str) -> Option<&str> {
    let t = line.trim();
    let eq = t.find('=')?;
    let k = t[..eq].trim();
    let ok = !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    ok.then_some(k)
}

/// sim 演练装配单变换：仓源装配单是部署真值（P0-3 起 `[runtime].hal_backend`
/// 必填、真件档声明 linux/esp32 等真后端），而演练环境（开发机/CI runner）无
/// 硬件面可兑现——沙箱副本改写为 sim 面：`hal_backend` → `"sim"`，可选真后端
/// 专面覆盖（`audio_backend`/`servo_backend`/`display_backend`）整行摘除。
/// 与 [`rr3_asm_up_rrd`] 的 rrd env、`--chain` 的子进程注入同一语义；机制与
/// 策略分离：仓源装配单零改，变换只落 `profiles/<名>.toml` 沙箱副本。改写后
/// 由调用方重解析复核（hal_backend 必填缺行即红，其余字段保真——与 manifest
/// entry 绝对化同一条"改写后复核"纪律）。
pub(crate) fn rr3_asm_sim_text(text: &str) -> Result<String, String> {
    let (out, saw_hal) = rr3_asm_transform(text);
    rr3_asm_hal_gate(saw_hal)?;
    Ok(out)
}

/// 逐行 sim 变换本体（返回变换文本与是否见到 `hal_backend`）。
fn rr3_asm_transform(text: &str) -> (String, bool) {
    let mut out = String::with_capacity(text.len() + 16);
    let mut in_runtime = false;
    let mut saw_hal = false;
    for line in text.lines() {
        if rr3_asm_line(line, &mut in_runtime, &mut saw_hal, &mut out) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    (out, saw_hal)
}

/// 变换后必填门：`hal_backend` 未见即红（无落点）。
fn rr3_asm_hal_gate(saw_hal: bool) -> Result<(), String> {
    if !saw_hal {
        return Err("装配单 [runtime] 缺 hal_backend（P0-3 必填）——sim 变换无落点".into());
    }
    Ok(())
}

/// 单行分派：段头与 [runtime] 段内改写（`true` = 本行已输出，不再原样追加）。
fn rr3_asm_line(line: &str, in_runtime: &mut bool, saw_hal: &mut bool, out: &mut String) -> bool {
    let t = line.trim_start();
    if t.starts_with('[') {
        *in_runtime = t.trim() == "[runtime]";
        out.push_str(line);
        out.push('\n');
        return true;
    }
    rr3_asm_runtime_line(line, *in_runtime, saw_hal, out)
}

/// [runtime] 段内单行改写：`true` = 本行已改写或摘除，不再原样输出。
fn rr3_asm_runtime_line(
    line: &str,
    in_runtime: bool,
    saw_hal: &mut bool,
    out: &mut String,
) -> bool {
    if !in_runtime {
        return false;
    }
    match rr3_profile_key_of(line) {
        Some("hal_backend") => {
            *saw_hal = true;
            out.push_str("hal_backend = \"sim\"\n");
            true
        }
        other => rr3_asm_opt_drop(other),
    }
}

/// 真后端专面（audio/servo/display）整行摘除（`true` = 不原样输出）。
fn rr3_asm_opt_drop(key: Option<&str>) -> bool {
    matches!(
        key,
        Some("audio_backend" | "servo_backend" | "display_backend")
    )
}

/// 装配单 sim 变换落位到演练 modules-root（`profiles/<名>.toml`，文件名保仓源）。
fn rr3_layout_profile(workdir: &Path, profile_src: &Path, text: &str) -> Result<PathBuf, String> {
    let profiles_dir = workdir.join("profiles");
    std::fs::create_dir_all(&profiles_dir).map_err(|e| format!("建 {profiles_dir:?} 失败: {e}"))?;
    let profile_name = rr3_layout_name(profile_src)?;
    let profile_dst = profiles_dir.join(profile_name);
    std::fs::write(&profile_dst, text).map_err(|e| format!("写 {profile_dst:?} 失败: {e}"))?;
    Ok(profile_dst)
}

/// 装配单文件名（保仓源名；无文件名即红）。
fn rr3_layout_name(profile_src: &Path) -> Result<&std::ffi::OsStr, String> {
    profile_src
        .file_name()
        .ok_or_else(|| format!("装配单路径 {} 无文件名", profile_src.display()))
}

/// 装配单文件名主干（接线档目录名，`hardware/<stem>/wiring.toml`）。
fn rr3_layout_stem(profile_src: &Path) -> Result<&str, String> {
    profile_src
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| format!("装配单路径 {} 无文件名", profile_src.display()))
}

/// 接线档同批装机（原文零改）：装载期校验⑦要求 hardware/<形态>/wiring.toml
/// 必在，仓源缺件在这里红，不让演练跑一半才红。
fn rr3_layout_wiring(repo: &Path, workdir: &Path, stem: &str) -> Result<(), String> {
    let wiring_src = repo.join("hardware").join(stem).join("wiring.toml");
    let wiring_dst_dir = workdir.join("hardware").join(stem);
    std::fs::create_dir_all(&wiring_dst_dir)
        .map_err(|e| format!("建 {wiring_dst_dir:?} 失败: {e}"))?;
    std::fs::copy(&wiring_src, wiring_dst_dir.join("wiring.toml"))
        .map(|_| ())
        .map_err(|e| format!("仓源接线档 {wiring_src:?} 拷贝失败: {e}（校验⑦要求装机档必在）"))
}

/// 一个模块的五件套装机（manifest entry 绝对化 + config + atoms 申报面）。
/// 拆成"manifest 面 / 旁件面"两件（CC 攻坚）：前者是身份改写的主件，后者是
/// 开机即读的旁件（缺件都必红），两段判据互不依赖。
fn rr3_layout_module(
    repo: &Path,
    workdir: &Path,
    module: &str,
) -> Result<Rr3AssembleEntry, String> {
    let src_dir = repo.join("modules").join(module);
    let dst_dir = workdir.join("modules").join(module);
    std::fs::create_dir_all(&dst_dir).map_err(|e| format!("建 {dst_dir:?} 失败: {e}"))?;
    let entry = rr3_layout_manifest(repo, &src_dir, &dst_dir, module)?;
    rr3_layout_sidecars(&src_dir, &dst_dir)?;
    Ok((module.to_string(), entry))
}

/// manifest 面：读仓源 manifest → 解析 → 入口绝对化改写（三道门）→ 落位，
/// 回写入产物的绝对入口。
fn rr3_layout_manifest(
    repo: &Path,
    src_dir: &Path,
    dst_dir: &Path,
    module: &str,
) -> Result<PathBuf, String> {
    let src_manifest = src_dir.join("module.toml");
    let (entry_abs, installed) = rr3_layout_installed(repo, &src_manifest, module)?;
    let dst_manifest = dst_dir.join("module.toml");
    rr3_layout_write(&dst_manifest, installed)?;
    Ok(entry_abs)
}

/// 仓源 manifest 读入并做 entry 绝对化改写（返回绝对入口与改写文本）。
fn rr3_layout_installed(
    repo: &Path,
    src_manifest: &Path,
    module: &str,
) -> Result<(PathBuf, String), String> {
    let (text, facts) = rr3_layout_src_facts(src_manifest)?;
    // 产物 = 与 rr3-sim 同 target-dir 的模块产物（批N32 件四起六模块全
    // Rust——brain 翻面后 shim 产物路径退役；lang 合法性由 rrd manifest
    // 解析保证）。target-dir 已由仓根配置移出源码区，定位走单点。
    let entry_abs = rr3_bin_dir(repo).join(&facts.entry);
    let installed = rr3_entry_rewrite(&text, &entry_abs, &facts.entry, module)?;
    Ok((entry_abs, installed))
}

/// 装机 manifest 落位（写失败点名目标路径）。
fn rr3_layout_write(dst: &Path, installed: String) -> Result<(), String> {
    std::fs::write(dst, installed).map_err(|e| format!("写 {dst:?} 失败: {e}"))
}

/// 读仓源 manifest 并解析（读不到/坏形都点名）。
fn rr3_layout_src_facts(
    src_manifest: &Path,
) -> Result<(String, rrd::manifest::Rr3ManifestFacts), String> {
    let text = std::fs::read_to_string(src_manifest).map_err(|e| {
        format!(
            "读不到仓源 manifest {}: {e}（装配演练要求 face 模块五件齐全）",
            src_manifest.display()
        )
    })?;
    let facts = rr3_manifest_parse(&text)
        .map_err(|errs| format!("仓源 manifest {src_manifest:?} 解析失败: {errs:?}"))?;
    Ok((text, facts))
}

/// 旁件面：config 同目录就位（模块开机即读，缺件必重启环）+ atoms/ 申报面
/// 整树同批装机（校验⑨ 从 `modules/<m>/atoms/*/assemble.toml` 读 demands，
/// 漏拷会让 rrd 判"件未申报 demands 而角色 kind=power"红、拒绝 bind——演练
/// 布局与装机布局在此处必须逐件等价）。
fn rr3_layout_sidecars(src_dir: &Path, dst_dir: &Path) -> Result<(), String> {
    let src_config = src_dir.join("config.toml");
    std::fs::copy(&src_config, dst_dir.join("config.toml")).map_err(|e| {
        format!("仓源 config {src_config:?} 拷贝失败: {e}——模块开机即读 config，缺件必重启环")
    })?;
    rr3_layout_atoms(src_dir, dst_dir)
}

/// atoms/ 申报面整树同批装机（缺席合法跳过）。
fn rr3_layout_atoms(src_dir: &Path, dst_dir: &Path) -> Result<(), String> {
    let atoms_src = src_dir.join("atoms");
    if atoms_src.is_dir() {
        rr3_copy_tree(&atoms_src, &dst_dir.join("atoms"))?;
    }
    Ok(())
}

/// 目录递归拷贝（std 无内置；只服务演练夹具，符号链接按文件内容拷贝）。
fn rr3_copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("建 {dst:?} 失败: {e}"))?;
    let rd = std::fs::read_dir(src).map_err(|e| format!("读目录 {src:?} 失败: {e}"))?;
    rr3_copy_entries(rd, src, dst)
}

/// 逐条目拷贝（遍历失败的文案带源目录）。
fn rr3_copy_entries(rd: std::fs::ReadDir, src: &Path, dst: &Path) -> Result<(), String> {
    for ent in rd {
        let ent = ent.map_err(|e| format!("遍历 {src:?} 失败: {e}"))?;
        rr3_copy_node(&ent, dst)?;
    }
    Ok(())
}

/// 单条目：目录递归、其余按文件拷贝。
fn rr3_copy_node(ent: &std::fs::DirEntry, dst: &Path) -> Result<(), String> {
    let from = ent.path();
    let to = dst.join(ent.file_name());
    if from.is_dir() {
        return rr3_copy_tree(&from, &to);
    }
    std::fs::copy(&from, &to)
        .map(|_| ())
        .map_err(|e| format!("拷贝 {from:?} 失败: {e}"))
}
