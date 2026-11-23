// SPDX-License-Identifier: Apache-2.0
//! 探针帧布局常量生成：真值点 `contracts/envelope.schema.json` 的
//! `properties.header.properties.fields.x-offset-table`，生成物
//! `tools/pi-periph-probe/envelope_gen.py`。
//!
//! 探针此前在 Python 侧手抄 magic/头长/偏移与坏头上限（多处 20000、40、44、32），
//! 契约或布局一动就要逐脚本对齐；本模块把偏移表**单源导出**，探针只读生成物。
//!
//! fail-closed：magic 行的线上字节必须等于字段值的小端字节、偏移/宽度必须是
//! u32、契约缺段/空表逐条点名——坏契约宁可红，也不生成带病副本。

use std::path::Path;

use serde_json::Value;

/// 生成物路径（仓根相对）。
pub(crate) const OUT: &str = "tools/pi-periph-probe/envelope_gen.py";
const SRC: &str = "contracts/envelope.schema.json";

/// 偏移表一行；`value`/`wire` 只有 magic 行携带（用于小端自证）。
struct Row {
    name: String,
    offset: u32,
    length: u32,
    value: Option<u32>,
    wire: Option<String>,
}

/// 契约 → envelope_gen.py 文本（坏契约折进 errs，不生成带病副本）。
pub(crate) fn rr3_probe_gen(root: &Path, errs: &mut Vec<String>) -> String {
    let doc: Value = serde_json::from_str(&crate::rr3_read(root, SRC))
        .unwrap_or_else(|e| crate::die(&[format!("{SRC} 解析失败: {e}")]));
    match probe_layout(&doc) {
        Ok(text) => text,
        Err(e) => {
            errs.extend(e);
            String::new()
        }
    }
}

/// 契约文档 → 生成文本（头长、偏移表、magic 自证三段各自点名）。
fn probe_layout(doc: &Value) -> Result<String, Vec<String>> {
    let header = doc
        .pointer("/properties/header")
        .ok_or_else(|| vec![format!("{SRC} 缺 properties.header")])?;
    let header_len =
        u32_of(header.get("x-byte-size"), "header.x-byte-size").map_err(|e| vec![e])?;
    let rows = layout_rows(header)?;
    let (magic, wire) = magic_of(&rows).map_err(|e| vec![e])?;
    Ok(render_probe(header_len, magic, &wire, &rows))
}

/// 偏移表逐行取数（缺段/空表/坏行都点名；坏行折进 errs 不短路）。
fn layout_rows(header: &Value) -> Result<Vec<Row>, Vec<String>> {
    let Some(arr) = header
        .pointer("/properties/fields/x-offset-table")
        .and_then(Value::as_array)
    else {
        return Err(vec![format!(
            "{SRC} 缺 properties.header.properties.fields.x-offset-table"
        )]);
    };
    if arr.is_empty() {
        return Err(vec![format!("{SRC} x-offset-table 为空（拒绝生成空表）")]);
    }
    let mut rows = Vec::new();
    let mut errs = Vec::new();
    for (i, v) in arr.iter().enumerate() {
        match row_of(v) {
            Ok(r) => rows.push(r),
            Err(e) => errs.push(format!("x-offset-table[{i}] {e}")),
        }
    }
    if errs.is_empty() {
        Ok(rows)
    } else {
        Err(errs)
    }
}

/// 单行取数：name/offset/length 必备，value/wire_bytes_hex 只有 magic 行需要。
fn row_of(v: &Value) -> Result<Row, String> {
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .ok_or("缺 name")?
        .to_string();
    let offset = u32_of(v.get("offset"), "offset")?;
    let length = u32_of(v.get("length"), "length")?;
    let value = v
        .get("value")
        .map(|x| u32_of(Some(x), "value"))
        .transpose()?;
    let wire = v
        .get("wire_bytes_hex")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(Row {
        name,
        offset,
        length,
        value,
        wire,
    })
}

/// JSON 无符号整数 → u32（负数/浮点/超界/缺键同一句点名）。
fn u32_of(v: Option<&Value>, what: &str) -> Result<u32, String> {
    v.and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| format!("{what} 非 u32 无符号整数（{v:?}）"))
}

/// magic 行：offset=0/length=4，字段值的小端字节必须等于 wire_bytes_hex。
fn magic_of(rows: &[Row]) -> Result<(u32, Vec<u8>), String> {
    let row = rows
        .iter()
        .find(|r| r.name == "magic")
        .ok_or("x-offset-table 缺 magic 行")?;
    if row.offset != 0 || row.length != 4 {
        return Err(format!(
            "magic 行须 offset=0/length=4（实为 {}/{}）",
            row.offset, row.length
        ));
    }
    let value = row.value.ok_or("magic 行缺字段值 value")?;
    let hex = row.wire.as_deref().ok_or("magic 行缺 wire_bytes_hex")?;
    let wire = hex_bytes(hex)?;
    if wire != value.to_le_bytes() {
        return Err(format!(
            "magic wire_bytes_hex {hex} ≠ 字段值 {value:#010X} 的小端字节"
        ));
    }
    Ok((value, wire))
}

