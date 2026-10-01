//! shm 传输：实时控制面初版（ADR-0011）。单写者多读者环形缓冲，丢最旧保最新。
//!
//! 语义（ADR-0011 裁定 1）：写者**永不阻塞、永不失败**（除帧长超环半容量外），
//! 区满直接覆盖最旧未读数据；读者游标落后超过半圈时重同步到最新帧边界并累加
//! 丢失计数（[`Rr3ShmReaderConn::rr3_lost`]）。可靠交付与反压是事务面（uds）的
//! 职责，两层语义相反正是分层的意义。
//!
//! 无锁协议（裁定 2）：区域头只留**一个**共享变量 `head`（已发布写游标，单调），
//! 环内记录 = 一整帧（128B 头 + payload）。读者拷出后复核 `head` 未变（seqlock 式）；
//! 配合"帧长 ≤ cap/2"的半容量守卫，拷贝期间写点物理上追不上读者窗口，接受帧必然
//! 完整——证明见 ADR-0011 裁定 2。
//!
//! trait 面（裁定 3）：[`Rr3ShmListener`] 实现 `Rr3TransportEndpoint` +
//! `Rr3TransportListener`（accept = 开一个新读者视图），写者/读者连接各实现
//! `Rr3TransportConn`——shm 连接天生单向，方向不匹配的方法恒报 `Io`，是"与 uds
//! 同 trait 面"的已知代价。核心环（本文件主体）平台无关；POSIX 附着
//! （`shm_open`/`mmap` 等手写 extern 面，**不引 libc crate**，裁定 5）集中在
//! 文件尾 cfg 门控段，只覆盖 linux / macos。
//!
//! 反射环（1k–10kHz）永远不进任何总线（含共享内存），见 ADR-0007 §裁定 1。

use std::io::Cursor;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use crate::protocol::header::{Rr3Header, RR3_IPC_HDR_SIZE};
use crate::transport::buf::{rr3_framebuf_read, Rr3FrameBuf};
use crate::transport::frame::Rr3FrameError;
use crate::transport::Rr3TransportConn;

/// 区域头字节数（固定 64B：magic/version/cap/head + reserved）
pub const RR3_SHM_HDR_SIZE: usize = 64;

const SHM_OFF_MAGIC: usize = 0;
const SHM_OFF_VERSION: usize = 4;
const SHM_OFF_CAP: usize = 8;
const SHM_OFF_HEAD: usize = 16;

/// 区域魔数；LE 线上字节为 52 52 33 53（"RR3S"）
pub const RR3_SHM_MAGIC: u32 = u32::from_le_bytes(*b"RR3S");

/// 区域布局版本（演进须经 ADR 升号）
pub const RR3_SHM_VERSION: u32 = 1;

/// 默认环容量 1 MiB（半容量守卫下最大帧 512 KiB；建大区走
/// `Rr3ShmListener::rr3_bind_with_cap`）
pub const RR3_SHM_RING_CAP_DEFAULT: u64 = 1024 * 1024;

/// 默认 shm 名（ADR-0011 裁定 4；POSIX shm 名须以 `/` 开头）
pub const RR3_SHM_NAME_DEFAULT: &str = "/rock3-ipc-shm";

/// 覆盖 shm 名的环境变量键（naming.md：`RR3_` 大写前缀）
pub const RR3_SHM_NAME_ENV: &str = "RR3_SHM_NAME";

/// trait 阻塞语义（`rr3_recv_buf`）的退避轮询间隔。通知原语（futex /
/// mach semaphore）按 ADR-0011"不做"清单不做；控制面本就以环率轮询读最新。
const RR3_SHM_RECV_POLL_US: u64 = 200;

/// 环容量下限：保证半容量 ≥ 2 KiB，至少容得下空 payload 帧（128B）还有富余
const RR3_SHM_MIN_CAP: u64 = 4096;

// ---------------------------------------------------------------------------
// 核心环（平台无关）：头访问、wrap 拷贝、发送与读取的共享逻辑
//
// unsafe 安全论据（裁定 5 的代码侧落地）：
// - `base` 指向 `RR3_SHM_HDR_SIZE + cap` 字节的进程共享内存（POSIX mmap 或测试
//   的对齐进程内缓冲），本模块只经原子量与 `copy_nonoverlapping` 触碰它，
//   不存在 Rust 引用别名；头内原子量偏移均为 4/8 对齐（base 由 mmap 页对齐或
//   测试的 `Vec<u64>` 保证 8 对齐）。
// - `head` 的写方只有写者（单写者裁定）；读者只 Acquire 读。
// ---------------------------------------------------------------------------

unsafe fn shm_atomic_u32(base: *mut u8, off: usize) -> *const AtomicU32 {
    base.add(off) as *const AtomicU32
}

unsafe fn shm_atomic_u64(base: *mut u8, off: usize) -> *const AtomicU64 {
    base.add(off) as *const AtomicU64
}

