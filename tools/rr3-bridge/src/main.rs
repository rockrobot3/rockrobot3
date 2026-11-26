// SPDX-License-Identifier: Apache-2.0
//! rr3-bridge —— UART↔UDS 传输级字节中继。不是模块：不进 manifest/装配单，
//! rrd 看到的是哨兵的完整 register。
//!
//! 连接模型：一条 `--uart <dev>` = 一条链路 = 一个哨兵 ↔ 一条到 rrd 的 UDS 连接（1:1）；
//! 多哨兵 = 多个实例。
//!
//! 字节透传：双向全双工、**零解析**——读多少转多少，帧边界归两端；桥不认识 128B 头、
//! 不校验 magic、不组帧、不改写字节，握手对它不可见。零策略：不持主题清单、不解析帧、
//! 无 serde/topic 字面量/magic 常量/chmod，依赖闭包 L0。
//!
//! 寿命三态（每链路独立）：UDS 未就绪→指数退避（1s×2 至 10s），期间 UART 上行丢弃
//! 计数（不缓冲，自愈靠 register 应答超时重注册）；UART 断链→关 UDS、持续重开设备；
//! UDS 断而 UART 存活→UART 口持续持有，重连后恢复透传；退出→逐条关 UDS、释放 UART。
//!
//! 开口残留由 `Rr3UartConn::open` 的 tcflush 净化。线程：每链路三条（上行/下行/主），
//! UART 侧 rx/tx 两个独立句柄——单句柄加锁会让阻塞读串行化双向，分体即全双工且无锁。

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

mod link;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}\n{}", USAGE.trim());
            return ExitCode::FAILURE;
        }
    };
    // SIGTERM/SIGINT → 置位退出标志（§5.1.5 净退：主循环逐条关 UDS、释放
    // UART 设备节点，退出码 0）。句柄经 OnceSlot 转交信号处理函数——C-ABI
    // 回调不能捕获环境，Async-signal-safety 上 AtomicBool::store 是合法操作。
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    link::rr3_signal_handlers(Arc::clone(&shutdown));
    let code = link::run(cfg, shutdown);
    ExitCode::from(code)
}

/// 用法文本（参数名 §5.1.2 钉死：--uart / --baud / --socket）。
const USAGE: &str =
    "用法: rr3-bridge --uart <串口设备> --socket <rrd套接字路径> [--baud <115200|9600|921600|1500000|2000000>]";

/// CLI 解析（未知参数即错——脚本工具的第一错法是静默忽略拼写错误）。
/// 波特率收 rr-ipc 档位表全部档（115200 缺省 / 9600 降速档 / 921600 批D②
/// 高速档 / 1.5M、2M 高速档——1.5M 是板固件 2026-10-30 起的定档），档位外即错：
/// 静默换档是线路事故——与 rr-ipc
/// `rr3_uart_baud_speed` 同一张表（真值单点），错误在解析期显形而不是深到 open 才炸。
pub fn parse_args(args: &[String]) -> Result<link::BridgeConfig, String> {
    let mut uart: Option<PathBuf> = None;
    let mut socket: Option<PathBuf> = None;
    let mut baud = link::RR3_BRIDGE_BAUD_DEFAULT;
    let mut i = 0;
    while i < args.len() {
        rr3_bridge_arg(&args[i], args, i, &mut uart, &mut socket, &mut baud)?;
        i += 2;
    }
    rr3_bridge_config(uart, socket, baud)
}

/// 单个旗标落位（`--uart` / `--socket` / `--baud`；未知旗标即错）。
fn rr3_bridge_arg(
    flag: &str,
    args: &[String],
    i: usize,
    uart: &mut Option<PathBuf>,
    socket: &mut Option<PathBuf>,
    baud: &mut u32,
) -> Result<(), String> {
    rr3_bridge_flag(flag)?;
    let v = rr3_arg_value(flag, args, i)?;
    rr3_bridge_set(flag, v, uart, socket, baud)
}

/// 旗标白名单门（未知旗标即错）。
fn rr3_bridge_flag(flag: &str) -> Result<(), String> {
    if !matches!(flag, "--uart" | "--socket" | "--baud") {
        return Err(format!("未知参数 {flag}（可用：--uart --socket --baud）"));
    }
    Ok(())
}

