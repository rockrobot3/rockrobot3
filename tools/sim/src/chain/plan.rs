//! 演练步骤编排：起跑段、外部模块上线、总线取证与留痕的分段调度。

use std::os::unix::net::UnixStream;
use std::process::Child;

use crate::assemble::rr3_ctl_status_rows as rr3_ctl_rows;
use crate::brain::wait_socket_exists;
use crate::drill::{
    rr3_section, rr3_wait_modules, Rr3SimArgs, RR3_SIM_MODULE_A, RR3_SIM_MODULE_B,
    RR3_SIM_MODULE_BRAIN,
};

use super::{
    rr3_chain_act, rr3_chain_bins, rr3_chain_finish, rr3_chain_fix, rr3_chain_judge,
    rr3_chain_manifests, rr3_chain_obs_up, rr3_chain_status, rr3_chain_tts, rr3_chain_turn,
    rr3_chain_up_ext, rr3_chain_up_rrd, rr3_obs_collect, Rr3ChainBins, Rr3ChainFix,
    RR3_CHAIN_READY_DEADLINE, RR3_SIM_MODULE_ACT,
};

/// 演练上下文（一次演练内的不变量；只读借用，便于按阶段拆函数）。
pub(crate) struct Rr3ChainCtx<'a> {
    pub(crate) bins: &'a Rr3ChainBins,
    pub(crate) fix: &'a Rr3ChainFix,
}

pub fn rr3_chain_drill(a: &Rr3SimArgs) -> Result<String, String> {
    let bins = rr3_chain_bins(a)?;
    let fix = rr3_chain_fix(a, &bins)?;
    let ctx = Rr3ChainCtx {
        bins: &bins,
        fix: &fix,
    };
    let mut children: Vec<(String, Child)> = Vec::new();
    let mut report = String::new();
    let outcome = rr3_chain_run(&ctx, &mut children, &mut report);
    rr3_chain_finish(a, &fix, children, outcome, &mut report)
}

/// 演练主体：写 manifest → 起 rrd → 注册观察者 → 起外部三模块 → 取证 → 巡检。
fn rr3_chain_run(
    ctx: &Rr3ChainCtx,
    children: &mut Vec<(String, Child)>,
    report: &mut String,
) -> Result<(), String> {
    let mut obs_stream = rr3_chain_boot(ctx, children, report)?;
    rr3_chain_up_exts(ctx, children, report)?;
    rr3_chain_evidence(ctx, &mut obs_stream, report)?;
    rr3_chain_status(ctx, report)
}

/// 起跑段：写 manifest → 起 rrd → 等 bind → 观察者注册（先于外部模块）。
fn rr3_chain_boot(
    ctx: &Rr3ChainCtx,
    children: &mut Vec<(String, Child)>,
    report: &mut String,
) -> Result<UnixStream, String> {
    let fix = ctx.fix;
    rr3_chain_mans(ctx)?;
    rr3_chain_up_rrd(ctx, children)?;
    // 等 bind，观察者**先于外部模块**注册（servo/state 一次性广播，晚注册漏收）
    wait_socket_exists(&fix.socket, RR3_CHAIN_READY_DEADLINE)?;
    rr3_chain_obs_up(fix, report)
}

/// 四份 wire manifest 落盘（写完即用）。
fn rr3_chain_mans(ctx: &Rr3ChainCtx) -> Result<(), String> {
    let fix = ctx.fix;
    for (name, text) in rr3_chain_manifests(
        &ctx.bins.sense,
        &ctx.bins.interact,
        &ctx.bins.act,
        &ctx.bins.brain,
    ) {
        let p = fix.workdir.join(format!("{name}.toml"));
        std::fs::write(&p, text).map_err(|e| format!("写 {p:?} 失败: {e}"))?;
    }
    Ok(())
}

/// 外部模块起跑：三外部进程 → 有界等四模块 running。
fn rr3_chain_up_exts(
    ctx: &Rr3ChainCtx,
    children: &mut Vec<(String, Child)>,
    report: &mut String,
) -> Result<(), String> {
    rr3_chain_up_ext(ctx, children)?;
    rr3_chain_mods_up(ctx, report)
}

/// 有界等四模块 running。
fn rr3_chain_mods_up(ctx: &Rr3ChainCtx, report: &mut String) -> Result<(), String> {
    rr3_wait_modules(
        || rr3_ctl_rows(&ctx.bins.rrctl, &ctx.fix.socket),
        &[
            (RR3_SIM_MODULE_A, "running"),
            (RR3_SIM_MODULE_B, "running"),
            (RR3_SIM_MODULE_BRAIN, "running"),
            (RR3_SIM_MODULE_ACT, "running"),
        ],
        RR3_CHAIN_READY_DEADLINE,
    )?;
    report.push_str(rr3_section("四模块 running（sense/interact/brain/act）").as_str());
    Ok(())
}

/// 观察者取证（判向 + 转向）与三段进程内日志留痕。
fn rr3_chain_evidence(
    ctx: &Rr3ChainCtx,
    obs_stream: &mut UnixStream,
    report: &mut String,
) -> Result<(), String> {
    let obs = rr3_obs_collect(obs_stream, RR3_CHAIN_READY_DEADLINE)?;
    let verdict = rr3_chain_judge(&obs)?;
    report.push_str(rr3_section("总线取证（判向 → 转向）").as_str());
    report.push_str(&format!("{verdict}\n"));
    rr3_chain_logs(ctx, report)
}

/// 三段进程内日志留痕（brain 动作数 / interact 挂起 / act 落角）。
fn rr3_chain_logs(ctx: &Rr3ChainCtx, report: &mut String) -> Result<(), String> {
    report.push_str(rr3_section("全链留痕（进程内日志）").as_str());
    rr3_chain_log_head(ctx, report)?;
    let intent = rr3_chain_act(ctx)?;
    report.push_str(&format!("act: {intent}\n"));
    Ok(())
}

/// 留痕前两段（brain 轮次 / interact 挂起）。
fn rr3_chain_log_head(ctx: &Rr3ChainCtx, report: &mut String) -> Result<(), String> {
    let turn = rr3_chain_turn(ctx)?;
    report.push_str(&format!("brain: {turn}\n"));
    let tts = rr3_chain_tts(ctx)?;
    report.push_str(&format!(
        "interact: {tts}（表情机 talk 自动挂起，sim 档无显示件）\n"
    ));
    Ok(())
}
