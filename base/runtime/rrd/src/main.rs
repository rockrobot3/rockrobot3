//! rrd —— base 层守护进程（architecture.md §6）。
//!
//! M2 起：装载所有权表 →（Unix）绑 UDS 监听、注册/心跳/看门狗/重启。
//! 非 Unix 平台启动即明确报错并以非 0 退出，绝不假称可跑。

use rr_ipc::RR3_CONTRACT_SCHEMA_VERSION;
use rr_registry::{Rr3TopicSet, RR3_MANIFEST_SCHEMA_VERSION};
use rrd::{cli, profile, rr3_banner, topics_load};
// 只在非 Unix 分支用得到：Unix 侧这句文案压根不参与编译，不 gate 会有未使用导入告警。
#[cfg(not(unix))]
use rrd::RRD_UNSUPPORTED;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = match cli::rr3_cli_parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // 三处路径（套接字/叠加层/装配单）在打印横幅**之前**一次性绝对化：
    // 横幅回显的必须正是 daemon 要用的那一个，凭证不得依赖 CWD。
    let (cfg, overlay) = match rr3_abs_config(cfg) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("rrd: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // 装载在两个平台都做：契约坏了要当场看见，而不是"等不到 UDS"。
    // 契约目录运行期落点（T5）：--contracts 显式 > 编译期仓内路径；topics/slots
    // 由装载入口各自 join，路径必须已存在（canonicalize 的 ENOENT 正是该报的错）。
    let contracts_dir = topics_load::rr3_contracts_dir(cfg.contracts.as_deref());
    let (topics, added) = match rr3_topics_read(&contracts_dir, overlay.as_deref()) {
        Some(t) => t,
        None => return std::process::ExitCode::FAILURE,
    };

    rr3_banner_print(&cfg, &topics, overlay.as_deref(), &added);
    rr3_run(cfg, topics)
}

/// 主题装载（叠加层合并）：失败逐条点名并交回 None，由 main 统一非 0 退出。
fn rr3_topics_read(
    contracts_dir: &std::path::Path,
    overlay: Option<&std::path::Path>,
) -> Option<(Rr3TopicSet, Vec<String>)> {
    match topics_load::rr3_topics_load(&contracts_dir.join("topics.toml"), overlay) {
        Ok(t) => Some(t),
        Err(errs) => {
            for e in &errs {
                eprintln!("rrd: 主题装载失败: {e}");
            }
            None
        }
    }
}

/// 横幅与可见凭证：叠加层/装配单都按绝对路径回显（与 CWD 无关）。
fn rr3_banner_print(
    cfg: &cli::RrdConfig,
    topics: &Rr3TopicSet,
    overlay: Option<&std::path::Path>,
    added: &[String],
) {
    println!(
        "{}",
        rr3_banner(
            RR3_CONTRACT_SCHEMA_VERSION,
            RR3_MANIFEST_SCHEMA_VERSION,
            &cfg.socket.display().to_string(),
            topics.rr3_topic_names().len()
        )
    );
    if let Some(ov) = overlay {
        // 叠加层的可见凭证（spec §6：非生产放行口）：绝对路径，与 CWD 无关
        rr3_banner_overlay(ov, added);
    }
    if let Some(p) = &cfg.profile {
        // 装配单的可见凭证：绝对路径（0.0.2 方案 Q3：装机正门），与 CWD 无关
        println!("rrd: 装配单已加载: {}", p.display());
    }
}

/// 叠加层横幅段：路径一行 + 每条新增主题一行（键序由调用方给）。
fn rr3_banner_overlay(ov: &std::path::Path, added: &[String]) {
    println!("rrd: 主题叠加层已加载（仅测试用）: {}", ov.display());
    for t in added {
        println!("rrd: 叠加主题 {t}");
    }
}

/// 启动参数的三处路径（套接字/叠加层/装配单）一次性绝对化：解析结果既是装载输入
/// 也是横幅可见凭证，必须在打印横幅**之前**定死——按 CLI 原词回显会让凭证依赖 CWD。
/// 任一失败即致命，Err 交回 main 统一按 `rrd: {e}` 报出（文案逐字不变）。
fn rr3_abs_config(
    mut cfg: cli::RrdConfig,
) -> Result<(cli::RrdConfig, Option<std::path::PathBuf>), String> {
    // 套接字路径在打印横幅**之前**绝对化：横幅回显的必须正是 daemon 要绑的那一个。
    // 按 CLI 原词回显（`--socket rel.sock`）会让运维照自己 shell 的 CWD 去连，而 daemon
    // 绑的是 rrd 的 CWD + rel.sock——两处各说一套，比直接报错更难查。
    // 绝对化失败即致命：静默回落原词等于把这条缝留在原地。
    cfg.socket = cli::rr3_socket_abs(&cfg.socket)?;

    // 叠加层路径在这里一次性绝对化：装载与横幅共用同一个解析结果（spec §6 的可见凭证）。
    // 解析失败即致命——横幅是"叠加层已生效"的可见凭证，按 CLI 原词回显会让它依赖 CWD。
    let overlay = match cfg.overlay.as_deref() {
        Some(ov) => Some(
            topics_load::rr3_topics_abs(ov).map_err(|e| format!("主题叠加层参数不可用: {e}"))?,
        ),
        None => None,
    };

    // 装配单路径在这里一次性绝对化（与叠加层同形）：装载与横幅凭证共用同一个解析结果。
    // 解析失败即致命——装配单**必须已存在**才可装载，canonicalize 的 ENOENT 正是该报的错。
    if let Some(p) = cfg.profile.as_deref() {
        cfg.profile =
            Some(profile::rr3_profile_abs(p).map_err(|e| format!("装配单参数不可用: {e}"))?);
    }
    Ok((cfg, overlay))
}

/// （Unix）把守护主循环交出去：它永不返回，除非 bind 失败。
#[cfg(unix)]
fn rr3_run(cfg: cli::RrdConfig, topics: Rr3TopicSet) -> std::process::ExitCode {
    match rrd::daemon::rr3_daemon_run(&cfg, topics) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// （非 Unix）到此为止：`conn`/`daemon` 两个模块压根没参与编译，如实说不支持。
#[cfg(not(unix))]
fn rr3_run(_cfg: cli::RrdConfig, _topics: Rr3TopicSet) -> std::process::ExitCode {
    eprintln!("{RRD_UNSUPPORTED}");
    std::process::ExitCode::FAILURE
}
