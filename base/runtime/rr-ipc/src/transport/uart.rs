// SPDX-License-Identifier: Apache-2.0
//! UART 传输（ADR-0008 第三实现位；线上帧格式不变）。
//!
//! 点对点字节流：宿主侧按 raw termios 打开串口，帧语义由 blanket impl 免费覆盖
//! （下底 [`Rr3ByteStream`](crate::transport::bytestream::Rr3ByteStream)），半帧 /
//! 坏 magic / 长度不符的拒收与 uds 同源。无"监听/accept"语义，故
//! [`Rr3TransportEndpoint`] 与 listener 两 trait 不适用——硬套点对点等于撒谎。
//!
//! 系统调用面：手写 `extern "C"` 壳（open/close/read/write/tcgetattr/tcsetattr/
//! cfmakeraw/cfsetispeed/cfsetospeed/tcflush/ioctl/fcntl），不引 libc crate；termios
//! 结构与常量按 Linux/macOS 分 `cfg`（`uart/sys.rs`），`const _` 断言结构尺寸。
//!
//! 排他开口：按 (进程, 路径) 的 flock 登记账本——每路径首个 open 开长寿命 guard fd
//! 并 `flock(LOCK_EX|LOCK_NB)`，同进程后续 open 只记账重入（桥 rx/tx 双开合法）；
//! 跨进程第二 opener 当场拒绝（ResourceBusy 点名）。不用 TIOCEXCL：Linux 连自家
//! 双开一起拦，macOS 的 TS_XCLUDE 对 O_NOCTTY 开口无效。外部进程（espflash/
//! monitor）不守 flock，须按 runbook 先清场。fd 一律 `O_CLOEXEC`，锁的释放点不得
//! 被子进程生命周期拖长；`Command::spawn` 复制 fd 表瞬间短暂持锁属合法瞬态。
//!
//! 波特率只收 [`rr3_uart_baud_speed`] 认的档位（115200 缺省 / 9600 / 921600 / 1.5M / 2M）。

use std::io::{Read, Write};
use std::path::Path;

/// 默认波特率（0.0.7 方案 §9.2：115200 起步）
pub const RR3_UART_BAUD_DEFAULT: u32 = 115200;

/// C `char`：Linux aarch64 上是无符号（char 无符号平台），x86_64-linux/macOS 有符号。
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
type CChar = u8;
#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
type CChar = i8;

mod sys;
use sys::*;

// no_std 显式化（0.0.7 批B 口径）：std prelude 不随 `#![no_std]` 隐式注入
use std::collections::HashMap;
use std::format;
use std::string::String;
use std::sync::{Mutex, OnceLock};

mod excl;
mod term;
#[cfg(test)]
mod tests;

pub use excl::rr3_uart_baud_speed;
use excl::*;
use term::*;

/// UART 连接：raw termios 串口 fd 的所有权包装。
///
/// 帧语义免费：实现 [`Read`] + [`Write`] 后经 std 单向 adapter 进窄缝
/// [`Rr3ByteStream`](crate::transport::bytestream::Rr3ByteStream)，再由 transport 的
/// blanket impl 覆盖 [`Rr3TransportConn`](crate::transport::Rr3TransportConn)，
/// 半帧/坏帧拒收与 uds 同源。
pub struct Rr3UartConn {
    fd: i32,
    /// 排他账本键（open 路径）；`from_fd` 测试注入路径不记账，为 None。
    excl_path: Option<String>,
    /// 上次成功写出的时刻（空闲前导判据，见 [`Self::tx_idle_preamble`]）。
    /// `None` = 历史未知（开口后首突发），同口径加前导。
    last_tx: Option<std::time::Instant>,
}

