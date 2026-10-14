//! 演练步骤的可测实现（F4）。
//!
//! 约定：凡是"会碰真实进程/套接字"的动作都由 `main.rs` 编排，本模块只放
//! **纯逻辑**（参数解析、路径定位、manifest 文本、`rr3-ctl` 输出解析、注入式轮询）
//! ——单测因此全部平台无关，④ 步在 Windows 上也能跑到它们。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 演练里两个真模块的身份（名字必须真实存在于 contracts/topics.toml 的 owner 集）。
/// brain/chain 演练里 A=sense（发布 pcm/vad_start，前缀归属按名字首段判）、B=interact。
pub const RR3_SIM_MODULE_A: &str = "rr3-sense";
pub const RR3_SIM_MODULE_B: &str = "rr3-interact";
/// 两面演练（drill）的主题（契约 event，owner=rr3-sense；C14① 2026-10-10 评审裁定：
/// asr/text 走 retired 后演练主题换成 vad_start，发布方身份随之换位——见下两常量）
pub const RR3_SIM_B_TOPIC: &str = "rr3-sense/audio/vad_start";
/// 两面演练（drill）的 boot resident 身份：RR3_SIM_B_TOPIC 的 owner 是 rr3-sense，
/// 前缀归属要求发布方叫 rr3-sense，而 resident 侧不发布——故 resident 用 rr3-interact、
/// 外部 publish_once 用 rr3-sense（C14① 同批换位，A/B 常量保持 brain/chain 语义不动）
pub const RR3_DRILL_RESIDENT: &str = RR3_SIM_MODULE_B;
/// 两面演练的外部发布者身份（publish_once：发布 [`RR3_SIM_B_TOPIC`] 后退场）
pub const RR3_DRILL_PUBLISHER: &str = RR3_SIM_MODULE_A;
/// --brain 演练（M4）里的第三个真模块：Python 认知层
pub const RR3_SIM_MODULE_BRAIN: &str = "rr3-brain";
/// 演练实例面板地址：板上常驻系统实例（deb 装机）的 rr3-interact 占缺省
/// 127.0.0.1:8080，演练必须换端口否则面板绑定失败拖垮模块（批F4 真板实测缺陷）
pub const RR3_SIM_PANEL_ADDR: &str = "127.0.0.1:18080";

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
    /// 走全链演练（0.0.6 批F4 sim 档，判据 10）：采集→意图→判向→转向→表情
    /// 六真进程 mock 闭环。与 --brain/--profile 互斥（各证一面，不合并）。
    pub chain: bool,
    /// 各二进制的显式覆盖（缺省按仓内 target/debug 布局推）
    pub rrd: Option<PathBuf>,
    pub sim_peer: Option<PathBuf>,
    pub rrctl: Option<PathBuf>,
    pub sense: Option<PathBuf>,
    pub interact: Option<PathBuf>,
    /// --chain 专用：rr3-act 的显式覆盖
    pub act: Option<PathBuf>,
    /// --brain 专用：音频后端（缺省 `sim`；`mac` = macOS 真麦/扬声器，与 rr-hal
    /// `RR3_HAL_BACKEND` 取值同白名单）。缺省不变的 CI 纪律：真件只显式启用。
    pub audio_backend: Option<String>,
    /// --brain 专用：brain 云模式（缺省 `mock`；`real` = 真云，需 `RR3_CLOUD_API_KEY`
    /// 经调用方环境传入——密钥铁律只走环境变量）。
    pub brain_cloud: Option<String>,
    /// --brain 专用：真云端点（`--brain-cloud real` 时**必给**，fail-closed 不落
    /// 演练 config 的死地址；mock 模式给了即拒——演练 config 不碰网络）。
    pub brain_base_url: Option<String>,
}

