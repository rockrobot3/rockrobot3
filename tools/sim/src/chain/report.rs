//! 演练结论与失败点名：取证判定、收尾结论文案与现场位置。

use std::process::Child;

use crate::brain::rr3_teardown_procs;
use crate::drill::{rr3_section, Rr3SimArgs};

use super::{
    Rr3ChainFix, Rr3ChainObs, RR3_CHAIN_DOA_DEG, RR3_CHAIN_LEFT_ANGLE, RR3_CHAIN_OK,
    RR3_CHAIN_OK_KEEP,
};

/// 取证判定（纯函数）：判向档落注入值的 ±2° 容差且分左档；落角 = 词表左档 70°。
/// 收集循环只等 presence，档位对不对在这里裁——证据与判定分离，tests 可直测。
pub fn rr3_chain_judge(obs: &Rr3ChainObs) -> Result<String, String> {
    let deg = rr3_chain_doa_ok(obs)?;
    let angle = rr3_chain_servo_ok(obs)?;
    Ok(format!(
        "doa={deg}°/left（注入 {RR3_CHAIN_DOA_DEG}° ±2° 容差内）→ servo={angle}°（词表左档）"
    ))
}

/// 判向档判定：落注入值的 ±2° 容差且分左档。
fn rr3_chain_doa_ok(obs: &Rr3ChainObs) -> Result<i64, String> {
    let deg = obs
        .doa_deg
        .ok_or("判向档缺失：onset 没携 doa（注入没生效或链路没走通）")?;
    rr3_chain_deg_ok(deg)?;
    rr3_chain_left(obs.doa_bin.as_deref())?;
    Ok(deg)
}

/// 判向档容差门：落注入值的 ±2° 内。
fn rr3_chain_deg_ok(deg: i64) -> Result<(), String> {
    if !(-62..=-58).contains(&deg) {
        return Err(format!(
            "注入 {RR3_CHAIN_DOA_DEG}° 的 ILD 反演必须落 ±2° 容差: {deg}°"
        ));
    }
    Ok(())
}

/// 分档判定：必须落左档（|deg| > 30 的注入档）。
fn rr3_chain_left(bin: Option<&str>) -> Result<(), String> {
    if bin != Some("left") {
        return Err(format!("|deg| > 30 必须落左档: {bin:?}"));
    }
    Ok(())
}

/// 转向落角判定：必须是词表左档角。
fn rr3_chain_servo_ok(obs: &Rr3ChainObs) -> Result<i64, String> {
    let angle = obs
        .servo_angle
        .ok_or("转向落角缺失：intent 动作没落到 servo/state（词表映射或 act 链路断）")?;
    if angle != RR3_CHAIN_LEFT_ANGLE {
        return Err(format!("词表左档必须落 {RR3_CHAIN_LEFT_ANGLE}°: {angle}°"));
    }
    Ok(angle)
}

/// 收尾：杀全部子进程；`--keep` 保留现场并给出手动收尾指引。
pub(crate) fn rr3_chain_finish(
    a: &Rr3SimArgs,
    fix: &Rr3ChainFix,
    children: Vec<(String, Child)>,
    outcome: Result<(), String>,
    report: &mut String,
) -> Result<String, String> {
    if a.keep {
        rr3_chain_keep(fix);
        return rr3_chain_done(outcome, report, fix, true, RR3_CHAIN_OK_KEEP);
    }
    rr3_teardown_procs(children, &fix.socket);
    rr3_chain_done(outcome, report, fix, false, RR3_CHAIN_OK)
}

/// `--keep`：打印现场保留与手动收尾指引（不杀子进程）。
fn rr3_chain_keep(fix: &Rr3ChainFix) {
    println!(
        "rr3-sim --chain: --keep 保留现场：socket={} workdir={}",
        fix.socket.display(),
        fix.workdir.display()
    );
    println!(
        "rr3-sim --chain: 手动收尾：kill 上述进程；rm -rf {:?}",
        fix.workdir
    );
}

/// 收尾结论：成功追加演练结论；失败带回现场位置（`--keep` 不追加日志路径）。
fn rr3_chain_done(
    outcome: Result<(), String>,
    report: &mut String,
    fix: &Rr3ChainFix,
    keep: bool,
    ok_text: &str,
) -> Result<String, String> {
    match outcome {
        Ok(()) => Ok(rr3_chain_ok_report(report, ok_text)),
        Err(e) => Err(rr3_chain_err(e, fix, keep)),
    }
}

/// 成功结论：分节标题 + 演练结论原文。
fn rr3_chain_ok_report(report: &mut String, ok_text: &str) -> String {
    report.push_str(rr3_section("演练结论（R9 档位逐项标注）").as_str());
    report.push_str(ok_text);
    std::mem::take(report)
}

/// 失败结论：`--keep` 原样带回，否则附现场日志位置。
fn rr3_chain_err(e: String, fix: &Rr3ChainFix, keep: bool) -> String {
    if keep {
        e
    } else {
        format!("{e}\n（现场日志在 {:?}）", fix.workdir)
    }
}