/// 建区时初始化头（ftruncate / 零填充缓冲已保证全区为 0，head 初值 0 无需写）。
/// 发布顺序 cap → version → magic(Release)：附着侧 Acquire 读到 magic 即可看见 cap。
unsafe fn rr3_shm_init_header(base: *mut u8, cap: u64) {
    (*shm_atomic_u64(base, SHM_OFF_CAP)).store(cap, Ordering::Relaxed);
    (*shm_atomic_u32(base, SHM_OFF_VERSION)).store(RR3_SHM_VERSION, Ordering::Relaxed);
    (*shm_atomic_u32(base, SHM_OFF_MAGIC)).store(RR3_SHM_MAGIC, Ordering::Release);
}

/// 头三字段的值检查（magic / version / cap 下限），返回环容量。
/// 拆成独立函数：区域侧经原子量取值后走这里，POSIX 探头（pread 回的字节缓冲
/// 只有 1 字节对齐，**不能**承载 u64 原子读）小端解析后也走这里。
fn rr3_shm_check_header(magic: u32, version: u32, cap: u64) -> std::io::Result<u64> {
    use std::io::{Error, ErrorKind};
    if magic != RR3_SHM_MAGIC {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("shm 区域 magic 不符: 0x{magic:08x}（对象不是本模块建区产物？）"),
        ));
    }
    if version != RR3_SHM_VERSION {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "shm 区域版本不符: {version}（本进程认 {}）",
                RR3_SHM_VERSION
            ),
        ));
    }
    if cap < RR3_SHM_MIN_CAP {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("shm 环容量 {cap} 低于下限 {RR3_SHM_MIN_CAP}"),
        ));
    }
    Ok(cap)
}

/// 校验附着对象头（区域侧探头映射上取值），返回环容量。发布顺序的对面：
/// Acquire 读 magic（配建区方的 Release 发布），cap/version 随 happens-before 可见。
/// 头不完整 / 陌生对象 → Err。
///
/// SAFETY：base 必须 8 对齐（mmap 页对齐 / 测试的 `Vec<u64>` 底座），
/// 覆盖 `RR3_SHM_HDR_SIZE` 字节。
unsafe fn rr3_shm_validate_header(base: *mut u8) -> std::io::Result<u64> {
    let magic = (*shm_atomic_u32(base, SHM_OFF_MAGIC)).load(Ordering::Acquire);
    let version = (*shm_atomic_u32(base, SHM_OFF_VERSION)).load(Ordering::Relaxed);
    let cap = (*shm_atomic_u64(base, SHM_OFF_CAP)).load(Ordering::Relaxed);
    rr3_shm_check_header(magic, version, cap)
}

/// 环内 wrap 拷入：物理槽 = 偏移 mod cap，跨折返分两段
unsafe fn rr3_ring_copy_to(base: *mut u8, cap: u64, off: u64, src: &[u8]) {
    let ring = base.add(RR3_SHM_HDR_SIZE);
    let p = (off % cap) as usize;
    let first = usize::min(src.len(), cap as usize - p);
    std::ptr::copy_nonoverlapping(src.as_ptr(), ring.add(p), first);
    std::ptr::copy_nonoverlapping(src.as_ptr().add(first), ring, src.len() - first);
}

/// 环内 wrap 拷出（与 [`rr3_ring_copy_to`] 对称）
unsafe fn rr3_ring_copy_from(base: *mut u8, cap: u64, off: u64, dst: &mut [u8]) {
    let ring = base.add(RR3_SHM_HDR_SIZE);
    let p = (off % cap) as usize;
    let first = usize::min(dst.len(), cap as usize - p);
    std::ptr::copy_nonoverlapping(ring.add(p), dst.as_mut_ptr(), first);
    std::ptr::copy_nonoverlapping(ring, dst.as_mut_ptr().add(first), dst.len() - first);
}

/// 写路径（裁定 1/2）：整帧 wrap 拷入 → head 单步 Release 发布。
/// 除"帧长超半容量"外永不失败、永不等待。
unsafe fn rr3_shm_send_bytes(base: *mut u8, cap: u64, bytes: &[u8]) -> Result<(), Rr3FrameError> {
    let size = bytes.len() as u64;
    if size > cap / 2 {
        return Err(Rr3FrameError::Io(format!(
            "帧长 {size} 超过 shm 环半容量 {}（大帧归 uds 承载，ADR-0011 裁定 4）",
            cap / 2
        )));
    }
    // 写者是 head 唯一写方，Relaxed 读自己的最新值即可
    let t = (*shm_atomic_u64(base, SHM_OFF_HEAD)).load(Ordering::Relaxed);
    rr3_ring_copy_to(base, cap, t, bytes);
    (*shm_atomic_u64(base, SHM_OFF_HEAD)).store(t + size, Ordering::Release);
    Ok(())
}

