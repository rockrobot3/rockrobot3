//! 摘要与错误：成功结论文案、逐模块登记、ota 留痕与日志尾部。

use std::path::Path;

use crate::drill::rr3_section;

use super::{rr3_ctl_maybe_fail, Rr3AsmCtx, Rr3AssembleEntry};

/// 结论：成功即收现场（失败保留供排障，错误信息点名 workdir 与现场日志）。
pub(crate) fn rr3_drill_verdict(
    outcome: Result<(), String>,
    owned_dir: bool,
    workdir: &Path,
    entries: &[Rr3AssembleEntry],
    profile_dst: &Path,
    report: String,
) -> Result<String, String> {
    match outcome {
        Ok(()) => Ok(rr3_drill_ok(
            owned_dir,
            workdir,
            entries,
            profile_dst,
            report,
        )),
        Err(e) => Err(format!("{e}\n（现场日志在 {workdir:?}）")),
    }
}

/// 成功结论：装配目录归本次演练所有才清场，再追加逐模块结论。
fn rr3_drill_ok(
    owned_dir: bool,
    workdir: &Path,
    entries: &[Rr3AssembleEntry],
    profile_dst: &Path,
    mut report: String,
) -> String {
    if owned_dir {
        let _ = std::fs::remove_dir_all(workdir);
    }
    report.push_str(rr3_section("演练结论").as_str());
    report.push_str(&format!(
        "装配演练 OK：{} 个 face 模块全 running + denied_publish = 0（装配单 {}）\n",
        entries.len(),
        profile_dst.display()
    ));
    report
}

/// 报告里逐模块登记 entry 装机路径。
pub(crate) fn rr3_asm_entries(entries: &[Rr3AssembleEntry], report: &mut String) {
    for (m, entry_abs) in entries {
        report.push_str(&format!("{m}  entry={}\n", entry_abs.display()));
    }
}

/// ota 预期恒拒：成功反而是意外，两种形态都写进报告。
pub(crate) fn rr3_asm_ota(ctx: &Rr3AsmCtx, report: &mut String) {
    match rr3_ctl_maybe_fail(ctx.rrctl, ctx.socket, "ota") {
        Ok(out) => report.push_str(&format!("意外成功：\n{out}")),
        Err((code, out)) => report.push_str(&format!("按预期被拒（exit {code}）:\n{out}")),
    }
}

/// 失败时带上 rrd.log 尾部（排查起点，不让人再跑一遍去找日志）
pub(crate) fn rr3_with_log_tail(msg: &str, log_path: &Path) -> String {
    let tail = std::fs::read_to_string(log_path)
        .map(|s| {
            let lines: Vec<&str> = s.lines().collect();
            let start = lines.len().saturating_sub(15);
            lines[start..].join("\n")
        })
        .unwrap_or_else(|e| format!("（读日志失败: {e}）"));
    format!("{msg}\nrrd.log 尾部（{}）:\n{tail}", log_path.display())
}
