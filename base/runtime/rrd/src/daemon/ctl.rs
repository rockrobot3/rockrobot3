//! 控制面收尾：SIGTERM/SIGINT 强制 flush、self-pipe 与 socket 访问控制放行。

use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::registry::RrdRegistry;
use crate::{rr3_lock, rr3_log};

/// ctl socket 的属主组（0.0.6 批F5b R12）：postinst 建系统组，运维用户由
/// 装机现场入组（deb 不该动用户）——socket 权限位因此成为 ADR-0010 裁定 2
/// "套接字文件权限就是访问控制"的落地值。
const RR3_CTL_SOCKET_GROUP: &str = "rockrobot3";

// ── R26 关停强制 flush（0.0.5 增补轮四 批W3）────────────────────────────────
//
// self-pipe 信号面：`signal(2)` 的处理器里只做 async-signal-safe 的一件事——
// 往管道写一个字节；快照 / 写盘 / 日志全部落在 `counters-term-flush` 普通线程
// （信号处理器里碰 Mutex/File 都是未定义行为，不能省这一层）。两个 libc 函数
// 手写 extern 壳不引依赖（同 rr-hal chmod/getgrnam 先例；原 rr-ipc shm 六函数
// 先例已随 L-32 摘面移除，0.0.8 批N19）。

extern "C" {
    fn signal(signum: i32, handler: usize) -> usize;
    // buf 用 POSIX 原型 `const void *`（c_void）：与 std 运行时同名符号签名一致，
    // 否则触发 rustc suspicious_runtime_symbol_definitions 警告（CI -D warnings 红）
    fn write(fd: i32, buf: *const std::ffi::c_void, count: usize) -> isize;
}

/// POSIX 标准信号号：SIGINT=2、SIGTERM=15
const RR3_SIGINT: i32 = 2;
const RR3_SIGTERM: i32 = 15;

/// self-pipe 发送端裸 fd（处理器读的唯一状态；-1 = 未安装）
static RR3_TERM_PIPE_FD: AtomicI32 = AtomicI32::new(-1);

/// 信号处理器：写一字节即返回。 errno 不保存（best-effort 唤醒，失手也只影响
/// 这一拍 flush，进程随后的默认退出路径照走）。
extern "C" fn rr3_term_pipe_wake(_sig: i32) {
    let fd = RR3_TERM_PIPE_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        let one: u8 = 1;
        unsafe {
            write(fd, &one as *const u8 as *const std::ffi::c_void, 1);
        }
    }
}

/// 安装 SIGTERM/SIGINT 强制 flush（R26）：合并写窗口内未落盘的尾部计数在关停
/// 路径一次性写全（systemctl stop / reboot 的 cgroup SIGTERM、Ctrl-C 都走这里）。
/// flush 完成 `exit(0)`——装了处理器信号就不再杀进程，必须自己退；flush 失败
/// 只落日志不挡关停（运维账不许挡产品路，与启动装载同一口径）。未装上（pipe
/// 失败/线程起不来）留痕跳过，SIGTERM 回落默认杀进程语义，不致命。
pub(super) fn rr3_flush_on_term(shared: Arc<Mutex<RrdRegistry>>, path: Option<PathBuf>) {
    let Some(rx) = rr3_flush_arm() else {
        return;
    };
    if let Err(e) = rr3_flush_spawn(rx, shared, path) {
        // 线程起不来：处理器已在位但无人读 self-pipe。Rust 把 SIGPIPE 置 IGN，
        // 信号处理器写空管道只会得 EPIPE 而不终止——必须在这里恢复默认处置，
        // 否则 systemctl stop 会一直等到 SIGKILL（与 EINTR 那条同源的吞信号故障）。
        rr3_log(&format!("counters 关停 flush 未装上（线程起不来）: {e}"));
        rr3_term_default();
    }
}

/// 起 flush 线程（读 self-pipe → 落盘 → exit）。
fn rr3_flush_spawn(
    rx: std::os::unix::net::UnixStream,
    shared: Arc<Mutex<RrdRegistry>>,
    path: Option<PathBuf>,
) -> std::io::Result<()> {
    thread::Builder::new()
        .name("counters-term-flush".into())
        .spawn(move || rr3_flush_wait(rx, shared, path))
        .map(|_| ())
}

/// 装管道与信号处理器：返回 self-pipe 读端；pipe 失败留痕跳过（SIGTERM 回落
/// 默认杀进程语义，不致命）。
fn rr3_flush_arm() -> Option<std::os::unix::net::UnixStream> {
    use std::os::unix::io::IntoRawFd;
    let (tx, rx) = match std::os::unix::net::UnixStream::pair() {
        Ok(pair) => pair,
        Err(e) => {
            rr3_log(&format!(
                "counters 关停 flush 未装上（self-pipe 失败）: {e}"
            ));
            return None;
        }
    };
    // 发送端交给处理器：取裸 fd 后刻意不 close（进程生命期一根管，泄漏即设计）
    RR3_TERM_PIPE_FD.store(tx.into_raw_fd(), Ordering::Relaxed);
    rr3_term_signals();
    Some(rx)
}

/// SIGTERM/SIGINT 接唤醒处理器（信号投递后由读端退出关停 flush）。
fn rr3_term_signals() {
    unsafe {
        signal(
            RR3_SIGTERM,
            rr3_term_pipe_wake as extern "C" fn(i32) as usize,
        );
        signal(
            RR3_SIGINT,
            rr3_term_pipe_wake as extern "C" fn(i32) as usize,
        );
    }
}

