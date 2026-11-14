//! 事后校验与断言：等模块 running、rr3-ctl 巡检与 denied_publish 归零门。

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::brain::wait_socket_exists;
use crate::drill::{rr3_parse_status, rr3_section, rr3_wait_modules};

use super::{rr3_asm_entries, rr3_asm_ota, rr3_with_log_tail, Rr3AsmCtx, Rr3AssembleEntry};

/// 装配演练就绪上界：冷启动 + 六进程注册也要够用
const RR3_ASSEMBLE_READY_DEADLINE: Duration = Duration::from_secs(60);

/// 等全模块 running → rr3-ctl 四命令巡检（status/health/logs/ota）。
pub(crate) fn rr3_asm_wait(ctx: &Rr3AsmCtx, report: &mut String) -> Result<(), String> {
    rr3_asm_wait_ready(ctx, report)?;
    rr3_asm_wait_ctl(ctx, report)
}

/// 阶段一：等 socket 与全部 face 模块 running，登记 boot 报告。
fn rr3_asm_wait_ready(ctx: &Rr3AsmCtx, report: &mut String) -> Result<(), String> {
    rr3_asm_boot_wait(ctx)?;
    report.push_str(rr3_section("装配单驱动 boot：全部 face 模块 running").as_str());
    rr3_asm_entries(ctx.entries, report);
    Ok(())
}

/// 有界等 socket 就位与全部 face 模块 running（失败附 rrd.log 尾部）。
fn rr3_asm_boot_wait(ctx: &Rr3AsmCtx) -> Result<(), String> {
    wait_socket_exists(ctx.socket, RR3_ASSEMBLE_READY_DEADLINE)?;
    let wanted = rr3_asm_wanted(ctx.entries);
    rr3_wait_modules(
        || rr3_ctl_status_rows(ctx.rrctl, ctx.socket),
        &wanted,
        RR3_ASSEMBLE_READY_DEADLINE,
    )
    .map_err(|e| rr3_with_log_tail(&e, ctx.log_path))?;
    Ok(())
}

/// 等跑期望表：每个 face 模块都要到 running。
fn rr3_asm_wanted(entries: &[Rr3AssembleEntry]) -> Vec<(&str, &str)> {
    entries
        .iter()
        .map(|(m, _)| (m.as_str(), "running"))
        .collect()
}

/// 阶段二：rr3-ctl 四命令巡检（status/health/logs/ota）。
fn rr3_asm_wait_ctl(ctx: &Rr3AsmCtx, report: &mut String) -> Result<(), String> {
    report.push_str(rr3_section("rr3-ctl status 巡检").as_str());
    let status_out = rr3_ctl_out(ctx.rrctl, ctx.socket, "status")?;
    report.push_str(&status_out);
    rr3_asm_denied(&status_out)?;
    rr3_asm_rows_ok(ctx)?;
    rr3_asm_tail_ctl(ctx, report)
}

/// 巡检后三段（health / logs / ota）留痕。
fn rr3_asm_tail_ctl(ctx: &Rr3AsmCtx, report: &mut String) -> Result<(), String> {
    report.push_str(rr3_section("rr3-ctl health").as_str());
    report.push_str(&rr3_ctl_out(ctx.rrctl, ctx.socket, "health")?);
    report.push_str(rr3_section("rr3-ctl logs（rrd 日志环尾部）").as_str());
    report.push_str(&rr3_ctl_out(ctx.rrctl, ctx.socket, "logs")?);
    report.push_str(rr3_section("rr3-ctl ota（预期恒拒，随 M6）").as_str());
    rr3_asm_ota(ctx, report);
    Ok(())
}

/// denied_publish = 0 是"事件真被接受"的线上证据（D9 验收判据）。
fn rr3_asm_denied(status_out: &str) -> Result<(), String> {
    if status_out.contains("denied_publish = 0") {
        Ok(())
    } else {
        Err(format!("denied_publish ≠ 0：发布被 rrd 拒了\n{status_out}"))
    }
}