/// 读路径单步（裁定 2）。`Ok(None)` = 本拍无完整可读帧
/// （空 / 未发布完 / 重同步 / 复核重试），不推进游标或已重同步。
unsafe fn rr3_shm_try_recv(
    base: *mut u8,
    cap: u64,
    cursor: &mut u64,
    lost: &mut u64,
) -> Result<Option<Rr3FrameBuf>, Rr3FrameError> {
    let h1 = (*shm_atomic_u64(base, SHM_OFF_HEAD)).load(Ordering::Acquire);
    if h1 == *cursor {
        return Ok(None);
    }
    if h1 - *cursor > cap / 2 {
        // 落后过半圈：重同步到最新帧边界（head 恒为帧尾即帧边界），丢失计数 +1
        *lost += 1;
        *cursor = h1;
        return Ok(None);
    }
    // cursor 恒为帧边界且 h1 > cursor → 该帧已完整发布（head 只落在帧尾），
    // 坏头 = 区域真损坏，如实报错而非静默重同步
    let mut hb = [0u8; RR3_IPC_HDR_SIZE];
    rr3_ring_copy_from(base, cap, *cursor, &mut hb);
    let hdr = Rr3Header::from_bytes(&hb)?;
    let size = RR3_IPC_HDR_SIZE as u64 + hdr.payload_len as u64;
    if size > cap / 2 {
        // 防御：send 已拒超半容量帧，正常不可达
        *lost += 1;
        *cursor = h1;
        return Ok(None);
    }
    if *cursor + size > h1 {
        // 防御：head 是帧尾，cursor 上的帧必然已发布完，正常不可达
        return Ok(None);
    }
    let mut bytes = vec![0u8; size as usize];
    rr3_ring_copy_from(base, cap, *cursor, &mut bytes);
    // seqlock 复核：拷贝期间 head 前进 → 写点随之前进，完整性证明失效，保守丢弃重试
    let h2 = (*shm_atomic_u64(base, SHM_OFF_HEAD)).load(Ordering::Acquire);
    if h2 != h1 {
        return Ok(None);
    }
    *cursor += size;
    let mut cur = Cursor::new(bytes.as_slice());
    Ok(Some(rr3_framebuf_read(&mut cur)?))
}

/// 解除映射（POSIX 段真 munmap；进程内测试区域 owned = false 不会走到这里）
fn rr3_shm_unmap(base: *mut u8, len: usize) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    unsafe {
        if !base.is_null() && len > 0 {
            posix::munmap_raw(base, len);
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (base, len);
    }
}

// ---------------------------------------------------------------------------
// 连接类型：写者单向发、读者单向收（裁定 3）。区域指针随类型走，
// `Send` 允许把连接挪去别的线程（uds 的 `UnixStream` 同为 Send）。
// ---------------------------------------------------------------------------

/// 写者连接：`rr3_send_buf` = 写路径；`rr3_recv_buf` 恒报 `Io`（单向发送）。
pub struct Rr3ShmWriterConn {
    base: *mut u8,
    cap: u64,
    /// munmap 长度；`owned = false`（进程内测试区域）时 Drop 不解除映射
    map_len: usize,
    owned: bool,
}

// SAFETY：区域是进程共享原始内存，本类型只暴露 `&mut self` 收发，无引用别名；
// 连接不含线程亲和资源（uds 的 UnixStream 同级）。
unsafe impl Send for Rr3ShmWriterConn {}

impl Rr3ShmWriterConn {
    /// 进程内区域构造（测试与同进程编排用；Drop 不解除映射）
    #[cfg(test)]
    pub(crate) fn rr3_from_raw(base: *mut u8, cap: u64) -> Self {
        Self {
            base,
            cap,
            map_len: 0,
            owned: false,
        }
    }
}

impl Drop for Rr3ShmWriterConn {
    fn drop(&mut self) {
        if self.owned {
            rr3_shm_unmap(self.base, self.map_len);
        }
    }
}

impl Rr3TransportConn for Rr3ShmWriterConn {
    fn rr3_send_buf(&mut self, mut buf: Rr3FrameBuf) -> Result<(), Rr3FrameError> {
        buf.rr3_seal(); // 幂等：未 seal 就 seal，已 seal 落同一串字节
        let bytes = buf.rr3_as_sealed_bytes()?;
        unsafe { rr3_shm_send_bytes(self.base, self.cap, bytes) }
    }

    fn rr3_recv_buf(&mut self) -> Result<Rr3FrameBuf, Rr3FrameError> {
        Err(Rr3FrameError::Io(
            "shm 写者连接不支持接收（单向发送，ADR-0011 裁定 3）".into(),
        ))
    }
}

/// 读者连接：`rr3_recv_buf`（阻塞，退避轮询）/ [`rr3_try_recv_buf`](Self::rr3_try_recv_buf)
/// （非阻塞单拍）；`rr3_send_buf` 恒报 `Io`（单向接收）。
pub struct Rr3ShmReaderConn {
    base: *mut u8,
    cap: u64,
    map_len: usize,
    owned: bool,
    /// 读者本地游标（恒为帧边界；不进区域——多读者 = 多个独立游标）
    cursor: u64,
    /// 重同步次数（每次意味着至少半圈帧未读）
    lost: u64,
}

