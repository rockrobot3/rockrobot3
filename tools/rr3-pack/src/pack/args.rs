// SPDX-License-Identifier: Apache-2.0
//! 参数面：用法、逐参解析与 entry 形制门。

use super::*;

fn usage() -> String {
    "用法: rr3-pack --payload <dir> --repo <dir> (--out <file.deb> [--arch <arch>] [--stage-only] | --install-root <dir>) [--profile <name>]\n说明: --install-root 直接把 usr/bin 三件、模块树与装载真值集落到目标根（Buildroot TARGET_DIR），不写 DEBIAN/systemd、不调 dpkg-deb；与 deb 轨共用同一套 entry 改写与真值集裁定。--payload 是两轨同源的注入产物目录 --payload 是两轨同源的注入产物目录（rrd、rr3-ctl、rr3-ota-ctl；rust 模块 rr3-modules/<name>/{bin,manifest.toml}，python 模块 rr3-modules/<name>/{pkg/,manifest.toml}）；--repo 是仓根（读 profiles/<name>.toml、contracts/{topics,slots,roles,devices}.toml、hardware/<name>/wiring.toml、modules/<name>/config.toml）。--profile 选装机装配单（缺省 distro 不破坏既有调用方；H4/C2 批K：出包链不再只认一份装配单，换板只改装配单）。--arch 缺省 arm64（出厂目标），容器冒烟传本机 dpkg 架构。--stage-only 只落 staging 树不调 dpkg-deb（macOS 本机无 dpkg，装配单测/演练用）。".to_string()
}

fn rr3_need(args: &[String], i: usize, flag: &str) -> Result<String, String> {
    args.get(i + 1)
        .filter(|v| !v.is_empty())
        .cloned()
        .ok_or_else(|| format!("{flag} 缺参数\n{}", usage()))
}

/// 单选项步进：值选项消费两槽、开关消费一槽；返回消费的槽数。
fn pack_arg(cfg: &mut Rr3PackConfig, args: &[String], i: usize) -> Result<usize, String> {
    match args[i].as_str() {
        "--stage-only" => {
            cfg.stage_only = true;
            Ok(1)
        }
        flag => pack_arg_value(cfg, args, i, flag),
    }
}

/// 值选项落位（五族分派；未知选项即拒，文案与旧版逐字一致）。
fn pack_arg_value(
    cfg: &mut Rr3PackConfig,
    args: &[String],
    i: usize,
    flag: &str,
) -> Result<usize, String> {
    if let Some(dst) = pack_arg_path(cfg, flag) {
        return pack_arg_set(dst, args, i, flag);
    }
    pack_arg_text(cfg, args, i, flag)
}

/// 路径族选项的目标位（三件；非路径族回 `None`）。
fn pack_arg_path<'a>(cfg: &'a mut Rr3PackConfig, flag: &str) -> Option<&'a mut PathBuf> {
    if flag == "--payload" {
        return Some(&mut cfg.payload);
    }
    if flag == "--repo" {
        return Some(&mut cfg.repo);
    }
    if flag == "--out" {
        return Some(&mut cfg.out);
    }
    if flag == "--install-root" {
        return Some(cfg.install_root.get_or_insert_with(PathBuf::new));
    }
    None
}

/// 文本族选项落位（两件 + 未知选项拒收）。
fn pack_arg_text(
    cfg: &mut Rr3PackConfig,
    args: &[String],
    i: usize,
    flag: &str,
) -> Result<usize, String> {
    match flag {
        "--profile" => pack_arg_set(&mut cfg.profile, args, i, flag),
        "--arch" => pack_arg_set(&mut cfg.arch, args, i, flag),
        other => Err(format!("未知选项 {other}\n{}", usage())),
    }
}

/// 取 flag 后的值写进目标位（缺值点名原文案）。
fn pack_arg_set<T: From<String>>(
    dst: &mut T,
    args: &[String],
    i: usize,
    flag: &str,
) -> Result<usize, String> {
    *dst = rr3_need(args, i, flag)?.into();
    Ok(2)
}

/// 解析参数。必给项缺席 / 未知选项一律报错带用法，不静默回落（fail-closed）。
pub fn rr3_pack_parse(args: &[String]) -> Result<Rr3PackConfig, String> {
    let mut cfg = Rr3PackConfig {
        payload: PathBuf::new(),
        repo: PathBuf::new(),
        out: PathBuf::new(),
        arch: "arm64".to_string(),
        profile: "distro".to_string(),
        stage_only: false,
        install_root: None,
    };
    let mut i = 0;
    while i < args.len() {
        i += pack_arg(&mut cfg, args, i)?;
    }
    rr3_pack_finish(cfg)
}

/// 必给项复核后交回配置（缺席带用法）。
fn rr3_pack_finish(cfg: Rr3PackConfig) -> Result<Rr3PackConfig, String> {
    rr3_pack_required(&cfg)?;
    Ok(cfg)
}

/// 必给项在场门：payload/repo 恒必给；出包出口 `--out` 与 `--install-root` 二选一
/// （同给是歧义，fail-closed 拒收，不猜调用方意图）。
fn rr3_pack_required(cfg: &Rr3PackConfig) -> Result<(), String> {
    if cfg.payload.as_os_str().is_empty() || cfg.repo.as_os_str().is_empty() {
        return Err(format!("--payload/--repo 必给\n{}", usage()));
    }
    let has_out = !cfg.out.as_os_str().is_empty();
    match (has_out, cfg.install_root.is_some()) {
        (true, true) => Err(format!(
            "--out 与 --install-root 互斥（deb 与目标根装配二选一）\n{}",
            usage()
        )),
        (false, false) => Err(format!(
            "须给 --out <file.deb> 或 --install-root <dir>\n{}",
            usage()
        )),
        _ => Ok(()),
    }
}

/// entry 可装机性：必须相对、且每段都是普通名（拒绝对路径与 `..`/`.` 段——
/// staging 按字面 join，含逃逸段的 entry 会把二进制写进安装根之外）。
/// 在仓六份 manifest 全是生成器落盘的裸模块名，这条门禁只为把"装机约定"从
/// 口头变成打包期硬门：绝对/逃逸 entry 一律点名拒绝，不猜测意图。
pub(super) fn rr3_entry_ok(entry: &str, module: &str) -> Result<(), String> {
    rr3_entry_relative(entry, module)?;
    rr3_entry_segments(entry, module)
}

/// entry 每段都必须是普通名（拒 `.`/`..` 段）。
fn rr3_entry_segments(entry: &str, module: &str) -> Result<(), String> {
    for c in Path::new(entry).components() {
        if !matches!(c, Component::Normal(_)) {
            return Err(format!(
                "模块 {module} 的 entry {entry} 含 . 或 .. 段（装机路径逃逸，打包拒绝）"
            ));
        }
    }
    Ok(())
}

/// entry 必须是相对路径（绝对路径点名拒绝）。
fn rr3_entry_relative(entry: &str, module: &str) -> Result<(), String> {
    if Path::new(entry).is_absolute() {
        return Err(format!(
            "模块 {module} 的 entry 是绝对路径 {entry}（在仓约定 entry 相对，装机绝对由打包器写入——见 modules/README.md）"
        ));
    }
    Ok(())
}
