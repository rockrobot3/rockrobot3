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
pub const OFF_RESERVED: usize = 44;

pub const LEN_MAGIC: usize = 4;
pub const LEN_OPCODE: usize = 2;
pub const LEN_FLAGS: usize = 2;
pub const LEN_TRACE_ID: usize = 8;
pub const LEN_TIMESTAMP_NS: usize = 8;
pub const LEN_SRC_TASK: usize = 8;
pub const LEN_DST_TASK: usize = 8;
pub const LEN_PAYLOAD_LEN: usize = 4;
pub const LEN_RESERVED: usize = 84;

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
const _: () = assert!(OFF_OPCODE == OFF_MAGIC + LEN_MAGIC, "opcode 偏移出现空洞/重叠");
const _: () = assert!(OFF_FLAGS == OFF_OPCODE + LEN_OPCODE, "flags 偏移出现空洞/重叠");
const _: () = assert!(OFF_TRACE_ID == OFF_FLAGS + LEN_FLAGS, "trace_id 偏移出现空洞/重叠");
const _: () = assert!(OFF_TIMESTAMP_NS == OFF_TRACE_ID + LEN_TRACE_ID, "timestamp_ns 偏移出现空洞/重叠");
const _: () = assert!(OFF_SRC_TASK == OFF_TIMESTAMP_NS + LEN_TIMESTAMP_NS, "src_task 偏移出现空洞/重叠");
const _: () = assert!(OFF_DST_TASK == OFF_SRC_TASK + LEN_SRC_TASK, "dst_task 偏移出现空洞/重叠");
const _: () = assert!(OFF_PAYLOAD_LEN == OFF_DST_TASK + LEN_DST_TASK, "payload_len 偏移出现空洞/重叠");
const _: () = assert!(OFF_RESERVED == OFF_PAYLOAD_LEN + LEN_PAYLOAD_LEN, "reserved 偏移出现空洞/重叠");
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
    pub const DEFINED: u16 = NOWAIT
        | SIGNAL
        | ZEROCOPY
        | CAP_CARRY
        | COMPRESS
        | ENCRYPT
        | URGENT
        | NOREPLY;
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
        match v {
            0x0001 => Some(Self::Request),
            0x0002 => Some(Self::Response),
            0x0003 => Some(Self::Event),
            0x0004 => Some(Self::Stream),
            0x0005 => Some(Self::Notification),
            _ => None,
        }
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
        }
    }

    /// 解析 128 字节头（小端）。IPC-T-001/002/010。
    pub fn from_bytes(buf: &[u8]) -> Result<Self, Rr3HdrError> {
        if buf.len() < RR3_IPC_HDR_SIZE {
            return Err(Rr3HdrError::TooShort { got: buf.len() });
        }
        let u16le = |o: usize| u16::from_le_bytes(buf[o..o + 2].try_into().unwrap());
        let u32le = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
        let u64le = |o: usize| u64::from_le_bytes(buf[o..o + 8].try_into().unwrap());

        let magic = u32le(OFF_MAGIC);
        if magic != MAGIC {
            return Err(Rr3HdrError::BadMagic { got: magic });
        }
        if buf[OFF_RESERVED..OFF_RESERVED + LEN_RESERVED]
            .iter()
            .any(|&b| b != 0)
        {
            return Err(Rr3HdrError::ReservedNotZero);
        }
        let payload_len = u32le(OFF_PAYLOAD_LEN);
        if payload_len > MAX_PAYLOAD_LEN {
            return Err(Rr3HdrError::PayloadTooLarge { len: payload_len });
        }
        Ok(Self {
            opcode: u16le(OFF_OPCODE),
            // 未知 flags 位原样保留、不报错：AOS-STD-IPC-004 要求接收方忽略未知位
            flags: u16le(OFF_FLAGS),
            trace_id: u64le(OFF_TRACE_ID),
            timestamp_ns: u64le(OFF_TIMESTAMP_NS),
            src_task: u64le(OFF_SRC_TASK),
            dst_task: u64le(OFF_DST_TASK),
            payload_len,
        })
    }

    /// 编码为 128 字节（reserved 自动全 0）。
    pub fn to_bytes(&self) -> [u8; RR3_IPC_HDR_SIZE] {
        let mut buf = [0u8; RR3_IPC_HDR_SIZE];
        buf[OFF_MAGIC..OFF_MAGIC + LEN_MAGIC].copy_from_slice(&MAGIC.to_le_bytes());
        buf[OFF_OPCODE..OFF_OPCODE + LEN_OPCODE].copy_from_slice(&self.opcode.to_le_bytes());
        buf[OFF_FLAGS..OFF_FLAGS + LEN_FLAGS].copy_from_slice(&self.flags.to_le_bytes());
        buf[OFF_TRACE_ID..OFF_TRACE_ID + LEN_TRACE_ID].copy_from_slice(&self.trace_id.to_le_bytes());
        buf[OFF_TIMESTAMP_NS..OFF_TIMESTAMP_NS + LEN_TIMESTAMP_NS]
            .copy_from_slice(&self.timestamp_ns.to_le_bytes());
        buf[OFF_SRC_TASK..OFF_SRC_TASK + LEN_SRC_TASK].copy_from_slice(&self.src_task.to_le_bytes());
        buf[OFF_DST_TASK..OFF_DST_TASK + LEN_DST_TASK].copy_from_slice(&self.dst_task.to_le_bytes());
        buf[OFF_PAYLOAD_LEN..OFF_PAYLOAD_LEN + LEN_PAYLOAD_LEN]
            .copy_from_slice(&self.payload_len.to_le_bytes());
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_json() -> serde_json::Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../contracts/envelope.schema.json");
        let raw = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("读不到契约文件 {path}: {e}"));
        serde_json::from_str(&raw).expect("envelope.schema.json 必须是合法 JSON")
    }

    fn offset_table() -> Vec<(String, usize, usize)> {
        schema_json()["properties"]["header"]["properties"]["fields"]["x-offset-table"]
            .as_array()
            .expect("缺 x-offset-table")
            .iter()
            .map(|f| {
                (
                    f["name"].as_str().unwrap().to_string(),
                    f["offset"].as_u64().unwrap() as usize,
                    f["length"].as_u64().unwrap() as usize,
                )
            })
            .collect()
    }

    fn contract_off_len(name: &str) -> (usize, usize) {
        // 契约三元组 (name, offset, length)，调用方只需后两项
        let (_n, off, len) = offset_table()
            .into_iter()
            .find(|f| f.0 == name)
            .unwrap_or_else(|| panic!("契约里缺字段 {name}"));
        (off, len)
    }

    /// IPC-T-001：Rust 偏移常量逐字段等于契约，偏移连续无空洞，长度合计 128
    #[test]
    fn header_offsets_match_contract() {
        let rust = [
            ("magic", OFF_MAGIC, LEN_MAGIC),
            ("opcode", OFF_OPCODE, LEN_OPCODE),
            ("flags", OFF_FLAGS, LEN_FLAGS),
            ("trace_id", OFF_TRACE_ID, LEN_TRACE_ID),
            ("timestamp_ns", OFF_TIMESTAMP_NS, LEN_TIMESTAMP_NS),
            ("src_task", OFF_SRC_TASK, LEN_SRC_TASK),
            ("dst_task", OFF_DST_TASK, LEN_DST_TASK),
            ("payload_len", OFF_PAYLOAD_LEN, LEN_PAYLOAD_LEN),
            ("reserved", OFF_RESERVED, LEN_RESERVED),
        ];
        for (name, off, len) in rust {
            let (c_off, c_len) = contract_off_len(name);
            assert_eq!((off, len), (c_off, c_len), "字段 {name} 与契约漂移");
        }
        let tbl = offset_table();
        assert_eq!(tbl.iter().map(|f| f.2).sum::<usize>(), RR3_IPC_HDR_SIZE);
        let mut expect_off = 0usize;
        for (name, off, len) in &tbl {
            assert_eq!(*off, expect_off, "契约偏移表在 {name} 处有空洞或重叠");
            expect_off += len;
        }
    }

    /// IPC-T-002：magic 不符必须被拒（含标准注释那套字节）
    #[test]
    fn header_rejects_bad_magic() {
        let hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, 96);
        let mut bytes = hdr.to_bytes();
        assert_eq!(&bytes[0..4], &[0x31, 0x45, 0x52, 0x41], "LE 落盘字节应为 31 45 52 41");
        // 标准 §7.2 注释/'ARE1' 那套字节，在 rock3 裁定下必须被拒
        bytes[0..4].copy_from_slice(&[0x41, 0x52, 0x45, 0x31]);
        assert_eq!(Rr3Header::from_bytes(&bytes), Err(Rr3HdrError::BadMagic { got: 0x3145_5241 }));
    }

    /// 与 golden 向量逐字节一致，且 decode→encode 往返无损。
    /// 这是**同机**字节基准（IPC-T-001 侧）；IPC-T-003 要求的 x86↔aarch64 交叉编译等值
    /// 测试当前 HEAD 没有，已记在 `contracts/envelope.schema.json` 的
    /// `x-rock3-v0.0.1-uncovered`，随 M2 落地。
    #[test]
    fn header_matches_golden_vector() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../contracts/golden/event-header-128.hex");
        let text = std::fs::read_to_string(path).expect("读不到 golden hex");
        let golden = hex_to_bytes(text.trim());
        assert_eq!(golden.len(), RR3_IPC_HDR_SIZE);

        let hdr = Rr3Header::from_bytes(&golden).expect("golden 向量应能解析");
        assert_eq!(hdr.opcode, opcode::SEND);
        assert_eq!(hdr.flags, 0);
        assert_eq!(hdr.trace_id, 0x1234_5678);
        assert_eq!(hdr.timestamp_ns, 100);
        assert_eq!(hdr.src_task, 7);
        assert_eq!(hdr.dst_task, 0);
        assert_eq!(hdr.payload_len, 96);
        assert_eq!(hdr.to_bytes().to_vec(), golden, "往返编码与 golden 向量不一致");
    }

    /// reserved 非全 0 必须拒绝（ADR-0006：扩展只允许写 reserved 且需 flags 标记）
    #[test]
    fn header_rejects_dirty_reserved() {
        let mut bytes = Rr3Header::rr3_hdr_broadcast(opcode::SEND, 0).to_bytes();
        assert_eq!(bytes[OFF_RESERVED..].len(), 84);
        bytes[OFF_RESERVED + 83] = 0xFF;
        assert_eq!(Rr3Header::from_bytes(&bytes), Err(Rr3HdrError::ReservedNotZero));
    }

    /// IPC-T-010：未知 flags 位不得报错（前向兼容）；已定义位组合正常解析
    #[test]
    fn header_ignores_unknown_flags() {
        let base = Rr3Header::rr3_hdr_broadcast(opcode::SEND, 0);
        for f in [
            flags::NOWAIT,
            flags::ZEROCOPY,
            flags::URGENT,
            flags::NOWAIT | flags::URGENT,
            flags::DEFINED,
        ] {
            let mut h = base;
            h.flags = f;
            let got = Rr3Header::from_bytes(&h.to_bytes()).expect("已定义 flags 应可解析");
            assert_eq!(got.flags, f);
        }
        let mut h = base;
        h.flags = 0x0100; // 标准未定义位：发送方禁止，接收方必须忽略
        assert_eq!(Rr3Header::from_bytes(&h.to_bytes()).unwrap().flags, 0x0100);
    }

    /// payload_len 超 64 MiB 必须拒绝（AOS-STD-IPC-021）
    #[test]
    fn header_rejects_oversize_payload_len() {
        let mut h = Rr3Header::rr3_hdr_broadcast(opcode::SEND, 0);
        h.payload_len = MAX_PAYLOAD_LEN;
        assert!(Rr3Header::from_bytes(&h.to_bytes()).is_ok());
        h.payload_len = MAX_PAYLOAD_LEN + 1;
        assert_eq!(
            Rr3Header::from_bytes(&h.to_bytes()),
            Err(Rr3HdrError::PayloadTooLarge { len: MAX_PAYLOAD_LEN + 1 })
        );
    }

    #[test]
    fn header_too_short() {
        assert_eq!(Rr3Header::from_bytes(&[0u8; 127]), Err(Rr3HdrError::TooShort { got: 127 }));
    }

    /// payload 类型枚举值 L1 冻结：改任一值该测试即红
    #[test]
    fn payload_type_values_are_frozen() {
        assert_eq!(Rr3PayloadType::Request as u16, 0x0001);
        assert_eq!(Rr3PayloadType::Response as u16, 0x0002);
        assert_eq!(Rr3PayloadType::Event as u16, 0x0003);
        assert_eq!(Rr3PayloadType::Stream as u16, 0x0004);
        assert_eq!(Rr3PayloadType::Notification as u16, 0x0005);
        assert_eq!(Rr3PayloadType::from_u16(0x0006), None);

        let pt = &schema_json()["properties"]["payload_type"];
        assert_eq!(pt["REQUEST"].as_i64().unwrap(), 1);
        assert_eq!(pt["RESPONSE"].as_i64().unwrap(), 2);
        assert_eq!(pt["EVENT"].as_i64().unwrap(), 3);
        assert_eq!(pt["STREAM"].as_i64().unwrap(), 4);
        assert_eq!(pt["NOTIFICATION"].as_i64().unwrap(), 5);
    }

    /// 错误码子集与契约逐值一致（ADR-0006 §3）
    #[test]
    fn status_codes_match_contract() {
        let ec = &schema_json()["properties"]["error_codes"];
        let pairs = [
            (Rr3StatusCode::Ok as i32, "AIRY_EOK"),
            (Rr3StatusCode::Invalid as i32, "AIRY_EINVAL"),
            (Rr3StatusCode::Busy as i32, "AIRY_EBUSY"),
            (Rr3StatusCode::Permission as i32, "AIRY_EPERM"),
            (Rr3StatusCode::TimedOut as i32, "AIRY_ETIMEDOUT"),
        ];
        for (rust_val, name) in pairs {
            assert_eq!(rust_val, ec[name].as_i64().unwrap() as i32, "{name} 值漂移");
        }
    }

    /// 契约节点里所有整数值成员（flags / opcode 的位-值表；字符串、数组、对象条目
    /// 如 x-layer-note / x-rock3-adopted 天然被过滤掉）
    fn contract_int_entries(obj: &serde_json::Value) -> std::collections::BTreeMap<String, i64> {
        obj.as_object()
            .expect("契约该节点必须是对象")
            .iter()
            .filter_map(|(k, v)| v.as_i64().map(|n| (k.clone(), n)))
            .collect()
    }

    /// magic 是本仓最 contested 字段：契约 `x-offset-table` 的 magic 条目
    /// value / hex / wire_bytes_hex 三种表示必须全部钉在 Rust `MAGIC` 常量上（ADR-0006）
    #[test]
    fn magic_contract_entries_match_rust() {
        let json = schema_json();
        let tbl = json["properties"]["header"]["properties"]["fields"]["x-offset-table"]
            .as_array()
            .expect("缺 x-offset-table");
        let magic = tbl
            .iter()
            .find(|f| f["name"].as_str() == Some("magic"))
            .expect("契约偏移表缺 magic 条目");
        assert_eq!(
            magic["value"].as_u64().expect("缺 magic value"),
            MAGIC as u64,
            "magic.value 与 Rust MAGIC 漂移"
        );
        let hex = magic["hex"].as_str().expect("缺 magic hex");
        assert_eq!(
            u32::from_str_radix(hex.trim_start_matches("0x"), 16)
                .unwrap_or_else(|e| panic!("magic.hex {hex} 非法: {e}")),
            MAGIC,
            "magic.hex 解析后与 Rust MAGIC 漂移"
        );
        let wire = magic["wire_bytes_hex"].as_str().expect("缺 magic wire_bytes_hex");
        assert_eq!(
            hex_to_bytes(wire),
            MAGIC.to_le_bytes().to_vec(),
            "magic.wire_bytes_hex 与 MAGIC 的 LE 落盘字节不一致"
        );
    }

    /// 契约 `properties.flags` 与 Rust `flags::` 双向逐值一致（值漂移 / 缺位 / 多位都算红），
    /// 且 DEFINED 掩码 == 契约位集按位或
    #[test]
    fn flags_match_contract() {
        let rust: std::collections::BTreeMap<&str, u16> = [
            ("AIRY_IPC_F_NOWAIT", flags::NOWAIT),
            ("AIRY_IPC_F_SIGNAL", flags::SIGNAL),
            ("AIRY_IPC_F_ZEROCOPY", flags::ZEROCOPY),
            ("AIRY_IPC_F_CAP_CARRY", flags::CAP_CARRY),
            ("AIRY_IPC_F_COMPRESS", flags::COMPRESS),
            ("AIRY_IPC_F_ENCRYPT", flags::ENCRYPT),
            ("AIRY_IPC_F_URGENT", flags::URGENT),
            ("AIRY_IPC_F_NOREPLY", flags::NOREPLY),
        ]
        .into_iter()
        .collect();
        let contract = contract_int_entries(&schema_json()["properties"]["flags"]);
        for name in contract.keys() {
            assert!(rust.contains_key(name.as_str()), "契约定义了 {name}，Rust flags 缺");
        }
        for name in rust.keys() {
            assert!(contract.contains_key(*name), "Rust 定义了 {name}，契约 flags 缺");
        }
        let mut mask = 0u16;
        for (name, c_val) in &contract {
            let r = rust[name.as_str()];
            assert_eq!(r as i64, *c_val, "{name} 值漂移");
            mask |= r;
        }
        assert_eq!(flags::DEFINED, mask, "DEFINED 掩码与契约位集合不一致");
    }

    /// 契约 `properties.opcode` 与 Rust `opcode::` 双向逐值一致（同 flags 的两向集合检查）
    #[test]
    fn opcode_values_match_contract() {
        let rust: std::collections::BTreeMap<&str, u16> = [
            ("SEND", opcode::SEND),
            ("RECV", opcode::RECV),
            ("SEND_BATCH", opcode::SEND_BATCH),
            ("CANCEL", opcode::CANCEL),
        ]
        .into_iter()
        .collect();
        let contract = contract_int_entries(&schema_json()["properties"]["opcode"]);
        for name in contract.keys() {
            assert!(rust.contains_key(name.as_str()), "契约定义了 {name}，Rust opcode 缺");
        }
        for name in rust.keys() {
            assert!(contract.contains_key(*name), "Rust 定义了 {name}，契约 opcode 缺");
        }
        for (name, c_val) in &contract {
            assert_eq!(
                rust[name.as_str()] as i64, *c_val,
                "opcode {name} 与契约漂移"
            );
        }
    }

    /// 传输路径与环境键逐字对齐 ADR-0006 §6 / naming.md
    #[test]
    fn transport_path_matches_adr() {
        assert_eq!(crate::transport::RR3_UDS_PATH_DEFAULT, "/var/run/rock3-ipc.sock");
        assert_eq!(crate::transport::RR3_BUS_PATH_ENV, "RR3_BUS_PATH");
        let t = &schema_json()["properties"]["transport"];
        assert_eq!(t["x-uds-path"].as_str().unwrap(), "/var/run/rock3-ipc.sock");
        assert_eq!(t["x-env-key"].as_str().unwrap(), "RR3_BUS_PATH");
    }

    fn hex_to_bytes(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("非法 hex"))
            .collect()
    }
}
