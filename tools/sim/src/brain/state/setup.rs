// SPDX-License-Identifier: Apache-2.0
//! 演练准备面：二进制定位/存在性校验、工作目录与桩 manifest/config 落盘。
//!
//! 全部是"演练前一次成型"的夹具构造，不碰总线；产物路径回给 `state.rs` 的
//! 上下文持有（`Rr3BrainCtx` 只读借用）。判据要点：五件产物缺件逐件点名各自
//! 构建命令；interact 与 brain 同目录，必须各用独立 config 名。
//!
//! 拆分动因：`brain/state.rs` 548 行，准备面与运行闭环分属两簇。

use std::path::{Path, PathBuf};

use super::*;

/// 演练二进制集（定位 + 存在性校验一次做完）。
pub(super) struct Rr3BrainBins {
    pub(super) rrd: PathBuf,
    pub(super) rrctl: PathBuf,
    pub(super) sense: PathBuf,
    pub(super) interact: PathBuf,
    pub(super) brain: PathBuf,
}

pub(super) fn rr3_brain_bins(a: &Rr3SimArgs) -> Result<Rr3BrainBins, String> {
    let repo = crate::drill::rr3_repo_locate(a)?;
    let dir = crate::drill::rr3_bin_dir(&repo);
    let (d_rrd, _peer, d_rrctl) = crate::drill::rr3_default_bins(&repo);
    let bins = Rr3BrainBins {
        rrd: a.rrd.clone().unwrap_or(d_rrd),
        rrctl: a.rrctl.clone().unwrap_or(d_rrctl),
        sense: a.sense.clone().unwrap_or(dir.join("rr3-sense")),
        interact: a.interact.clone().unwrap_or(dir.join("rr3-interact")),
        brain: a.brain_bin.clone().unwrap_or(dir.join("rr3-brain")),
    };
    rr3_brain_bins_check(&bins)?;
    Ok(bins)
}

/// 五件产物存在性校验（缺件逐件点名各自构建命令）。
pub(super) fn rr3_brain_bins_check(bins: &Rr3BrainBins) -> Result<(), String> {
    rr3_require_bins_ext(&[
        (&bins.rrd, "cargo build -p rrd（在 base/runtime/ 下）"),
        (
            &bins.rrctl,
            "cargo build --manifest-path tools/rrctl/Cargo.toml",
        ),
        (
            &bins.sense,
            "cargo build -p rr3-sense（在 modules/rr3-sense/ 下）",
        ),
        (
            &bins.interact,
            "cargo build -p rr3-interact（在 modules/rr3-interact/ 下）",
        ),
        (
            &bins.brain,
            "cargo build -p rr3-brain（在 modules/rr3-brain/ 下）",
        ),
    ])
}

/// 演练夹具：工作目录 + 套接字 + 三份 manifest + 两份 config（写完即用）。
pub(super) struct Rr3BrainFix {
    pub(super) workdir: PathBuf,
    pub(super) socket: PathBuf,
    pub(super) brain_cfg: PathBuf,
    pub(super) interact_cfg: PathBuf,
    pub(super) m_sense: PathBuf,
    pub(super) m_interact: PathBuf,
    pub(super) m_brain: PathBuf,
}

pub(super) fn rr3_brain_fix(
    a: &Rr3SimArgs,
    bins: &Rr3BrainBins,
    cloud_real: bool,
    base_url: &str,
) -> Result<Rr3BrainFix, String> {
    let (workdir, socket) = rr3_brain_place(a)?;

    let (brain_cfg, interact_cfg) = rr3_brain_cfgs(&workdir, cloud_real, base_url)?;
    rr3_mods_write(&workdir, bins)?;
    let m_sense = workdir.join(format!("{RR3_SIM_MODULE_A}.toml"));
    let m_interact = workdir.join(format!("{RR3_SIM_MODULE_B}.toml"));
    let m_brain = workdir.join(format!("{RR3_SIM_MODULE_BRAIN}.toml"));
    Ok(Rr3BrainFix {
        workdir,
        socket,
        brain_cfg,
        interact_cfg,
        m_sense,
        m_interact,
        m_brain,
    })
}

/// 演练 config 两份落点（写盘归 [`rr3_brain_cfg_write`]；返回两路径）。
pub(super) fn rr3_brain_cfgs(
    workdir: &Path,
    cloud_real: bool,
    base_url: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let brain_cfg = workdir.join("config.toml");
    let interact_cfg = workdir.join("interact-config.toml");
    rr3_brain_cfg_write(&brain_cfg, &interact_cfg, cloud_real, base_url)?;
    Ok((brain_cfg, interact_cfg))
}

/// 三份 wire manifest 落盘（写完即用）。
pub(super) fn rr3_mods_write(workdir: &Path, bins: &Rr3BrainBins) -> Result<(), String> {
    for (name, text) in rr3_brain_manifests(&bins.sense, &bins.interact, &bins.brain) {
        let p = workdir.join(format!("{name}.toml"));
        std::fs::write(&p, text).map_err(|e| format!("写 {p:?} 失败: {e}"))?;
    }
    Ok(())
}

/// 演练工作目录与套接字：`--socket` 显式给定则同目录，否则临时目录按进程号。
pub(super) fn rr3_brain_place(a: &Rr3SimArgs) -> Result<(PathBuf, PathBuf), String> {
    let workdir = rr3_brain_workdir(a);
    std::fs::create_dir_all(&workdir).map_err(|e| format!("建 {workdir:?} 失败: {e}"))?;
    let socket = a.socket.clone().unwrap_or_else(|| workdir.join("bus.sock"));
    let _ = std::fs::remove_file(&socket);
    Ok((workdir, socket))
}

/// 演练工作目录：`--socket` 显式给定取其父目录，否则临时目录按进程号。
pub(super) fn rr3_brain_workdir(a: &Rr3SimArgs) -> PathBuf {
    match &a.socket {
        Some(s) => s.parent().unwrap_or(Path::new(".")).to_path_buf(),
        None => std::env::temp_dir().join(format!("rr3-sim-brain-{}", std::process::id())),
    }
}

/// 演练 config 两份：brain 云档 + interact 独立 config。
pub(super) fn rr3_brain_cfg_write(
    brain_cfg: &Path,
    interact_cfg: &Path,
    cloud_real: bool,
    base_url: &str,
) -> Result<(), String> {
    std::fs::write(brain_cfg, rr3_brain_cfg_text(cloud_real, base_url))
        .map_err(|e| format!("写演练 config 失败: {e}"))?;
    // interact 是同一 workdir 里的外部进程：其 manifest 平铺 workdir，缺省 config.toml
    // 会撞上面 brain 的 config.toml（同目录同名）——I-11 起 interact 必读 config，
    // 故写独立 interact-config.toml 并 env 指定（与 chain.rs 同一纪律）。
    std::fs::write(interact_cfg, crate::chain::rr3_chain_itact_cfg())
        .map_err(|e| format!("写 interact config 失败: {e}"))
}