/// 巡检快照里每个 face 模块都须在 running（掉线即红）。
fn rr3_asm_rows_ok(ctx: &Rr3AsmCtx) -> Result<(), String> {
    let rows = rr3_ctl_status_rows(ctx.rrctl, ctx.socket);
    for (m, _) in ctx.entries {
        if !rr3_row_running(&rows, m) {
            return Err(format!("巡检快照里 {m} 不在 running（掉线？）：{rows:?}"));
        }
    }
    Ok(())
}

/// 快照里 `m` 是否仍在 running。
fn rr3_row_running(rows: &[(String, u64, String)], m: &str) -> bool {
    rows.iter().any(|(n, _, p)| n == m && p == "running")
}

/// 轮询用的 status 快照（main.rs run / chain / brain 各演练共用同一真值）：
/// 连接失败（rrd 尚未就绪）当空表处理，由 deadline 收口
pub(crate) fn rr3_ctl_status_rows(rrctl: &Path, socket: &Path) -> Vec<(String, u64, String)> {
    rr3_ctl_cmd(rrctl, socket, "status")
        .output()
        .map(|o| rr3_parse_status(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// rr3-ctl 子命令的命令面（--bus-path + 命令，三处读面共用）。
fn rr3_ctl_cmd(rrctl: &Path, socket: &Path, cmd: &str) -> Command {
    let mut c = Command::new(rrctl);
    c.arg("--bus-path").arg(socket).arg(cmd);
    c
}

/// 跑一次 rr3-ctl 子命令，stdout 原样带回（巡检报告素材）；意外失败即 Err
pub(crate) fn rr3_ctl_out(rrctl: &Path, socket: &Path, cmd: &str) -> Result<String, String> {
    let out = rr3_ctl_cmd(rrctl, socket, cmd)
        .output()
        .map_err(|e| format!("跑 rr3-ctl {cmd} 失败: {e}"))?;
    let s = String::from_utf8_lossy(&out.stdout).into_owned();
    rr3_ctl_check(cmd, &out.status, &out.stderr, s)
}

/// 退出码门：非 0 即点名失败，成功补尾换行。
fn rr3_ctl_check(
    cmd: &str,
    status: &std::process::ExitStatus,
    stderr: &[u8],
    s: String,
) -> Result<String, String> {
    if !status.success() {
        return Err(rr3_ctl_fail(cmd, status, stderr, s));
    }
    Ok(rr3_ctl_nl(s))
}

/// stdout 收口：输出不以换行结尾时补一行（报告拼接不粘连）。
fn rr3_ctl_nl(mut s: String) -> String {
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// 非预期失败收口：stdout 原样 + stderr 续接，点名退出码。
fn rr3_ctl_fail(
    cmd: &str,
    status: &std::process::ExitStatus,
    stderr: &[u8],
    mut s: String,
) -> String {
    s.push_str(&String::from_utf8_lossy(stderr));
    format!("rr3-ctl {cmd} 意外失败（exit {status}）:\n{s}")
}

/// 预期会失败的那一类（ota）：把 (退出码, 合并输出) 带回来由调用方裁定
pub(crate) fn rr3_ctl_maybe_fail(
    rrctl: &Path,
    socket: &Path,
    cmd: &str,
) -> Result<String, (i32, String)> {
    let out = rr3_ctl_cmd(rrctl, socket, cmd)
        .output()
        .map_err(|e| (1, format!("跑 rr3-ctl {cmd} 失败: {e}")))?;
    let code = out.status.code().unwrap_or(1);
    let s = rr3_ctl_merged(&out.stdout, &out.stderr);
    if out.status.success() {
        Ok(s)
    } else {
        Err((code, s))
    }
}

/// stdout 原文 + stderr 续接（预期失败面的合并输出）。
fn rr3_ctl_merged(stdout: &[u8], stderr: &[u8]) -> String {
    let mut s = String::from_utf8_lossy(stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(stderr));
    s
}
