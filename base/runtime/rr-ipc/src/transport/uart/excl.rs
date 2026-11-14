//! 串口排他账本：flock 非阻塞抢占、进程内路径表与忙错文案。

use super::*;

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
    /// `fcntl(2)` 是变参：这里只用 (fd, F_GETFL) 与 (fd, F_SETFL, flags) 两形状。
    #[link_name = "fcntl"]
    fn c_fcntl(fd: i32, cmd: i32, ...) -> i32;
}

/// 波特率 → `speed_t`。115200（默认）/ 9600（P3 合法降速）/ 921600 / 1.5M / 2M
/// 五档：音频上行 16k 双声道 b64 后 ≈ 87KB/s，921600 有效 92.16KB/s 只剩 5% 余量，
/// 提档是唯一不动音频口径的解法（1.5M 有效 187.5KB/s = 占 46%，板固件 2026-10-30
/// 起定档 1.5M）。档位只代表"软件侧认识"，**物理可行性由接线决定**（2026-10-20
/// 杜邦线实测 2M 滑移乱码、1.5M 间歇乱码；2026-10-30 换 PL011 三线直连 + FT232
/// VCCIO 跳线订到 3.3V 后重评，1.5M 混跑 300s×2 合计 ERR=4/6000 ≈ 0.067%，与
/// PL011@2M 同量级——残余公共底为杜邦线物理层，命令面防线无害化，见
/// docs/status.md），换线/换板后须重测。不在档位上报错
/// ——静默降到最近的档位等于把线路参数交给运气。
pub fn rr3_uart_baud_speed(baud: u32) -> std::io::Result<SpeedT> {
    let known: [(u32, SpeedT); 5] = [
        (115200, B115200),
        (9600, B9600),
        (921600, B921600),
        (1_500_000, B1500000),
        (2_000_000, B2000000),
    ];
    if let Some((_, speed)) = known.iter().find(|(b, _)| *b == baud) {
        return Ok(*speed);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "不支持的波特率 {baud}：只支持 115200（默认）、9600（降速档）、921600/1500000/2000000（高速档）"
        ),
    ))
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
pub(super) fn excl_enter(path: &str) -> std::io::Result<()> {
    let mut m = excl_map().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(g) = m.get_mut(path) {
        g.refs += 1;
        return Ok(());
    }
    let fd = rr3_excl_grab(path)?;
    m.insert(String::from(path), ExclGuard { fd, refs: 1 });
    Ok(())
}

/// 开 guard fd（O_NONBLOCK 防 carrier 阻塞）并取排他 non-blocking 锁。
fn rr3_excl_grab(path: &str) -> std::io::Result<i32> {
    let mut cpath = path.as_bytes().to_vec();
    cpath.push(0);
    // SAFETY：cpath 以 NUL 结尾，open 只读它；guard fd 不收发数据，O_NONBLOCK
    // 只为 open(2) 自身不在 carrier 上悬住。O_CLOEXEC：guard fd 是持锁面，
    // exec 换体（桥进程 spawn 运维工具等）时必须随手撤走——锁的释放点不得
    // 被不相干的子进程生命周期拖长（批N21 件一实录：无 CLOEXEC 时继承副本
    // 把锁扣到子进程退场，同账本重开撞假"跨进程占用"）。
    let fd = unsafe {
        c_open(
            cpath.as_ptr() as *const CChar,
            O_RDWR | O_NOCTTY | O_NONBLOCK | O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    rr3_excl_lock(fd)?;
    Ok(fd)
}

/// 对 guard fd 取排他 non-blocking 锁：被占即点名 ResourceBusy，失败归还 fd。
fn rr3_excl_lock(fd: i32) -> std::io::Result<()> {
    // SAFETY：合法 fd；LOCK_EX|LOCK_NB 非阻塞取锁，被占即 EWOULDBLOCK。
    if unsafe { c_flock(fd, LOCK_EX | LOCK_NB) } != 0 {
        let e = std::io::Error::last_os_error();
        // SAFETY：合法 fd，锁失败则归还。
        unsafe { c_close(fd) };
        return Err(rr3_excl_busy_err(e));
    }
    Ok(())
}

/// 取锁失败的回错：被占即点名 ResourceBusy，其余原样上报。
fn rr3_excl_busy_err(e: std::io::Error) -> std::io::Error {
    if e.kind() != std::io::ErrorKind::WouldBlock {
        return e;
    }
    // EWOULDBLOCK（macOS=35/Linux=11 同 kind）：节点被另一个进程锁着。
    // （ErrorKind::ResourceBusy 1.83 才 stable，MSRV 1.82 用 Other + 文案点名）
    std::io::Error::other(format!("串口已被其他进程占用（flock 排他）：{e}"))
}

/// 退账：refs 减到零关 guard fd（flock 随最后一次 close 释放），并移除账目。
pub(super) fn excl_exit(path: &str) {
    let mut m = excl_map().lock().unwrap_or_else(|p| p.into_inner());
    let Some(g) = m.get_mut(path) else {
        return;
    };
    g.refs -= 1;
    if g.refs == 0 {
        // SAFETY：guard fd 是 excl_enter 开出的合法描述符，账退尽即归还。
        unsafe { c_close(g.fd) };
        m.remove(path);
    }
}
