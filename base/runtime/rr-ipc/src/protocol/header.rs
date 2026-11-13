//! A-IPC 128B 定长消息头（Layout C）常量与编解码。
//!
//! 偏移/长度必须与 `contracts/envelope.schema.json` 的 `x-offset-table` 完全一致，
//! 由本模块 `tests` 在 `cargo test` 时机器校验。

/// 消息头字节数（AOS-STD-IPC-001）
pub const RR3_IPC_HDR_SIZE: usize = 128;

/// 魔数字段值；LE 落盘后线上字节为 31 45 52 41（AOS-STD-IPC-020）
pub const MAGIC: u32 = 0x4152_4531;

/// payload_len 上限 64 MiB（AOS-STD-IPC-021）
pub const MAX_PAYLOAD_LEN: u32 = 64 * 1024 * 1024;

pub const OFF_MAGIC: usize = 0;
pub const OFF_OPCODE: usize = 4;
pub const OFF_FLAGS: usize = 6;
pub const OFF_TRACE_ID: usize = 8;
pub const OFF_TIMESTAMP_NS: usize = 16;
pub const OFF_SRC_TASK: usize = 24;
pub const OFF_DST_TASK: usize = 32;
pub const OFF_PAYLOAD_LEN: usize = 40;
/// 载荷 CRC32 偏移（预留区首 4 字节，2026-10-30 启用；见 [`rr3_crc32`]）。
pub const OFF_CRC32: usize = 44;
/// 预留区偏移（CRC 之后的剩余部分，仍必须全 0）。
pub const OFF_RESERVED: usize = 48;

pub const LEN_MAGIC: usize = 4;
pub const LEN_OPCODE: usize = 2;
pub const LEN_FLAGS: usize = 2;
pub const LEN_TRACE_ID: usize = 8;
pub const LEN_TIMESTAMP_NS: usize = 8;
pub const LEN_SRC_TASK: usize = 8;
pub const LEN_DST_TASK: usize = 8;
pub const LEN_PAYLOAD_LEN: usize = 4;
/// 载荷 CRC32 宽度。
pub const LEN_CRC32: usize = 4;
pub const LEN_RESERVED: usize = 80;

// ---------------------------------------------------------------------------
// IPC-T-003 编译期布局断言（ADR-0006 后果项）。
//
// 这些 `const _` 在 **const 求值期**校验：任意目标——含 `cargo check --target
// aarch64-unknown-linux-gnu` 交叉编译——只要编译通过，下列偏移连续性、头长合计、
// magic 小端线上字节与 payload 类型枚举值即已成立，不依赖 `cargo test` 有没有人跑。
// 它们与运行期守护（本模块 tests 与 tests/envelope_meta.rs 读契约 JSON 的那批）分工：
// 这里钉"布局自洽"，那里钉"与契约文件一致"；跨架构等值本身由 `tools/ci/ipct003.sh`
// 三方对拍（登记于 conventions.md §2，不进七步门）。
const _: () = assert!(OFF_MAGIC == 0 && LEN_MAGIC == 4, "magic 必须在偏移 0、宽 4");
const _: () = assert!(
    OFF_OPCODE == OFF_MAGIC + LEN_MAGIC,
    "opcode 偏移出现空洞/重叠"
);
const _: () = assert!(
    OFF_FLAGS == OFF_OPCODE + LEN_OPCODE,
    "flags 偏移出现空洞/重叠"
);
const _: () = assert!(
    OFF_TRACE_ID == OFF_FLAGS + LEN_FLAGS,
    "trace_id 偏移出现空洞/重叠"
);
const _: () = assert!(
    OFF_TIMESTAMP_NS == OFF_TRACE_ID + LEN_TRACE_ID,
    "timestamp_ns 偏移出现空洞/重叠"
);
const _: () = assert!(
    OFF_SRC_TASK == OFF_TIMESTAMP_NS + LEN_TIMESTAMP_NS,
    "src_task 偏移出现空洞/重叠"
);
const _: () = assert!(
    OFF_DST_TASK == OFF_SRC_TASK + LEN_SRC_TASK,
    "dst_task 偏移出现空洞/重叠"
);
const _: () = assert!(
    OFF_PAYLOAD_LEN == OFF_DST_TASK + LEN_DST_TASK,
    "payload_len 偏移出现空洞/重叠"
);
const _: () = assert!(
    OFF_CRC32 == OFF_PAYLOAD_LEN + LEN_PAYLOAD_LEN,
    "crc32 偏移出现空洞/重叠"
);
const _: () = assert!(
    OFF_RESERVED == OFF_CRC32 + LEN_CRC32,
    "reserved 偏移出现空洞/重叠"
);
const _: () = assert!(
    OFF_RESERVED + LEN_RESERVED == RR3_IPC_HDR_SIZE,
    "偏移合计必须恰为 128（Layout C）"
);
// magic 的 LE 线上字节 31 45 52 41（contracts/README.md 歧义裁定：采字段值 0x41524531 小端存储）
const _: () = assert!(
    MAGIC.to_le_bytes()[0] == 0x31
        && MAGIC.to_le_bytes()[1] == 0x45
        && MAGIC.to_le_bytes()[2] == 0x52
        && MAGIC.to_le_bytes()[3] == 0x41,
    "magic LE 线上字节必须为 31 45 52 41"
);
// payload 类型枚举值 L1 冻结（AOS-STD-IPC-005）；改任一值这里先于任何测试变红
const _: () = assert!(
    Rr3PayloadType::Request as u16 == 0x0001
        && Rr3PayloadType::Response as u16 == 0x0002
        && Rr3PayloadType::Event as u16 == 0x0003
        && Rr3PayloadType::Stream as u16 == 0x0004
        && Rr3PayloadType::Notification as u16 == 0x0005,
    "payload 类型枚举值冻结（L1），不得变更"
);

