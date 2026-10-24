//! base64 编码（RFC 4648 标准字母表 + 填充）——线格式层的二进制承载原语。
//!
//! 0.0.5 批C① 自 rr3-sense 塌缩归一（方案 §12.3）：PCM 二进制进 JSON 的唯一通道
//! 属线格式（0.0.7 批B 字节层下沉的同层），sense 不该私带线格式工具。手写不引
//! 第三方编解码依赖，RFC 4648 §10 的权威向量钉死正确性。

// Vec/String 取自 alloc 而非 std——std 目标上两者是同一类型，零行为差异；
// 裸金属目标配 `-Z build-std=alloc,core` 即可编译（body.rs 同款）。
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

const RR3_B64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// base64 解码失败原因（fail-closed：非法输入一律拒，不静默截断/补零）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3Base64Error {
    /// 长度不是 4 的倍数（RFC 4648 带填充的组宽）
    BadLength { got: usize },
    /// 出现字母表外的字符
    BadChar { at: usize, c: char },
    /// `=` 位置非法（非末组、非尾部，或填充后又接数据字符）
    BadPadding { at: usize },
}

/// ASCII → 6bit 反向查表；非字母表字符置 0xFF（哨兵）。
const fn rr3_b64_rev_table() -> [u8; 256] {
    let mut t = [0xFFu8; 256];
    let mut i = 0;
    while i < 64 {
        t[RR3_B64_ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    t
}

/// base64 编码（带填充）。每组 3 字节 → 4 字符；尾部不足 3 字节以 `=` 补位。
pub fn rr3_base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(RR3_B64_ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(RR3_B64_ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            RR3_B64_ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            RR3_B64_ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// base64 解码（RFC 4648 标准字母表带填充）。非法输入一律 `Err`——
/// `=` 只许出现在末组尾部、字母表外字符即拒（fail-closed，不做"尽力而为"）。
pub fn rr3_base64_decode(s: &str) -> Result<Vec<u8>, Rr3Base64Error> {
    let bytes = s.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err(Rr3Base64Error::BadLength { got: bytes.len() });
    }
    let table = rr3_b64_rev_table();
    let groups = bytes.len() / 4;
    let mut out = Vec::with_capacity(groups * 3);
    for g in 0..groups {
        let grp = &bytes[g * 4..g * 4 + 4];
        let last = g + 1 == groups;
        let mut vals = [0u8; 4];
        let mut pad = 0usize;
        for (i, &c) in grp.iter().enumerate() {
            if c == b'=' {
                // 填充只允许在末组的末两位；`==` 前不得再插数据（i<2 即 `=` 占数据位）
                if !last || i < 2 {
                    return Err(Rr3Base64Error::BadPadding { at: g * 4 + i });
                }
                pad += 1;
            } else {
                if pad > 0 {
                    return Err(Rr3Base64Error::BadPadding { at: g * 4 + i });
                }
                let v = table[c as usize];
                if v == 0xFF {
                    return Err(Rr3Base64Error::BadChar {
                        at: g * 4 + i,
                        c: c as char,
                    });
                }
                vals[i] = v;
            }
        }
        let n = ((vals[0] as u32) << 18)
            | ((vals[1] as u32) << 12)
            | ((vals[2] as u32) << 6)
            | (vals[3] as u32);
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4648 §10 的测试向量（权威字面量，不是实现回抄）
    #[test]
    fn base64_matches_rfc4648_vectors() {
        let cases: [(&[u8], &str); 7] = [
            (b"", ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
        ];
        for (input, want) in cases {
            assert_eq!(rr3_base64_encode(input), want, "input={input:?}");
        }
    }

    /// 解码以同一份 RFC 4648 向量钉死，并证明 encode∘decode 是恒等
    #[test]
    fn base64_decode_matches_rfc4648_vectors_and_roundtrips() {
        let cases: [(&[u8], &str); 7] = [
            (b"", ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
        ];
        for (want, encoded) in cases {
            assert_eq!(rr3_base64_decode(encoded).unwrap(), want, "encoded={encoded}");
            assert_eq!(rr3_base64_encode(want), encoded, "encoded={encoded}");
        }
    }

    /// 任意长度往返（覆盖 16bit 样点流的实际形态：640B 块 = 856 字符）
    #[test]
    fn base64_decode_roundtrips_arbitrary_lengths() {
        for len in [1usize, 2, 3, 4, 5, 319, 320, 640, 960] {
            let raw: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let enc = rr3_base64_encode(&raw);
            assert_eq!(rr3_base64_decode(&enc).unwrap(), raw, "len={len}");
        }
    }

    /// fail-closed：非法长度 / 越界字符 / 错位填充一律拒
    #[test]
    fn base64_decode_rejects_malformed_input() {
        // 长度非 4 倍数
        assert_eq!(
            rr3_base64_decode("Zg="),
            Err(Rr3Base64Error::BadLength { got: 3 })
        );
        // 字母表外字符（`*` 在第 1 位）
        assert_eq!(
            rr3_base64_decode("Z*=="),
            Err(Rr3Base64Error::BadChar { at: 1, c: '*' })
        );
        // 非末组的填充
        assert_eq!(
            rr3_base64_decode("Zg==Zg=="),
            Err(Rr3Base64Error::BadPadding { at: 2 })
        );
        // 填充占据数据位（第 0 位 `=`）
        assert_eq!(
            rr3_base64_decode("=AAA"),
            Err(Rr3Base64Error::BadPadding { at: 0 })
        );
        // 填充之后又接数据字符
        assert_eq!(
            rr3_base64_decode("AA=A"),
            Err(Rr3Base64Error::BadPadding { at: 3 })
        );
    }
}
