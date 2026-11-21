//! tts_pcm 收流缓冲（自 dispatch.rs 抽出）：先流后点名的载荷通路与上限闸门。
//!
//! 独立成件让协议层（rpc 点名对账）与机制层（dispatch 收流）共同依赖缓冲
//! 数据形状，而不是协议层反向引用机制层——模块依赖保持单向。

/// tts_pcm 流收缓冲（MC-1）：brain 语音下行**先流后点名**——流是载荷通路，
/// `rr3-interact/tts/text` 点名是"该不该播"的唯一判据（0.0.5 方案 §20 第 1 条
/// (a)）。Event 帧逐帧解码进缓冲，点名对账后排空播放；对账不过整缓冲作废
/// （残流不跨轮滞留）。
#[derive(Default)]
pub(crate) struct Rr3TtsPcmBuf {
    bytes: Vec<u8>,
}

/// 全量收流上界（32MiB）：正常一轮 TTS 是秒级音频（≤数 MB），上界防"只流不
/// 点名"或故障模块把 interact 内存顶穿。超界帧丢弃，点名对账必然失败→整轮拒播。
const RR3_TTS_BUF_CAP: usize = 32 * 1024 * 1024;

impl Rr3TtsPcmBuf {
    /// 收一帧：`pcm_b64` 解码追加（STREAM 语义有序到达，帧序即字节序）。
    /// 解码失败/超上界上抛，由调用方弃帧——缺口由点名的对账兜住，不静默。
    pub(crate) fn rr3_tts_buf_push(&mut self, pcm_b64: &str) -> Result<(), String> {
        self.rr3_tts_push_cap(pcm_b64, RR3_TTS_BUF_CAP)
    }

    /// 带帽收帧（帽参数显式化，单测用小帽钉边界）：先按 base64 上界估算，
    /// 超帽时不解码、不分配。
    pub(crate) fn rr3_tts_push_cap(&mut self, pcm_b64: &str, cap: usize) -> Result<(), String> {
        rr3_push_room(cap, self.bytes.len(), pcm_b64.len() / 4 * 3)?;
        let chunk = rr_ipc::rr3_base64_decode(pcm_b64)
            .map_err(|e| format!("tts_pcm 帧 base64 解码失败: {e:?}"))?;
        rr3_push_room(cap, self.bytes.len(), chunk.len())?;
        self.bytes.extend_from_slice(&chunk);
        Ok(())
    }

    /// 缓冲实收字节数（对账的一侧）
    pub(crate) fn rr3_tts_buf_len(&self) -> usize {
        self.bytes.len()
    }

    /// 排空：取走全部字节，缓冲归零（残流不跨轮滞留）。
    pub(crate) fn rr3_tts_buf_take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }
}

/// 收流缓冲余量判据（解码前后的估算值与实解值同文案）。
fn rr3_push_room(cap: usize, have: usize, add: usize) -> Result<(), String> {
    if have.saturating_add(add) > cap {
        return Err(format!(
            "tts_pcm 缓冲超界（>{}B）：丢弃本帧，后续点名对账将拒播",
            cap
        ));
    }
    Ok(())
}