/// 传输层操作码（与 payload 类型分属不同层次）
pub mod opcode {
    pub const SEND: u16 = 0x0001;
    pub const RECV: u16 = 0x0002;
    pub const SEND_BATCH: u16 = 0x0003;
    pub const CANCEL: u16 = 0x0004;
}

/// 头 flags 位。标准定义的全部 8 位；rock3 采用 NOWAIT/SIGNAL/ZEROCOPY/URGENT/NOREPLY，
/// CAP_CARRY/COMPRESS/ENCRYPT 已定义但不使用（ADR-0006 备选否决项）。
pub mod flags {
    pub const NOWAIT: u16 = 0x0001;
    pub const SIGNAL: u16 = 0x0002;
    pub const ZEROCOPY: u16 = 0x0004;
    pub const CAP_CARRY: u16 = 0x0008;
    pub const COMPRESS: u16 = 0x0010;
    pub const ENCRYPT: u16 = 0x0020;
    pub const URGENT: u16 = 0x0040;
    pub const NOREPLY: u16 = 0x0080;

    /// 标准已定义位掩码（AOS-STD-IPC-004 的前向兼容约定）。
    ///
    /// 实际行为（v0.0.1 不做运行时校验，也不得据此声称已强制）：
    /// `to_bytes`/`from_bytes` 对 `flags` 是**原样透传**——掩码之外的位既不报错也不清零，
    /// 编解码两个方向都保留（见 `header_ignores_unknown_flags`）。标准对发送方"应只置已定义位"
    /// 的期望属调用方自律与 M2 传输层的职责，本 crate 不拦截；接收方忽略未知位正是 IPC-T-010。
    pub const DEFINED: u16 =
        NOWAIT | SIGNAL | ZEROCOPY | CAP_CARRY | COMPRESS | ENCRYPT | URGENT | NOREPLY;
}

/// 5 种 payload 类型，枚举值 L1 永久稳定（AOS-STD-IPC-005）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3PayloadType {
    /// RPC 调用（JSON-RPC 2.0 body）
    Request = 0x0001,
    /// RPC 响应
    Response = 0x0002,
    /// 事件总线，尽力投递
    Event = 0x0003,
    /// 有序流：音频 PCM / 关节状态（实时控制面）
    Stream = 0x0004,
    /// 心跳 / 系统通知
    Notification = 0x0005,
}

