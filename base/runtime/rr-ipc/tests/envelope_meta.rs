//! 契约**元数据**守护（最终整枝评审 Group A / C1 与 I3）。
//!
//! 这里每条断言读的都不是 Rust 常量自己对自己，而是 `contracts/envelope.schema.json`
//! 里的键——评审证明的四个"文档里写着、门禁里没人管"的值：
//!
//! - `properties.header.x-byte-size`：头条真相"128B 消息头"。必须同时等于
//!   Rust `RR3_IPC_HDR_SIZE`、`x-offset-table` 的 length 合计、`contracts/golden/
//!   event-header-128.hex` 的解码字节数。
//! - `…fields.x-offset-sum`：字符串形态的**第二份记账**，必须与自身加数、与偏移表、
//!   与 128 三处一致——两处记账不许各说各话。
//! - `…payload_len.minimum|maximum`：`MAX_PAYLOAD_LEN`（`src/protocol/header.rs`）实现的
//!   就是这两个契约值，故必须由契约侧钉，而不是 Rust 比 Rust。
//! - 顶层 `x-schema-version`：`RR3_CONTRACT_SCHEMA_VERSION`（rrd 横幅的
//!   "contracts schema v1"）此前无物断言，现与 JSON 逐值对齐。
//!
//! 路径深度与 `src/protocol/header.rs` 同一写法：`CARGO_MANIFEST_DIR` 是
//! `base/runtime/rr-ipc`，回到仓库根要 **三级** `../../../`，不是两级。
//! golden hex 文件以单个 LF 结尾，读入后必须 `.trim()` 再解码。

use rr_ipc::{
    opcode, Rr3HdrError, Rr3Header, MAX_PAYLOAD_LEN, RR3_CONTRACT_SCHEMA_VERSION, RR3_IPC_HDR_SIZE,
};

fn schema_json() -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../contracts/envelope.schema.json"
    );
    let raw =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读不到契约文件 {path}: {e}"));
    serde_json::from_str(&raw).expect("envelope.schema.json 必须是合法 JSON")
}

/// 契约偏移表的 (name, offset, length) 三元组
fn offset_table(json: &serde_json::Value) -> Vec<(String, usize, usize)> {
    json["properties"]["header"]["properties"]["fields"]["x-offset-table"]
        .as_array()
        .expect("缺 x-offset-table")
        .iter()
        .map(|f| {
            (
                f["name"].as_str().unwrap_or_default().to_string(),
                f["offset"].as_u64().unwrap_or(u64::MAX) as usize,
                f["length"].as_u64().unwrap_or(u64::MAX) as usize,
            )
        })
        .collect()
}

/// 偏移表里某个字段的完整 JSON 条目（minimum/maximum 这类约束挂在字段上）
fn contract_field<'a>(json: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    json["properties"]["header"]["properties"]["fields"]["x-offset-table"]
        .as_array()
        .expect("缺 x-offset-table")
        .iter()
        .find(|f| f["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("契约偏移表缺字段 {name}"))
}

fn golden_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../contracts/golden/event-header-128.hex"
    );
    let text = std::fs::read_to_string(path).expect("读不到 golden hex 向量");
    hex_to_bytes(text.trim())
}

fn hex_to_bytes(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16).unwrap_or_else(|e| panic!("非法 hex {s:?}: {e}"))
        })
        .collect()
}

/// C1①：`x-byte-size` 是三份真相的交汇处（Rust 常量 / 偏移表合计 / golden 实际字节数）
#[test]
fn contract_byte_size_matches_rust_offsets_and_golden() {
    let json = schema_json();
    let byte_size = json["properties"]["header"]["x-byte-size"]
        .as_u64()
        .expect("契约缺 properties.header.x-byte-size");

    assert_eq!(
        byte_size as usize, RR3_IPC_HDR_SIZE,
        "契约 x-byte-size({byte_size}) 与 Rust RR3_IPC_HDR_SIZE({RR3_IPC_HDR_SIZE}) 漂移"
    );

    let table_sum: usize = offset_table(&json).iter().map(|f| f.2).sum();
    assert_eq!(
        byte_size as usize, table_sum,
        "契约 x-byte-size({byte_size}) 与 x-offset-table 的 length 合计({table_sum}) 漂移"
    );

    let golden = golden_bytes();
    assert_eq!(
        byte_size as usize,
        golden.len(),
        "契约 x-byte-size({byte_size}) 与 contracts/golden/event-header-128.hex 的解码长度({}) 漂移",
        golden.len()
    );
    // 只比长度还不够：golden 必须真按这个尺寸解析得动、且能被 Rust 逐字节复现
    let hdr = Rr3Header::from_bytes(&golden).expect("golden 向量必须能按契约尺寸解析");
    assert_eq!(
        hdr.to_bytes().to_vec(),
        golden,
        "Rust 编码必须逐字节复现 golden 向量"
    );
}

