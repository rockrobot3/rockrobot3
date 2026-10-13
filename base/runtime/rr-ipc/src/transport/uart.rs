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
//! tcsetattr/cfmakeraw/cfsetispeed/cfsetospeed/tcflush/ioctl），**不引 libc crate**——
//! shm 裁定 5 同款仓内纪律，rr-ipc 依赖表保持为空。termios 结构与常量 Linux/macOS 分
//! `cfg` 各钉一份（数值互不相同：Linux B115200 = `0o10002`，macOS 直接用字面
//! 波特率；O_NOCTTY、VMIN/VTIME 下标、结构布局均异）——每一份都以各自平台
//! 头文件为准，并用 `const _` 断言结构尺寸，编译期先拦 ABI 漂移。
//!
//! 排他开口（0.0.7 批G，批E E7 排障实锤处置）：**按 (进程, 路径) 的 flock 登记
//! 账本**。每路径在本进程内的首个 open 开一支长寿命 guard fd（O_NONBLOCK 防
//! carrier 阻塞）并对它 `flock(LOCK_EX|LOCK_NB)`；同进程后续 open 只记账重入
//! （桥 rx/tx 对同一节点双开是仓内合法形状，`open_uart_pair`）；账退到零关
//! guard 放锁。跨进程的第二个 opener 在 guard 的 flock 上当场被拒（EWOULDBLOCK
//! → ResourceBusy 点名）——E7 的雷（macOS 允许重复 open 同一 USB 串口节点，
//! 残留旧桥/旧 harness 静默截走数据）正是跨进程形状，拦住即结案。
//!
//! 为什么不用 TIOCEXCL：Linux 内核对同节点**一切**后续 open 硬拦（不分进程），
//! 会把桥自家 rx/tx 双开一起拦死；macOS（xnu）的 TS_XCLUDE 只拦"tty 已挂
//! controlling session 且 opener 非其 leader"，本模块 O_NOCTTY 开口无 session，
//! pty 探针实测（2026-10-11）TIOCEXCL 后二次 open 仍成功——两头都不成立，弃用。
//! flock 是 advisory，但仓内全部串口消费者（桥/harness/运维脚本）都经
//! [`Rr3UartConn::open`] 这一个咽喉，同一把锁即对全体自家人硬约束；外部进程
//! （espflash/monitor）不守 flock，与旧状同，须按 runbook 先清场再操作。
//! 精度注记：账本按**路径字符串**记账，同一节点的别名路径（symlink）不记账
//! ——本仓所有消费方都用同一固定路径写法，别名逃逸留 runbook 纪律兜底。
//!
//! 波特率只收 [`rr3_uart_baud_speed`] 认识的三档（115200 默认 / 9600 降速 /
//! 921600 高速）：9600 是 0.0.7 方案 P3 的合法降速处置；921600 是 0.0.5 增补轮五
//! 批D② 按带宽账加的高速档（音频上行双通道 16kHz×16bit ≈ 512kbps，115200 载
//! 不动、921600 有效 ≈ 737kbps 可承载）。缺省 115200 不变（Q1=(a) 裁定），高速
//! 档只显式选择——不预设更大的波特率动物园。

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
    /// macOS 字面波特率（SDK sys/termios.h：`#define B921600 921600`，批D② 高速档）
    pub const B921600: SpeedT = 921600;
    /// SDK sys/termios.h：`#define NCCS 20`、`#define VMIN 16`、`#define VTIME 17`
    pub const NCCS: usize = 20;
    pub const VMIN: usize = 16;
    pub const VTIME: usize = 17;
    /// SDK sys/termios.h：`#define TCIFLUSH 1`（BSD 与 Linux 数值不同，不得混用）
    pub const TCIFLUSH: i32 = 1;
    /// SDK sys/fcntl.h：`#define O_NONBLOCK 0x0004`（与 Linux 数值不同，不得混用）
    pub const O_NONBLOCK: i32 = 0x0004;
    /// SDK sys/signal.h：`struct pollfd`（int + short + short = 8B，无填充）
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PollFd {
        pub fd: i32,
        pub events: i16,
        pub revents: i16,
    }
    const _: () = assert!(size_of::<PollFd>() == 8, "macOS struct pollfd ABI 漂移");
    /// SDK sys/signal.h：`#define POLLIN 0x0001`
    pub const POLLIN: i16 = 0x0001;
    /// macOS：`typedef unsigned int nfds_t;`（与 Linux u64 不同，经 c_poll 对表）
    pub type NfdsT = u32;

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
    /// Linux uapi asm-generic/termbits.h：`#define B921600 0010007`（八进制，批D②）
    pub const B921600: SpeedT = 0o10007;
    /// Linux：`#define NCCS 32`、`#define VMIN 6`、`#define VTIME 5`
    pub const NCCS: usize = 32;
    pub const VMIN: usize = 6;
    pub const VTIME: usize = 5;
    /// Linux uapi asm-generic/termbits.h：`#define TCIFLUSH 0`（与 macOS=1 不同值）
    pub const TCIFLUSH: i32 = 0;
    /// Linux uapi asm-generic/fcntl.h：`#define O_NONBLOCK 04000`（八进制，与
    /// macOS 数值不同，不得混用）
    pub const O_NONBLOCK: i32 = 0o4000;
    /// glibc sys/poll.h：`struct pollfd`（int + short + short = 8B，无填充）
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PollFd {
        pub fd: i32,
        pub events: i16,
        pub revents: i16,
    }
    const _: () = assert!(size_of::<PollFd>() == 8, "Linux struct pollfd ABI 漂移");
    /// Linux uapi asm-generic/fcntl.h：`#define POLLIN 0x001`
    pub const POLLIN: i16 = 0x001;
    /// Linux：`typedef unsigned long int nfds_t;`（与 macOS u32 不同，经 c_poll 对表）
    pub type NfdsT = u64;

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