// SAFETY：同写者连接；cursor/lost 为读者私有，无共享。
unsafe impl Send for Rr3ShmReaderConn {}

impl Rr3ShmReaderConn {
    /// 进程内区域构造（测试与同进程编排用；Drop 不解除映射）
    #[cfg(test)]
    pub(crate) fn rr3_from_raw(base: *mut u8, cap: u64) -> Self {
        Self {
            base,
            cap,
            map_len: 0,
            owned: false,
            cursor: 0,
            lost: 0,
        }
    }

    /// 非阻塞读一拍：有完整新帧返回 `Some`，否则 `None`
    /// （空 / 未发布完 / 重同步 / seqlock 复核重试）。
    pub fn rr3_try_recv_buf(&mut self) -> Result<Option<Rr3FrameBuf>, Rr3FrameError> {
        unsafe { rr3_shm_try_recv(self.base, self.cap, &mut self.cursor, &mut self.lost) }
    }

    /// 已重同步次数（丢失的粗粒度可观测面；帧级条数不可知——被覆盖即无凭据）
    pub fn rr3_lost(&self) -> u64 {
        self.lost
    }
}

impl Drop for Rr3ShmReaderConn {
    fn drop(&mut self) {
        if self.owned {
            rr3_shm_unmap(self.base, self.map_len);
        }
    }
}

impl Rr3TransportConn for Rr3ShmReaderConn {
    fn rr3_send_buf(&mut self, _buf: Rr3FrameBuf) -> Result<(), Rr3FrameError> {
        Err(Rr3FrameError::Io(
            "shm 读者连接不可发送（单向接收，ADR-0011 裁定 3）".into(),
        ))
    }

    fn rr3_recv_buf(&mut self) -> Result<Rr3FrameBuf, Rr3FrameError> {
        loop {
            if let Some(buf) = self.rr3_try_recv_buf()? {
                return Ok(buf);
            }
            std::thread::sleep(Duration::from_micros(RR3_SHM_RECV_POLL_US));
        }
    }
}