/// 空闲多久后的宿主→板突发需要前导字节（判据实验与机理见
/// [`Rr3UartConn::tx_idle_preamble`]）。
const TX_PREAMBLE_IDLE: std::time::Duration = std::time::Duration::from_millis(20);
/// 前导字节与正式字节之间的间隔：给板面留出消费前导（空行）并回到窥视点的
/// 时间。1.5M 下一字节 6.7µs，2ms 足够板面走完 `read_line` 空行分支。
const TX_PREAMBLE_GAP: std::time::Duration = std::time::Duration::from_millis(2);

impl Rr3UartConn {
    /// 打开串口设备并配 raw 模式（默认 115200）。开口即排他登记（见模块注记
    /// "排他开口"）：同进程重入放行（桥 rx/tx 双开形状），跨进程第二个 opener
    /// 当场被拒。
    pub fn open(path: &Path) -> std::io::Result<Self> {
        Self::open_with_baud(path, RR3_UART_BAUD_DEFAULT)
    }

    /// 打开串口设备并配 raw 模式，波特率显式给定（白名单见 [`rr3_uart_baud_speed`]）。
    pub fn open_with_baud(path: &Path, baud: u32) -> std::io::Result<Self> {
        let speed = rr3_uart_baud_speed(baud)?;
        // 排他登记先行：guard 拿不到就不开数据 fd，报错点名叫人（fail-closed）。
        let key = path.to_string_lossy().into_owned();
        excl_enter(&key)?;
        use std::os::unix::ffi::OsStrExt;
        let mut cpath = path.as_os_str().as_bytes().to_vec();
        cpath.push(0);
        // SAFETY：cpath 以 NUL 结尾，open 只读它；两参形状与 open(2) 变参声明一致。
        // O_NONBLOCK：open(2) 自身不得在 carrier 上悬住——隔离型 USB-TTL 适配器
        // 的 DCD 经隔离器悬空恒"无载波"，阻塞 open 永挂（2026-10-24 隔离棒现场
        // 实锤；guard fd 同理由见 excl_enter）。O_CLOEXEC 同 excl_enter guard：
        // 通信 fd 不随 spawn 泄漏进子进程（description 副本钉住 termios 配置与
        // 队列状态，同批N21 件一根因面）。非阻塞位在 configure 成功后由
        // [`rr3_uart_clear_nb`] 撤除，读写语义与旧阻塞版逐点等价。
        let fd = unsafe {
            c_open(
                cpath.as_ptr() as *const CChar,
                O_RDWR | O_NOCTTY | O_NONBLOCK | O_CLOEXEC,
            )
        };
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            excl_exit(&key);
            return Err(e);
        }
        // 撤 O_NONBLOCK：Read/Write 是阻塞语义（VMIN=1 等 ≥1 字节），收尾失败则归还 fd。
        if let Err(e) = configure(fd, speed).and_then(|()| rr3_uart_clear_nb(fd)) {
            // SAFETY：fd 是刚 open 成功的合法描述符，配置失败则归还。
            unsafe { c_close(fd) };
            excl_exit(&key);
            return Err(e);
        }
        Ok(Self {
            fd,
            excl_path: Some(key),
            last_tx: None,
        })
    }

    /// 从已打开的 fd 建连接并配 raw 模式（单测经 pty 注入用；**取得 fd 所有权**）。
    /// 不参与排他账本（fd 非本模块 open 所出，guard 无从挂靠）。
    #[cfg(test)]
    pub(crate) fn from_fd(fd: i32, baud: u32) -> std::io::Result<Self> {
        let speed = rr3_uart_baud_speed(baud)?;
        configure(fd, speed)?;
        Ok(Self {
            fd,
            excl_path: None,
            // 测试注入路径（pty）：显式置"刚写过"——单测按精确字节对拍，前导会
            // 打乱；前导行为另有专测（tx_idle_preamble 用例）。
            last_tx: Some(std::time::Instant::now()),
        })
    }

    /// 冲掉**当前**输入队列（TCIFLUSH 只冲收不冲发）。开口即净只覆盖开口时刻
    /// 之前的积压——开口本身触发的复位（CP2102 DTR/RTS 翻转 → ESP32 EN）会让
    /// 板端开机 banner 在开口 flush **之后**才陆续进队，宿主发首帧前须再冲一次
    /// （批C 真板 e2e 实证：1.2s settle 后 banner 残留致首读 BadMagic）。
    pub fn rr3_flush_input(&self) -> std::io::Result<()> {
        // SAFETY：fd 属 self，是 open/from_fd 配置过的合法描述符。
        if unsafe { c_tcflush(self.fd, TCIFLUSH) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// 超时读（机制面，0.0.5 批D④ d4 600s 实锤新增）：`poll(2)` 等就绪至
    /// `timeout_ms`，就绪即 `read(2)` 一次。`Ok(0)` = 超时**无数据**——"此刻
    /// 无数据"语义，不是 EOF。缺省 [`Read::read`] 的 VMIN=1/VTIME=0 纯阻塞
    /// 语义保持不变（桥/哨兵等其余用户的正当语义）；本方法只给"必须能醒来"
    /// 的收流端一个出口——d4 实锤：驱动层丢字节后帧层等幻影帧尾纯阻塞永不
    /// 返回（幻影帧头 magic 线上字节 "1ERA" 全落在 b64 字母表内，b64 载荷里
    /// 撞上即合法幻影帧头）。EINTR 折叠为 `Ok(0)`（此轮无数据，调用方重试自便）。
    pub fn rr3_read_timeout(&mut self, buf: &mut [u8], timeout_ms: i32) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut pfd = PollFd {
            fd: self.fd,
            events: POLLIN,
            revents: 0,
        };
        // SAFETY：pfd 活到调用返回，fd 属 self。
        let r = unsafe { c_poll(&mut pfd, 1, timeout_ms) };
        if r < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                return Ok(0);
            }
            return Err(e);
        }
        if r == 0 {
            return Ok(0); // 超时：此刻无数据（非 EOF）
        }
        loop {
            // SAFETY：buf 活到调用返回，fd 属 self。就绪后读一次，部分读如实上交。
            let n = unsafe {
                c_read(
                    self.fd,
                    buf.as_mut_ptr() as *mut std::ffi::c_void,
                    buf.len(),
                )
            };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            return Ok(n as usize);
        }
    }
}

