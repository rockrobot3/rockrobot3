//! 帧/事件字段解析：dst/Event/topic/字段四道门与 pcm_b64 解码。

use rr_ipc::{rr3_base64_decode, rr3_body_split, Rr3Frame, Rr3PayloadType};
use serde_json::Value;

use crate::{RR3_AUDIO_TOPIC, RR3_HOST_TASK};

/// 一条音频事件（自 Board Event 帧解析出）。
pub(crate) struct AudioChunk {
    pub(crate) seq: u32,
    pub(crate) rate: u32,
    pub(crate) ms: u64,
    /// s16le 交错原始字节（解码后）
    pub(crate) samples: Vec<u8>,
}

/// Event 帧解析（四道门全过才 Ok，任一失败 Err 点名——与板侧 frame_recv 同款
/// fail-closed 口径）：dst=宿主门 → payload Event 类型门 → topic 门 → 字段/b64 门。
pub(crate) fn parse_audio_event(frame: &Rr3Frame) -> Result<AudioChunk, String> {
    rr3_audio_dst(frame)?;
    audio_chunk_fields(&audio_event_body(&frame.payload)?)
}

/// dst 门：事件帧必须寄给宿主位形。
fn rr3_audio_dst(frame: &Rr3Frame) -> Result<(), String> {
    if frame.header.dst_task != RR3_HOST_TASK {
        return Err(format!(
            "dst_task={} 非宿主位形 {RR3_HOST_TASK}",
            frame.header.dst_task
        ));
    }
    Ok(())
}

/// payload 前缀类型门 → 事件体 JSON → topic 门（失败点名原文案）。
fn audio_event_body(payload: &[u8]) -> Result<Value, String> {
    let v = rr3_event_value(payload)?;
    rr3_audio_topic_ok(&v)?;
    Ok(v)
}

/// payload 前缀类型门 → 事件体 JSON 解析（次序与拒收文案保持原样）。
fn rr3_event_value(payload: &[u8]) -> Result<Value, String> {
    let (ty, body) = rr3_body_split(payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
    rr3_event_type(ty)?;
    serde_json::from_slice(&body).map_err(|e| format!("事件体非 JSON: {e}"))
}

/// payload 类型门：必须是 Event。
fn rr3_event_type(ty: Rr3PayloadType) -> Result<(), String> {
    if ty != Rr3PayloadType::Event {
        return Err(format!("payload 类型非 Event: {ty:?}"));
    }
    Ok(())
}

/// topic 门（失败点名原文案）。
fn rr3_audio_topic_ok(v: &Value) -> Result<(), String> {
    if v.get("topic").and_then(Value::as_str) != Some(RR3_AUDIO_TOPIC) {
        return Err(format!(
            "topic 非 {}: {:?}",
            RR3_AUDIO_TOPIC,
            v.get("topic")
        ));
    }
    Ok(())
}

/// 事件体字段门 → s16le 解码出分片（字段次序与拒收文案保持原样）。
fn audio_chunk_fields(v: &Value) -> Result<AudioChunk, String> {
    if v.get("format").and_then(Value::as_str) != Some("s16le") {
        return Err("format 非 s16le".into());
    }
    Ok(AudioChunk {
        seq: rr3_u32_field(v, "seq")?,
        rate: rr3_u32_field(v, "rate")?,
        ms: v.get("ms").and_then(Value::as_u64).ok_or("缺 ms")?,
        samples: rr3_pcm_samples(v)?,
    })
}

/// 必给 u32 字段（缺字段点名原文案）。
fn rr3_u32_field(v: &Value, key: &str) -> Result<u32, String> {
    v.get(key)
        .and_then(Value::as_u64)
        .map(|n| n as u32)
        .ok_or_else(|| format!("缺 {key}"))
}

/// `pcm_b64` → s16le 字节（缺字段/解码失败点名原文案）。
fn rr3_pcm_samples(v: &Value) -> Result<Vec<u8>, String> {
    let b64 = v
        .get("pcm_b64")
        .and_then(Value::as_str)
        .ok_or("缺 pcm_b64")?;
    rr3_base64_decode(b64).map_err(|e| format!("pcm_b64 解码失败: {e:?}"))
}