use imp::{NfdsT, PollFd, SpeedT, Termios, B115200, B921600, B9600, O_NOCTTY, O_NONBLOCK, O_RDWR, POLLIN, TCIFLUSH, VMIN, VTIME};

// no_std 显式化（0.0.7 批B 口径）：std prelude 不随 `#![no_std]` 隐式注入
use std::collections::HashMap;
use std::format;
use std::string::String;
use std::sync::{Mutex, OnceLock};

const TCSANOW: i32 = 0;
/// `flock(2)` 操作码（sys/file.h，macOS 与 Linux 同值）：排他锁 + 非阻塞。
const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;

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
    /// `flock(2)`：open file description 级排他锁（排他账本的锁面，见模块注记）。
    #[link_name = "flock"]
    fn c_flock(fd: i32, operation: i32) -> i32;
    #[link_name = "cfmakeraw"]
    fn c_cfmakeraw(termios_p: *mut Termios);
    #[link_name = "cfsetispeed"]
    fn c_cfsetispeed(termios_p: *mut Termios, speed: SpeedT) -> i32;
    #[link_name = "cfsetospeed"]
    fn c_cfsetospeed(termios_p: *mut Termios, speed: SpeedT) -> i32;
    /// `poll(2)`：nfds_t 宽度平台不同（macOS u32 / Linux u64），经 [`NfdsT`] 对表。
    #[link_name = "poll"]
    fn c_poll(fds: *mut PollFd, nfds: NfdsT, timeout: i32) -> i32;
}

