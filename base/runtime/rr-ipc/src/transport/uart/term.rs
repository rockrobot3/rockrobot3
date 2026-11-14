//! raw termios 配置：波特率映射、VMIN/VTIME 与非阻塞位。

use super::*;

/// 取当前 termios（tcgetattr 失败即回 os error）。
pub(super) fn rr3_termios_of(fd: i32) -> std::io::Result<Termios> {
    let mut t = Termios::zeroed();
    // SAFETY：t 是本栈上的合法 termios，tcgetattr 只写它。
    if unsafe { c_tcgetattr(fd, &mut t) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(t)
}

/// 读侧档位：`0` = 阻塞读满（VMIN=1/VTIME=0），否则 VMIN=0/VTIME=ds。
pub(super) fn rr3_vtime(t: &mut Termios, ds: u8) {
    if ds == 0 {
        t.c_cc[VMIN] = 1;
        t.c_cc[VTIME] = 0;
    } else {
        t.c_cc[VMIN] = 0;
        t.c_cc[VTIME] = ds;
    }
}

/// raw 模式配置：取当前 termios → cfmakeraw → 显式 VMIN=1/VTIME=0（阻塞读满语义，
/// 不靠 cfmakeraw 之后残留的寄存器初值）→ 收发波特率 → TCSANOW 生效 →
/// `tcflush(TCIFLUSH)` 净化输入队列。
///
/// 开口即净（0.0.7 批D1 敞口的正式处置，批E 落地）：USB 串口驱动输入缓冲会残留
/// 上轮会话的字节（CP2102 实证），不冲掉则开口首读必是垃圾（BadMagic）。这里
/// 冲的是**开口时刻之前**积压的队列——对线上语义零改写：开口后的字节一概不碰，
/// 帧层拒收路径照旧兜底。
pub(super) fn configure(fd: i32, speed: SpeedT) -> std::io::Result<()> {
    let t = rr3_raw_termios(fd, speed)?;
    rr3_apply_termios(fd, &t)
}

/// 构造 raw termios：取当前值 → cfmakeraw → CLOCAL|CREAD + VMIN/VTIME → 波特率。
fn rr3_raw_termios(fd: i32, speed: SpeedT) -> std::io::Result<Termios> {
    let mut t = rr3_termios_of(fd)?;
    // SAFETY：合法指针；cfmakeraw 只改 termios 位面（清回显/规范化/流控）。
    unsafe { c_cfmakeraw(&mut t) };
    // CLOCAL|CREAD：TTL 链路本无 modem，载波检测语义必须整体摘除——不置位时
    // 阻塞 open/读写以 DCD 为闸（隔离型适配器 DCD 悬空恒"无载波"，开口即永
    // 挂，2026-10-24 现场实锤）。置位后开口不再依赖载波，与线缆对端无关。
    t.c_cflag |= CLOCAL | CREAD;
    t.c_cc[VMIN] = 1;
    t.c_cc[VTIME] = 0;
    rr3_uart_speed(&mut t, speed)?;
    Ok(t)
}

/// 落盘生效：TCSANOW 设 termios，随后 `tcflush(TCIFLUSH)` 净化开口前的输入队列。
fn rr3_apply_termios(fd: i32, t: &Termios) -> std::io::Result<()> {
    // SAFETY：合法指针；TCSANOW 立即生效。
    if unsafe { c_tcsetattr(fd, TCSANOW, t) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    rr3_termios_flush(fd)
}

/// 冲掉开口前积压的输入队列（TCIFLUSH 只冲收不冲发）。
fn rr3_termios_flush(fd: i32) -> std::io::Result<()> {
    // SAFETY：合法指针。
    if unsafe { c_tcflush(fd, TCIFLUSH) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// 收发两侧波特率（ispeed/ospeed 同值）。
fn rr3_uart_speed(t: &mut Termios, speed: SpeedT) -> std::io::Result<()> {
    // SAFETY：合法指针。
    if unsafe { c_cfsetispeed(t, speed) } != 0 || unsafe { c_cfsetospeed(t, speed) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// 撤除 fd 上的 `O_NONBLOCK` 位（开口序列收尾）：open 带 O_NONBLOCK 进场只为
/// 不在 carrier 上悬住，配置完成后读写要走阻塞语义——非阻塞 fd 上 VMIN/VTIME
/// 失效、read 缺数据返 EAGAIN，会把"暂无数据"当真错误抛出。
pub(super) fn rr3_uart_clear_nb(fd: i32) -> std::io::Result<()> {
    // SAFETY：合法 fd；F_GETFL 无变参。
    let flags = unsafe { c_fcntl(fd, F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    rr3_nb_clear(fd, flags)
}

/// 按已取到的 flags 撤 `O_NONBLOCK`（F_SETFL 带一个 int 变参）。
fn rr3_nb_clear(fd: i32, flags: i32) -> std::io::Result<()> {
    // SAFETY：合法 fd。
    if unsafe { c_fcntl(fd, F_SETFL, flags & !O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
