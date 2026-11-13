//! 仓根/二进制定位与清单文本装配（自 drill.rs 抽出的同源实现）。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;

/// 从 `start` 向上找仓根：判定标记是 `contracts/topics.toml` 存在——
/// 它是单一事实源（SC 层），比认 `Cargo.toml`（会撞上子 crate）或 `.git`（裸拷贝仓没有）可靠。
pub fn rr3_find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join("contracts/topics.toml").is_file() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// 构建产物目录（单点）：仓根 `.cargo/config.toml` 把 target-dir 移出源码区，
/// 缺省二进制路径**不得**按 `<crate>/target/debug` 推——源码区没有产物，编译
/// 成功也会被判"不在树"。以当前进程所在目录为准（cargo run 与各 crate 共用
/// 同一 target-dir）；`cargo test` 落在 `deps/` 子层，退一级。判定标记是
/// `rrd` 或 `rr3-sim` 在位；缺失时回退仓内旧布局，仍由 [`rr3_require_bins`]
/// 点名构建命令显式失败。
pub fn rr3_bin_dir(repo: &Path) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = rr3_run_dir(&exe) {
            return d;
        }
    }
    repo.join("base/runtime/target/debug")
}

/// 当前可执行文件所在产物目录（`deps/` 子层退一级）；无同层标记回 None。
fn rr3_run_dir(exe: &Path) -> Option<PathBuf> {
    let mut d = exe.parent()?.to_path_buf();
    if d.file_name() == Some(std::ffi::OsStr::new("deps")) {
        d = d.parent()?.to_path_buf();
    }
    rr3_bin_marker(&d).then_some(d)
}

/// 同层有 rrd 或 rr3-sim 产物即认作 bin 目录。
fn rr3_bin_marker(d: &Path) -> bool {
    d.join("rrd").is_file() || d.join("rr3-sim").is_file()
}

/// 三个二进制的缺省路径（全部随 [`rr3_bin_dir`] 同层）。不在 `paths.rs` 单独
/// 设文件：布局是三个常量，单测钉在 `rr3_default_bins` 上。
pub fn rr3_default_bins(repo: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let dir = rr3_bin_dir(repo);
    (dir.join("rrd"), dir.join("sim_peer"), dir.join("rr3-ctl"))
}

/// 各演练共用的仓根定位（main/chain/brain/assemble 四处同款底座）：
/// `--repo` 优先，缺省从当前目录向上找 [`rr3_find_repo_root`]。
pub fn rr3_repo_locate(a: &Rr3SimArgs) -> Result<PathBuf, String> {
    match &a.repo {
        Some(p) => Ok(p.clone()),
        None => std::env::current_dir()
            .ok()
            .and_then(|cwd| rr3_find_repo_root(&cwd))
            .ok_or(
                "找不到仓根（缺 contracts/topics.toml）：请在仓内运行，或 --repo <路径>"
                    .to_string(),
            ),
    }
}

/// 二进制就位检查：缺了就点名 `cargo build` 命令，不让人对着 ENOENT 猜
pub fn rr3_require_bins(rrd: &Path, peer: &Path, rrctl: &Path) -> Result<(), String> {
    for (p, build) in [
        (rrd, "cargo build -p rrd（在 base/runtime/ 下）"),
        (
            peer,
            "cargo build -p rrd（在 base/runtime/ 下，sim_peer 同包产出）",
        ),
        (rrctl, "cargo build --manifest-path tools/rrctl/Cargo.toml"),
    ] {
        if !p.is_file() {
            return Err(format!("缺二进制 {}：先 {build}", p.display()));
        }
    }
    Ok(())
}

/// 二进制就位检查的扩展版（--brain 演练用）：调用方自己列 (路径, 构建指引) 对，
/// 检查逻辑与 `rr3_require_bins` 同一真值。
pub fn rr3_require_bins_ext(bins: &[(&Path, &str)]) -> Result<(), String> {
    for (p, build) in bins {
        if !p.is_file() {
            return Err(format!("缺二进制 {}：先 {build}", p.display()));
        }
    }
    Ok(())
}

/// 快照里还没到期望相位的模块名（报错时点名用）。
fn rr3_missing<'a>(rows: &[(String, u64, String)], want: &[(&'a str, &str)]) -> Vec<&'a str> {
    want.iter()
        .filter(|(name, phase)| !rows.iter().any(|(n, _, p)| n == name && p == *phase))
        .map(|(name, _)| *name)
        .collect()
}

