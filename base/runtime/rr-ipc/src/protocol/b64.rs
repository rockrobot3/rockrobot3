//! base64 编码（RFC 4648 标准字母表 + 填充）——线格式层的二进制承载原语。
//!
//! 0.0.5 批C① 自 rr3-sense 塌缩归一（方案 §12.3）：PCM 二进制进 JSON 的唯一通道
//! 属线格式（0.0.7 批B 字节层下沉的同层），sense 不该私带线格式工具。手写不引
//! 第三方编解码依赖，RFC 4648 §10 的权威向量钉死正确性。

// Vec/String 取自 alloc 而非 std——std 目标上两者是同一类型，零行为差异；
// 裸金属目标配 `-Z build-std=alloc,core` 即可编译（body.rs 同款）。
extern crate alloc;

use alloc::string::String;

const RR3_B64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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
}
