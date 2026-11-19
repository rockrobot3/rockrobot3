//! staging 装配：骨架/模块/真值/配置/落位与自检。

use super::*;

#[path = "stage/module.rs"]
mod module;
use module::*;

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

/// 入口产物单一清单：rrd / rr3-ctl / rr3-ota-ctl（在场门与落盘共用）。
const RR3_PAYLOAD_BINS: [&str; 3] = ["rrd", "rr3-ctl", "rr3-ota-ctl"];

/// 入口产物在场门（缺席逐件点名）。
fn rr3_payload_bins(cfg: &Rr3PackConfig) -> Result<(), String> {
    for bin in RR3_PAYLOAD_BINS {
        rr3_bin_ready(cfg, bin)?;
    }
    Ok(())
}

/// 单件注入产物在场门（缺席点名）。
fn rr3_bin_ready(cfg: &Rr3PackConfig, bin: &str) -> Result<(), String> {
    let p = cfg.payload.join(bin);
    if !p.is_file() {
        return Err(format!(
            "注入产物 {bin} 缺席: {}（约定 --payload 下有 rrd、rr3-ctl、rr3-ota-ctl、rr3-modules/<name>/{{bin,manifest.toml}}）",
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

/// 入口产物落 usr/bin（错误文案带产物名）。
fn rr3_stage_bins(cfg: &Rr3PackConfig, stage: &Path) -> Result<(), String> {
    for bin in RR3_PAYLOAD_BINS {
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