// ---------------------------------------------------------------------------
// POSIX 附着（裁定 5）：手写最小 extern 面，不引 libc crate。
// 只覆盖 linux / macos；其余平台核心环与进程内构造仍可用。
// ---------------------------------------------------------------------------

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod posix {
    use std::ffi::CString;
    use std::io::{self, ErrorKind};
    use std::os::raw::{c_char, c_int, c_long, c_uint, c_void};
    use std::path::{Path, PathBuf};

    use super::{
        rr3_shm_init_header, rr3_shm_validate_header, Rr3ShmReaderConn, Rr3ShmWriterConn,
        RR3_SHM_HDR_SIZE, RR3_SHM_MIN_CAP, RR3_SHM_RING_CAP_DEFAULT,
    };
    use crate::transport::{Rr3TransportEndpoint, Rr3TransportListener};

    // O_* 常量按目标平台取值（本段只覆盖 linux / macos 两支）
    pub const O_RDWR: c_int = 0x0002;
    #[cfg(target_os = "macos")]
    pub const O_CREAT: c_int = 0x0200;
    #[cfg(target_os = "macos")]
    pub const O_EXCL: c_int = 0x0800;
    #[cfg(target_os = "linux")]
    pub const O_CREAT: c_int = 0x0040;
    #[cfg(target_os = "linux")]
    pub const O_EXCL: c_int = 0x0080;
    const O_RDONLY: c_int = 0;
    const PROT_READ: c_int = 0x1;
    const PROT_WRITE: c_int = 0x2;
    const MAP_SHARED: c_int = 0x01;

    // 6 个稳定 POSIX 函数。shm fd 只支持 mmap（macOS 上 read/pread 对 shm 对象
    // 直接 ENXIO/ESPIPE），头校验走 64B 探头映射 + 原子量（映射页对齐，偏移 4/8
    // 对齐成立）。off_t 在 64 位 linux/macos 均为 c_long 同宽；mode_t 为 c_uint。
    extern "C" {
        // glibc 把 mode 声明为变参（...），macOS 为定参——两种 ABI 下以 c_uint
        // 传 mode 等价（libc crate 同样按变参声明）
        fn shm_open(name: *const c_char, oflag: c_int, ...) -> c_int;
        fn shm_unlink(name: *const c_char) -> c_int;
        fn ftruncate(fd: c_int, length: c_long) -> c_int;
        fn mmap(
            addr: *mut c_void,
            len: usize,
            prot: c_int,
            flags: c_int,
            fd: c_int,
            offset: c_long,
        ) -> *mut c_void;
        fn munmap(addr: *mut c_void, len: usize) -> c_int;
        fn close(fd: c_int) -> c_int;
    }

    fn cstring_name(path: &Path) -> io::Result<CString> {
        let s = path
            .to_str()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "shm 名不是 UTF-8"))?;
        if !s.starts_with('/') {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("shm 名必须以 '/' 开头: {s}"),
            ));
        }
        CString::new(s).map_err(|_| io::Error::new(ErrorKind::InvalidInput, "shm 名含 NUL"))
    }

    /// mmap；MAP_FAILED（-1）转 io::Error
    unsafe fn mmap_region(fd: c_int, len: usize, prot: c_int) -> io::Result<*mut u8> {
        let p = mmap(std::ptr::null_mut(), len, prot, MAP_SHARED, fd, 0);
        if p as usize == usize::MAX {
            return Err(io::Error::last_os_error());
        }
        Ok(p as *mut u8)
    }

    pub(super) unsafe fn munmap_raw(base: *mut u8, len: usize) {
        let _ = munmap(base as *mut c_void, len);
    }

    /// 清理残留对象。不存在 = 已干净，`Ok(())`（与 uds 删 socket 文件同口径）。
    pub(super) fn unlink_if_exists(path: &Path) -> io::Result<()> {
        let name = cstring_name(path)?;
        // SAFETY：name 是合法 NUL 结尾 C 串
        if unsafe { shm_unlink(name.as_ptr()) } != 0 {
            let e = io::Error::last_os_error();
            if e.kind() == ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        } else {
            Ok(())
        }
    }

    /// 建区：`O_CREAT|O_EXCL`（对位 uds"不允许 bind 到已存在路径"）→ ftruncate →
    /// mmap 初始化头 → munmap/close。对象持久到 shm_unlink（与 socket 文件同形），
    /// 监听器只持有名字。
    pub(super) fn create_region(path: &Path, cap: u64) -> io::Result<Rr3ShmListener> {
        if cap < RR3_SHM_MIN_CAP {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("环容量 {cap} 低于下限 {RR3_SHM_MIN_CAP}"),
            ));
        }
        let name = cstring_name(path)?;
        // SAFETY：name 合法；mode 以 c_uint 传（见 extern 声明注释）
        let fd = unsafe { shm_open(name.as_ptr(), O_RDWR | O_CREAT | O_EXCL, 0o600 as c_uint) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // 后续任一步失败都统一走 err 分支关闭 fd，不留半初始化对象给附着方
        let result = (|| -> io::Result<Rr3ShmListener> {
            let total = RR3_SHM_HDR_SIZE as c_long + cap as c_long;
            // SAFETY：fd 是 shm_open 的合法描述符
            if unsafe { ftruncate(fd, total) } != 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY：对象刚被 ftruncate 到 total，映射区间全在对象内
            let base = unsafe { mmap_region(fd, total as usize, PROT_READ | PROT_WRITE)? };
            unsafe { rr3_shm_init_header(base, cap) };
            unsafe { munmap_raw(base, total as usize) };
            Ok(Rr3ShmListener {
                name: path.to_path_buf(),
            })
        })();
        let _ = unsafe { close(fd) };
        result
    }

    /// 附着：open（不 O_CREAT）→ 64B 探头映射校验头 → 按头内 cap 全量映射。
    /// shm fd 只有 mmap 一条读路（macOS 上 read/pread 对 shm 对象直接报错）；
    /// 探头映射页对齐，头内原子量（Acquire 读 magic，配写者的 Release 发布）
    /// 对齐与序都成立。已知边界：对**外来**同名对象（大小 < 64B 或 0）的探头
    /// 读取会 SIGBUS——同名互踩属配置错误（uds 侧同名的形状是干净报错，这里
    /// 记为本实现已知边界，随 rrd 接线再评估是否加防护）；建区方 open 与
    /// ftruncate 之间的纳秒级窗口同理，附着方在 bind 返回之后附着即可避开。
    pub(super) fn attach_region(path: &Path, writable: bool) -> io::Result<(*mut u8, u64, usize)> {
        let name = cstring_name(path)?;
        let oflag = if writable { O_RDWR } else { O_RDONLY };
        // SAFETY：name 合法；只读附着不传 mode
        let fd = unsafe { shm_open(name.as_ptr(), oflag) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            // 探头：只映头 64B（mmap 长度按页取整由内核处理，不越对象语义）
            // SAFETY：对象存在（shm_open 成功）；访问限头区
            let probe = unsafe { mmap_region(fd, RR3_SHM_HDR_SIZE, PROT_READ)? };
            let checked = unsafe { rr3_shm_validate_header(probe) };
            unsafe { munmap_raw(probe, RR3_SHM_HDR_SIZE) };
            let cap = checked?;
            let total = RR3_SHM_HDR_SIZE + cap as usize;
            let prot = if writable {
                PROT_READ | PROT_WRITE
            } else {
                PROT_READ
            };
            // SAFETY：头校验通过说明对象由本模块建区产物 ftruncate 到 total
            let base = unsafe { mmap_region(fd, total, prot)? };
            Ok((base, cap, total))
        })();
        let _ = unsafe { close(fd) };
        result
    }

    /// shm 监听端点（写者侧锚点）：只持有名字的对象句柄。
    /// `rr3_bind` 建区（默认容量），`rr3_accept` 开新读者视图，
    /// `rr3_writer_conn` 取写者连接。单写者由规约保证（一区域一 bind、
    /// 写者连接只由 bind 方取得），内核不强制。
    pub struct Rr3ShmListener {
        name: PathBuf,
    }

    impl Rr3TransportEndpoint for Rr3ShmListener {
        /// 删上一次运行残留的 shm 对象。不存在不报错。
        fn rr3_cleanup(path: &Path) -> io::Result<()> {
            unlink_if_exists(path)
        }

        /// 建区（默认容量）。调用前先 `rr3_cleanup`：`O_EXCL` 不允许绑到已存在对象。
        fn rr3_bind(path: &Path) -> io::Result<Self> {
            create_region(path, RR3_SHM_RING_CAP_DEFAULT)
        }
    }

    impl Rr3TransportListener for Rr3ShmListener {
        type Conn = Rr3ShmReaderConn;

        /// accept = 在既有区域上开一个**新读者视图**（独立游标；shm 无连接握手，
        /// 附着即到）。一次 accept 的 `Err` 只属于那一次。
        fn rr3_accept(&self) -> io::Result<Rr3ShmReaderConn> {
            let (base, cap, total) = attach_region(&self.name, false)?;
            Ok(Rr3ShmReaderConn {
                base,
                cap,
                map_len: total,
                owned: true,
                cursor: 0,
                lost: 0,
            })
        }
    }

    impl Rr3ShmListener {
        /// 建大区（trait 的 `rr3_bind` 签名只有 path，用默认容量——与 uds 对齐）。
        pub fn rr3_bind_with_cap(path: &Path, cap: u64) -> io::Result<Self> {
            create_region(path, cap)
        }

        /// 取写者连接（独立映射，指向同一对象）。
        pub fn rr3_writer_conn(&self) -> io::Result<Rr3ShmWriterConn> {
            let (base, cap, total) = attach_region(&self.name, true)?;
            Ok(Rr3ShmWriterConn {
                base,
                cap,
                map_len: total,
                owned: true,
            })
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use posix::Rr3ShmListener;

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Rr3ShmReaderConn {
    /// 跨进程读者附着入口（与 uds 的 `Rr3UdsListener::rr3_connect` 对位）：
    /// 按名打开既有区域并映射为只读视图，游标从头开始追。
    pub fn rr3_attach(path: &std::path::Path) -> std::io::Result<Self> {
        let (base, cap, total) = posix::attach_region(path, false)?;
        Ok(Self {
            base,
            cap,
            map_len: total,
            owned: true,
            cursor: 0,
            lost: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::header::opcode;

    /// 进程内对齐区域（8 字节对齐满足头内 u64 原子量）；leak 到测试进程结束
    /// （测试进程退出即回收，不引清理复杂度）。
    fn raw_region(cap: u64) -> (*mut u8, u64) {
        let total_words = (RR3_SHM_HDR_SIZE + cap as usize).div_ceil(8);
        let mut mem = vec![0u64; total_words];
        let base = mem.as_mut_ptr() as *mut u8;
        std::mem::forget(mem);
        unsafe { rr3_shm_init_header(base, cap) };
        (base, cap)
    }

    fn frame(payload: &[u8], trace: u64) -> Rr3FrameBuf {
        let mut h = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
        h.trace_id = trace;
        let mut b = Rr3FrameBuf::rr3_new(h);
        b.rr3_payload_mut().copy_from_slice(payload);
        b
    }

    /// 布局常量自洽：头 64B、偏移连续、magic LE 字节 = "RR3S"、版本 1
    #[test]
    fn shm_header_layout_is_contiguous() {
        assert_eq!(RR3_SHM_HDR_SIZE, 64);
        assert_eq!(SHM_OFF_MAGIC, 0);
        assert_eq!(SHM_OFF_VERSION, SHM_OFF_MAGIC + 4);
        assert_eq!(SHM_OFF_CAP, SHM_OFF_VERSION + 4);
        assert_eq!(SHM_OFF_HEAD, SHM_OFF_CAP + 8);
        assert_eq!(SHM_OFF_HEAD + 8, 24, "头区 24..64 留 reserved（全 0）");
        assert_eq!(RR3_SHM_MAGIC.to_le_bytes(), *b"RR3S");
        assert_eq!(RR3_SHM_VERSION, 1);
    }

    /// 单读者逐帧往返（含大帧跨环折返的两段拷贝路径），lost 恒 0，读空返回 None
    #[test]
    fn shm_send_recv_round_trip_single_reader() {
        let (base, cap) = raw_region(64 * 1024);
        let mut w = Rr3ShmWriterConn::rr3_from_raw(base, cap);
        let mut r = Rr3ShmReaderConn::rr3_from_raw(base, cap);
        for i in 0..64u64 {
            let payload = format!("payload-{i}").into_bytes();
            w.rr3_send_buf(frame(&payload, i)).expect("send");
            let got = r.rr3_try_recv_buf().expect("recv").expect("应有帧");
            assert_eq!(got.rr3_payload(), &payload[..]);
            assert_eq!(got.rr3_header().trace_id, i);
        }
        assert_eq!(r.rr3_lost(), 0);
        assert_eq!(r.rr3_try_recv_buf().expect("读空不报错"), None);

        // 大帧（30KB × 3，共 90KB > 64KB 环）强制写读两个方向都跨折返
        for i in 0..3u64 {
            let payload: Vec<u8> = (0..30_000).map(|k| (k + i) as u8).collect();
            w.rr3_send_buf(frame(&payload, 1000 + i))
                .expect("大帧 send");
            let got = r.rr3_try_recv_buf().expect("recv").expect("应有帧");
            assert_eq!(got.rr3_payload(), &payload[..], "折返帧字节漂移");
        }
        assert_eq!(r.rr3_lost(), 0);
    }

    /// 读者落后过半圈：重同步到最新帧边界、lost 计数，随后从新边界继续追
    #[test]
    fn shm_reader_lag_resync_counts_lost() {
        let (base, cap) = raw_region(8 * 1024); // 半容量 4KiB
        let mut w = Rr3ShmWriterConn::rr3_from_raw(base, cap);
        let mut r = Rr3ShmReaderConn::rr3_from_raw(base, cap);
        // 192B × 256 = 48KiB ≈ 6 圈，读者全程未读
        for i in 0..256u64 {
            w.rr3_send_buf(frame(&[b'x'; 64], i)).expect("send");
        }
        assert_eq!(r.rr3_try_recv_buf().expect("重同步拍"), None);
        assert!(r.rr3_lost() >= 1, "重同步必须计入 lost");
        w.rr3_send_buf(frame(b"fresh", 999)).expect("send");
        let got = r
            .rr3_try_recv_buf()
            .expect("recv")
            .expect("重同步后应读到新帧");
        assert_eq!(got.rr3_payload(), b"fresh");
    }

    /// 超半容量帧发送侧拒绝（Io），区域不受影响
    #[test]
    fn shm_writer_rejects_oversize_frame() {
        let (base, cap) = raw_region(4096); // 半容量 2KiB
        let mut w = Rr3ShmWriterConn::rr3_from_raw(base, cap);
        let mut r = Rr3ShmReaderConn::rr3_from_raw(base, cap);
        let big = vec![0u8; 3000];
        let err = w.rr3_send_buf(frame(&big, 1)).unwrap_err();
        assert!(
            matches!(err, Rr3FrameError::Io(_)),
            "超半容量应报 Io: {err:?}"
        );
        w.rr3_send_buf(frame(b"ok", 2)).expect("拒绝后区域不受影响");
        let got = r.rr3_try_recv_buf().expect("recv").expect("应有帧");
        assert_eq!(got.rr3_payload(), b"ok");
    }

    /// 多读者独立游标：追得上的读者零丢失，停读者重同步
    #[test]
    fn shm_multi_reader_independent_cursors() {
        let (base, cap) = raw_region(8 * 1024);
        let mut w = Rr3ShmWriterConn::rr3_from_raw(base, cap);
        let mut r1 = Rr3ShmReaderConn::rr3_from_raw(base, cap);
        let mut r2 = Rr3ShmReaderConn::rr3_from_raw(base, cap);
        w.rr3_send_buf(frame(b"first", 1)).expect("send");
        for r in [&mut r1, &mut r2] {
            let got = r.rr3_try_recv_buf().expect("recv").expect("两读者都应读到");
            assert_eq!(got.rr3_payload(), b"first");
        }
        // r1 逐帧跟读（192B × 64 = 12KiB，不跟读会把自己也甩过半圈）
        for i in 2..66u64 {
            w.rr3_send_buf(frame(&[b'y'; 64], i)).expect("send");
            let got = r1.rr3_try_recv_buf().expect("recv").expect("r1 应追上");
            assert_eq!(got.rr3_header().trace_id, i);
        }
        assert_eq!(r1.rr3_lost(), 0, "跟读读者零丢失");
        assert_eq!(r2.rr3_try_recv_buf().expect("r2 重同步拍"), None);
        assert!(r2.rr3_lost() >= 1, "停读者必须重同步并计数");
        w.rr3_send_buf(frame(b"tail", 99)).expect("send");
        let got = r2
            .rr3_try_recv_buf()
            .expect("recv")
            .expect("r2 重同步后应读到");
        assert_eq!(got.rr3_payload(), b"tail");
    }

    /// trait 阻塞语义：recv_buf 等到 20ms 后才到的帧
    #[test]
    fn shm_recv_buf_blocks_until_frame_arrives() {
        let (base, cap) = raw_region(64 * 1024);
        let mut r = Rr3ShmReaderConn::rr3_from_raw(base, cap);
        // 连接（Send）整体进线程；裸指针直接进闭包会被 Send 检查拦下
        let mut w = Rr3ShmWriterConn::rr3_from_raw(base, cap);
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            w.rr3_send_buf(frame(b"late", 7)).expect("send");
        });
        let got = r.rr3_recv_buf().expect("阻塞 recv 应拿到帧");
        assert_eq!(got.rr3_payload(), b"late");
        sender.join().expect("写线程不该 panic");
    }

    /// 区域头损坏（head 已发布但帧头是垃圾）→ 如实报 Header 错，不静默
    #[test]
    fn shm_corrupt_header_reports_error_not_silent() {
        let (base, cap) = raw_region(64 * 1024);
        // 伪造"已发布"：head 前进到 256，环内还是全 0（坏 magic）
        unsafe { (*shm_atomic_u64(base, SHM_OFF_HEAD)).store(256, Ordering::Release) };
        let mut r = Rr3ShmReaderConn::rr3_from_raw(base, cap);
        let err = r.rr3_try_recv_buf().unwrap_err();
        assert!(
            matches!(err, Rr3FrameError::Header(_)),
            "坏头应报 Header: {err:?}"
        );
    }

    // ---------------- POSIX 附着段（真 shm_open / mmap） ----------------

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    mod posix_tests {
        use super::*;
        use crate::transport::{Rr3TransportEndpoint, Rr3TransportListener};
        use std::path::Path;
        use std::sync::atomic::AtomicU32;

        /// 唯一临时 shm 名（macOS POSIX shm 名上限 31 字节，故取短前缀）
        fn tmp_name(tag: &str) -> String {
            static N: AtomicU32 = AtomicU32::new(0);
            format!(
                "/rr3t-{}-{}-{}",
                tag,
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            )
        }

        /// cleanup 幂等：不存在不报错；bind 后的残留清得掉
        #[test]
        fn shm_posix_cleanup_is_idempotent() {
            let name = tmp_name("cleanup");
            let p = Path::new(&name);
            Rr3ShmListener::rr3_cleanup(p).expect("不存在也须 Ok");
            let _l = Rr3ShmListener::rr3_bind(p).expect("bind 应成功");
            Rr3ShmListener::rr3_cleanup(p).expect("残留须被清掉");
            Rr3ShmListener::rr3_cleanup(p).expect("再清一次仍 Ok");
        }

        /// O_EXCL：未清理不得重复 bind（对位 uds 不允许 bind 到已存在路径）
        #[test]
        fn shm_posix_bind_twice_without_cleanup_fails() {
            let name = tmp_name("excl");
            let p = Path::new(&name);
            let _l = Rr3ShmListener::rr3_bind(p).expect("首次 bind 应成功");
            assert!(
                Rr3ShmListener::rr3_bind(p).is_err(),
                "未清理重复 bind 应失败"
            );
            Rr3ShmListener::rr3_cleanup(p).expect("清理");
        }

        /// 真·共享内存全链路：bind → accept 两读者 → writer_conn → 独立 mmap 间往返；
        /// 随后 r1 跟读零丢失、r2 停读重同步（同一内核对象、与跨进程同一系统调用面）
        #[test]
        fn shm_posix_bind_attach_round_trip() {
            let name = tmp_name("rt");
            let p = Path::new(&name);
            Rr3ShmListener::rr3_cleanup(p).expect("预清理");
            let listener = Rr3ShmListener::rr3_bind_with_cap(p, 64 * 1024).expect("bind");
            let mut r1 = listener.rr3_accept().expect("读者视图 1");
            let mut r2 = listener.rr3_accept().expect("读者视图 2");
            let mut w = listener.rr3_writer_conn().expect("写者连接");

            w.rr3_send_buf(frame(b"one", 1)).expect("send");
            for r in [&mut r1, &mut r2] {
                let got = r.rr3_try_recv_buf().expect("recv").expect("两读者都应读到");
                assert_eq!(got.rr3_payload(), b"one");
            }
            // 192B × 300 = 57.6KiB > 半容量 32KiB：r2 全程停读被甩过半圈，r1 跟读零丢失
            for i in 2..302u64 {
                w.rr3_send_buf(frame(&[b'z'; 64], i)).expect("send");
                let got = r1.rr3_try_recv_buf().expect("recv").expect("r1 应追上");
                assert_eq!(got.rr3_header().trace_id, i);
            }
            assert_eq!(r1.rr3_lost(), 0);
            assert_eq!(r2.rr3_try_recv_buf().expect("r2 重同步拍"), None);
            assert!(r2.rr3_lost() >= 1);
            w.rr3_send_buf(frame(b"tail", 99)).expect("send");
            let got = r2
                .rr3_try_recv_buf()
                .expect("recv")
                .expect("r2 应读到 tail");
            assert_eq!(got.rr3_payload(), b"tail");
            Rr3ShmListener::rr3_cleanup(p).expect("清理");
        }

        /// 附着不存在的对象：干净报错（不建区、不悬挂）
        #[test]
        fn shm_posix_attach_missing_object_errors() {
            let name = tmp_name("missing");
            assert!(Rr3ShmReaderConn::rr3_attach(Path::new(&name)).is_err());
        }
    }
}
