//! 演练步骤的可测实现（F4）。
//!
//! 约定：凡是"会碰真实进程/套接字"的动作都由 `main.rs` 编排，本模块只放
//! **纯逻辑**（参数解析、路径定位、manifest 文本、`rr3-ctl` 输出解析、注入式轮询）
//! ——单测因此全部平台无关，④ 步在 Windows 上也能跑到它们。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 演练里两个真模块的身份与行为（名字必须真实存在于 contracts/topics.toml 的
/// owner 集：rr3-interact 要发布 `rr3-interact/asr/text`，前缀归属按名字首段判）
pub const RR3_SIM_MODULE_A: &str = "rr3-sense";
pub const RR3_SIM_MODULE_B: &str = "rr3-interact";
/// B 发布的主题（契约 event，owner=rr3-interact）
pub const RR3_SIM_B_TOPIC: &str = "rr3-interact/asr/text";
/// --brain 演练（M4）里的第三个真模块：Python 认知层
pub const RR3_SIM_MODULE_BRAIN: &str = "rr3-brain";

/// 演练参数（全部有默认值：在仓内任意目录直接 `rr3-sim` 即可跑）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3SimArgs {
    /// 套接字路径（缺省：临时目录下按进程号起名）
    pub socket: Option<PathBuf>,
    /// 仓根（缺省：从当前目录向上找 `contracts/topics.toml`）
    pub repo: Option<PathBuf>,
    /// 收尾不杀 rrd（保留现场人工排查）
    pub keep: bool,
    /// 走 --brain 演练（rrd + rr3-sense(sim) + rr3-brain + rr3-interact 四真进程）
    pub brain: bool,
    /// 走装配演练（D9）：`--profile <装配单>`——装配单驱动 rrd 装配链 boot 全部
    /// face 模块（仓源 manifest 原文仅 entry 绝对化 + config 同目录），六模块真进程
    /// 巡检 `denied_publish = 0`。与 --brain 互斥（parse 处同给即拒，Q3 同款）。
    pub profile: Option<PathBuf>,
    /// 各二进制的显式覆盖（缺省按仓内 target/debug 布局推）
    pub rrd: Option<PathBuf>,
    pub sim_peer: Option<PathBuf>,
    pub rrctl: Option<PathBuf>,
    pub sense: Option<PathBuf>,
    pub interact: Option<PathBuf>,
}

/// 解析 `rr3-sim` 参数。未知参数即错（脚本工具的第一错法是静默忽略拼写错误）。
pub fn rr3_sim_args_parse(args: &[String]) -> Result<Rr3SimArgs, String> {
    let mut a = Rr3SimArgs {
        socket: None,
        repo: None,
        keep: false,
        brain: false,
        profile: None,
        rrd: None,
        sim_peer: None,
        rrctl: None,
        sense: None,
        interact: None,
    };
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "--keep" => a.keep = true,
            "--brain" => a.brain = true,
            "--socket" | "--repo" | "--rrd" | "--sim-peer" | "--rrctl" | "--sense" | "--interact"
            | "--profile" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| format!("{flag} 缺参数"))?
                    .clone();
                if v.starts_with('-') {
                    return Err(format!("{flag} 缺参数：收到疑似开关的 {v}"));
                }
                let p = PathBuf::from(v);
                match flag {
                    "--socket" => a.socket = Some(p),
                    "--repo" => a.repo = Some(p),
                    "--rrd" => a.rrd = Some(p),
                    "--sim-peer" => a.sim_peer = Some(p),
                    "--sense" => a.sense = Some(p),
                    "--interact" => a.interact = Some(p),
                    "--profile" => a.profile = Some(p),
                    _ => a.rrctl = Some(p),
                }
                i += 1;
            }
            other => {
                return Err(format!(
                    "未知参数 {other}（可用：--socket --repo --keep --brain --profile --rrd --sim-peer --rrctl --sense --interact）"
                ))
            }
        }
        i += 1;
    }
    if a.brain && a.profile.is_some() {
        return Err(
            "--brain 与 --profile 同给：对话闭环演练与装配演练互斥（各证一面，不合并）".to_string(),
        );
    }
    Ok(a)
}

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

/// 三个二进制的缺省路径（仓内 target/debug 布局）。不在 `paths.rs` 单独设文件：
/// 布局是三个常量，单测钉在 `rr3_default_bins` 上。
pub fn rr3_default_bins(repo: &Path) -> (PathBuf, PathBuf, PathBuf) {
    (
        repo.join("base/runtime/target/debug/rrd"),
        repo.join("base/runtime/target/debug/sim_peer"),
        repo.join("tools/rrctl/target/debug/rr3-ctl"),
    )
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

/// 演练用 manifest 文本。与 rrd/tests/support 的 `rr3_manifest` 同形但**不共用**：
/// 那是 base/runtime 的测试夹具，tools/ 不能反向依赖测试代码。
/// `entry` 由调用方传**绝对路径**（sim_peer 二进制），注册门禁（`rr3_manifest_entry_is_absolute`）要求它。
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
pub fn rr3_manifest_text_ext(
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
        if n == 0 || line.trim().is_empty() {
            continue; // 表头与空行
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 3 {
            if let Ok(task) = parts[1].parse::<u64>() {
                rows.push((parts[0].to_string(), task, parts[2].to_string()));
            }
        }
    }
    rows
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
        let missing: Vec<&str> = want
            .iter()
            .filter(|(name, phase)| !rows.iter().any(|(n, _, p)| n == name && p == *phase))
            .map(|(name, _)| *name)
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        if t0.elapsed() >= deadline {
            return Err(format!(
                "超时（{deadline:?}）：仍未达成 {:?}，最后一次快照 {rows:?}",
                want.iter()
                    .map(|(n, p)| format!("{n}/{p}"))
                    .collect::<Vec<_>>()
            ));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// 巡检报告的分节标题（纯函数：报告形状在这里定，main 只负责打印）。
/// 前后都断行：分节要独立成行，标题后直接拼内容也不粘连。
pub fn rr3_section(title: &str) -> String {
    format!("\n=== {title} ===\n")
}