impl Rr3PayloadType {
    /// 由类型枚举值解析。payload 体的前缀宽度已在 M2 冻结为 2 字节（`protocol::body`，
    /// `RR3_BODY_TYPE_PREFIX_LEN`），本函数就是它把线上字节还原成枚举的唯一入口。
    pub fn from_u16(v: u16) -> Option<Self> {
        let known: [(u16, Self); 5] = [
            (0x0001, Self::Request),
            (0x0002, Self::Response),
            (0x0003, Self::Event),
            (0x0004, Self::Stream),
            (0x0005, Self::Notification),
        ];
        known.iter().find(|(k, _)| *k == v).map(|(_, t)| *t)
    }
}

/// RESPONSE.status —— AIRY_E* 子集（ADR-0006 §3）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3StatusCode {
    Ok = 0,
    Invalid = -2,
    Busy = -16,
    Permission = -22,
    TimedOut = -1004,
}

/// 头解析错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3HdrError {
    /// 字节数不足 128
    TooShort { got: usize },
    /// magic 字段值不符（IPC-T-002）
    BadMagic { got: u32 },
    /// reserved 区非全 0（ADR-0006：扩展必须经 flags 标记）
    ReservedNotZero,
    /// payload_len 超 64 MiB
    PayloadTooLarge { len: u32 },
}

/// 消息头视图（解析结果，不含 payload）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3Header {
    pub opcode: u16,
    pub flags: u16,
    pub trace_id: u64,
    pub timestamp_ns: u64,
    pub src_task: u64,
    pub dst_task: u64,
    pub payload_len: u32,
    /// 载荷 CRC32（`0xEDB88320` 反射多项式，初值/异或均为 `0xFFFF_FFFF`）。
    /// **必须与 payload 一致**——发送侧由 [`rr3_header_encode`] 自动写入，
    /// 接收侧在拿到 payload 后必须校验（[`rr3_crc32`]）。头部自身完整性由 magic /
    /// 保留区全零 / payload_len 护栏三重限定，故 CRC 只覆盖载荷。
    pub payload_crc32: u32,
}

/// 保留区必须全零（线上形状漂移的当场签名）。
fn rr3_reserved_zero(buf: &[u8]) -> bool {
    buf[OFF_RESERVED..OFF_RESERVED + LEN_RESERVED]
        .iter()
        .all(|&b| b == 0)
}

/// 头层前置判据：magic 对齐 + 保留区全零（形状漂移的当场签名）。
fn rr3_hdr_precheck(magic: u32, buf: &[u8]) -> Result<(), Rr3HdrError> {
    if magic != MAGIC {
        return Err(Rr3HdrError::BadMagic { got: magic });
    }
    if !rr3_reserved_zero(buf) {
        return Err(Rr3HdrError::ReservedNotZero);
    }
    Ok(())
}

/// 头层前置判据（长度 → magic → 保留区 → payload_len 上界），返回 payload_len。
fn rr3_hdr_guard(buf: &[u8]) -> Result<u32, Rr3HdrError> {
    if buf.len() < RR3_IPC_HDR_SIZE {
        return Err(Rr3HdrError::TooShort { got: buf.len() });
    }
    let u32le = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
    let magic = u32le(OFF_MAGIC);
    rr3_hdr_precheck(magic, buf)?;
    rr3_hdr_len_max(u32le(OFF_PAYLOAD_LEN))
}

/// 载荷长度上界（超过即拒，防自称长度撑爆上限）。
fn rr3_hdr_len_max(payload_len: u32) -> Result<u32, Rr3HdrError> {
    if payload_len > MAX_PAYLOAD_LEN {
        return Err(Rr3HdrError::PayloadTooLarge { len: payload_len });
    }
    Ok(payload_len)
}

impl Rr3Header {
    /// 广播目标（dst_task = 0，标准不为其设独立标志位）
    pub fn rr3_hdr_broadcast(opcode: u16, payload_len: u32) -> Self {
        Self {
            opcode,
            flags: 0,
            trace_id: 0,
            timestamp_ns: 0,
            src_task: 0,
            dst_task: 0,
            payload_len,
            payload_crc32: 0,
        }
    }

