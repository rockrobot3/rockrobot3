// SPDX-License-Identifier: Apache-2.0
//! systemd 自身看门狗：把 `WATCHDOG=1` 发到 `$NOTIFY_SOCKET`（P2-2）。
//!
//! 模块 watchdog 只能发现"模块死了"；rrd 自己死锁时，模块监督线程也随之停摆。
//! 本模块让 systemd 的 `WatchdogSec=` 有判据：watchdog 线程每 5s 喂一次，
//! 30s 未喂即由 systemd 重启整个守护。`NOTIFY_SOCKET` 缺位（本地/容器直跑）
//! 时静默 no-op，不制造假失败。

use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// 喂狗间隔（毫秒）：watchdog 线程按它限频。
pub(super) const NOTIFY_MS: u64 = 5_000;

static NOTIFY: OnceLock<Option<Vec<u8>>> = OnceLock::new();
static WARNED: AtomicBool = AtomicBool::new(false);

fn rr3_notify_addr_from(raw: &OsStr) -> Option<Vec<u8>> {
    let bytes = raw.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() + 1);
    if bytes[0] == b'@' {
        out.push(0);
        out.extend_from_slice(&bytes[1..]);
    } else {
        out.extend_from_slice(bytes);
    }
    out.push(0);
    Some(out)
}

fn rr3_notify_addr() -> Option<&'static [u8]> {
    NOTIFY
        .get_or_init(|| {
            std::env::var_os("NOTIFY_SOCKET")
                .as_deref()
                .and_then(rr3_notify_addr_from)
        })
        .as_deref()
}

fn rr3_notify_warn(what: &str) {
    if !WARNED.swap(true, Ordering::Relaxed) {
        crate::rr3_log(&format!(
            "systemd notify 失败（{what}）——rrd 自身看门狗不会刷新"
        ));
    }
}

fn rr3_notify_send(msg: &str) {
    let Some(addr) = rr3_notify_addr() else {
        return;
    };
    let Ok(payload) = CString::new(msg) else {
        return;
    };
    // SAFETY: socket 参数为常量；返回值在下一行检查，失败不继续使用。
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM, 0) };
    if fd < 0 {
        rr3_notify_warn("socket");
        return;
    }
    // SAFETY: sockaddr_un 是 POD，全零是合法初始态；随后只写 family/path。
    let mut sa: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    sa.sun_family = libc::AF_UNIX as libc::sa_family_t;
    if addr.len() > sa.sun_path.len() {
        // SAFETY: fd 由本函数打开，仅关闭一次。
        let _ = unsafe { libc::close(fd) };
        rr3_notify_warn("NOTIFY_SOCKET 过长");
        return;
    }
    for (i, b) in addr.iter().enumerate() {
        sa.sun_path[i] = *b as libc::c_char;
    }
    let len = (std::mem::size_of::<libc::sockaddr_un>() - sa.sun_path.len() + addr.len())
        as libc::socklen_t;
    // SAFETY: fd 有效、sa 已初始化、payload 是 NUL 结尾字符串、len 与 addr 一致。
    let rc = unsafe {
        libc::sendto(
            fd,
            payload.as_ptr() as *const libc::c_void,
            payload.as_bytes().len(),
            0,
            &sa as *const libc::sockaddr_un as *const libc::sockaddr,
            len,
        )
    };
    // SAFETY: fd 由本函数打开，仅关闭一次。
    let _ = unsafe { libc::close(fd) };
    if rc < 0 {
        rr3_notify_warn(&format!("sendto: {}", std::io::Error::last_os_error()));
    }
}

/// 喂一次 systemd 看门狗（watchdog 线程每 [`NOTIFY_MS`] 调用）。
pub(super) fn rr3_notify_watchdog() {
    rr3_notify_send("WATCHDOG=1");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_addr_marks_abstract_socket() {
        assert_eq!(
            rr3_notify_addr_from(OsStr::new("@rr3")),
            Some(b"\0rr3\0".to_vec())
        );
        assert_eq!(
            rr3_notify_addr_from(OsStr::new("/run/systemd/notify")),
            Some(b"/run/systemd/notify\0".to_vec())
        );
        assert_eq!(rr3_notify_addr_from(OsStr::new("")), None);
    }
}
