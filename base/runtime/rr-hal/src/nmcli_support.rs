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

/// 夹具/冒烟用例的 nmcli 子进程超时档（**非计时用例**用）：假脚本与真 nmcli
/// 都毫秒级返回，本档只防"夹具真挂死"，取生产 config `timeout_ms` 同档
/// （`modules/rr3-connect/config.toml` = 20000）。此前各逻辑用例硬写 5 s，
/// 假脚本 spawn 在 macOS 并行负载下偶发超 5 s，会让 parse/持久化语义用例假红
/// （0.0.5 批M MC-10 前置修）。**超时守护本体**由各 `*_timeout_kills_slow_child`
/// 用例以 100 ms 档独立钉，不用本档。
#[cfg(test)]
pub(crate) const RR3_TEST_NM_TIMEOUT: Duration = Duration::from_secs(20);

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
pub(crate) fn rr3_nm_read_capped(
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
pub(crate) fn rr3_nm_read_capped(s: &mut impl Read, deadline: Instant) -> Vec<u8> {
    capped_loop(s, deadline)
}

/// 子进程输出字节上限：nmcli 的 terse 输出是行文本（扫描表每 AP 一行），1 MiB
/// 已远超真实环境；设帽防"nmcli 被替换/异常刷屏"把内存吃穿（deadline 只管时间，
/// 不管字节量——2026-11-09 静态审计）。
const RR3_NM_OUT_CAP: usize = 1 << 20;

/// 两平台共用的读循环主体：EOF/到点/超帽/错误四出口。超帽截断返回，调用方按
/// 自身的解析/报错路径处理残文（不在这里判业务语义）。
fn capped_loop(s: &mut impl Read, deadline: Instant) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        if Instant::now() >= deadline || out.len() >= RR3_NM_OUT_CAP {
            break;
        }
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let room = RR3_NM_OUT_CAP - out.len();
                out.extend_from_slice(&buf[..n.min(room)]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(RR3_NM_POLL);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    out
}

/// 错误文案里的 argv 脱敏：Wi-Fi 凭据只以 `psk`/`password` 键的下一项或
/// `*.psk=...` 形式出现；超时/非零错误不得把口令写进日志（凭据泄漏面）。
fn rr3_nm_redact(args: &[&str]) -> String {
    let mut out: Vec<String> = Vec::with_capacity(args.len());
    let mut mask_next = false;
    for a in args {
        if mask_next {
            out.push("***".to_string());
            mask_next = false;
            continue;
        }
        if let Some((k, _)) = a.split_once('=') {
            if k == "psk" || k == "password" || k.ends_with(".psk") {
                out.push(format!("{k}=***"));
            } else {
                out.push((*a).to_string());
            }
            continue;
        }
        out.push((*a).to_string());
        if *a == "psk" || *a == "password" || a.ends_with(".psk") {
            mask_next = true;
        }
    }
    out.join(" ")
}

/// 起一次 nmcli 子进程（terse 模式由调用方的 args 自带），带超时守护：
/// spawn 后轮询 `try_wait`，到点 kill 并显式回错（0.0.5 批I N-2）。超时分支
/// **回错在前、不读管道**——kill 只收直子进程，其孙进程（如 shell fork 的
/// sleep）仍持管道写端，read_to_end 会等到孙退出才返回，超时兜底反被拖垮；
/// 超时回错本就不需要输出。正常退出后读管道也套同一档 deadline（见
/// [`rr3_nm_read_capped`]）：真 nmcli 不 fork 幸存孙进程，读立即到
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
                rr3_nm_redact(args),
            ),
        )));
    }
    // 超时路径已在上面回错——这里只在正常退出后走到。读管道各挂一档
    // deadline（rr3_nm_read_capped）：真 nmcli 毫秒级 EOF，守护只对
    // "脚本被替换成 fork 幸存孙进程的假件"一类旁路兜底，不让调用面被拖住
    let stdout = child
        .stdout
        .take()
        .map(|mut s| rr3_nm_read_capped(&mut s, Instant::now() + timeout))
        .unwrap_or_default();
    let stderr = child
        .stderr
        .take()
        .map(|mut s| rr3_nm_read_capped(&mut s, Instant::now() + timeout))
        .unwrap_or_default();
    if !status.map(|s| s.success()).unwrap_or(false) {
        return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
            &format!(
                "nmcli 退出非零（{}）: {}",
                rr3_nm_redact(args),
                String::from_utf8_lossy(&stderr).trim()
            ),
        )));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 超时/非零错误里的 argv 必须脱敏：ssid 与非凭据值保留，psk 值打码。
    #[test]
    fn redact_hides_wifi_psk() {
        let args = [
            "con",
            "modify",
            "rr3-provision",
            "802-11-wireless.ssid",
            "Home",
            "802-11-wireless-security.psk",
            "s3cret",
            "802-11-wireless-security.key-mgmt",
            "wpa-psk",
        ];
        let s = rr3_nm_redact(&args);
        assert!(s.contains("Home"), "ssid 保留: {s}");
        assert!(s.contains("wpa-psk"), "非凭据值保留: {s}");
        assert!(!s.contains("s3cret"), "口令不得出现: {s}");
        assert!(s.contains("***"), "凭据位必须打码: {s}");
        let eq = rr3_nm_redact(&["con", "modify", "x", "802-11-wireless-security.psk=s3cret"]);
        assert!(
            !eq.contains("s3cret") && eq.contains("802-11-wireless-security.psk=***"),
            "等号形式同样脱敏: {eq}"
        );
    }

    /// 读帽：源源不断的输入也只收到 1MiB（deadline 管时间，帽管字节量）。
    #[test]
    fn read_stops_at_byte_cap() {
        let mut src = std::io::repeat(b'x').take((RR3_NM_OUT_CAP as u64) * 4);
        let out = capped_loop(&mut src, Instant::now() + Duration::from_secs(5));
        assert_eq!(out.len(), RR3_NM_OUT_CAP, "超帽即截断");
    }
}
