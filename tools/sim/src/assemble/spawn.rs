//! 起进程与运行环境：工作目录、仓根定位、产物校验、rrd 命令面与收尾。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::drill::{rr3_default_bins, rr3_require_bins_ext, Rr3SimArgs};

use super::{rr3_asm_wait, rr3_assemble_layout, rr3_drill_verdict, Rr3AssembleEntry};

/// 本次运行的唯一戳（纳秒）：缺省工作目录据此与上一轮隔离。
fn rr3_sim_stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// 演练夹具定位（工作目录 + socket）：缺省工作目录每次运行唯一（pid 会被容器
/// 快速复用，单 pid 命名会让上一轮遗留模块混进本轮）；显式 `--socket` 时目录归
/// 调用方，不删其父目录。回（目录是否归本次演练所有, workdir, socket）。
fn rr3_drill_stage(a: &Rr3SimArgs) -> Result<(bool, PathBuf, PathBuf), String> {
    let (owned_dir, workdir) = rr3_stage_workdir(a);
    rr3_stage_reset_dir(owned_dir, &workdir)?;
    let socket = rr3_stage_socket(a, &workdir);
    let _ = std::fs::remove_file(&socket);
    Ok((owned_dir, workdir, socket))
}

/// 目录重置：归本次演练所有才删，随后建空目录。
fn rr3_stage_reset_dir(owned_dir: bool, workdir: &Path) -> Result<(), String> {
    if owned_dir {
        let _ = std::fs::remove_dir_all(workdir);
    }
    std::fs::create_dir_all(workdir).map_err(|e| format!("建 {workdir:?} 失败: {e}"))
}

/// 套接字位置：显式 `--socket` 优先，否则 workdir 下的 bus.sock。
fn rr3_stage_socket(a: &Rr3SimArgs, workdir: &Path) -> PathBuf {
    a.socket.clone().unwrap_or_else(|| workdir.join("bus.sock"))
}

/// 工作目录归属与位置：显式 `--socket` 归调用方（取其父目录），否则按 pid+戳唯一。
fn rr3_stage_workdir(a: &Rr3SimArgs) -> (bool, PathBuf) {
    let owned_dir = a.socket.is_none();
    let workdir = match &a.socket {
        Some(s) => s.parent().unwrap_or(Path::new(".")).to_path_buf(),
        None => std::env::temp_dir().join(format!(
            "rr3-sim-assemble-{}-{}",
            std::process::id(),
            rr3_sim_stamp()
        )),
    };
    (owned_dir, workdir)
}

/// 收尾：杀 rrd（子模块喂狗失败自退）；`--keep` 保留现场并打印手动收尾指引。
fn rr3_drill_teardown(
    keep: bool,
    child: &mut std::process::Child,
    pid: u32,
    socket: &Path,
    workdir: &Path,
) {
    if keep {
        println!(
            "rr3-sim --profile: --keep 保留现场：socket={} workdir={}",
            socket.display(),
            workdir.display()
        );
        println!("rr3-sim --profile: 手动收尾：kill {pid}；rm -rf {workdir:?}");
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(socket);
}

pub fn rr3_assemble_drill(a: &Rr3SimArgs) -> Result<String, String> {
    // ---- 0. 定位：仓根与二进制 ----
    let (repo, rrd, rrctl, profile_src) = rr3_asm_locate(a)?;

    // ---- 1. 夹具：演练 modules-root（装机布局）----
    let (owned_dir, workdir, socket, profile_dst, entries) =
        rr3_asm_fixture(a, &repo, &profile_src, &rrd, &rrctl)?;

    let log_path = workdir.join("rrd.log");
    let ctx = Rr3AsmCtx {
        rrd: &rrd,
        rrctl: &rrctl,
        socket: &socket,
        profile_dst: &profile_dst,
        log_path: &log_path,
        entries: &entries,
        repo: &repo,
    };
    let mut report = String::new();
    let (mut rrd_child, rrd_pid) = rr3_asm_up_rrd(&ctx)?;
    let outcome = rr3_asm_wait(&ctx, &mut report);

    rr3_drill_teardown(a.keep, &mut rrd_child, rrd_pid, &socket, &workdir);
    rr3_drill_verdict(outcome, owned_dir, &workdir, &entries, &profile_dst, report)
}

/// 定位：仓根、rrd/rrctl 产物与装配单源路径（缺 --profile 即内部错）。
fn rr3_asm_locate(a: &Rr3SimArgs) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf), String> {
    let profile_src = a
        .profile
        .clone()
        .ok_or("内部错误：装配演练缺 --profile（main 路由保证）")?;
    let repo = crate::drill::rr3_repo_locate(a)?;
    let (d_rrd, _peer, d_rrctl) = rr3_default_bins(&repo);
    let rrd = a.rrd.clone().unwrap_or(d_rrd);
    let rrctl = a.rrctl.clone().unwrap_or(d_rrctl);
    Ok((repo, rrd, rrctl, profile_src))
}

