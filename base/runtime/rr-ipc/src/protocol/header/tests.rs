//! header 的单测（自同名单测模块原样抽出，判据不变）。

use super::*;
// no_std 显式化（0.0.7 批B）：std prelude 不再随 `#![no_std]` 隐式注入
use std::string::{String, ToString};
use std::vec::Vec;

fn schema_json() -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../contracts/envelope.schema.json"
    );
    let raw =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读不到契约文件 {path}: {e}"));
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
    assert_eq!(
        &bytes[0..4],
        &[0x31, 0x45, 0x52, 0x41],
        "LE 落盘字节应为 31 45 52 41"
    );
    // 标准 §7.2 注释/'ARE1' 那套字节，在 rock3 裁定下必须被拒
    bytes[0..4].copy_from_slice(&[0x41, 0x52, 0x45, 0x31]);
    assert_eq!(
        Rr3Header::from_bytes(&bytes),
        Err(Rr3HdrError::BadMagic { got: 0x3145_5241 })
    );
}

/// 与 golden 向量逐字节一致，且 decode→encode 往返无损。
/// 这是**同机**字节基准（IPC-T-001 侧）；IPC-T-003 要求的 x86↔aarch64 交叉编译等值
/// 由契约外对拍脚本 `tools/ci/ipct003.sh` 承载（CI ipct003 job `--strict` 四路全绿，
/// 2026-10-11 起 IPC-T-003 已移入 `contracts/envelope.schema.json` 的 coverage）。
#[test]
fn header_matches_golden_vector() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../contracts/golden/event-header-128.hex"
    );
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
    assert_eq!(
        hdr.to_bytes().to_vec(),
        golden,
        "往返编码与 golden 向量不一致"
    );
}

/// reserved 非全 0 必须拒绝（ADR-0006：扩展只允许写 reserved 且需 flags 标记）
#[test]
fn header_rejects_dirty_reserved() {
    let mut bytes = Rr3Header::rr3_hdr_broadcast(opcode::SEND, 0).to_bytes();
    assert_eq!(bytes[OFF_RESERVED..].len(), LEN_RESERVED);
    bytes[OFF_RESERVED + LEN_RESERVED - 1] = 0xFF;
    assert_eq!(
        Rr3Header::from_bytes(&bytes),
        Err(Rr3HdrError::ReservedNotZero)
    );
}

/// CRC32 已知向量（编译期钉死实现，防多项式/反射写错）：`"123456789"` → 0xCBF43926。
#[test]
fn crc32_known_vectors() {
    assert_eq!(rr3_crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(rr3_crc32(b""), 0);
    // 头字段 CRC 可独立序列化/还原（CRC 只覆盖载荷，头自身不参与）
    let mut h = Rr3Header::rr3_hdr_broadcast(opcode::SEND, 4);
    h.payload_crc32 = 0xDEAD_BEEF;
    let b = h.to_bytes();
    assert_eq!(
        u32::from_le_bytes(b[OFF_CRC32..OFF_CRC32 + 4].try_into().unwrap()),
        0xDEAD_BEEF
    );
    assert_eq!(
        Rr3Header::from_bytes(&b).unwrap().payload_crc32,
        0xDEAD_BEEF
    );
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
        Err(Rr3HdrError::PayloadTooLarge {
            len: MAX_PAYLOAD_LEN + 1
        })
    );
}

#[test]
fn header_too_short() {
    assert_eq!(
        Rr3Header::from_bytes(&[0u8; 127]),
        Err(Rr3HdrError::TooShort { got: 127 })
    );
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
    let wire = magic["wire_bytes_hex"]
        .as_str()
        .expect("缺 magic wire_bytes_hex");
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
        assert!(
            rust.contains_key(name.as_str()),
            "契约定义了 {name}，Rust flags 缺"
        );
    }
    for name in rust.keys() {
        assert!(
            contract.contains_key(*name),
            "Rust 定义了 {name}，契约 flags 缺"
        );
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
        assert!(
            rust.contains_key(name.as_str()),
            "契约定义了 {name}，Rust opcode 缺"
        );
    }
    for name in rust.keys() {
        assert!(
            contract.contains_key(*name),
            "Rust 定义了 {name}，契约 opcode 缺"
        );
    }
    for (name, c_val) in &contract {
        assert_eq!(
            rust[name.as_str()] as i64,
            *c_val,
            "opcode {name} 与契约漂移"
        );
    }
}

/// 传输路径与环境键逐字对齐 ADR-0006 §6 / naming.md
#[test]
fn transport_path_matches_adr() {
    assert_eq!(
        crate::transport::RR3_UDS_PATH_DEFAULT,
        "/var/run/rock3-ipc.sock"
    );
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
