//! TTS 流式接收（W7-b2）：HTTPS 4 KB 读块 → WAV/裸 PCM 识别 → 全局淡入淡出 →
//! 16 KB 播放环。不再把 512 KB 回包聚合成 Vec，也不再有"全量 PCM 播放队列"。
//!
//! 头处理：首块若以 `RIFF` 开头则缓到头齐后走 [`rr3_wav_info`]（扩展 fmt 也兼容，
//! 上限 1 KB 头窗）；否则按裸 s16le 单声道 16 kHz 处理。WAV 的 `data` 声明长度
//! 优先，声明为 0/越界时用 Content-Length 截断（流式编码器写 0 是常态）。
//!
//! 失败语义：整形/播放环失败立即中止并点名，绝不把半条流记成播完；HTTP 层在首块
//! 之后不再重试（重放音频比失败更糟），首块之前仍按契约 2 次/500 ms。

use rr_audio_mech::{rr3_pcm_ms, rr3_wav_info, Rr3PcmStream, Rr3WavInfo};

use super::cfg::rr3_cloud_cfg;
use super::http::{rr3_http_post_stream, Rr3StreamSink};
use super::MAX_PCM;
use crate::playout::{
    rr3_play_abort, rr3_play_begin, rr3_play_end, rr3_play_feed, PLAY_FADE_FRAMES,
};

/// TTS 请求与播放采样率（契约固定 16000）。
const TTS_RATE: u32 = 16_000;
/// WAV 头缓冲上限（标准 44B；扩展 fmt 留余量）。
const HEAD_CAP: usize = 1_024;
/// 成形块缓冲（4 KB 源 → 单声道最多 8 KB 立体声）。
const OUT_CAP: usize = 8_192;

/// TTS 流式接收端（HTTP sink；单线程，云 worker 独占）。
struct Rr3TtsSink {
    head: Vec<u8>,
    out: Vec<u8>,
    stream: Option<Rr3PcmStream>,
    channels: u16,
    /// 响应 Content-Length（None = chunked/未知）。
    total: Option<usize>,
    /// 还可接收的源字节（WAV 声明长度/裸流上限；尾随容器字节忽略）。
    left: usize,
    /// WAV 声明总长（Some = 收尾时必须收满；裸流按 HTTP 长度对账）。
    expect: Option<usize>,
    started: bool,
}

impl Rr3TtsSink {
    fn new() -> Self {
        Self {
            head: Vec::with_capacity(HEAD_CAP),
            out: Vec::with_capacity(OUT_CAP),
            stream: None,
            channels: 1,
            total: None,
            left: 0,
            expect: None,
            started: false,
        }
    }

    /// 起流：建整形器 + 播放环，置 started（此后 HTTP 层不得重试）。
    fn start(
        &mut self,
        channels: u16,
        total_src: Option<usize>,
        left: usize,
        expect: Option<usize>,
    ) -> Result<(), String> {
        let stream =
            Rr3PcmStream::rr3_new(channels, total_src, PLAY_FADE_FRAMES).map_err(str::to_string)?;
        rr3_play_begin().map_err(str::to_string)?;
        self.channels = channels;
        self.stream = Some(stream);
        self.left = left;
        self.expect = expect;
        self.started = true;
        self.head.clear();
        Ok(())
    }

    /// 源载荷 → 整形 → 播放环（按 `left` 截断，尾随字节忽略）。
    fn feed(&mut self, data: &[u8]) -> Result<(), String> {
        let n = data.len().min(self.left);
        self.left -= n;
        if n == 0 {
            return Ok(());
        }
        let Self { stream, out, .. } = self;
        let stream = stream.as_mut().ok_or("TTS 未起流")?;
        out.clear();
        stream
            .rr3_feed(&data[..n], out)
            .map_err(|e| format!("PCM 整形失败: {e}"))?;
        if !out.is_empty() {
            rr3_play_feed(&out[..]).map_err(str::to_string)?;
        }
        Ok(())
    }

    /// 收尾：整帧校验 → 播放环 end（由播放线程按真实排空计 played/fail）。
    fn finish(&mut self) -> Result<(usize, u64), String> {
        let Some(stream) = self.stream.as_ref() else {
            return Err("TTS 未起流（回包为空或 WAV 头不完整）".into());
        };
        stream.rr3_finish().map_err(str::to_string)?;
        let src = stream.rr3_src_bytes();
        if src == 0 {
            return Err("TTS 无有效 PCM".into());
        }
        if let Some(want) = self.expect {
            if src < want {
                return Err(format!("TTS 回包不完整：{src}/{want}B"));
            }
        }
        let want_ms = rr3_pcm_ms(src, self.channels, TTS_RATE);
        rr3_play_end(want_ms, src).map_err(str::to_string)?;
        Ok((src, want_ms))
    }