    /// 解析 128 字节头（小端）。IPC-T-001/002/010。**不校验 CRC**（此处的入参只有
    /// 头，CRC 覆盖载荷）——校验点在拿到 payload 的调用方（见 [`rr3_crc32`]）。
    pub fn from_bytes(buf: &[u8]) -> Result<Self, Rr3HdrError> {
        let payload_len = rr3_hdr_guard(buf)?;
        // 长度守卫已保证 buf.len() ≥ 128，任意常量偏移 + 宽度都不越界，
        // try_into 定长切片必然成功——unwrap 是守卫的直读推论，不是风险点。
        let u16le = |o: usize| u16::from_le_bytes(buf[o..o + 2].try_into().unwrap());
        let u32le = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
        let u64le = |o: usize| u64::from_le_bytes(buf[o..o + 8].try_into().unwrap());

        Ok(Self {
            opcode: u16le(OFF_OPCODE),
            // 未知 flags 位原样保留、不报错：AOS-STD-IPC-004 要求接收方忽略未知位
            flags: u16le(OFF_FLAGS),
            trace_id: u64le(OFF_TRACE_ID),
            timestamp_ns: u64le(OFF_TIMESTAMP_NS),
            src_task: u64le(OFF_SRC_TASK),
            dst_task: u64le(OFF_DST_TASK),
            payload_len,
            payload_crc32: u32le(OFF_CRC32),
        })
    }

    /// 编码为 128 字节（reserved 自动全 0）。const：`cargo check` 任意目标即求值，
    /// golden 对拍（本文件末尾 `const _`）因此在每个目标编译期成立——0.0.7 批B 第④路。
    ///
    /// 写出的 `payload_crc32` 取结构体字段值：**发送路径不得直接用本函数**（不知道
    /// 载荷就填不出 CRC），要用 [`rr3_header_encode`]。
    pub const fn to_bytes(&self) -> [u8; RR3_IPC_HDR_SIZE] {
        let mut buf = [0u8; RR3_IPC_HDR_SIZE];
        put_le(&mut buf, OFF_MAGIC, MAGIC as u64, LEN_MAGIC);
        put_le(&mut buf, OFF_OPCODE, self.opcode as u64, LEN_OPCODE);
        put_le(&mut buf, OFF_FLAGS, self.flags as u64, LEN_FLAGS);
        put_le(&mut buf, OFF_TRACE_ID, self.trace_id, LEN_TRACE_ID);
        put_le(
            &mut buf,
            OFF_TIMESTAMP_NS,
            self.timestamp_ns,
            LEN_TIMESTAMP_NS,
        );
        put_le(&mut buf, OFF_SRC_TASK, self.src_task, LEN_SRC_TASK);
        put_le(&mut buf, OFF_DST_TASK, self.dst_task, LEN_DST_TASK);
        put_le(
            &mut buf,
            OFF_PAYLOAD_LEN,
            self.payload_len as u64,
            LEN_PAYLOAD_LEN,
        );
        put_le(&mut buf, OFF_CRC32, self.payload_crc32 as u64, LEN_CRC32);
        buf
    }
}

/// 载荷 CRC32（IEEE 802.3，反射多项式 `0xEDB88320`，初值与结果异或 `0xFFFF_FFFF`）。
///
/// 逐位实现而非查表：板侧 16KB TX 环与 8KB 级任务栈下，表驱动要多占 1KB `.rodata`
/// 与一次查表访存；11KB 分片的逐位计算在 240MHz 上是数百微秒级，代价可接受，换来
/// 零表、零依赖、const 可求值（测试里可直接在编译期钉向量）。
pub const fn rr3_crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0;
    while i < bytes.len() {
        crc = rr3_crc_byte(crc, bytes[i]);
        i += 1;
    }
    !crc
}