impl Rr3UartConn {
    /// 读侧超时档（十分之一秒粒度；`0` = 恢复缺省纯阻塞 VMIN=1/VTIME=0）。
    ///
    /// **为什么需要**：宿主对板侧的 `rr3.method` 是"发一帧等一帧"的同步往返，
    /// 缺省阻塞读在板侧死机/复位/线断时**永不返回**——调用线程（rrd 的 HAL 面、
    /// 面板 RPC）整条挂死，连接也无法报废重开。置 VMIN=0/VTIME=n 后，静默超过
    /// n×100ms 的读回 `Ok(0)`，帧层按 Closed 上报，调用方据此报废连接（下次
    /// 调用重开并 `rr3_flush_input` 净化残字节）。
    ///
    /// 语义边界：VTIME 是**字节间**静默窗口，不是整帧总时长——帧内字节间隔在
    /// 1.5M 下 ~6.7µs，正常链路远小于任一台法档位；只有"根本没人发"才会触发。
    /// 对桥/哨兵等本来就该阻塞等的收流端不适用，故只作显式调用。
    pub fn rr3_set_read_timeout(&self, ds: u8) -> std::io::Result<()> {
        let mut t = rr3_termios_of(self.fd)?;
        rr3_vtime(&mut t, ds);
        // SAFETY：合法指针；TCSANOW 立即生效。
        if unsafe { c_tcsetattr(self.fd, TCSANOW, &t) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Read for Rr3UartConn {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            // SAFETY：buf 活到调用返回，fd 属 self。
            let n = unsafe {
                c_read(
                    self.fd,
                    buf.as_mut_ptr() as *mut std::ffi::c_void,
                    buf.len(),
                )
            };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue; // EINTR 重试语义与 read_fill 的 Interrupted 分支同源
                }
                return Err(e);
            }
            return Ok(n as usize);
        }
    }
}