    /// 头/首块判定：裸 PCM 立即起流；RIFF 等到 data 块可解析（≤ [`HEAD_CAP`]）。
    fn head_ready(&mut self) -> Result<(), String> {
        if self.head.len() < 4 {
            return Ok(());
        }
        if &self.head[0..4] != b"RIFF" {
            return self.start_raw();
        }
        if self.head.len() < 44 {
            return Ok(());
        }
        self.start_wav()
    }

    /// 裸 s16le：首块即起流，后续块直投。
    fn start_raw(&mut self) -> Result<(), String> {
        let total = self.total;
        let head = core::mem::take(&mut self.head);
        self.start(1, total, total.unwrap_or(MAX_PCM), None)?;
        self.feed(&head)
    }

    /// WAV：解析头 → 定总长 → 起流；data 块头未到则继续收。
    fn start_wav(&mut self) -> Result<(), String> {
        let Some(info) = self.wav_info()? else {
            return Ok(());
        };
        if info.sample_rate != TTS_RATE {
            return Err(format!("TTS 采样率 {} ≠ {TTS_RATE}", info.sample_rate));
        }
        let total = self.wav_total(&info)?;
        let head = core::mem::take(&mut self.head);
        self.start(info.channels, Some(total), total, Some(total))?;
        let payload = head.get(info.data_off..).unwrap_or(&[]);
        self.feed(payload)
    }

    /// 头窗内解析 WAV；`Ok(None)` = data 块头未到、需继续收。
    fn wav_info(&self) -> Result<Option<Rr3WavInfo>, String> {
        match rr3_wav_info(&self.head) {
            Ok(Some(info)) => Ok(Some(info)),
            Ok(None) => Err("WAV 头识别失败".into()),
            Err(e) if e == "WAV 缺 data 块" => {
                if self.head.len() >= HEAD_CAP {
                    Err("WAV 头超出 1KB 窗口".into())
                } else {
                    Ok(None)
                }
            }
            Err(e) => Err(format!("WAV 头不合法: {e}")),
        }
    }

    /// `data` 总长：声明值优先；0/越界用 Content-Length 截断；都不可知即拒。
    fn wav_total(&self, info: &Rr3WavInfo) -> Result<usize, String> {
        let declared = info.data_size;
        let total = match self.total.map(|c| c.saturating_sub(info.data_off)) {
            Some(avail) => {
                if declared == 0 || declared > avail {
                    avail
                } else {
                    declared
                }
            }
            None => declared,
        };
        if total == 0 {
            return Err("WAV data 长度未知或为空".into());
        }
        Ok(total)
    }
}

impl Rr3StreamSink for Rr3TtsSink {
    fn rr3_reset(&mut self) {
        *self = Self::new();
    }

    fn rr3_begin(&mut self, content_len: Option<usize>) {
        self.total = content_len;
    }

    fn rr3_chunk(&mut self, data: &[u8]) -> Result<(), String> {
        if self.stream.is_some() {
            return self.feed(data);
        }
        self.head.extend_from_slice(data);
        self.head_ready()
    }

    fn rr3_started(&self) -> bool {
        self.started
    }

    fn rr3_abort(&mut self) {
        if self.started {
            rr3_play_abort();
        }
    }
}

/// TTS 一条：流式收包并播放；返回 `(PCM 字节, 应播毫秒)`。
///
/// 由 `cloud` / `dialog` 共用；失败已点名（含阶段），调用方只做策略降级。
pub(crate) fn rr3_tts_stream(text: &str) -> Result<(usize, u64), String> {
    let cfg = rr3_cloud_cfg().map_err(|e| format!("云配置缺席: {e}"))?;
    let body = rr_cloud_proto::rr3_tts_body(&cfg.model, text, &cfg.voice, TTS_RATE);
    let mut sink = Rr3TtsSink::new();
    let sent = rr3_http_post_stream(
        "TTS接口",
        &cfg.url(),
        &cfg.key,
        &body,
        "application/json",
        &mut sink,
        MAX_PCM,
    )?;
    match sink.finish() {
        Ok((src, want_ms)) if sent >= src => Ok((src, want_ms)),
        Ok((src, _)) => Err(format!("TTS 回包与 PCM 长度不符 sent={sent} pcm={src}")),
        Err(e) => {
            sink.rr3_abort();
            Err(e)
        }
    }
}