/// 单字节推进（8 次反射移位，IEEE 802.3 多项式）。
const fn rr3_crc_byte(mut crc: u32, b: u8) -> u32 {
    crc ^= b as u32;
    let mut k = 0;
    while k < 8 {
        crc = if crc & 1 != 0 {
            (crc >> 1) ^ 0xEDB8_8320
        } else {
            crc >> 1
        };
        k += 1;
    }
    crc
}

/// 编出带正确 CRC 的 128B 头（**发送路径的唯一入口**）：CRC 覆盖整段 payload。
///
/// 头里的 `payload_len` 与载荷长度不一致时 CRC 仍会算出来，但长度门（
/// [`Rr3FrameError::LengthMismatch`](crate::protocol::wire::Rr3FrameError::LengthMismatch)）
/// 会先拒绝——两层各自独立，不互相代偿。
pub fn rr3_header_encode(hdr: &Rr3Header, payload: &[u8]) -> [u8; RR3_IPC_HDR_SIZE] {
    let mut h = *hdr;
    h.payload_crc32 = rr3_crc32(payload);
    h.to_bytes()
}

/// 小端写 `len` 字节进 `buf[off..]`（const fn：不依赖 `copy_from_slice` 的 const 稳定版本，
/// MSRV 面保持干净）。
const fn put_le(buf: &mut [u8], off: usize, v: u64, len: usize) {
    let mut i = 0;
    while i < len {
        buf[off + i] = (v >> (8 * i)) as u8;
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// IPC-T-003 第④路（0.0.7 批B）：golden 头编译期对拍。
//
// 编码器 `to_bytes` 已 const 化：`cargo +esp check`（xtensa 裸机 none-elf 目标，
// 具体写法见 tools/ci/rr-core-xtensa-check.sh——机制核源码不写板级目标名，批E 零策略门）
// 等**任意目标**编译 rr-ipc 时，下面的 `const _` 都会在该目标的编译器里重新求值——
// 求值通过即"该目标编译出的编码器产出与 golden 文件逐字节一致"，无需运行任何代码。
// 运行期 dump 对拍（x86 / aarch64 / arm64 容器）由 `tools/ci/ipct003.sh` 承担，
// xtensa 真机运行期 dump 归批C UART 夹具（同一判据的第二重证据）。
// golden 路径以 CARGO_MANIFEST_DIR 构建期注入（基-32：契约根不靠手写跨仓层数，
// 目录动一层不再炸机制核）——"路径无关"须注入后才成立（裁-12）。
const GOLDEN_HEADER_HEX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../contracts/golden/event-header-128.hex"
));

/// golden 文件前 256 hex 字符（文件 257 字节含尾换行，运行期测试另有全文校验）。
const fn rr3_golden_hdr_hex() -> [u8; 256] {
    let raw: &[u8] = GOLDEN_HEADER_HEX.as_bytes();
    let mut out = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        out[i] = raw[i];
        i += 1;
    }
    out
}

/// 与 `ipct003_dump` 的 `header` 行同一条头（payload_len=96）。
const fn golden_header_bytes() -> [u8; 256] {
    let hdr = Rr3Header {
        opcode: opcode::SEND,
        flags: 0,
        trace_id: 0x1234_5678,
        timestamp_ns: 100,
        src_task: 7,
        dst_task: 0,
        payload_len: 96,
        // golden 头是**无载荷**的纯头（CRC 覆盖载荷，此处填 0 与 golden 文件一致）
        payload_crc32: 0,
    };
    let bytes = hdr.to_bytes();
    let mut out = [0u8; 256];
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut i = 0;
    while i < bytes.len() {
        out[i * 2] = HEX[(bytes[i] >> 4) as usize];
        out[i * 2 + 1] = HEX[(bytes[i] & 0x0f) as usize];
        i += 1;
    }
    out
}

/// const 可用的逐字节相等（数组 `==` 在 const 求值里是 nightly 面，手写保持稳定版可编译）。
const fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

const _: () = assert!(
    bytes_eq(&golden_header_bytes(), &rr3_golden_hdr_hex()),
    "rr-ipc 编码器输出与 contracts/golden/event-header-128.hex 不一致——契约字节冻结被破坏（改动编码器或头布局必须同步重出 golden）"
);

#[cfg(test)]
mod tests;
