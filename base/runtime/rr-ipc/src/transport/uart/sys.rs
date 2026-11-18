//! 平台差异面：Linux（含 aarch64/musl，uapi asm-generic/termbits.h）与 macOS
//! （BSD termios，SDK sys/termios.h 实测核对 2026-10-07）。两份定义都不得混用；
//! 结构尺寸用 `const _` 断言在编译期钉死，先于任何运行路径拦 ABI 漂移。
//!
//! 拆分动因：`uart.rs` 528 行，平台常量/结构属纯定义面，与连接策略分属两簇。

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
    /// 更高档（字面值，macOS 无 `B1500000` 宏）：USB 串口驱动按字面值分频，
    /// 上限取决于适配器（CP2102/FT232R 各档实测为准，不是编译期保证）。
    pub const B1500000: SpeedT = 1_500_000;
    pub const B2000000: SpeedT = 2_000_000;
    /// SDK sys/termios.h：`#define NCCS 20`、`#define VMIN 16`、`#define VTIME 17`
    pub const NCCS: usize = 20;
    pub const VMIN: usize = 16;
    pub const VTIME: usize = 17;
    /// SDK sys/termios.h：`#define TCIFLUSH 1`（BSD 与 Linux 数值不同，不得混用）
    pub const TCIFLUSH: i32 = 1;
    /// SDK sys/fcntl.h：`#define O_NONBLOCK 0x0004`（与 Linux 数值不同，不得混用）
    pub const O_NONBLOCK: i32 = 0x0004;
    /// SDK sys/fcntl.h：`#define O_CLOEXEC 0x01000000`（与 Linux 数值不同，不得混用）
    pub const O_CLOEXEC: i32 = 0x0100_0000;
    /// SDK sys/termios.h：`#define CLOCAL 0x00008000`（忽略载波——TTL 链路无
    /// modem，隔离型适配器 DCD 经隔离器悬空恒"无载波"，不置位则 open(2) 阻塞
    /// 等载波永挂）
    pub const CLOCAL: u64 = 0x0000_8000;
    /// SDK sys/termios.h：`#define CREAD 0x00000800`（接收使能）
    pub const CREAD: u64 = 0x0000_0800;
    /// SDK sys/fcntl.h：`#define F_GETFL 3`、`#define F_SETFL 4`（两平台同值）
    pub const F_GETFL: i32 = 3;
    pub const F_SETFL: i32 = 4;
    /// SDK sys/signal.h：`struct pollfd`（int + short + short = 8B，无填充）
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PollFd {
        pub fd: i32,
        pub events: i16,
        pub revents: i16,
    }
    const _: () = assert!(
        std::mem::size_of::<PollFd>() == 8,
        "macOS struct pollfd ABI 漂移"
    );
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
    const _: () = assert!(
        std::mem::size_of::<Termios>() == 72,
        "macOS struct termios ABI 漂移"
    );

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
    /// Linux uapi asm-generic/termbits.h：`#define B1500000 0010012` / `B2000000 0010013`
    /// （八进制；PL011 时钟 48MHz 下 1.5M=32 分频、2M=24 分频，均为整分频）
    pub const B1500000: SpeedT = 0o10012;
    pub const B2000000: SpeedT = 0o10013;
    /// Linux：`#define NCCS 32`、`#define VMIN 6`、`#define VTIME 5`
    pub const NCCS: usize = 32;
    pub const VMIN: usize = 6;
    pub const VTIME: usize = 5;
    /// Linux uapi asm-generic/termbits.h：`#define TCIFLUSH 0`（与 macOS=1 不同值）
    pub const TCIFLUSH: i32 = 0;
    /// Linux uapi asm-generic/fcntl.h：`#define O_NONBLOCK 04000`（八进制，与
    /// macOS 数值不同，不得混用）
    pub const O_NONBLOCK: i32 = 0o4000;
    /// Linux uapi asm-generic/fcntl.h：`#define O_CLOEXEC 02000000`（八进制，与
    /// macOS 数值不同，不得混用）
    pub const O_CLOEXEC: i32 = 0o2000000;
    /// Linux uapi asm-generic/termbits.h：`#define CLOCAL 0004000`、`#define CREAD
    /// 0000200`（八进制，与 macOS 数值不同不得混用——TTL 链路无 modem，不置
    /// CLOCAL 则 open(2) 阻塞等载波永挂）
    pub const CLOCAL: u32 = 0o4000;
    pub const CREAD: u32 = 0o200;
    /// Linux uapi fcntl.h：`#define F_GETFL 3`、`#define F_SETFL 4`（两平台同值）
    pub const F_GETFL: i32 = 3;
    pub const F_SETFL: i32 = 4;
    /// glibc sys/poll.h：`struct pollfd`（int + short + short = 8B，无填充）
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PollFd {
        pub fd: i32,
        pub events: i16,
        pub revents: i16,
    }
    const _: () = assert!(
        std::mem::size_of::<PollFd>() == 8,
        "Linux struct pollfd ABI 漂移"
    );
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
    const _: () = assert!(
        std::mem::size_of::<Termios>() == 60,
        "Linux struct termios ABI 漂移"
    );

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

pub(crate) use imp::{
    NfdsT, PollFd, SpeedT, Termios, B115200, B1500000, B2000000, B921600, B9600, CLOCAL, CREAD,
    F_GETFL, F_SETFL, O_CLOEXEC, O_NOCTTY, O_NONBLOCK, O_RDWR, POLLIN, TCIFLUSH, VMIN, VTIME,
};
