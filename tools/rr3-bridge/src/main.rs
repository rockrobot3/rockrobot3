//! rr3-bridge —— 0.0.7 批E 层④ 桥件（0.0.7 方案 §5.1 形状钉死直写，R15/R20 零冲突）。
//!
//! **定位**（§5.1.1）：UART↔UDS 的 transport 级字节中继（连接多路器）。不是模块——
//! 不进 manifest 注册面、不进装配单 bindings；rrd 侧看到的是哨兵的完整 register。
//!
//! **连接模型**（§5.1.2）：一条 `--uart <dev>` = 一条 UART 链路 = 一个 ESP32 哨兵
//! ↔ 恰好一条 UDS 连接（连 rrd 的 `rock3-ipc.sock`），1:1 映射。多哨兵 = 多个
//! `--uart` 实例（多进程，多路器语义的形状预留；批E 单路验收）。
//!
//! **字节透传语义**（§5.1.3）：双向全双工、**零解析**——UART 读多少转发多少
//! （固定块搬运），帧边界归两端的帧读写层；桥不认识 128B 头、不校验 magic、
//! 不组帧、不改写任何字节（src/dst/trace 原样）。握手是帧级协议（§5.1.4），
//! 哨兵自己发 register 帧，桥对握手不可见。
//!
//! **零策略四条断言**（§5.1.8，源码即证据）：不持主题清单 / 不解析帧 /
//! 不改写字节 / 不做权限特判——本 crate 无 serde、无 topic 字面量、无 magic
//! 常量、无 chmod/chown，依赖闭包 L0（std + rr-ipc）。
//!
//! **寿命管理三态**（§5.1.5，每条链路独立）：
//! - UDS 未就绪：connect 指数退避重试（1s 起步 ×2 至 10s 封顶，恢复即连）；
//!   期间 UART 上行丢弃 + 丢字节计数日志（不缓冲——自愈依据 rrd-rpc.md §5
//!   TimedOut：哨兵 register 应答超时即重试/重启重注册，rrd 语义零改动）；
//! - UART 断链：读/写侧 EOF/错误 → 关 UDS → 持续重开 UART 设备节点 → 成功后
//!   回 UDS 重试循环——哨兵重启后重新 register，全链自愈；
//! - UDS 断链而 UART 存活：UART 口持续持有，UDS 重连后恢复透传；
//! - 桥自身退出：SIGTERM/SIGINT → 逐条关 UDS → 释放 UART 设备节点（退出码 0）。
//!
//! **开口帧同步**（批D1 敞口处置）：残留字节由 rr-ipc 的 `Rr3UartConn::open`
//! 开口净化（tcflush）兜住，桥不再有自己的清缓冲逻辑——开口后的字节桥一概不碰。
//!
//! **任务模型**：每条 UART 链路三条线程——上行（UART 读→UDS 写）、下行（UDS
//! 读→UART 写）、主线程（寿命管理）。UART 侧用**两个独立句柄**（rx 只读 / tx
//! 只写，同一设备节点 open 两次）：单句柄 + 互斥锁会让阻塞读持锁串行化双向；
//! tty 输入队列设备级共享、只读方消费，输出队列只写方投递——rx/tx 分体即
//! 全双工且无锁。下行线程持读阻塞可接受：rrd 断链以 FIN 及时显形（EOF），
//! 不会把寿命管理卡在旧流上。

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
    link::register_signal_handlers(Arc::clone(&shutdown));
    let code = link::run(cfg, shutdown);
    ExitCode::from(code)
}

/// 用法文本（参数名 §5.1.2 钉死：--uart / --baud / --socket）。
const USAGE: &str =
    "用法: rr3-bridge --uart <串口设备> --socket <rrd套接字路径> [--baud <115200|9600|921600|1500000|2000000>]";

/// CLI 解析（未知参数即错——脚本工具的第一错法是静默忽略拼写错误）。
/// 波特率收 rr-ipc 档位表全部档（115200 缺省 / 9600 降速档 / 921600 批D②
/// 高速档 / 1.5M、2M 试用档——2M 是板固件 2026-10-30 起的落板档），档位外即错：
/// 静默换档是线路事故——与 rr-ipc
/// `rr3_uart_baud_speed` 同一张表（真值单点），错误在解析期显形而不是深到 open 才炸。
pub fn parse_args(args: &[String]) -> Result<link::BridgeConfig, String> {
    let mut uart: Option<PathBuf> = None;
    let mut socket: Option<PathBuf> = None;
    let mut baud = link::RR3_BRIDGE_BAUD_DEFAULT;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--uart" | "--socket" | "--baud" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| format!("{0} 缺参数", args[i]))?;
                if v.starts_with('-') {
                    return Err(format!("{0} 缺参数：收到疑似开关的 {v}", args[i]));
                }
                match args[i].as_str() {
                    "--uart" => uart = Some(PathBuf::from(v)),
                    "--socket" => socket = Some(PathBuf::from(v)),
                    _ => {
                        baud = v
                            .parse::<u32>()
                            .map_err(|_| format!("--baud 应为数字，实际 {v}"))?;
                    }
                }
                i += 1;
            }
            other => return Err(format!("未知参数 {other}（可用：--uart --socket --baud）")),
        }
        i += 1;
    }
    let uart = uart.ok_or("缺 --uart（UART 设备路径）")?;
    let socket = socket.ok_or("缺 --socket（rrd 套接字路径）")?;
    // 档位校验在这里显形（rr3_uart_baud_speed 同表，三档白名单真值单点在
    // rr-ipc）；桥不自持档位清单。
    rr_ipc::transport::uart::rr3_uart_baud_speed(baud)
        .map_err(|e| format!("--baud {baud} 不可用: {e}"))?;
    Ok(link::BridgeConfig { uart, baud, socket })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_requires_uart_and_socket() {
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
    fn parse_rejects_unknown_flag_and_missing_value() {
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
    fn parse_baud_tier_table() {
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
        assert!(parse_args(&args(&[
            "--uart", "/dev/x", "--socket", "/tmp/s", "--baud", "230400"
        ]))
        .is_err(), "档位外必须在解析期报错（静默换档是线路事故）");
        assert!(parse_args(&args(&[
            "--uart", "/dev/x", "--socket", "/tmp/s", "--baud", "abc"
        ]))
        .is_err());
    }
}
