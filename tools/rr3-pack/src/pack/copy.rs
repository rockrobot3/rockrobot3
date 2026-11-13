//! 落盘原语：目录树拷贝、清单改写、模块名枚举与 py shim。

use super::*;

/// python 模块的 entry 装机产物：shim 脚本（机制保留位——批N32 件四起仓内六模块
/// 全 Rust，暂无消费模块；装机布局多一层 `pkg/`，config 位只有打包器知道）。
/// `pkg_dir` 是**装机绝对**包树目录（其下含 `<py_module>/` 包），
/// 包名 = 目录名连字符转下划线（`rr3-brain` → `rr3_brain`）；`config_path` 是装机绝对
/// config 位，经 `RR3_BRAIN_CONFIG` 注入（brain rr3_config_load 的文档化优先口，
/// 0.0.6 批F1：python 模块 config 契约当前即 brain 契约）。
pub(super) fn rr3_python_shim_text(pkg_dir: &Path, py_module: &str, config_path: &Path) -> String {
    format!(
        "#!/bin/sh\nRR3_BRAIN_CONFIG={} PYTHONPATH={} exec python3 -m {py_module}\n",
        config_path.display(),
        pkg_dir.display()
    )
}

/// 递归拷贝目录树（python pkg 落位用）。只处理普通文件与目录；符号链接/特殊文件
/// 一律点名拒绝——注入产物是仓内包的纯副本，出现链接是装配契约事故，不静默跟随。
pub(super) fn rr3_copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("建目录 {} 失败: {e}", dst.display()))?;
    let entries = fs::read_dir(src).map_err(|e| format!("读目录 {} 失败: {e}", src.display()))?;
    rr3_copy_entries(entries, src, dst)
}

/// 逐条目复制（目录读错的文案带来源路径）。
fn rr3_copy_entries(entries: fs::ReadDir, src: &Path, dst: &Path) -> Result<(), String> {
    for e in entries {
        let e = e.map_err(|err| format!("读目录 {} 失败: {err}", src.display()))?;
        rr3_copy_entry(&e, dst)?;
    }
    Ok(())
}

/// 单个条目：目录递归、普通文件复制，其余（链接/特殊文件）点名拒绝。
fn rr3_copy_entry(e: &fs::DirEntry, dst: &Path) -> Result<(), String> {
    let from = e.path();
    let to = dst.join(e.file_name());
    let ft = e
        .file_type()
        .map_err(|err| format!("取 {} 类型失败: {err}", from.display()))?;
    if ft.is_dir() {
        return rr3_copy_dir(&from, &to);
    }
    rr3_copy_node(ft, from, to)
}

/// 非目录条目：普通文件复制，链接/特殊文件点名拒绝。
fn rr3_copy_node(ft: fs::FileType, from: PathBuf, to: PathBuf) -> Result<(), String> {
    if ft.is_file() {
        return rr3_copy_plain(&from, &to);
    }
    Err(format!(
        "python pkg 树含非普通文件（{}）——注入产物须是仓内包目录的纯副本，打包拒绝",
        from.display()
    ))
}

/// 普通文件复制（错误带来源路径）。
fn rr3_copy_plain(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to).map_err(|err| format!("复制 {} 失败: {err}", from.display()))?;
    Ok(())
}

/// 打包面的 entry 改写（B45 起委托 rrd 单点 [`rrd::manifest::rr3_manifest_rewrite`]）：
/// 打包与演练布局共用同一实现，四道门与文案不再各写一份。
pub(super) fn rr3_manifest_rewrite(
    text: &str,
    installed_entry: &str,
    facts_entry: &str,
    module: &str,
) -> Result<String, String> {
    rrd::manifest::rr3_manifest_rewrite(text, installed_entry, facts_entry, module)
}

/// 列出注入产物里的模块名（升序——staging 落位与出错点名都要确定性，read_dir 序不可靠）。
pub(super) fn rr3_module_names(payload: &Path) -> Result<Vec<String>, String> {
    let dir = payload.join("rr3-modules");
    let mut names = rr3_names_read(&dir)?;
    names.sort();
    rr3_names_nonempty(&names, &dir)?;
    Ok(names)
}

/// 注入目录下的子目录名（read_dir 序不可靠，排序归调用方）。
fn rr3_names_read(dir: &Path) -> Result<Vec<String>, String> {
    let entries =
        fs::read_dir(dir).map_err(|e| format!("读注入目录 {} 失败: {e}", dir.display()))?;
    rr3_names_of(entries, dir)
}

/// 遍历注入目录条目，取出子目录名（排序归调用方）。
fn rr3_names_of(entries: fs::ReadDir, dir: &Path) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for e in entries {
        let e = e.map_err(|err| format!("读注入目录 {} 失败: {err}", dir.display()))?;
        names.extend(rr3_dir_name(&e));
    }
    Ok(names)
}

/// 单条目的目录名（非目录回 None）。
fn rr3_dir_name(e: &fs::DirEntry) -> Option<String> {
    e.path()
        .is_dir()
        .then(|| e.file_name().to_string_lossy().into_owned())
}

/// 模块名单非空门（空注入目录不得出包）。
fn rr3_names_nonempty(names: &[String], dir: &Path) -> Result<(), String> {
    if names.is_empty() {
        return Err(format!(
            "注入目录 {} 无模块（约定 rr3-modules/<name>/{{bin,manifest.toml}}，至少一个模块）",
            dir.display()
        ));
    }
    Ok(())
}
