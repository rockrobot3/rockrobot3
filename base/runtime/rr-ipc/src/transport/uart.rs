//! UART 传输（0.0.7 批C，ADR-0008 第三实现位；Q1 裁定：线上帧格式不变）。
//!
//! 点对点字节流：宿主侧按 raw termios 打开串口设备（`/dev/ttyUSB*` /
//! `/dev/tty.usbserial*`），帧语义由 blanket impl
//! [`Rr3TransportConn`](crate::transport::Rr3TransportConn) 免费覆盖——批J G1 换底后
//! 下底是窄缝 [`Rr3ByteStream`](crate::transport::bytestream::Rr3ByteStream)（本模块
//! 的 Read+Write 实现经 std 单向 adapter 进缝），半帧/坏 magic/长度不符的拒收路径
//! 与 uds 完全同源，不写第二份帧层。
//!
//! 板侧（0.0.7 层②）另写：宿主侧这个实现是"开 `/dev/ttyUSB` 的"不是板侧实现；
//! 板侧接总线只需直接实现两个 `usize` 方法（形状用例见 `transport/mod.rs` tests）。
//!
//! 接缝纪律（0.0.7 方案 §5）：只落 conn 面，**不改 trait 签名、不新增关联类型**。
//! UART 无"监听/accept"语义（点对点，无连接洪泛），[`Rr3TransportEndpoint`] 与
//! [`Rr3TransportListener`](crate::transport::Rr3TransportListener) 两 trait 不适用
//! ——uds 的 endpoint trait 面是"端点可洪泛"形状的产物，硬套点对点等于撒谎。
//!
//! 系统调用面：手写 `extern "C"` 声明壳（open/close/read/write/tcgetattr/
//! tcsetattr/cfmakeraw/cfsetispeed/cfsetospeed/tcflush），**不引 libc crate**——
//! shm 裁定 5 同款仓内纪律，rr-ipc 依赖表保持为空。termios 结构与常量 Linux/macOS 分
//! `cfg` 各钉一份（数值互不相同：Linux B115200 = `0o10002`，macOS 直接用字面
//! 波特率；O_NOCTTY、VMIN/VTIME 下标、结构布局均异）——每一份都以各自平台
//! 头文件为准，并用 `const _` 断言结构尺寸，编译期先拦 ABI 漂移。
//!
//! 波特率只收 [`rr3_uart_baud_speed`] 认识的两档（115200 默认 / 9600）：9600 是
//! 0.0.7 方案 P3 的合法降速处置，不预设更大的波特率动物园。

use std::io::{Read, Write};
use std::path::Path;

/// 默认波特率（0.0.7 方案 §9.2：115200 起步）
pub const RR3_UART_BAUD_DEFAULT: u32 = 115200;

// ---------------------------------------------------------------------------
// 平台差异面：Linux（含 aarch64/musl，uapi asm-generic/termbits.h）与 macOS
// （BSD termios，SDK sys/termios.h 实测核对 2026-10-07）。两份定义都不得混用。
// ---------------------------------------------------------------------------

/// C `char`：Linux aarch64 上是无符号（char 无符号平台），x86_64-linux/macOS 有符号。
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
type CChar = u8;
#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
type CChar = i8;

#[cfg(target_os = "macos")]
mod imp {
    /// macOS：`speed_t`/`tcflag_t` = `unsigned long`（u64）
    pub type SpeedT = u64;
    pub const O_RDWR: i32 = 0x2;
    /// macOS SDK sys/fcntl.h：`#define O_NOCTTY 0x00020000`
    pub const O_NOCTTY: i32 = 0x0002_0000;
    /// macOS 直接用字面波特率当 speed 常量（SDK sys/termios.h：`#define B115200 115200`）
    pub const B115200: SpeedT = 115200;
    pub const B9600: SpeedT = 9600;
    /// SDK sys/termios.h：`#define NCCS 20`、`#define VMIN 16`、`#define VTIME 17`
    pub const NCCS: usize = 20;
    pub const VMIN: usize = 16;
    pub const VTIME: usize = 17;
    /// SDK sys/termios.h：`#define TCIFLUSH 1`（BSD 与 Linux 数值不同，不得混用）
    pub const TCIFLUSH: i32 = 1;

    /// BSD `struct termios`：4×u64 flags + c_cc[20] + 2×u64 speed。
    /// 尺寸 72 = 32 + 20 + (4 对齐填充) + 16；`const _` 断言在编译期钉死。
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Termios {
        pub c_iflag: u64,
        pub c_oflag: u64,
        pub c_cflag: u64,
        pub c_lflag: u64,
        pub c_cc: [u8; NCCS],
        pub c_ispeed: u64,
        pub c_ospeed: u64,
    }
    const _: () = assert!(size_of::<Termios>() == 72, "macOS struct termios ABI 漂移");