impl Rr3UartConn {
    /// 空闲后首突发的**前导字节**（宿主→板方向；2026-10-30 帧面真机首验的根因处置）。
    ///
    /// **现象**：同一物理链路上，空闲后的宿主→板突发其**首字节**偶发失步（误码只坏
    /// 一个字节）——行面命令表现为 `ERR unknown cmd "NAN"` 一类首字符坏（混跑实测
    /// 0.067–0.083%），帧面则整帧被降级成行（`LINE_CAP`=128 < 帧长，板面回
    /// `ERR line too long`，请求**无痕消失**；frame_probe 实测 10 条中 2–3 条）。
    ///
    /// **判定实验**（frame_probe）：每帧前插 1 个 `\n` + 10ms → 三轮 **30/30**；
    /// 只插 200ms 帧间隔 → 仍 2–3 条失败（排除时序竞态）。前导替首字节挡枪：
    /// 板面对孤立 `\n` 是空行静默跳过；失步（若发生）只落在前导上，正式字节在
    /// 已同步的线上出发。
    ///
    /// **机理（假设，电气层）**：空闲后首突发的起始位边沿在线上振铃/反射下被
    /// 误判，采样错位只影响该字节。物理层根治选项 = TX 线串 100–330Ω 阻尼。
    ///
    /// **安全性**：前导只在空闲 > [`TX_PREAMBLE_IDLE`] 时插入并隔 [`TX_PREAMBLE_GAP`]
    /// 再发正式字节；同一 `write_all` 循环内的后续 `write` 距前次写出仅 µs 级，
    /// 不会再触发。桥的转发块若真被拆成两半（对端一次 write 全部送达，实际
    /// µs 级跟到），也因间隔远小于阈值而不插。
    fn tx_idle_preamble(&mut self) {
        let idle = self.last_tx.is_none_or(|t| t.elapsed() >= TX_PREAMBLE_IDLE);
        if !idle {
            return;
        }
        self.rr3_preamble_byte();
    }

    /// 写出前导 `\n` 并隔挡：写成功才记时刻（失败不拦路，正式字节自会报错）。
    fn rr3_preamble_byte(&mut self) {
        let one = [0x0Au8];
        // SAFETY：one 活到调用返回，fd 属 self。
        let n = unsafe { c_write(self.fd, one.as_ptr() as *const std::ffi::c_void, 1) };
        if n == 1 {
            std::thread::sleep(TX_PREAMBLE_GAP);
            self.last_tx = Some(std::time::Instant::now());
        }
        // 前导写失败不拦路：正式字节的写出路径自会报错并交调用方处置。
    }
}

impl Write for Rr3UartConn {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // 空闲前导（见 tx_idle_preamble）：本方法宿主→板全部写路径的单点——
        // rr3_send_buf（帧）经 std adapter 的 write 循环到达这里，桥/收流端的
        // write_all（行）经默认实现同样到达。
        self.tx_idle_preamble();
        // SAFETY：buf 活到调用返回，fd 属 self。部分写交给 write_all 循环处理。
        let n = unsafe { c_write(self.fd, buf.as_ptr() as *const std::ffi::c_void, buf.len()) };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        self.last_tx = Some(std::time::Instant::now());
        Ok(n as usize)
    }

    /// tty 无用户态缓冲，write_all 落内核即视为提交（tcdrain 级排空不做——
    /// 帧层 flush 语义要的是"字节已交付内核"，不是"线路已发完"）。
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Rr3UartConn {
    fn drop(&mut self) {
        // SAFETY：fd 是 open/from_fd 取得的合法描述符，close 一次即归还。
        unsafe { c_close(self.fd) };
        // 参与排他账本的（open 路径出身的）退账；refs 归零即放锁。
        if let Some(p) = &self.excl_path {
            excl_exit(p);
        }
    }
}