/// 已校验旗标的值落位（`--uart` / `--socket` / `--baud`）。
fn rr3_bridge_set(
    flag: &str,
    v: &str,
    uart: &mut Option<PathBuf>,
    socket: &mut Option<PathBuf>,
    baud: &mut u32,
) -> Result<(), String> {
    if flag == "--uart" {
        *uart = Some(PathBuf::from(v));
    } else if flag == "--socket" {
        *socket = Some(PathBuf::from(v));
    } else {
        *baud = rr3_arg_baud(v)?;
    }
    Ok(())
}

/// 必给项与档位裁决（档位校验在这里显形：rr3_uart_baud_speed 同表，三档白名单
/// 真值单点在 rr-ipc；桥不自持档位清单）。
fn rr3_bridge_config(
    uart: Option<PathBuf>,
    socket: Option<PathBuf>,
    baud: u32,
) -> Result<link::BridgeConfig, String> {
    let uart = uart.ok_or("缺 --uart（UART 设备路径）")?;
    let socket = socket.ok_or("缺 --socket（rrd 套接字路径）")?;
    rr3_bridge_baud(baud)?;
    Ok(link::BridgeConfig { uart, baud, socket })
}

/// 档位门（三档白名单真值单点在 rr-ipc，桥不自持档位清单）。
fn rr3_bridge_baud(baud: u32) -> Result<(), String> {
    rr_ipc::transport::uart::rr3_uart_baud_speed(baud)
        .map_err(|e| format!("--baud {baud} 不可用: {e}"))?;
    Ok(())
}

/// 取值：缺值或疑似开关一律拒（静默忽略拼写错误是脚本工具的第一错法）。
fn rr3_arg_value<'a>(flag: &str, args: &'a [String], i: usize) -> Result<&'a str, String> {
    let v = args.get(i + 1).ok_or_else(|| format!("{flag} 缺参数"))?;
    rr3_arg_switch_ok(flag, v)?;
    Ok(v)
}

/// 值不像开关（漏值/拼写错误都点名拒）。
fn rr3_arg_switch_ok(flag: &str, v: &str) -> Result<(), String> {
    if v.starts_with('-') {
        return Err(format!("{flag} 缺参数：收到疑似开关的 {v}"));
    }
    Ok(())
}

/// 波特率数字面（档位白名单另在 parse_args 末尾统一裁决）。
fn rr3_arg_baud(v: &str) -> Result<u32, String> {
    v.parse::<u32>()
        .map_err(|_| format!("--baud 应为数字，实际 {v}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_uart_socket() {
        assert!(parse_args(&args(&[])).is_err());
        assert!(parse_args(&args(&["--uart", "/dev/ttyUSB0"])).is_err());
        assert!(parse_args(&args(&["--socket", "/tmp/s.sock"])).is_err());
        let ok = parse_args(&args(&[
            "--uart",
            "/dev/ttyUSB0",
            "--socket",
            "/tmp/s.sock",
        ]))
        .expect("最小参数应通过");
        assert_eq!(ok.uart, PathBuf::from("/dev/ttyUSB0"));
        assert_eq!(ok.socket, PathBuf::from("/tmp/s.sock"));
        assert_eq!(ok.baud, link::RR3_BRIDGE_BAUD_DEFAULT);
    }

    #[test]
    fn parse_rej_miss_value() {
        assert!(
            parse_args(&args(&["--uark", "/dev/x"])).is_err(),
            "未知参数必须报错"
        );
        assert!(parse_args(&args(&["--uart"])).is_err(), "缺值必须报错");
        assert!(
            parse_args(&args(&["--uart", "--socket", "/tmp/s.sock"])).is_err(),
            "疑似开关的值必须报错"
        );
    }

    #[test]
    fn parse_tier_table() {
        let ok = parse_args(&args(&[
            "--uart", "/dev/x", "--socket", "/tmp/s", "--baud", "9600",
        ]))
        .expect("9600 是 P3 合法降速档");
        assert_eq!(ok.baud, 9600);
        let hi = parse_args(&args(&[
            "--uart", "/dev/x", "--socket", "/tmp/s", "--baud", "921600",
        ]))
        .expect("921600 是批D② 合法高速档");
        assert_eq!(hi.baud, 921600);
        assert!(
            parse_args(&args(&[
                "--uart", "/dev/x", "--socket", "/tmp/s", "--baud", "230400"
            ]))
            .is_err(),
            "档位外必须在解析期报错（静默换档是线路事故）"
        );
        assert!(parse_args(&args(&[
            "--uart", "/dev/x", "--socket", "/tmp/s", "--baud", "abc"
        ]))
        .is_err());
    }
}
