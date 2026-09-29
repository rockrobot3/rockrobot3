//! rr3-ctl —— 总线侧运维 CLI（骨架）。
//!
//! 规划能力（architecture.md §3）：查在册模块与健康状态、取日志、触发 OTA。
//! v0.0.1 只交付**命令面**：参数解析与输出形状先定下来，真正的 UDS 往返随 M2
//! （rrd 上线后才有东西可问）。命令实现不得绕过 rrd 的鉴权语义。

mod commands;

use rr_ipc::{RR3_BUS_PATH_ENV, RR3_UDS_PATH_DEFAULT};

fn usage() -> ! {
    eprintln!("用法: rr3-ctl [--bus-path <path>] <命令>");
    eprintln!(
        "命令: {}（均属 M2，当前只回显计划）",
        commands::COMMANDS.join(" | ")
    );
    std::process::exit(2)
}

/// 解析成 (`--bus-path` 值, 命令名)。`--bus-path` 缺值即用法错误。
fn parse(args: &[String]) -> (Option<String>, Option<String>) {
    let mut bus = None;
    let mut cmd = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--bus-path" {
            i += 1;
            match args.get(i) {
                Some(v) => bus = Some(v.clone()),
                None => usage(),
            }
        } else if cmd.is_none() {
            cmd = Some(args[i].clone());
        }
        i += 1;
    }
    (bus, cmd)
}

/// 套接字路径优先级：命令行 > `RR3_BUS_PATH` > 契约默认值
fn resolve_bus_path(override_path: Option<&str>) -> String {
    override_path
        .map(str::to_string)
        .or_else(|| std::env::var(RR3_BUS_PATH_ENV).ok())
        .unwrap_or_else(|| RR3_UDS_PATH_DEFAULT.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (bus, cmd) = parse(&args);
    let Some(cmd) = cmd.as_deref() else { usage() };
    match commands::run(cmd, &resolve_bus_path(bus.as_deref())) {
        Ok(report) => println!("{report}"),
        Err(e) => {
            eprintln!("rr3-ctl {cmd}: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_extracts_command_and_bus_override() {
        assert_eq!(parse(&v(&["status"])), (None, Some("status".into())));
        assert_eq!(
            parse(&v(&["--bus-path", "/tmp/a.sock", "health"])),
            (Some("/tmp/a.sock".into()), Some("health".into()))
        );
        // 开关在命令之后同样生效
        assert_eq!(
            parse(&v(&["health", "--bus-path", "/tmp/b.sock"])),
            (Some("/tmp/b.sock".into()), Some("health".into()))
        );
        assert_eq!(parse(&v(&[])), (None, None));
    }

    #[test]
    fn bus_path_override_beats_default() {
        assert_eq!(resolve_bus_path(Some("/tmp/x.sock")), "/tmp/x.sock");
        let expected = std::env::var(RR3_BUS_PATH_ENV)
            .unwrap_or_else(|_| RR3_UDS_PATH_DEFAULT.to_string());
        assert_eq!(resolve_bus_path(None), expected);
    }
}