/// 解析 `rr3-sim` 参数。未知参数即错（脚本工具的第一错法是静默忽略拼写错误）。
pub fn rr3_sim_args_parse(args: &[String]) -> Result<Rr3SimArgs, String> {
    let mut a = Rr3SimArgs {
        socket: None,
        repo: None,
        keep: false,
        brain: false,
        profile: None,
        chain: false,
        rrd: None,
        sim_peer: None,
        rrctl: None,
        sense: None,
        interact: None,
        act: None,
        audio_backend: None,
        brain_cloud: None,
        brain_base_url: None,
    };
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "--keep" => a.keep = true,
            "--brain" => a.brain = true,
            "--chain" => a.chain = true,
            "--socket" | "--repo" | "--rrd" | "--sim-peer" | "--rrctl" | "--sense" | "--interact"
            | "--act" | "--profile" | "--audio-backend" | "--brain-cloud" | "--brain-base-url" => {
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
                    "--act" => a.act = Some(p),
                    "--profile" => a.profile = Some(p),
                    "--audio-backend" => a.audio_backend = Some(p.to_string_lossy().into_owned()),
                    "--brain-cloud" => a.brain_cloud = Some(p.to_string_lossy().into_owned()),
                    "--brain-base-url" => a.brain_base_url = Some(p.to_string_lossy().into_owned()),
                    _ => a.rrctl = Some(p),
                }
                i += 1;
            }
            other => {
                return Err(format!(
                    "未知参数 {other}（可用：--socket --repo --keep --brain --chain --profile --rrd --sim-peer --rrctl --sense --interact --act --audio-backend --brain-cloud --brain-base-url）"
                ))
            }
        }
        i += 1;
    }
    if (a.brain || a.profile.is_some()) && a.chain {
        return Err(
            "--chain 与 --brain/--profile 同给：全链闭环、对话闭环与装配演练互斥（各证一面，不合并）"
                .to_string(),
        );
    }
    if a.brain && a.profile.is_some() {
        return Err(
            "--brain 与 --profile 同给：对话闭环演练与装配演练互斥（各证一面，不合并）".to_string(),
        );
    }
    // --brain 专用旗标的归属与取值把关（脚本工具的第一错法：静默忽略拼写/错配）
    const AUDIO_BACKENDS: [&str; 5] = ["sim", "linux", "esp32", "mac", "placeholder"];
    const BRAIN_CLOUDS: [&str; 2] = ["mock", "real"];
    if let Some(b) = &a.audio_backend {
        if !a.brain {
            return Err("--audio-backend 仅 --brain 演练适用".to_string());
        }
        if !AUDIO_BACKENDS.contains(&b.as_str()) {
            return Err(format!(
                "--audio-backend 取值不认: {b}（允许 sim/linux/esp32/mac/placeholder——与 RR3_HAL_BACKEND 同白名单）"
            ));
        }
    }
    if let Some(c) = &a.brain_cloud {
        if !a.brain {
            return Err("--brain-cloud 仅 --brain 演练适用".to_string());
        }
        if !BRAIN_CLOUDS.contains(&c.as_str()) {
            return Err(format!("--brain-cloud 取值不认: {c}（允许 mock/real）"));
        }
    }
    if let Some(u) = &a.brain_base_url {
        if !a.brain {
            return Err("--brain-base-url 仅 --brain 演练适用".to_string());
        }
        if a.brain_cloud.as_deref() != Some("real") {
            return Err(
                "--brain-base-url 仅 --brain-cloud real 适用：mock 云不碰网络，端点给了也不会被读"
                    .to_string(),
            );
        }
        if !u.starts_with("http://") && !u.starts_with("https://") {
            return Err(format!("--brain-base-url 须以 http(s):// 开头: {u}"));
        }
    }
    if a.brain_cloud.as_deref() == Some("real") && a.brain_base_url.is_none() {
        return Err(
            "--brain-cloud real 必须显式给 --brain-base-url（演练 config 缺省是死地址，fail-closed 不落）"
                .to_string(),
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
