//! payload 体前缀：前 2 字节 = `Rr3PayloadType` 的冻结枚举值（LE），其后是 JSON 体。
//!
//! 契约把这一位显式留给 M2（`contracts/envelope.schema.json` 的
//! `x-rock3-deferred: ["payload_body_encoding"]`）。本模块就是那个落点：
//! 帧分派的依据来自头之外的固定宽度字段，而不是"看 JSON 里有没有某个键"——
//! 后者等于把鉴权路径交给发送方可伪造的内容（详见计划"与 spec 的偏离 4"）。

use crate::protocol::header::Rr3PayloadType;

/// 类型前缀宽度（字节）。冻结为 2：够放 u16 枚举值，且 2 字节对齐不留空洞
pub const RR3_BODY_TYPE_PREFIX_LEN: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3BodyError {
    /// payload 比前缀还短（连类型都没带全）
    TooShort { got: usize },
    /// 前缀值不在冻结枚举里（0x0006 起属未来追加，v0.0.1 必须拒）
    UnknownType { v: u16 },
}

/// 编码：`[type:u16 LE][body]`
pub fn rr3_body_encode(ty: Rr3PayloadType, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(RR3_BODY_TYPE_PREFIX_LEN + body.len());
    out.extend_from_slice(&(ty as u16).to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// 解码：拆出类型与体（体是新拥有的 Vec，调用方随便 move）
pub fn rr3_body_split(raw: &[u8]) -> Result<(Rr3PayloadType, Vec<u8>), Rr3BodyError> {
    if raw.len() < RR3_BODY_TYPE_PREFIX_LEN {
        return Err(Rr3BodyError::TooShort { got: raw.len() });
    }
    let mut buf = [0u8; RR3_BODY_TYPE_PREFIX_LEN];
    buf.copy_from_slice(&raw[..RR3_BODY_TYPE_PREFIX_LEN]);
    let v = u16::from_le_bytes(buf);
    let ty = Rr3PayloadType::from_u16(v).ok_or(Rr3BodyError::UnknownType { v })?;
    Ok((ty, raw[RR3_BODY_TYPE_PREFIX_LEN..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 前缀必须是 LE 的枚举值本身：跨架构一致性的第一道锚（IPC-T-003 同源）
    #[test]
    fn prefix_is_little_endian_enum_value() {
        for (ty, want) in [
            (Rr3PayloadType::Request, [0x01u8, 0x00]),
            (Rr3PayloadType::Response, [0x02, 0x00]),
            (Rr3PayloadType::Event, [0x03, 0x00]),
            (Rr3PayloadType::Stream, [0x04, 0x00]),
            (Rr3PayloadType::Notification, [0x05, 0x00]),
        ] {
            let got = rr3_body_encode(ty, b"x");
            assert_eq!(
                &got[..RR3_BODY_TYPE_PREFIX_LEN],
                &want[..],
                "{ty:?} 前缀字节错"
            );
            assert_eq!(
                &got[RR3_BODY_TYPE_PREFIX_LEN..],
                b"x",
                "体必须紧跟前缀，不插填充"
            );
        }
    }

    #[test]
    fn round_trip_all_types() {
        for ty in [
            Rr3PayloadType::Request,
            Rr3PayloadType::Response,
            Rr3PayloadType::Event,
            Rr3PayloadType::Stream,
            Rr3PayloadType::Notification,
        ] {
            let body = br#"{"topic":"rr3-sima/ping/state"}"#;
            let (got_ty, got_body) = rr3_body_split(&rr3_body_encode(ty, body)).unwrap();
            assert_eq!(got_ty, ty);
            assert_eq!(got_body, body);
        }
    }

    /// 空体合法（心跳帧可以只带前缀？不合法的是"没带前缀"——空体本身要能过）
    #[test]
    fn empty_body_is_allowed() {
        let raw = rr3_body_encode(Rr3PayloadType::Event, &[]);
        assert_eq!(
            rr3_body_split(&raw).unwrap(),
            (Rr3PayloadType::Event, Vec::new())
        );
    }

    #[test]
    fn rejects_truncated_prefix() {
        assert_eq!(
            rr3_body_split(&[0x03]),
            Err(Rr3BodyError::TooShort { got: 1 })
        );
        assert_eq!(rr3_body_split(&[]), Err(Rr3BodyError::TooShort { got: 0 }));
    }

    /// 冻结值之外一律拒：不许"未知就当 Event"放行（那是 fail-open）
    #[test]
    fn rejects_unknown_type_value() {
        for v in [0x0006u16, 0x0000, 0xFFFF] {
            assert_eq!(
                rr3_body_split(&v.to_le_bytes()),
                Err(Rr3BodyError::UnknownType { v }),
                "{v:#06x} 必须被拒"
            );
        }
    }
}