/// 十六进制文本 → 字节（非 ASCII/奇数长度/非法字符逐条点名，不切片 panic）。
fn hex_bytes(s: &str) -> Result<Vec<u8>, String> {
    if !s.is_ascii() {
        return Err(format!("{s} 非 ASCII 十六进制文本"));
    }
    let mut out = Vec::new();
    let mut chunks = s.as_bytes().chunks_exact(2);
    for c in &mut chunks {
        let pair = std::str::from_utf8(c).map_err(|e| format!("{s} 不可读: {e}"))?;
        out.push(u8::from_str_radix(pair, 16).map_err(|e| format!("{s} 非十六进制: {e}"))?);
    }
    if !chunks.remainder().is_empty() {
        return Err(format!("{s} 非偶数长度十六进制文本"));
    }
    Ok(out)
}

/// 生成 envelope_gen.py 文本（头注点名生成器与真值点，手改必被门 ⑨-d 抓红）。
fn render_probe(header_len: u32, magic: u32, wire: &[u8], rows: &[Row]) -> String {
    let esc: String = wire.iter().map(|b| format!("\\x{b:02x}")).collect();
    let spaced: Vec<String> = wire.iter().map(|b| format!("{b:02x}")).collect();
    let body: String = rows
        .iter()
        .filter(|r| r.name != "magic")
        .map(|r| {
            let name = r.name.to_ascii_uppercase();
            format!("OFF_{name} = {}\nLEN_{name} = {}\n", r.offset, r.length)
        })
        .collect();
    format!(
        "# SPDX-License-Identifier: Apache-2.0\n\
         # @generated by tools/rr3-codegen —— DO NOT EDIT（手改必被门 ⑨-d 重跑 diff 抓红）。\n\
         # 真值点：{SRC}（A-IPC 消息头偏移表，L1 冻结）。\n\
         # 探针只读本文件：帧面常量不在 Python 侧手抄第二份。\n\n\
         #: 帧头 magic 的线上字节（字段值 {magic:#010X} 小端落盘 = {spaced}）。\n\
         MAGIC = b\"{esc}\"\n\
         #: magic 字段值（u32）。\n\
         MAGIC_U32 = {magic:#010X}\n\
         #: 帧头固定长度（{header_len}B）。\n\
         HEADER_LEN = {header_len}\n\n\
         # 以下偏移/长度逐行取自契约 x-offset-table（顺序同契约）。\n\
         {body}",
        spaced = spaced.join(" "),
    )
}

#[cfg(test)]
mod tests {
    use super::{render_probe, Row};

    /// 生成文本的形状回归：magic 转义/字段值/头长/偏移逐行、magic 行不重复出
    /// OFF_MAGIC、@generated 头注在位（手改必被门 ⑨-d 抓红）。
    #[test]
    fn render_probe_pinned() {
        let rows = vec![
            Row {
                name: "magic".into(),
                offset: 0,
                length: 4,
                value: Some(0x4152_4531),
                wire: Some("31455241".into()),
            },
            Row {
                name: "opcode".into(),
                offset: 4,
                length: 2,
                value: None,
                wire: None,
            },
            Row {
                name: "payload_crc32".into(),
                offset: 44,
                length: 4,
                value: None,
                wire: None,
            },
        ];
        let out = render_probe(128, 0x4152_4531, &[0x31, 0x45, 0x52, 0x41], &rows);
        assert!(
            out.starts_with(
                "# SPDX-License-Identifier: Apache-2.0\n# @generated by tools/rr3-codegen"
            ),
            "{out}"
        );
        assert!(out.contains("MAGIC = b\"\\x31\\x45\\x52\\x41\""), "{out}");
        assert!(out.contains("MAGIC_U32 = 0x41524531"), "{out}");
        assert!(out.contains("HEADER_LEN = 128"), "{out}");
        assert!(out.contains("OFF_OPCODE = 4\nLEN_OPCODE = 2"), "{out}");
        assert!(
            out.contains("OFF_PAYLOAD_CRC32 = 44\nLEN_PAYLOAD_CRC32 = 4"),
            "{out}"
        );
        assert!(
            !out.contains("OFF_MAGIC"),
            "magic 行只出 MAGIC/MAGIC_U32，不再重复出偏移对"
        );
    }
}