/// 演练用 manifest 文本。与 rrd/tests/support 的 `rr3_manifest` 同形但**不共用**：
/// 那是 base/runtime 的测试夹具，tools/ 不能反向依赖测试代码。
/// `entry` 由调用方传**绝对路径**（sim_peer 二进制），注册门禁（`rr3_manifest_abs`）要求它。
pub fn rr3_manifest_text(
    name: &str,
    entry_abs: &Path,
    hb_ms: u64,
    wd_ms: u64,
    policy: &str,
    publishes: &[&str],
) -> String {
    let pubs: Vec<String> = publishes.iter().map(|t| format!("\"{t}\"")).collect();
    format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "{}"

[events]
publishes = [{}]
subscribes = ["rr3-runtime/module/state"]

[resources]
restart_policy = "{policy}"

[health]
heartbeat_ms = {hb_ms}
watchdog_timeout_ms = {wd_ms}
"#,
        entry_abs.display(),
        pubs.join(", ")
    )
}

/// 演练用 manifest 文本的全量形状（--brain 演练用）：publishes / subscribes /
/// exposes 三面都可指定，`type` 可标 brain（rr3-brain 是 manifest.type=brain 的
/// 唯一在册模块）。与 `rr3_manifest_text` 同形但三面显式——既有两面演练继续走
/// 旧签名，新形状单独可测。
// 参数逐一对应 manifest 契约字段（wire 形状生成器，不是内部 API）——显式豁免。
#[allow(clippy::too_many_arguments)]
pub fn rr3_manifest_ext(
    name: &str,
    entry_abs: &Path,
    module_type: &str,
    lang: &str,
    hb_ms: u64,
    wd_ms: u64,
    policy: &str,
    publishes: &[&str],
    subscribes: &[&str],
    exposes: &[&str],
) -> String {
    let list = |xs: &[&str]| {
        xs.iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut out = format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "{module_type}"
lang = "{lang}"
entry = "{}"

[events]
publishes = [{}]
subscribes = [{}]
"#,
        entry_abs.display(),
        list(publishes),
        list(subscribes),
    );
    if !exposes.is_empty() {
        out.push_str(&format!("\n[rpc]\nexposes = [{}]\n", list(exposes)));
    }
    out.push_str(&format!(
        "\n[resources]\nrestart_policy = \"{policy}\"\n\n[health]\nheartbeat_ms = {hb_ms}\nwatchdog_timeout_ms = {wd_ms}\n"
    ));
    out
}

/// 解析 `rr3-ctl status` 的人读表（F3 裁定 6 的输出形状）：
/// 首行表头；数据行三列（空白分隔，task 列必须是数）；`denied_publish = N` 行跳过。
/// 模块名不含空白（naming.md 的命名面），所以按空白切列是安全的。
pub fn rr3_parse_status(out: &str) -> Vec<(String, u64, String)> {
    let mut rows = Vec::new();
    for (n, line) in out.lines().enumerate() {
        if rr3_status_skip(n, line) {
            continue; // 表头与空行
        }
        if let Some(row) = rr3_status_row(line) {
            rows.push(row);
        }
    }
    rows
}

/// 表头与空行跳过（第 0 行恒为表头）。
fn rr3_status_skip(n: usize, line: &str) -> bool {
    n == 0 || line.trim().is_empty()
}

/// 数据行三列（空白分隔，task 列必须是数）。
fn rr3_status_row(line: &str) -> Option<(String, u64, String)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() != 3 {
        return None;
    }
    let task = parts[1].parse::<u64>().ok()?;
    Some((parts[0].to_string(), task, parts[2].to_string()))
}

/// 轮询到 `want` 里每个名字都以 `phase` 出现（注入 `poll`，可测、可换 rr3-ctl 的调用方式）。
/// 返回 Err 时点名还缺谁——报错要能直接当排查起点用。
pub fn rr3_wait_modules(
    mut poll: impl FnMut() -> Vec<(String, u64, String)>,
    want: &[(&str, &str)],
    deadline: Duration,
) -> Result<(), String> {
    let t0 = Instant::now();
    loop {
        let rows = poll();
        let missing = rr3_missing(&rows, want);
        if missing.is_empty() {
            return Ok(());
        }
        if t0.elapsed() >= deadline {
            return Err(rr3_wait_timeout(want, &rows, deadline));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// 超时文案：点名仍缺谁与最后一次快照。
fn rr3_wait_timeout(
    want: &[(&str, &str)],
    rows: &[(String, u64, String)],
    deadline: Duration,
) -> String {
    format!(
        "超时（{deadline:?}）：仍未达成 {:?}，最后一次快照 {rows:?}",
        want.iter()
            .map(|(n, p)| format!("{n}/{p}"))
            .collect::<Vec<_>>()
    )
}

/// 巡检报告的分节标题（纯函数：报告形状在这里定，main 只负责打印）。
/// 前后都断行：分节要独立成行，标题后直接拼内容也不粘连。
pub fn rr3_section(title: &str) -> String {
    format!("\n=== {title} ===\n")
}
