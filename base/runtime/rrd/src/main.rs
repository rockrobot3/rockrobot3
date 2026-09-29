//! rrd —— base 层守护进程（architecture.md §6）。
//!
//! M2 起：装载所有权表 →（Unix）绑 UDS 监听、注册/心跳/看门狗/重启。
//! 非 Unix 平台启动即明确报错并以非 0 退出，绝不假称可跑。

use std::path::Path;

use rr_ipc::RR3_CONTRACT_SCHEMA_VERSION;
use rr_registry::RR3_MANIFEST_SCHEMA_VERSION;
use rrd::{cli, rr3_banner, topics_load, RRD_UNSUPPORTED};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = match cli::rr3_cli_parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // 叠加层路径在这里一次性绝对化：装载与横幅共用同一个解析结果（spec §6 的可见凭证）。
    // 解析失败即致命——横幅是"叠加层已生效"的可见凭证，按 CLI 原词回显会让它依赖 CWD。
    let overlay = match cfg.overlay.as_deref() {
        Some(ov) => match topics_load::rr3_topics_absolute_path(ov) {
            Ok(abs) => Some(abs),
            Err(e) => {
                eprintln!("rrd: 主题叠加层参数不可用: {e}");
                return std::process::ExitCode::FAILURE;
            }
        },
        None => None,
    };

    // 装载在两个平台都做：契约坏了要当场看见，而不是"等不到 UDS"
    let (topics, added) = match topics_load::rr3_topics_load(
        Path::new(topics_load::RR3_CONTRACT_TOPICS_PATH),
        overlay.as_deref(),
    ) {
        Ok(t) => t,
        Err(errs) => {
            for e in &errs {
                eprintln!("rrd: 主题装载失败: {e}");
            }
            return std::process::ExitCode::FAILURE;
        }
    };

    println!(
        "{}",
        rr3_banner(
            RR3_CONTRACT_SCHEMA_VERSION,
            RR3_MANIFEST_SCHEMA_VERSION,
            &cfg.socket.display().to_string(),
            topics.rr3_topic_set_names().len()
        )
    );
    if let Some(ov) = &overlay {
        // 叠加层的可见凭证（spec §6：非生产放行口）：绝对路径，与 CWD 无关
        println!("rrd: 主题叠加层已加载（仅测试用）: {}", ov.display());
        for t in &added {
            println!("rrd: 叠加主题 {t}");
        }
    }

    if !cfg!(unix) {
        eprintln!("{RRD_UNSUPPORTED}");
        return std::process::ExitCode::FAILURE;
    }
    println!("rrd: 已进入 Unix 分支，监听阶段随 Task 8 落地");
    std::process::ExitCode::SUCCESS
}
