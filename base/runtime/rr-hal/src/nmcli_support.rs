//! nmcli 子进程承载的公共机制层（0.0.5 批M M-γ，MC-6 抽共享）：terse 转义/字段
//! 提取与"spawn + 超时守护 + 管道读"的执行函数，自 [`crate::linux_link`] 原样迁入——
//! NM terse 协议语义与子进程超时守护各是一个事实，一个事实一个真值点，不允许
//! 链路读数与配网凭据存储（MC-6 [`crate::provision_store`]）两处各抄一份。
//!
//! 编译门 `cfg(any(target_os = "linux", test))`：linux 全编译（两个后端消费）；
//! macOS 非 test 面不编译（无消费方——选择函数 linux 分支体在非 linux 平台被
//! cfg 剔除）；macOS test 面编译——MC-6 的"NMCONF_HOME 夹具主机可测"判据要求
//! 凭据存储用例在门禁主机直跑（linux_link 的单重平台门不让测试上主机，MC-6
//! 不能沿用）。

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{Rr3ErrText, Rr3HalError};

/// 子进程存活轮询间隔：超时判定的分辨率。nmcli 正常毫秒级返回，10 ms 轮询
/// 的空转代价可忽略；比它粗会让超时迟到，比它细纯烧 CPU。
pub(crate) const RR3_NM_POLL: Duration = Duration::from_millis(10);

/// terse 值的反转义：NM terse 模式把值里的 `:` 与 `\` 分别转义成 `\:`
/// 与 `\\`（否则冒号分隔表无法承载含冒号的 SSID）。
pub(crate) fn rr3_nm_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(':') => out.push(':'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push(other);
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// terse 输出里取一个字段值：逐行找 `key:` 前缀，冒号后整段返回
/// （值内可能有转义冒号，不能再 split；取值后调用方自行反转义）。
pub(crate) fn rr3_nm_field<'a>(out: &'a str, key: &str) -> Option<&'a str> {
    out.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
}

/// 带 deadline 的管道读尽：fd 置非阻塞后轮询读，EOF 或到点为止，到点返回
/// 已读内容（调用方拿残缺输出走各自的报错/回落路径，不无限等）。为什么
/// 不用阻塞版 `read_to_end`：kill 只收直子进程，幸存孙进程仍持写端时会
/// 一直等到孙退出——超时对管道面就设不了防（批H5 审查补档）。
/// 非阻塞位不切回去也无妨：fd 随 ChildStdout 丢弃即销毁，无跨调用状态。
#[cfg(target_os = "linux")]
pub(crate) fn rr3_nm_read_to_end_capped(
    s: &mut (impl Read + std::os::fd::AsRawFd),
    deadline: Instant,
) -> Vec<u8> {
    let flags = unsafe { libc::fcntl(s.as_raw_fd(), libc::F_GETFL) };
    if flags >= 0 {
        unsafe { libc::fcntl(s.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
    }
    capped_loop(s, deadline)
}

/// 非 linux 面的退化版：libc 依赖挂 Cargo.toml 的 linux 门（macOS 开发主机
/// 无 libc），读退化为阻塞 `read_to_end`。capped 语义守护的是"幸存孙进程
/// 持写端"旁路——该旁路只在 linux 面的假件编排里成立；本机测试假脚本不
/// fork 孙进程，EOF 立到，退化版等价。超时守护本体（try_wait 轮询 + kill）
/// 是纯 std，两平台同一份。
#[cfg(not(target_os = "linux"))]
pub(crate) fn rr3_nm_read_to_end_capped(s: &mut impl Read, deadline: Instant) -> Vec<u8> {
    capped_loop(s, deadline)
}

/// 两平台共用的读循环主体：EOF/到点/错误三出口。
fn capped_loop(s: &mut impl Read, deadline: Instant) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        if Instant::now() >= deadline {
            break;
        }
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(RR3_NM_POLL);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    out
}

/// 起一次 nmcli 子进程（terse 模式由调用方的 args 自带），带超时守护：
/// spawn 后轮询 `try_wait`，到点 kill 并显式回错（0.0.5 批I N-2）。超时分支
/// **回错在前、不读管道**——kill 只收直子进程，其孙进程（如 shell fork 的
/// sleep）仍持管道写端，read_to_end 会等到孙退出才返回，超时兜底反被拖垮；
/// 超时回错本就不需要输出。正常退出后读管道也套同一档 deadline（见
/// [`rr3_nm_read_to_end_capped`]）：真 nmcli 不 fork 幸存孙进程，读立即到
/// EOF，守护只对"假脚本 fork 出幸存孙进程"一类旁路兜底。
pub(crate) fn rr3_nm_run(
    nmcli: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<String, Rr3HalError> {
    let mut child = Command::new(nmcli)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
                "起 nmcli 失败: {e}（NetworkManager/nmcli 不在位？）"
            )))
        })?;
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let mut status: Option<std::process::ExitStatus> = None;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                status = Some(st);
                break;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break;
                }
                std::thread::sleep(RR3_NM_POLL);
            }
            Err(e) => {
                return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
                    &format!("nmcli 等待失败: {e}"),
                )))
            }
        }
    }
    if timed_out {
        return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
            &format!(
                "nmcli 超时（>{}ms）被终止: {}",
                timeout.as_millis(),
                args.join(" "),
            ),
        )));
    }
    // 超时路径已在上面回错——这里只在正常退出后走到。读管道各挂一档
    // deadline（rr3_nm_read_to_end_capped）：真 nmcli 毫秒级 EOF，守护只对
    // "脚本被替换成 fork 幸存孙进程的假件"一类旁路兜底，不让调用面被拖住
    let stdout = child
        .stdout
        .take()
        .map(|mut s| rr3_nm_read_to_end_capped(&mut s, Instant::now() + timeout))
        .unwrap_or_default();
    let stderr = child
        .stderr
        .take()
        .map(|mut s| rr3_nm_read_to_end_capped(&mut s, Instant::now() + timeout))
        .unwrap_or_default();
    if !status.map(|s| s.success()).unwrap_or(false) {
        return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
            &format!(
                "nmcli 退出非零（{:?}）: {}",
                args,
                String::from_utf8_lossy(&stderr).trim()
            ),
        )));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}