/// flush 线程主体：平时睡在 self-pipe read；醒来一次 = 一个 TERM/INT，落盘后
/// 即关停（exit 不返回）。EINTR 必须重试——std 的 read 不自动吞信号中断，
/// 若按"非 Ok(1) 即收场"处理，一次 EINTR 就让本线程退出，而处理器已在位，
/// 守护从此不再响应默认终止语义（systemd stop 等到 SIGKILL，尾部计数丢）。
fn rr3_flush_wait(
    mut rx: std::os::unix::net::UnixStream,
    shared: Arc<Mutex<RrdRegistry>>,
    path: Option<PathBuf>,
) {
    use std::io::Read;
    let mut one = [0u8; 1];
    loop {
        match rx.read(&mut one) {
            Ok(1) => {
                rr3_flush_store(&shared, path.as_deref());
                std::process::exit(0);
            }
            Ok(_) => {
                // self-pipe 写端是进程生命期的静态 fd，EOF 理论上不发生；
                // 真发生则恢复默认信号处置后收场——宁可让 SIGTERM 回到
                // "直接杀进程"，也不能留一个吞信号的空壳线程。
                rr3_log("counters 关停 flush 线程异常收场（self-pipe EOF），恢复默认信号处置");
                rr3_term_default();
                return;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                rr3_log(&format!(
                    "counters 关停 flush 线程读 self-pipe 失败（{e}），恢复默认信号处置"
                ));
                rr3_term_default();
                return;
            }
        }
    }
}

/// 关停落盘一段：快照 → 持久化 → 成败留痕（失败不挡关停，运维账不许挡产品路）。
fn rr3_flush_store(shared: &Mutex<RrdRegistry>, path: Option<&std::path::Path>) {
    // 关停起点先行留痕：若后续 flush 卡在磁盘上，这行是"信号已收到、正在落盘"
    // 与"信号都没到"的分界。
    rr3_log("收到终止信号（SIGTERM/SIGINT），开始关停 flush");
    let snap = rr3_lock(shared).rr3_reg_counters();
    match path.map(|p| crate::counters::rr3_counters_store(p, &snap)) {
        Some(Ok(())) => rr3_log("counters 关停 flush 已落盘（SIGTERM/SIGINT）"),
        Some(Err(e)) => rr3_log(&format!("counters 关停 flush 失败（不挡关停）: {e}")),
        None => rr3_log("counters 关停 flush 跳过（未启用持久化）"),
    }
}

/// 恢复 SIGTERM/SIGINT 的默认处置（SIG_DFL = 0）：flush 线程若异常收场，
/// 后续终止信号仍按内核默认语义杀进程，不被已安装的处理器永久吞掉。
fn rr3_term_default() {
    unsafe {
        signal(RR3_SIGTERM, 0);
        signal(RR3_SIGINT, 0);
    }
}

/// ctl socket 访问控制放行（0.0.6 批F5b R12，ADR-0010 权限模型增补）：bind 后
/// `chmod 0o660`（owner + 属主组可读写，other 无份——修真板首发现敞口
/// `srwxr-xr-x root:root` 非组用户连不上）+ `chgrp rockrobot3`（组不存在时
/// 留痕跳过，chmod 位仍生效——紧权限的敞口比守护起不来轻得多，**失败不拒启**）。
/// 放行是尽力而为的部署面语义：sim 直跑/CI 全 root 无感（root 恒可连）。
///
/// libc 仅声明壳（`chmod`/`getgrnam`/`chown`，musl 静态零额外 C 依赖，R13）；
/// errno 逐调用取出（连续调用互不覆盖）。
pub(super) fn rr3_socket_acl_grant(path: &std::path::Path) {
    use std::os::unix::ffi::OsStrExt;
    let Ok(cpath) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        rr3_log("socket 放行跳过：路径含 NUL（不可能的缺省值）");
        return;
    };
    if !rr3_acl_chmod(&cpath) {
        return; // chgrp 无从谈起，权限面交回调用方日志
    }
    rr3_acl_group(&cpath);
}

/// 放行第 1 步：`chmod 0o660`（other 位清掉，属主组位落上）。false = chmod 失败。
fn rr3_acl_chmod(cpath: &std::ffi::CString) -> bool {
    let rc = unsafe { libc::chmod(cpath.as_ptr(), 0o660) };
    if rc != 0 {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        rr3_log(&format!(
            "socket 放行 chmod 失败（errno {errno}，保留 umask 权限如旧）"
        ));
        return false;
    }
    true
}

/// 放行第 2 步：`chgrp` 到属主组（组不在位留痕跳过不视为错）。
fn rr3_acl_group(cpath: &std::ffi::CString) {
    let Ok(cgroup) = std::ffi::CString::new(RR3_CTL_SOCKET_GROUP) else {
        return;
    };
    let gr = unsafe { libc::getgrnam(cgroup.as_ptr()) };
    if gr.is_null() {
        rr3_log(&format!(
            "socket 属主组 {RR3_CTL_SOCKET_GROUP} 不在位（postinst 未跑/开发机），保留 root:root——组内用户暂连不上"
        ));
        return;
    }
    let gid = unsafe { (*gr).gr_gid };
    rr3_acl_chown(cpath, gid);
}

/// 放行第 2 步的落位：`chown` 到属主组（失败留痕不拒启）。
fn rr3_acl_chown(cpath: &std::ffi::CString, gid: libc::gid_t) {
    let uid = libc::uid_t::MAX; // chown 不改属主（uid_t 无符号，MAX 即 POSIX 的 (uid_t)-1）
    let rc = unsafe { libc::chown(cpath.as_ptr(), uid, gid) };
    if rc != 0 {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        rr3_log(&format!(
            "socket 放行 chgrp 失败（errno {errno}，chmod 0o660 已生效）"
        ));
    }
}