/// C1②：`x-offset-sum` 与偏移表是**两处独立记账**，三者（自身加数 / 自身结果 / 偏移表）
/// 必须互相咬合，且结果就是 Rust 头长度 128——任何一处被单独改动都该红
#[test]
fn contract_offset_sum_agrees_with_offset_table_and_rust() {
    let json = schema_json();
    let raw = json["properties"]["header"]["properties"]["fields"]["x-offset-sum"]
        .as_str()
        .expect("契约缺 x-offset-sum（应为 \"4+2+… = 128\" 字符串）");
    let (lhs, rhs) = raw
        .split_once('=')
        .unwrap_or_else(|| panic!("x-offset-sum 必须是 \"a+b+… = N\" 形式，实际 {raw:?}"));
    let stated: usize = rhs
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("x-offset-sum 的结果 {rhs:?} 不是数字: {e}"));
    let addends: Vec<usize> = lhs
        .split('+')
        .map(|t| {
            t.trim()
                .parse()
                .unwrap_or_else(|e| panic!("x-offset-sum 的加数 {t:?} 不是数字: {e}"))
        })
        .collect();

    assert_eq!(
        stated, RR3_IPC_HDR_SIZE,
        "x-offset-sum 的结果必须是 Rust 头长度 {RR3_IPC_HDR_SIZE}，实际 {stated}（原文 {raw:?}）"
    );
    let recomputed: usize = addends.iter().sum();
    assert_eq!(
        recomputed, stated,
        "x-offset-sum 自身两处记账不一致：加数合计 {recomputed}，却写作 {stated}（原文 {raw:?}）"
    );

    let table_lengths: Vec<usize> = offset_table(&json).iter().map(|f| f.2).collect();
    assert_eq!(
        addends, table_lengths,
        "x-offset-sum 的加数必须与 x-offset-table 各字段长度逐一对应（偏移表 {table_lengths:?}）"
    );
    assert_eq!(
        recomputed, RR3_IPC_HDR_SIZE,
        "偏移表长度合计必须等于 Rust 头长度 {RR3_IPC_HDR_SIZE}，实际 {recomputed}"
    );
}

/// C1③：`payload_len` 的 minimum / maximum 就是 Rust 判界的那两个值。
/// maximum 钉 Rust `MAX_PAYLOAD_LEN`；minimum 钉 Rust 侧真实下界 0
///（`payload_len = 0` 表示"无 payload"，是合法值而非错误），并用解析器行为复核
#[test]
fn payload_len_bounds_match_contract() {
    let json = schema_json();
    let field = contract_field(&json, "payload_len");
    let c_min = field["minimum"]
        .as_u64()
        .expect("契约缺 payload_len.minimum");
    let c_max = field["maximum"]
        .as_u64()
        .expect("契约缺 payload_len.maximum");

    assert_eq!(
        c_max, MAX_PAYLOAD_LEN as u64,
        "契约 payload_len.maximum({c_max}) 与 Rust MAX_PAYLOAD_LEN({MAX_PAYLOAD_LEN}) 漂移"
    );
    assert_eq!(
        c_min, 0,
        "契约 payload_len.minimum({c_min}) 与 Rust 侧实际下界 0（0 = 无 payload）漂移"
    );

    // 行为复核：契约边界必须是 Rust 解析器**恰好**放行/拒绝的那两个值
    let mut h = Rr3Header::rr3_hdr_broadcast(opcode::SEND, 0);
    h.payload_len = u32::try_from(c_min).expect("payload_len.minimum 必须落在 u32 域内");
    assert!(
        Rr3Header::from_bytes(&h.to_bytes()).is_ok(),
        "契约下界 {c_min} 必须可解析（无 payload 是合法消息）"
    );
    let max = u32::try_from(c_max).expect("payload_len.maximum 必须落在 u32 域内");
    h.payload_len = max;
    assert!(
        Rr3Header::from_bytes(&h.to_bytes()).is_ok(),
        "契约上界 {max} 必须被 Rust 解析器接受"
    );
    h.payload_len = max + 1;
    assert_eq!(
        Rr3Header::from_bytes(&h.to_bytes()),
        Err(Rr3HdrError::PayloadTooLarge { len: max + 1 }),
        "契约上界 +1 必须被 Rust 解析器拒绝"
    );
}

/// I3：`RR3_CONTRACT_SCHEMA_VERSION`（rrd 横幅里的 "contracts schema v1"）此前无人断言；
/// 现与 `contracts/envelope.schema.json` 顶层 `x-schema-version`（2026-09-29 追加的元数据键）逐值对齐
#[test]
fn contract_schema_version_metadata_matches_rust() {
    let json = schema_json();
    let c = json["x-schema-version"]
        .as_u64()
        .expect("契约缺顶层 x-schema-version（2026-09-29 追加的元数据键）");
    assert_eq!(
        c,
        RR3_CONTRACT_SCHEMA_VERSION as u64,
        "契约 x-schema-version({c}) 与 Rust RR3_CONTRACT_SCHEMA_VERSION({RR3_CONTRACT_SCHEMA_VERSION}) 漂移"
    );
}