    impl Termios {
        /// 全零初值（tcgetattr 之前只是接收缓冲，值无意义）
        pub const fn zeroed() -> Self {
            Self {
                c_iflag: 0,
                c_oflag: 0,
                c_cflag: 0,
                c_lflag: 0,
                c_cc: [0; NCCS],
                c_ispeed: 0,
                c_ospeed: 0,
            }
        }
    }
}

#[cfg(all(target_os = "linux", not(target_os = "espidf")))]
mod imp {
    /// Linux：`speed_t`/`tcflag_t` = `unsigned int`（u32）
    pub type SpeedT = u32;
    pub const O_RDWR: i32 = 0x2;
    /// Linux uapi fcntl.h：`#define O_NOCTTY 0400`
    pub const O_NOCTTY: i32 = 0o400;
    /// Linux uapi asm-generic/termbits.h：`#define B115200 0010002`（八进制）
    pub const B115200: SpeedT = 0o10002;
    pub const B9600: SpeedT = 0o14;
    /// Linux：`#define NCCS 32`、`#define VMIN 6`、`#define VTIME 5`
    pub const NCCS: usize = 32;
    pub const VMIN: usize = 6;
    pub const VTIME: usize = 5;
    /// Linux uapi asm-generic/termbits.h：`#define TCIFLUSH 0`（与 macOS=1 不同值）
    pub const TCIFLUSH: i32 = 0;

    /// glibc/musl `struct termios`：4×u32 flags + c_line + c_cc[32] + 2×u32 speed。
    /// 尺寸 60 = 16 + 1 + 32 + 8（对齐到 4）；`const _` 断言在编译期钉死。
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Termios {
        pub c_iflag: u32,
        pub c_oflag: u32,
        pub c_cflag: u32,
        pub c_lflag: u32,
        pub c_line: u8,
        pub c_cc: [u8; NCCS],
        pub c_ispeed: u32,
        pub c_ospeed: u32,
    }
    const _: () = assert!(size_of::<Termios>() == 60, "Linux struct termios ABI 漂移");

    impl Termios {
        /// 全零初值（tcgetattr 之前只是接收缓冲，值无意义）
        pub const fn zeroed() -> Self {
            Self {
                c_iflag: 0,
                c_oflag: 0,
                c_cflag: 0,
                c_lflag: 0,
                c_line: 0,
                c_cc: [0; NCCS],
                c_ispeed: 0,
                c_ospeed: 0,
            }
        }
    }
}

use imp::{SpeedT, Termios, B115200, B9600, O_NOCTTY, O_RDWR, TCIFLUSH, VMIN, VTIME};

// no_std 显式化（0.0.7 批B 口径）：std prelude 不随 `#![no_std]` 隐式注入
use std::format;

const TCSANOW: i32 = 0;

extern "C" {
    /// `open(2)` 是变参：这里只用两参形状（path, oflag）
    #[link_name = "open"]
    fn c_open(path: *const CChar, oflag: i32, ...) -> i32;
    #[link_name = "close"]
    fn c_close(fd: i32) -> i32;
    #[link_name = "read"]
    fn c_read(fd: i32, buf: *mut std::ffi::c_void, count: usize) -> isize;
    #[link_name = "write"]
    fn c_write(fd: i32, buf: *const std::ffi::c_void, count: usize) -> isize;
    #[link_name = "tcgetattr"]
    fn c_tcgetattr(fd: i32, termios_p: *mut Termios) -> i32;
    #[link_name = "tcsetattr"]
    fn c_tcsetattr(fd: i32, optional_actions: i32, termios_p: *const Termios) -> i32;
    #[link_name = "tcflush"]
    fn c_tcflush(fd: i32, queue_selector: i32) -> i32;
    #[link_name = "cfmakeraw"]
    fn c_cfmakeraw(termios_p: *mut Termios);
    #[link_name = "cfsetispeed"]
    fn c_cfsetispeed(termios_p: *mut Termios, speed: SpeedT) -> i32;
    #[link_name = "cfsetospeed"]
    fn c_cfsetospeed(termios_p: *mut Termios, speed: SpeedT) -> i32;
}

/// 波特率 → `speed_t`。只认两档：115200（默认）与 9600（P3 合法降速）。
/// 不在档位上报错——静默降到最近的档位等于把线路参数交给运气。
pub fn rr3_uart_baud_speed(baud: u32) -> std::io::Result<SpeedT> {
    match baud {
        115200 => Ok(B115200),
        9600 => Ok(B9600),
        other => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("不支持的波特率 {other}：只支持 115200（默认）与 9600（P3 降速档）"),
        )),
    }
}