/// 波特率 → `speed_t`。只认三档：115200（默认）、9600（P3 合法降速）与 921600
/// （批D② 音频上行高速档）。不在档位上报错——静默降到最近的档位等于把线路参数
/// 交给运气。
pub fn rr3_uart_baud_speed(baud: u32) -> std::io::Result<SpeedT> {
    match baud {
        115200 => Ok(B115200),
        9600 => Ok(B9600),
        921600 => Ok(B921600),
        other => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "不支持的波特率 {other}：只支持 115200（默认）、9600（P3 降速档）与 921600（批D② 高速档）"
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// 排他登记账本（flock 的同进程重入豁免）：键 = 设备路径字符串，值 = guard fd
// + 引用数。桥 rx/tx 同节点双开（open_uart_pair）在同进程内合法重入；跨进程
// 的第二个 opener 撞 guard 的 flock 当场被拒。
// ---------------------------------------------------------------------------

/// 一条路径的排他 guard：fd 长寿命（保住 flock 直到本进程最后一个 conn 退场），
/// refs = 本进程内以该路径 open 的 conn 数。
struct ExclGuard {
    fd: i32,
    refs: u32,
}

static EXCL: OnceLock<Mutex<HashMap<String, ExclGuard>>> = OnceLock::new();

fn excl_map() -> &'static Mutex<HashMap<String, ExclGuard>> {
    EXCL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 登记入账：该路径首次出现则开 guard fd（O_NONBLOCK 防 carrier 阻塞）取
/// flock；取不到即 ResourceBusy 点名（E7 "残留旧进程静默截流" 的现场）。
fn excl_enter(path: &str) -> std::io::Result<()> {
    let mut m = excl_map().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(g) = m.get_mut(path) {
        g.refs += 1;
        return Ok(());
    }
    let mut cpath = path.as_bytes().to_vec();
    cpath.push(0);
    // SAFETY：cpath 以 NUL 结尾，open 只读它；guard fd 不收发数据，O_NONBLOCK
    // 只为 open(2) 自身不在 carrier 上悬住。
    let fd = unsafe {
        c_open(
            cpath.as_ptr() as *const CChar,
            O_RDWR | O_NOCTTY | O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY：合法 fd；LOCK_EX|LOCK_NB 非阻塞取锁，被占即 EWOULDBLOCK。
    if unsafe { c_flock(fd, LOCK_EX | LOCK_NB) } != 0 {
        let e = std::io::Error::last_os_error();
        // SAFETY：合法 fd，锁失败则归还。
        unsafe { c_close(fd) };
        return if e.kind() == std::io::ErrorKind::WouldBlock {
            // EWOULDBLOCK（macOS=35/Linux=11 同 kind）：节点被另一个进程锁着。
            // （ErrorKind::ResourceBusy 1.83 才 stable，MSRV 1.82 用 Other + 文案点名）
            Err(std::io::Error::other(format!(
                "串口已被其他进程占用（flock 排他）：{e}"
            )))
        } else {
            Err(e)
        };
    }
    m.insert(String::from(path), ExclGuard { fd, refs: 1 });
    Ok(())
}

/// 退账：refs 减到零关 guard fd（flock 随最后一次 close 释放），并移除账目。
fn excl_exit(path: &str) {
    let mut m = excl_map().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(g) = m.get_mut(path) {
        g.refs -= 1;
        if g.refs == 0 {
            // SAFETY：guard fd 是 excl_enter 开出的合法描述符，账退尽即归还。
            unsafe { c_close(g.fd) };
            m.remove(path);
        }
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
    /// 排他账本键（open 路径）；`from_fd` 测试注入路径不记账，为 None。
    excl_path: Option<String>,
}

impl Rr3UartConn {
    /// 打开串口设备并配 raw 模式（默认 115200）。开口即排他登记（见模块注记
    /// "排他开口"）：同进程重入放行（桥 rx/tx 双开形状），跨进程第二个 opener
    /// 当场被拒。
    pub fn open(path: &Path) -> std::io::Result<Self> {
        Self::open_with_baud(path, RR3_UART_BAUD_DEFAULT)
    }

    /// 打开串口设备并配 raw 模式，波特率显式给定（只收 115200/9600/921600）。
    pub fn open_with_baud(path: &Path, baud: u32) -> std::io::Result<Self> {
        let speed = rr3_uart_baud_speed(baud)?;
        // 排他登记先行：guard 拿不到就不开数据 fd，报错点名叫人（fail-closed）。
        let key = path.to_string_lossy().into_owned();
        excl_enter(&key)?;
        use std::os::unix::ffi::OsStrExt;
        let mut cpath = path.as_os_str().as_bytes().to_vec();
        cpath.push(0);
        // SAFETY：cpath 以 NUL 结尾，open 只读它；两参形状与 open(2) 变参声明一致。
        let fd = unsafe { c_open(cpath.as_ptr() as *const CChar, O_RDWR | O_NOCTTY) };
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            excl_exit(&key);
            return Err(e);
        }
        match configure(fd, speed) {
            Ok(()) => Ok(Self {
                fd,
                excl_path: Some(key),
            }),
            Err(e) => {
                // SAFETY：fd 是刚 open 成功的合法描述符，配置失败则归还。
                unsafe { c_close(fd) };
                excl_exit(&key);
                Err(e)
            }
        }
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
                c_read(self.fd, buf.as_mut_ptr() as *mut std::ffi::c_void, buf.len())
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
        // 参与排他账本的（open 路径出身的）退账；refs 归零即放锁。
        if let Some(p) = &self.excl_path {
            excl_exit(p);
        }
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
        let len = name
            .iter()
            .position(|&c| c == 0)
            .expect("openpty name 未 NUL 结尾");
        let path: std::vec::Vec<u8> = name[..len].iter().map(|&c| c as u8).collect();
        let path = String::from_utf8(path).expect("pty 路径非 UTF-8");
        // SAFETY：master/slave 都是 openpty 刚出的合法 fd，各自 File 接管并负责 close。
        unsafe {
            (
                std::fs::File::from_raw_fd(master),
                std::fs::File::from_raw_fd(slave),
                std::path::PathBuf::from(path),
            )
        }
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

    /// 超时读（机制面新增护栏，d4 600s 实锤）：静默必须 `Ok(0)` 且**按时醒来**
    /// （不死锁不空转）；来数据必须读到，且超时档读过的字节与原帧逐字节一致。
    #[test]
    fn uart_read_timeout_idle_zero_then_data() {
        let (mut master, mut conn) = pty_pair(RR3_UART_BAUD_DEFAULT);
        let mut buf = [0u8; 64];
        let t0 = std::time::Instant::now();
        assert_eq!(
            conn.rr3_read_timeout(&mut buf, 80).unwrap(),
            0,
            "静默必须 Ok(0)（此刻无数据语义）"
        );
        assert!(
            t0.elapsed() >= std::time::Duration::from_millis(70),
            "必须等满超时才返回（未按时醒来 = 语义漂移）"
        );
        use std::io::Write as _;
        let f = Rr3Frame {
            header: hdr(3),
            payload: b"t/o".to_vec(),
        };
        let bytes = rr3_frame_to_bytes(&f).unwrap();
        master.write_all(&bytes).unwrap();
        // 就绪读可能是部分读，循环收满整帧
        let mut got = std::vec::Vec::new();
        while got.len() < bytes.len() {
            let n = conn
                .rr3_read_timeout(&mut buf, 2000)
                .expect("有数据必须能读");
            assert!(n > 0, "有数据不得 Ok(0)");
            got.extend_from_slice(&buf[..n]);
        }
        assert_eq!(
            got, bytes,
            "超时档读出的字节必须与原帧逐字节一致"
        );
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

    /// 波特率档位表：三档内放行，档位外拒绝（静默换档是线路事故）。
    #[test]
    fn uart_baud_speed_table() {
        assert!(rr3_uart_baud_speed(115200).is_ok());
        assert!(rr3_uart_baud_speed(9600).is_ok());
        assert!(rr3_uart_baud_speed(921600).is_ok());
        assert!(rr3_uart_baud_speed(0).is_err());
        assert!(rr3_uart_baud_speed(230400).is_err(), "档位动物园不得静默扩张");
    }

    /// 排他开口·同进程重入（批G G0 守门用例）：同一节点同进程两次 open 都放行
    /// ——桥 rx/tx 双开（open_uart_pair）是仓内合法形状，flock 账本只对跨进程
    /// 生效。走真实 open(path) 入口。
    #[test]
    fn uart_open_same_process_pair_reentrant() {
        let (_master, _slave_keepalive, slave_path) = pty_pair_with_path();
        let _rx = Rr3UartConn::open(&slave_path).expect("首次 open 必须成功");
        let _tx = Rr3UartConn::open(&slave_path).expect("同进程双开（桥 rx/tx 形状）必须放行");
        // 双双 drop 退账到零，随后第三次 open 仍须成功（账目可复用，无泄漏）。
        drop(_rx);
        drop(_tx);
        let _again = Rr3UartConn::open(&slave_path).expect("账退尽后重开必须成功");
    }

    /// 排他开口·跨进程被拒（批G G0 守门用例，批E E7 实锤）：另一进程持锁时本
    /// 进程 open 必须以文案点名拒绝——E7 "残留旧桥静默截流" 的现场形状。
    /// 子进程探针：复用本测试二进制，holder 模式下按父进程传入的 pty 路径 open，
    /// 就绪文件握手（harness 自身的 stdout 输出不可作握手信道），父进程关 stdin
    /// 即收兵。
    #[test]
    fn uart_excl_cross_process_second_process_rejected() {
        let (_master, _slave_keepalive, slave_path) = pty_pair_with_path();
        if std::env::var("RR3_UART_EXCL_PROBE").as_deref() == Ok("holder") {
            let path = std::env::var("RR3_UART_EXCL_PATH").expect("holder 必须拿到 pty 路径");
            let conn =
                Rr3UartConn::open(std::path::Path::new(&path)).expect("holder open 必须成功");
            let ready = std::env::var("RR3_UART_EXCL_READY").expect("holder 必须拿到就绪文件路径");
            std::fs::write(&ready, b"R").expect("holder 写就绪文件失败");
            let mut b = [0u8; 1];
            let _ = std::io::stdin().read(&mut b); // 父进程关 stdin 即收兵
            drop(conn);
            return;
        }
        let ready = std::env::temp_dir().join(format!("rr3-excl-probe-{}.r", std::process::id()));
        let _ = std::fs::remove_file(&ready);
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "transport::uart::tests::uart_excl_cross_process_second_process_rejected",
                "--exact",
                "--nocapture",
            ])
            .env("RR3_UART_EXCL_PROBE", "holder")
            .env("RR3_UART_EXCL_PATH", &slave_path)
            .env("RR3_UART_EXCL_READY", &ready)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn 自身测试二进制失败");
        // 等就绪文件（holder 持锁成功的信号），10s 上限。
        let mut ready_seen = false;
        for _ in 0..200 {
            if ready.exists() {
                ready_seen = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if matches!(child.try_wait(), Ok(Some(_))) || !ready_seen {
            let _ = std::fs::remove_file(&ready);
            panic!("holder 子进程 10s 内未就绪（pty 路径：{slave_path:?}）");
        }
        match Rr3UartConn::open(&slave_path) {
            Ok(_) => {
                drop(child.stdin.take());
                let _ = child.wait();
                let _ = std::fs::remove_file(&ready);
                panic!("跨进程二次 open 必须被拒（flock 排他）");
            }
            Err(err) => {
                let msg = format!("{err}");
                assert_eq!(
                    err.kind(),
                    std::io::ErrorKind::Other,
                    "跨进程被拒必须 Other+文案点名（实得 {msg}）"
                );
                assert!(
                    msg.contains("flock 排他"),
                    "错误文案必须点名 flock 排他：{msg}"
                );
            }
        }
        drop(child.stdin.take()); // 关 stdin 收兵
        let _ = child.wait();
        let _ = std::fs::remove_file(&ready);
    }
}
