//! 语音段 flash spool 记录机制（ADR-0031 裁定 1/3）：分区布局、记录头、顺序写、
//! 分块读与 CRC 校验。零平台依赖——具体擦/写/读由调用方实现 [`Rr3SpoolIo`]。
//!
//! 布局（偏移相对 spool 分区起点）：
//!
//! ```text
//! 0x0000  4KB 记录头扇区（16B 头 + 其余保留；整扇区擦除即失效）
//! 0x1000  ... 语音段载荷（顺序写，按 4KB 扇区惰性擦除）
//! ```
//!
//! 记录头（16B，全部小端）：
//!
//! ```text
//! 0..4   magic "RR3S"
//! 4      version = 1
//! 5      kind    = 0（s16le PCM）
//! 6..8   seq u16
//! 8..12  len u32（载荷字节数）
//! 12..16 crc32（载荷，IEEE 802.3，与 rr-ipc 同一份实现）
//! ```
//!
//! 断电语义：`begin` 先擦头扇区，`finish` 最后才写头——中途断电只留下无头载荷，
//! [`Rr3Spool::rr3_load`] 判为无有效记录，绝不把半段当完整段上传。载荷 CRC 覆盖
//! 全段，读侧可逐块续算对账（[`Rr3Spool::rr3_crc_check`]）。

use rr_ipc::{rr3_crc32_final, rr3_crc32_init, rr3_crc32_update};

/// 记录头 magic（`b"RR3S"`）。
pub const RR3_SPOOL_MAGIC: [u8; 4] = *b"RR3S";
/// 记录头版本。
pub const RR3_SPOOL_VER: u8 = 1;
/// 载荷类型：s16le PCM。
pub const RR3_SPOOL_PCM: u8 = 0;
/// 记录头字节数。
pub const RR3_SPOOL_HDR: usize = 16;
/// 头扇区/擦除粒度（ESP32 flash 扇区）。
pub const RR3_SPOOL_SECTOR: usize = 4096;
/// 载荷起点（头扇区之后）。
pub const RR3_SPOOL_DATA: usize = RR3_SPOOL_SECTOR;

/// spool 存储后端：擦/写/读按**分区内偏移**寻址，长度单位为字节。
///
/// 契约：`rr3_erase` 的 `off`/`len` 按 [`RR3_SPOOL_SECTOR`] 对齐；`rr3_write`
/// 只写已擦除区；失败必须如实返回错误，不得静默截断。
pub trait Rr3SpoolIo {
    /// 平台错误（ESP 侧为 `esp_err_t`；测试用 `()`）。
    type Err;
    /// 分区总容量（含头扇区）。
    fn rr3_cap(&self) -> usize;
    fn rr3_erase(&mut self, off: usize, len: usize) -> Result<(), Self::Err>;
    fn rr3_write(&mut self, off: usize, bytes: &[u8]) -> Result<(), Self::Err>;
    fn rr3_read(&self, off: usize, buf: &mut [u8]) -> Result<(), Self::Err>;
}

/// 一条有效记录的身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3SegInfo {
    pub seq: u16,
    pub len: u32,
    pub crc32: u32,
}

/// 记录头解析失败原因（`Magic` 之外的损坏都必须显式点名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3HdrErr {
    /// magic 不匹配（空区/已擦除/记录未提交）。
    Magic,
    /// 版本或载荷类型不认识。
    Version,
    /// 长度为 0（空记录不允许提交）。
    Empty,
    /// 长度超过分区载荷容量（头可信但内容不可信）。
    Len,
}

/// spool 操作错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3SpoolErr<E> {
    Io(E),
    /// 分区容量不满足布局（不是 4KB 对齐或放不下头扇区）。
    Layout,
    /// 状态不允许（未 `begin` 就写、已 `finish` 后再写等）。
    State,
    /// 本段超过载荷容量。
    TooLarge,
    /// 空段不允许 `finish`。
    Empty,
    /// 读偏移越过段尾。
    Range,
    /// 头写回后读回不一致（写链路故障）。
    HdrMismatch,
    /// 头 magic 可信但字段不可信（版本/类型/长度损坏）。
    BadHdr(Rr3HdrErr),
}

impl<E> Rr3SpoolErr<E> {
    fn rr3_io(e: E) -> Self {
        Self::Io(e)
    }
}