/// UART 连接：raw termios 串口 fd 的所有权包装。
///
/// 帧语义免费：实现 [`Read`] + [`Write`] 后经 std 单向 adapter 进窄缝
/// [`Rr3ByteStream`](crate::transport::bytestream::Rr3ByteStream)，再由 transport 的
/// blanket impl 覆盖 [`Rr3TransportConn`](crate::transport::Rr3TransportConn)，
/// 半帧/坏帧拒收与 uds 同源。
pub struct Rr3UartConn {
    fd: i32,
}

impl Rr3UartConn {
    /// 打开串口设备并配 raw 模式（默认 115200）。
    pub fn open(path: &Path) -> std::io::Result<Self> {
        Self::open_with_baud(path, RR3_UART_BAUD_DEFAULT)
    }

    /// 打开串口设备并配 raw 模式，波特率显式给定（只收 115200/9600）。
    pub fn open_with_baud(path: &Path, baud: u32) -> std::io::Result<Self> {
        let speed = rr3_uart_baud_speed(baud)?;
        use std::os::unix::ffi::OsStrExt;
        let mut cpath = path.as_os_str().as_bytes().to_vec();
        cpath.push(0);
        // SAFETY：cpath 以 NUL 结尾，open 只读它；两参形状与 open(2) 变参声明一致。
        let fd = unsafe { c_open(cpath.as_ptr() as *const CChar, O_RDWR | O_NOCTTY) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        match configure(fd, speed) {
            Ok(()) => Ok(Self { fd }),
            Err(e) => {
                // SAFETY：fd 是刚 open 成功的合法描述符，配置失败则归还。
                unsafe { c_close(fd) };
                Err(e)
            }
        }
    }

    /// 从已打开的 fd 建连接并配 raw 模式（单测经 pty 注入用；**取得 fd 所有权**）。
    #[cfg(test)]
    pub(crate) fn from_fd(fd: i32, baud: u32) -> std::io::Result<Self> {
        let speed = rr3_uart_baud_speed(baud)?;
        configure(fd, speed)?;
        Ok(Self { fd })
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
fn configure(fd: i32, speed: SpeedT) -> std::io::Result<()> {
    let mut t = Termios::zeroed();
    // SAFETY：t 是本栈上的合法 termios，tcgetattr 只写它。
    if unsafe { c_tcgetattr(fd, &mut t) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY：合法指针；cfmakeraw 只改 termios 位面（清回显/规范化/流控）。
    unsafe { c_cfmakeraw(&mut t) };
    t.c_cc[VMIN] = 1;
    t.c_cc[VTIME] = 0;
    // SAFETY：合法指针。
    if unsafe { c_cfsetispeed(&mut t, speed) } != 0 || unsafe { c_cfsetospeed(&mut t, speed) } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY：合法指针；TCSANOW 立即生效。
    if unsafe { c_tcsetattr(fd, TCSANOW, &t) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY：合法指针；冲掉开口前积压的输入队列（TCIFLUSH 只冲收不冲发）。
    if unsafe { c_tcflush(fd, TCIFLUSH) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
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

impl Write for Rr3UartConn {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // SAFETY：buf 活到调用返回，fd 属 self。部分写交给 write_all 循环处理。
        let n = unsafe { c_write(self.fd, buf.as_ptr() as *const std::ffi::c_void, buf.len()) };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // no_std 显式化（0.0.7 批B 口径）：std prelude 不随 `#![no_std]` 隐式注入
    use crate::protocol::header::{opcode, Rr3HdrError, Rr3Header};
    use crate::transport::buf::Rr3FrameBuf;
    use crate::transport::frame::{rr3_frame_to_bytes, Rr3Frame, Rr3FrameError};
    use crate::transport::Rr3TransportConn;
    use std::os::unix::io::FromRawFd;
    // no_std 显式化：String 不在 no_std prelude 里，测试面按 std 路径显式引入
    use std::string::String;
    use std::vec;

    extern "C" {
        /// pty 对（测试伪串口）。glibc ≥2.34 并入 libc，musl/macOS 原生就有。
        fn openpty(
            amaster: *mut i32,
            aslave: *mut i32,
            name: *mut CChar,
            termp: *const Termios,
            winp: *const std::ffi::c_void,
        ) -> i32;
    }

    fn hdr(payload_len: u32) -> Rr3Header {
        let mut h = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload_len);
        h.trace_id = 0x1234_5678;
        h.timestamp_ns = 100;
        h.src_task = 7;
        h
    }

    /// 伪串口对：master 是裸 fd（测试经 File 写入），slave 走与真设备同一条
    /// termios 配置路径（from_fd → cfmakeraw/VMIN/speed/tcsetattr）。
    fn pty_pair(baud: u32) -> (std::fs::File, Rr3UartConn) {
        let mut master = 0i32;
        let mut slave = 0i32;
        // SAFETY：两个出参都是本栈 i32，其余按 NULL 传。
        let rc = unsafe {
            openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert_eq!(rc, 0, "openpty 失败：{}", std::io::Error::last_os_error());
        let conn = Rr3UartConn::from_fd(slave, baud).expect("slave termios 配置失败");
        // SAFETY：master 是 openpty 刚出的合法 fd，File 接管并负责 close。
        (unsafe { std::fs::File::from_raw_fd(master) }, conn)
    }

    /// 伪串口对（带 slave 设备路径）：openpty 的 `name` 出参给出 slave 路径
    /// （macOS /dev/ttysNNN、Linux /dev/pts/N），供"按路径 open"的用例走真实
    /// `open()` 入口（含 configure 全链：termios + 开口净化）。返回的 slave File
    /// 由调用方持有保活（不参与收发，仅保证 slave 端在线——openpty 的出参 fd
    /// 不归还就会丢，测试进程短命，随进程回收）。
    #[cfg(test)]
    fn pty_pair_with_path() -> (std::fs::File, std::fs::File, std::path::PathBuf) {
        let mut master = 0i32;
        let mut slave = 0i32;
        let mut name = [0 as CChar; 128];
        // SAFETY：三个出参都是本栈合法内存，其余按 NULL 传。
        let rc = unsafe {
            openpty(
                &mut master,
                &mut slave,
                name.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert_eq!(rc, 0, "openpty 失败：{}", std::io::Error::last_os_error());
        let len = name.iter().position(|&c| c == 0).expect("openpty name 未 NUL 结尾");
        let path: std::vec::Vec<u8> = name[..len].iter().map(|&c| c as u8).collect();
        let path = String::from_utf8(path).expect("pty 路径非 UTF-8");
        // SAFETY：master/slave 都是 openpty 刚出的合法 fd，各自 File 接管并负责 close。
        unsafe { (std::fs::File::from_raw_fd(master), std::fs::File::from_raw_fd(slave), std::path::PathBuf::from(path)) }
    }

    /// 半帧用例需要"读超时"而不是阻塞：把读侧切到 VMIN=0/VTIME=1（100ms 静默即返回 0）。
    fn set_read_timeout(conn: &Rr3UartConn) {
        let mut t = Termios::zeroed();
        // SAFETY：合法栈上 termios。
        assert_eq!(unsafe { c_tcgetattr(conn.fd, &mut t) }, 0);
        t.c_cc[VMIN] = 0;
        t.c_cc[VTIME] = 1;
        // SAFETY：合法指针。
        assert_eq!(unsafe { c_tcsetattr(conn.fd, TCSANOW, &t) }, 0);
    }

    /// 正常帧：整帧写入 → 整帧读回（阻塞路径）；反向发送同一条 blanket impl 面验证。
    #[test]
    fn uart_round_trip_normal_frame() {
        let (mut master, mut conn) = pty_pair(RR3_UART_BAUD_DEFAULT);
        let f = Rr3Frame {
            header: hdr(5),
            payload: b"hello"[..].to_vec(),
        };
        let bytes = rr3_frame_to_bytes(&f).unwrap();
        use std::io::Write as _;
        master.write_all(&bytes).unwrap();

        let got = conn.rr3_recv_buf().expect("正常帧必须整帧读回");
        assert_eq!(
            Rr3Frame::try_from(got).unwrap(),
            f,
            "PTY 往返逐字节一致（raw 模式不得吃改写任何字节）"
        );

        // 反向：conn 发送（blanket impl → write_all + flush）→ master 读回
        let back = Rr3Frame {
            header: hdr(3),
            payload: b"abc"[..].to_vec(),
        };
        conn.rr3_send_buf(Rr3FrameBuf::from(&back)).unwrap();
        let mut echo = vec![0u8; bytes_len(&back)];
        master.read_exact(&mut echo).unwrap();
        assert_eq!(echo, rr3_frame_to_bytes(&back).unwrap());
    }

    fn bytes_len(f: &Rr3Frame) -> usize {
        crate::protocol::header::RR3_IPC_HDR_SIZE + f.payload.len()
    }

    /// 坏 magic：头层拒收路径在真实 fd 上保持同语义（Header(BadMagic)），且
    /// 拒收后连接不报废——后续正常帧照常收发。
    #[test]
    fn uart_rejects_bad_magic_then_resumes() {
        let (mut master, mut conn) = pty_pair(RR3_UART_BAUD_DEFAULT);
        let mut bad = hdr(2).to_bytes().to_vec();
        bad[0..4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        use std::io::Write as _;
        master.write_all(&bad).unwrap();
        assert!(
            matches!(
                conn.rr3_recv_buf(),
                Err(Rr3FrameError::Header(Rr3HdrError::BadMagic { .. }))
            ),
            "坏 magic 必须被头层拒收"
        );

        let f = Rr3Frame {
            header: hdr(1),
            payload: b"z"[..].to_vec(),
        };
        master.write_all(&rr3_frame_to_bytes(&f).unwrap()).unwrap();
        let got = conn.rr3_recv_buf().expect("拒收后必须能重新同步收帧");
        assert_eq!(Rr3Frame::try_from(got).unwrap(), f);
    }

    /// 半帧：只写 64 字节即静默——读侧 VTIME 超时吐 0，read_fill 计数给出
    /// `TornHeader { got: 64 }`（半帧不得当新帧起始）；随后整帧到达仍能整帧读回
    /// （runbook ⑤ 的"注入后不死锁"要求在单测里的等价形状）。
    #[test]
    fn uart_rejects_torn_header_then_resyncs() {
        let (mut master, mut conn) = pty_pair(RR3_UART_BAUD_DEFAULT);
        set_read_timeout(&conn);
        let mut torn = hdr(9).to_bytes().to_vec();
        torn.truncate(64);
        use std::io::Write as _;
        master.write_all(&torn).unwrap();
        assert_eq!(
            conn.rr3_recv_buf(),
            Err(Rr3FrameError::TornHeader { got: 64 }),
            "半帧必须按已读计数拒收"
        );

        // 恢复阻塞语义再收完整帧（与真设备默认路径一致）
        let f = Rr3Frame {
            header: hdr(4),
            payload: b"res4"[..].to_vec(),
        };
        master.write_all(&rr3_frame_to_bytes(&f).unwrap()).unwrap();
        let got = conn.rr3_recv_buf().expect("半帧后新帧必须完整读回");
        assert_eq!(Rr3Frame::try_from(got).unwrap(), f);
    }

    /// 设备不存在 → open 报 IO 错（错误路径不 panic、不静默）。
    #[test]
    fn uart_open_rejects_missing_device() {
        assert!(Rr3UartConn::open(Path::new("/nonexistent/rr3-uart-test")).is_err());
    }

    /// 开口即净（批D1 敞口处置的守门用例）：开口**之前**积压在输入队列的垃圾
    /// （坏 magic 头字节，CP2102 残留同形）必须被 configure 的 tcflush 冲掉——
    /// 随后开口写入的完整帧整帧读回，垃圾不得混入。走真实 open(path) 入口。
    #[test]
    fn uart_open_flushes_stale_input() {
        let (mut master, _slave_keepalive, slave_path) = pty_pair_with_path();
        use std::io::Write as _;
        // 残留字节：一条坏 magic 头 + 尾巴（若不被冲掉，开口首读必 BadMagic）
        let mut stale = hdr(4).to_bytes().to_vec();
        stale[0..4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        stale.extend_from_slice(b"leftover");
        master.write_all(&stale).unwrap();
        // 开口（configure 末尾 tcflush(TCIFLUSH) 冲队列）
        let mut conn = Rr3UartConn::open_with_baud(&slave_path, RR3_UART_BAUD_DEFAULT)
            .expect("按路径 open pty slave 失败");
        // 开口后的新帧必须整帧读回（残留不得当帧前缀混入）
        let f = Rr3Frame {
            header: hdr(5),
            payload: b"fresh"[..].to_vec(),
        };
        master.write_all(&rr3_frame_to_bytes(&f).unwrap()).unwrap();
        let got = conn.rr3_recv_buf().expect("开口后首帧必须完整读回");
        assert_eq!(
            Rr3Frame::try_from(got).unwrap(),
            f,
            "开口前积压字节必须被 tcflush 净化，开口后字节零改写"
        );
    }

    /// 波特率档位表：两档内放行，档位外拒绝（静默换档是线路事故）。
    #[test]
    fn uart_baud_speed_table() {
        assert!(rr3_uart_baud_speed(115200).is_ok());
        assert!(rr3_uart_baud_speed(9600).is_ok());
        assert!(rr3_uart_baud_speed(921600).is_err());
        assert!(rr3_uart_baud_speed(0).is_err());
    }
}
