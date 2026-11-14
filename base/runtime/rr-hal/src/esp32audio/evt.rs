//! 板帧分类与音频 Event 四道门解析。

use rr_ipc::{rr3_base64_decode, rr3_body_split, Rr3Header, Rr3PayloadType};
use rr_periph_proto::cmd::Rr3BoardFrame;
use serde_json::Value;

use super::RR3_HOST_TASK;

/// 音频 PCM 上行主题（在册 stream L1：contracts/topics.toml；事实源
/// modules/rr3-sense composite::`RR3_PCM_TOPIC`，本 crate 不加依赖边，同值锚定）。
pub(super) const RR3_AUDIO_TOPIC: &str = "rr3-sense/audio/pcm";
/// 坏帧打点周期：首条立即点名（证明坏帧形态），其后每累计 `RR3_ERR_LOG_EVERY`
/// 条记一次。此前按 3 取模是持续误码时每 3 帧刷一条——线面整段坏掉时会变成日志
/// 风暴（2026-11-09 静态审计：音频读循环是帧率驱动，不是事件驱动）。
pub(super) const RR3_ERR_LOG_EVERY: u64 = 1_000;

/// 坏帧打点判据：首条必打，之后按累计周期抽样（连续坏帧期间不刷屏）。
pub(super) fn rr3_err_log_now(err_total: u64) -> bool {
    err_total == 1 || err_total % RR3_ERR_LOG_EVERY == 0
}

/// 板面入帧分类（b1）：Response（命令旁路的板侧应答）放行记账，其余走音频
/// 四道门——旁路回执不再污 err_streak（帧界损坏记账语义保住）。协议级判别
/// （dst 门 + payload 前缀门 + Response 解析）在 rr-periph-proto；音频载荷门序
/// 与解码留在本件消费面。
#[derive(Debug)]
pub(super) enum Rr3BoardFrameIn {
    /// 音频 Event（四道门全过）
    Audio(Rr3Esp32AudioChunk),
    /// 板侧请求应答（执行**前**回成功，status.md 对表实录）：id 回显 + 业务
    /// 结果——ok 形状即 pre-execution ack。
    Reply { id: u64, err: Option<String> },
}

pub(super) fn rr3_esp32_classify(
    hdr: &Rr3Header,
    payload: &[u8],
) -> Result<Rr3BoardFrameIn, String> {
    match rr_periph_proto::cmd::rr3_board_classify(hdr, payload)? {
        Rr3BoardFrame::Reply { id, err } => Ok(Rr3BoardFrameIn::Reply { id, err }),
        Rr3BoardFrame::Event => rr3_esp32_evt_parse(hdr, payload).map(Rr3BoardFrameIn::Audio),
    }
}

/// 一条音频事件（四道门全过后的解码前载荷）。
#[derive(Debug)]
pub(super) struct Rr3Esp32AudioChunk {
    /// 板面实测采样率（流就绪日志用；open 已按板事实校验配置）
    pub(super) rate: u32,
    /// s16le 交错原始字节（b64 解码后）
    pub(super) samples: Vec<u8>,
}

/// 四道门之一：dst 位形门（非宿主位形即拒，点名实际值）。
fn rr3_evt_dst(hdr: &Rr3Header) -> Result<(), String> {
    if hdr.dst_task != RR3_HOST_TASK {
        return Err(format!(
            "dst_task={} 非宿主位形 {RR3_HOST_TASK}",
            hdr.dst_task
        ));
    }
    Ok(())
}

/// 四道门之二：payload 前缀类型门（须 Event）+ 事件体 JSON 解析。
fn rr3_evt_body(payload: &[u8]) -> Result<Value, String> {
    let (ty, body) = rr3_body_split(payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
    if ty != Rr3PayloadType::Event {
        return Err(format!("payload 类型非 Event: {ty:?}"));
    }
    serde_json::from_slice(&body).map_err(|e| format!("事件体非 JSON: {e}"))
}

/// 四道门之三/四：topic 门 + 字段门（format/rate/seq/ms/pcm_b64），返回
/// (采样率, b64 载荷)。字段检查次序即报错次序（首缺即点名）。
fn rr3_evt_fields(v: &Value) -> Result<(u32, &str), String> {
    rr3_evt_topic(v).and_then(|()| rr3_evt_format(v))?;
    let rate = v.get("rate").and_then(Value::as_u64).ok_or("缺 rate")? as u32;
    rr3_evt_time(v)?;
    let b64 = v
        .get("pcm_b64")
        .and_then(Value::as_str)
        .ok_or("缺 pcm_b64")?;
    Ok((rate, b64))
}

/// format 门（板面流固定 s16le）。
fn rr3_evt_format(v: &Value) -> Result<(), String> {
    if v.get("format").and_then(Value::as_str) != Some("s16le") {
        return Err("format 非 s16le".into());
    }
    Ok(())
}

/// seq / ms 必填（次序即报错次序）。
fn rr3_evt_time(v: &Value) -> Result<(), String> {
    if v.get("seq").and_then(Value::as_u64).is_none() {
        return Err("缺 seq".into());
    }
    if v.get("ms").and_then(Value::as_u64).is_none() {
        return Err("缺 ms".into());
    }
    Ok(())
}

/// topic 门（在册主题，点名实际值）。
fn rr3_evt_topic(v: &Value) -> Result<(), String> {
    if v.get("topic").and_then(Value::as_str) != Some(RR3_AUDIO_TOPIC) {
        return Err(format!(
            "topic 非 {}: {:?}",
            RR3_AUDIO_TOPIC,
            v.get("topic")
        ));
    }
    Ok(())
}

/// Event 帧解析（四道门全过才 Ok，任一失败 Err 点名——与板侧 frame_recv 同款
/// fail-closed 口径；与 tools/rr3-audio-rec `parse_audio_event` 同值锚定）：
/// dst=宿主门 → payload Event 类型门 → topic 门 → 字段/b64 门。
pub(super) fn rr3_esp32_evt_parse(
    hdr: &Rr3Header,
    payload: &[u8],
) -> Result<Rr3Esp32AudioChunk, String> {
    let v = rr3_evt_dst(hdr).and_then(|()| rr3_evt_body(payload))?;
    let (rate, b64) = rr3_evt_fields(&v)?;
    let samples = rr3_base64_decode(b64).map_err(|e| format!("pcm_b64 解码失败: {e:?}"))?;
    Ok(Rr3Esp32AudioChunk { rate, samples })
}