/// 编出 16B 记录头（`len == 0` 视为编码错误，由 `finish` 先挡）。
pub fn rr3_hdr_encode(info: &Rr3SegInfo) -> [u8; RR3_SPOOL_HDR] {
    let mut b = [0u8; RR3_SPOOL_HDR];
    b[0..4].copy_from_slice(&RR3_SPOOL_MAGIC);
    b[4] = RR3_SPOOL_VER;
    b[5] = RR3_SPOOL_PCM;
    b[6..8].copy_from_slice(&info.seq.to_le_bytes());
    b[8..12].copy_from_slice(&info.len.to_le_bytes());
    b[12..16].copy_from_slice(&info.crc32.to_le_bytes());
    b
}

/// 解析 16B 记录头；任何不可信字段都回错误，不猜。
pub fn rr3_hdr_decode(raw: &[u8; RR3_SPOOL_HDR]) -> Result<Rr3SegInfo, Rr3HdrErr> {
    if raw[0..4] != RR3_SPOOL_MAGIC[..] {
        return Err(Rr3HdrErr::Magic);
    }
    if raw[4] != RR3_SPOOL_VER || raw[5] != RR3_SPOOL_PCM {
        return Err(Rr3HdrErr::Version);
    }
    let len = u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]);
    if len == 0 {
        return Err(Rr3HdrErr::Empty);
    }
    Ok(Rr3SegInfo {
        seq: u16::from_le_bytes([raw[6], raw[7]]),
        len,
        crc32: u32::from_le_bytes([raw[12], raw[13], raw[14], raw[15]]),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Rec,
    Done,
}

/// 顺序写 / 分块读的 spool 状态机（单线程独占；不并发）。
pub struct Rr3Spool<D: Rr3SpoolIo> {
    dev: D,
    /// 载荷容量（分区容量 - 头扇区）。
    cap: usize,
    state: State,
    seq: u16,
    len: u32,
    crc: u32,
    /// 本段已擦除到的载荷偏移（4KB 对齐）。
    erased: usize,
}

impl<D: Rr3SpoolIo> Rr3Spool<D> {
    /// 绑定后端并校验布局；容量不是 4KB 整数倍或放不下头扇区时回 `Layout`。
    pub fn rr3_new(dev: D) -> Result<Self, Rr3SpoolErr<D::Err>> {
        let cap = dev.rr3_cap();
        if cap <= RR3_SPOOL_DATA || cap % RR3_SPOOL_SECTOR != 0 {
            return Err(Rr3SpoolErr::Layout);
        }
        Ok(Self {
            dev,
            cap: cap - RR3_SPOOL_DATA,
            state: State::Idle,
            seq: 0,
            len: 0,
            crc: rr3_crc32_init(),
            erased: 0,
        })
    }

    /// 载荷容量（字节）。
    pub fn rr3_cap(&self) -> usize {
        self.cap
    }

    /// 读头：`Ok(Some)` 有效记录；`Ok(None)` 无记录（空区/未提交）；`Err(BadHdr)`
    /// 头 magic 可信但字段损坏；`Err(Io)` 读链路故障——四者语义分开，不把
    /// 读失败或损坏当"空"。
    pub fn rr3_load(&mut self) -> Result<Option<Rr3SegInfo>, Rr3SpoolErr<D::Err>> {
        let mut raw = [0u8; RR3_SPOOL_HDR];
        self.dev
            .rr3_read(0, &mut raw)
            .map_err(Rr3SpoolErr::rr3_io)?;
        let info = match rr3_hdr_decode(&raw) {
            Ok(info) => info,
            Err(Rr3HdrErr::Magic) => {
                self.state = State::Idle;
                return Ok(None);
            }
            Err(e) => {
                self.state = State::Idle;
                return Err(Rr3SpoolErr::BadHdr(e));
            }
        };
        if info.len as usize > self.cap {
            self.state = State::Idle;
            return Err(Rr3SpoolErr::BadHdr(Rr3HdrErr::Len));
        }
        self.state = State::Done;
        self.seq = info.seq;
        self.len = info.len;
        self.crc = info.crc32;
        self.erased = 0;
        Ok(Some(info))
    }

    /// 开新段：先擦头扇区（旧记录立即失效），载荷按写进度惰性擦除。
    pub fn rr3_begin(&mut self, seq: u16) -> Result<(), Rr3SpoolErr<D::Err>> {
        self.state = State::Idle;
        if let Err(e) = self.dev.rr3_erase(0, RR3_SPOOL_SECTOR) {
            return Err(Rr3SpoolErr::Io(e));
        }
        self.seq = seq;
        self.len = 0;
        self.crc = rr3_crc32_init();
        self.erased = 0;
        self.state = State::Rec;
        Ok(())
    }

    /// 顺序写一块：跨扇区时先擦目标扇区；超容量立即判死（不截断、不静默）。
    pub fn rr3_push(&mut self, block: &[u8]) -> Result<(), Rr3SpoolErr<D::Err>> {
        if self.state != State::Rec {
            return Err(Rr3SpoolErr::State);
        }
        if block.is_empty() {
            return Ok(());
        }
        let end = self.len as usize + block.len();
        if end > self.cap {
            self.state = State::Idle;
            return Err(Rr3SpoolErr::TooLarge);
        }
        let need = (end + RR3_SPOOL_SECTOR - 1) & !(RR3_SPOOL_SECTOR - 1);
        while self.erased < need {
            let off = RR3_SPOOL_DATA + self.erased;
            if let Err(e) = self.dev.rr3_erase(off, RR3_SPOOL_SECTOR) {
                self.state = State::Idle;
                return Err(Rr3SpoolErr::Io(e));
            }
            self.erased += RR3_SPOOL_SECTOR;
        }
        if let Err(e) = self
            .dev
            .rr3_write(RR3_SPOOL_DATA + self.len as usize, block)
        {
            self.state = State::Idle;
            return Err(Rr3SpoolErr::Io(e));
        }
        self.crc = rr3_crc32_update(self.crc, block);
        self.len = end as u32;
        Ok(())
    }

    /// 提交：写头并读回逐字节对账；只有对账通过才算本段有效。
    pub fn rr3_finish(&mut self) -> Result<Rr3SegInfo, Rr3SpoolErr<D::Err>> {
        if self.state != State::Rec {
            return Err(Rr3SpoolErr::State);
        }
        if self.len == 0 {
            self.state = State::Idle;
            return Err(Rr3SpoolErr::Empty);
        }
        let info = Rr3SegInfo {
            seq: self.seq,
            len: self.len,
            crc32: rr3_crc32_final(self.crc),
        };
        let raw = rr3_hdr_encode(&info);
        if let Err(e) = self.dev.rr3_write(0, &raw) {
            self.state = State::Idle;
            return Err(Rr3SpoolErr::Io(e));
        }
        let mut back = [0u8; RR3_SPOOL_HDR];
        if let Err(e) = self.dev.rr3_read(0, &mut back) {
            self.state = State::Idle;
            return Err(Rr3SpoolErr::Io(e));
        }
        if back != raw {
            self.state = State::Idle;
            return Err(Rr3SpoolErr::HdrMismatch);
        }
        self.state = State::Done;
        Ok(info)
    }

    /// 分块读载荷：`off` 相对载荷起点；返回实际读出字节数（0 = 正好到段尾）。
    pub fn rr3_read(&mut self, off: u32, buf: &mut [u8]) -> Result<usize, Rr3SpoolErr<D::Err>> {
        if self.state != State::Done {
            return Err(Rr3SpoolErr::State);
        }
        if off > self.len {
            return Err(Rr3SpoolErr::Range);
        }
        let rest = (self.len - off) as usize;
        let n = rest.min(buf.len());
        if n == 0 {
            return Ok(0);
        }
        self.dev
            .rr3_read(RR3_SPOOL_DATA + off as usize, &mut buf[..n])
            .map_err(Rr3SpoolErr::rr3_io)?;
        Ok(n)
    }

    /// 载荷 CRC 对账：用调用方缓冲顺序读全段续算，与头里的 crc 比对。
    ///
    /// 读侧边上传边对账时可直接复用 [`rr3_crc32_update`]；本方法用于本地自检。
    pub fn rr3_crc_check(&mut self, buf: &mut [u8]) -> Result<bool, Rr3SpoolErr<D::Err>> {
        if self.state != State::Done {
            return Err(Rr3SpoolErr::State);
        }
        if buf.is_empty() {
            return Err(Rr3SpoolErr::Range);
        }
        let mut crc = rr3_crc32_init();
        let mut off = 0u32;
        while off < self.len {
            let n = self.rr3_read(off, buf)?;
            if n == 0 {
                return Err(Rr3SpoolErr::Range);
            }
            crc = rr3_crc32_update(crc, &buf[..n]);
            off += n as u32;
        }
        Ok(rr3_crc32_final(crc) == self.crc)
    }

    /// 使记录失效：只擦头扇区（载荷留待下次 `begin` 惰性擦除）。
    pub fn rr3_clear(&mut self) -> Result<(), Rr3SpoolErr<D::Err>> {
        self.dev
            .rr3_erase(0, RR3_SPOOL_SECTOR)
            .map_err(Rr3SpoolErr::rr3_io)?;
        self.state = State::Idle;
        self.len = 0;
        self.erased = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
