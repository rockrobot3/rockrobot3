//! rr3-ctl —— 总线侧运维 CLI。
//!
//! 规划能力（architecture.md §3）：查在册模块与健康状态、取日志、触发 OTA、
//! 读运维计数、系统重启、装配快照与契约审计。F3（ADR-0010）起子命令经 UDS 真连
//! rrd 的事务面（0.0.4 批B 起 CLI 与 ctl 白名单对齐；续轮十五补 wiring/contracts）：
//! 一次性控制连接，一问一答即断；`ota` 随 M6，rrd 侧恒拒；`reboot` 缺省恒拒，
//! 解锁面在装机环境（rrd 执行面双重门），CLI 不加本地门。命令实现不得绕过 rrd 的
//! 鉴权语义。

mod commands;

use rr_ipc::{RR3_BUS_PATH_ENV, RR3_UDS_PATH_DEFAULT};

fn usage() -> ! {
    eprintln!("用法: rr3-ctl [--bus-path <path>] <命令>");
    eprintln!(
        "命令: {}（status/health/logs/counters/wiring/contracts 实查 rrd；\
         ota 恒拒随 M6；reboot 缺省恒拒，解锁面在装机环境）",
        commands::COMMANDS.join(" | ")
    );
    eprintln!(
        "      tail <topic> [--count N] [--secs S]（注册为观察者订阅事件，\
         N 条或超时退出；JSONL 走 stdout）"
    );
    eprintln!("      logs --unit <name>（直读 <socket>.log/<name>.log 最近 50 行；不发总线）");
    std::process::exit(2)
}

/// 命令行解析结果：总线覆盖路径、命令名、命令后的余参。
struct Rr3Args {
    bus: Option<String>,
    cmd: Option<String>,
    rest: Vec<String>,
}

/// 解析成 (`--bus-path` 值, 命令名, 余参)。`--bus-path` 缺值即用法错误。
fn parse(args: &[String]) -> Rr3Args {
    let mut out = Rr3Args {
        bus: None,
        cmd: None,
        rest: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        rr3_parse_step(args, &mut i, &mut out);
        i += 1;
    }
    out
}

/// 单参数落位：`--bus-path` 消费下一槽；首个裸参是命令名，其后全部进 `rest`。
fn rr3_parse_step(args: &[String], i: &mut usize, out: &mut Rr3Args) {
    if args[*i] == "--bus-path" {
        *i += 1;
        out.bus = Some(rr3_bus_value(args.get(*i)));
    } else if out.cmd.is_none() {
        out.cmd = Some(args[*i].clone());
    } else {
        out.rest.push(args[*i].clone());
    }
}

/// `--bus-path` 的值（缺值即用法错误退出）。
fn rr3_bus_value(v: Option<&String>) -> String {
    match v {
        Some(v) => v.clone(),
        None => usage(),
    }
}

/// 套接字路径优先级：命令行 > `RR3_BUS_PATH` > 契约默认值
fn resolve_bus_path(override_path: Option<&str>) -> String {
    override_path
        .map(str::to_string)
        .or_else(rr3_env_bus)
        .unwrap_or_else(rr3_bus_default)
}

/// `RR3_BUS_PATH` 环境变量（未设即 None）。
fn rr3_env_bus() -> Option<String> {
    std::env::var(RR3_BUS_PATH_ENV).ok()
}

/// 契约默认总线路径。
fn rr3_bus_default() -> String {
    RR3_UDS_PATH_DEFAULT.to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = parse(&args);
    let Some(cmd) = parsed.cmd.as_deref() else {
        usage()
    };
    let bus = resolve_bus_path(parsed.bus.as_deref());
    rr3_dispatch(cmd, &bus, &parsed.rest).map_or_else(
        |e| {
            eprintln!("rr3-ctl {cmd}: {e}");
            std::process::exit(1);
        },
        |report| {
            // tail 的事件行是流式直写 stdout 的，收尾报告为空时不留空行
            if !report.is_empty() {
                println!("{report}");
            }
        },
    );
}

/// 命令分派：`tail` 是注册型观察者（持续收帧）；`logs --unit` 直读 per-unit
/// 日志文件（不走总线）；其余八个走一次性 ctl 连接，不接受余参（多传即错，
/// 避免脚本里的参数被静默吞掉）。
fn rr3_dispatch(cmd: &str, bus: &str, rest: &[String]) -> Result<String, String> {
    if cmd == "tail" {
        return commands::tail::rr3_tail_run(bus, rest);
    }
    if cmd == "logs" && !rest.is_empty() {
        return commands::rr3_unit_log_tail(bus, rest);
    }
    if !rest.is_empty() {
        return Err(format!("{cmd} 不接受参数：{}", rest.join(" ")));
    }
    commands::run(cmd, bus)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_extracts_command_and_bus_override() {
        let a = parse(&v(&["status"]));
        assert_eq!((a.bus.as_deref(), a.cmd.as_deref()), (None, Some("status")));
        assert!(a.rest.is_empty());

        let b = parse(&v(&["--bus-path", "/tmp/a.sock", "health"]));
        assert_eq!(
            (b.bus.as_deref(), b.cmd.as_deref()),
            (Some("/tmp/a.sock"), Some("health"))
        );
        // 开关在命令之后同样生效
        let c = parse(&v(&["health", "--bus-path", "/tmp/b.sock"]));
        assert_eq!(
            (c.bus.as_deref(), c.cmd.as_deref()),
            (Some("/tmp/b.sock"), Some("health"))
        );
        let d = parse(&v(&[]));
        assert_eq!((d.bus.as_deref(), d.cmd.as_deref()), (None, None));
    }

    #[test]
    fn parse_keeps_tail_args_in_order() {
        let a = parse(&v(&["tail", "rr3-sense/audio/vad_start", "--count", "3"]));
        assert_eq!(a.cmd.as_deref(), Some("tail"));
        assert_eq!(a.rest, v(&["rr3-sense/audio/vad_start", "--count", "3"]));
        // 全局开关不落进余参
        let b = parse(&v(&["tail", "--secs", "5", "--bus-path", "/tmp/x", "t"]));
        assert_eq!(b.bus.as_deref(), Some("/tmp/x"));
        assert_eq!(b.rest, v(&["--secs", "5", "t"]));
    }

    #[test]
    fn bus_path_override_beats_default() {
        assert_eq!(resolve_bus_path(Some("/tmp/x.sock")), "/tmp/x.sock");
        let expected =
            std::env::var(RR3_BUS_PATH_ENV).unwrap_or_else(|_| RR3_UDS_PATH_DEFAULT.to_string());
        assert_eq!(resolve_bus_path(None), expected);
    }
}