/// 夹具 + 装机布局 + 产物存在性（返回定位、装配单与模块清单）。
fn rr3_asm_fixture(
    a: &Rr3SimArgs,
    repo: &Path,
    profile_src: &Path,
    rrd: &Path,
    rrctl: &Path,
) -> Result<(bool, PathBuf, PathBuf, PathBuf, Vec<Rr3AssembleEntry>), String> {
    let (owned_dir, workdir, socket) = rr3_drill_stage(a)?;
    let (profile_dst, entries) = rr3_assemble_layout(repo, &workdir, profile_src)?;
    rr3_asm_bins(rrd, rrctl, &entries)?;
    Ok((owned_dir, workdir, socket, profile_dst, entries))
}

/// 装配演练上下文（一次演练内的不变量；只读借用，便于按阶段拆函数）。
pub(crate) struct Rr3AsmCtx<'a> {
    pub(crate) rrd: &'a Path,
    pub(crate) rrctl: &'a Path,
    pub(crate) socket: &'a Path,
    pub(crate) profile_dst: &'a Path,
    pub(crate) log_path: &'a Path,
    pub(crate) entries: &'a [Rr3AssembleEntry],
    pub(crate) repo: &'a Path,
}

/// 产物存在性 fail-fast（点名 cargo build 指引，先例句式）。
fn rr3_asm_bins(rrd: &Path, rrctl: &Path, entries: &[Rr3AssembleEntry]) -> Result<(), String> {
    let mut bins: Vec<(&Path, String)> = vec![
        (rrd, "cargo build -p rrd（在 base/runtime/ 下）".into()),
        (
            rrctl,
            "cargo build --manifest-path tools/rrctl/Cargo.toml".into(),
        ),
    ];
    for (module, entry_abs) in entries {
        bins.push((
            entry_abs.as_path(),
            format!("cargo build -p {module}（在 modules/{module}/ 下）"),
        ));
    }
    let bin_refs: Vec<(&Path, &str)> = bins.iter().map(|(p, h)| (*p, h.as_str())).collect();
    rr3_require_bins_ext(&bin_refs)
}

/// 起 rrd：装配单驱动 boot（参数与 rrd.service 的 ExecStart 同形）。
fn rr3_asm_up_rrd(ctx: &Rr3AsmCtx) -> Result<(std::process::Child, u32), String> {
    let child = rr3_asm_rrd_cmd(ctx)
        .stdout(rr3_asm_log(ctx)?)
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("起 rrd 失败: {e}"))?;
    let pid = child.id();
    println!(
        "rr3-sim --profile: rrd 已起 pid={pid}，socket={}，装配单={}",
        ctx.socket.display(),
        ctx.profile_dst.display()
    );
    Ok((child, pid))
}

/// rrd 命令面（装配单驱动 boot，参数与 rrd.service 的 ExecStart 同形；stdout 由调用方接）。
fn rr3_asm_rrd_cmd(ctx: &Rr3AsmCtx) -> Command {
    let mut cmd = Command::new(ctx.rrd);
    cmd.arg("--socket")
        .arg(ctx.socket)
        .arg("--contracts")
        .arg(ctx.repo.join("contracts"))
        .arg("--profile")
        .arg(ctx.profile_dst)
        // mock 注入走 rrd env，boot spawn 的子模块全部继承：
        // HAL 后端全落 sim；sim 探针（MC-4：非 test 面不再默认 sim——开发机/CI
        // 无 linux sysfs 硬件面，不显式选 sim 会被 fail-closed 判缺格、模块拒绝起）；
        // burst 波形让 VAD 周期发边沿；brain 云端走 mock
        .env("RR3_HAL_BACKEND", "sim")
        .env("RR3_PROBE", "sim")
        .env("RR3_HAL_SIM_WAVE", "burst")
        .env("RR3_BRAIN_CLOUD", "mock");
    cmd
}

/// rrd 日志文件（stdout 落盘；建失败点名路径）。
fn rr3_asm_log(ctx: &Rr3AsmCtx) -> Result<Stdio, String> {
    Ok(Stdio::from(
        std::fs::File::create(ctx.log_path)
            .map_err(|e| format!("建 {:?} 失败: {e}", ctx.log_path))?,
    ))
}
