//! 对话整轮执行（W6-1/W6-2）：语音段 → 云 ASR → 意图 → 云 LLM → 云 TTS →
//! 本板播放 → 表情回落。**线程归 cloud worker**（与 TTS 朗读共用一条 24 KB TLS
//! 栈；W6-3 真板内存门实锤：独立 dialog 线程把 free heap 压到 18 KB，httpd 起栈
//! 失败）。
//!
//! 分工（计划册 §3.2）：VAD/切段在 mic（audio-in），本模块只吃**已完成的一段**
//! 16 kHz 单声道 PCM；三跳请求体/应答解析全部走 [`rr_cloud_proto`] 共享纯函数，
//! 传输走 [`crate::cloud`] 的 `esp_http_client` 面（不引第二份 HTTP 栈）。
//!
//! 意图（W6-1）：[`rr_intent_mech`] 产出结构化动作；S3 单板只有 servo 执行面
//! （relay 动作如实点名跳过），舵机作业走运行面 `Job::Move`，表情四态
//! （listen/think/talk/idle）由 `rr3_expr_set` 切换。
//!
//! 心跳：轮内每步续 `dialog`（超时窗 10 s）；空转心跳由 cloud worker 负责。
//! 播放等待按 `tts_played/tts_fail` 计数收敛，最长 = 估算时长 + 3 s 余量。
//!
//! 凭据零回显：配置只经 [`crate::cloud::rr3_dialog_cfg`] 读 NVS，日志不出 key。

use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use rr_autonomy::Rr3Task;
use rr_intent_mech::wake::{rr3_wake_decide, Rr3WakeAction, Rr3WakeCfg};
use rr_intent_mech::Rr3IntentAction;

use crate::board::MIC_SR_HZ;
use crate::state::{lock_bus, now_ms, push_job, rr3_expr_set, Bus, Job};

/// 单轮补全上限（契约 §2 默认 150）。
const RR3_LLM_MAX_TOKENS: i64 = 150;
/// 系统人设（策略量；回答短、口语化，避免 TTS 长文本）。
const RR3_SYSTEM_PROMPT: &str = "你是桌面机器人小R，用简短口语化的中文回答，一般不超过50字。";
/// 朗读文本上限：180 B ≈ 60 汉字（TTS 512 KB ≈ 16 s 的安全上界；LLM 人设要求
/// ≤50 字，这里做确定性截断，不把超长回复送 TTS）。
const MAX_REPLY: usize = 180;
/// 播放等待轮询步长与余量。
const PLAY_POLL_MS: u64 = 200;
const PLAY_SLACK_MS: u64 = 3_000;
/// 云端失败时的降级播报（契约 §4：不得静默弃轮；TTS 也失败则只留串口点名）。
const RR3_DEGRADE_TEXT: &str = "网络不太通畅，请稍后再试。";
/// S3 唤醒配置：与 classic `modules/rr3-brain/config.toml` 的 `[rr3-brain.wake]`
/// 同值（enable=true、短语表、ack、30s 跟进窗）。云端 ASR 对"洛克"不稳定，
/// 机制核保留"你好"兜底；未命中且窗外 → 静默（不 LLM、不 TTS）。
static S3_WAKE_CFG: LazyLock<Rr3WakeCfg> = LazyLock::new(|| Rr3WakeCfg {
    enable: true,
    phrases: vec!["你好洛克".to_string(), "你好老师".to_string()],
    ack: "我在呢，有什么可以帮您？".to_string(),
    followup_ms: 30_000,
});
/// 最近一次唤醒命中时刻（ms；`None` = 从未唤醒）。cloud worker 单线程调用；
/// xtensa 无 64 位原子，故用 `Mutex` 而非 `AtomicU64`。
static LAST_WAKE_MS: Mutex<Option<u64>> = Mutex::new(None);

/// `/api/board/dialog` body → 单声道 PCM：接受 16 kHz mono WAV 或裸 s16le；
/// 立体声/异采样率显式拒（板侧不做重采样，避免把错误输入送云端）。
pub(crate) fn rr3_dialog_body(body: &[u8]) -> Result<Vec<i16>, String> {
    let (rate, channels, data) = match rr_audio_mech::rr3_wav_pcm(body) {
        Ok(Some(wav)) => (wav.sample_rate, wav.channels, wav.data),
        Ok(None) => (MIC_SR_HZ, 1u16, body),
        Err(e) => return Err(format!("WAV 不合法: {e}")),
    };
    if rate != MIC_SR_HZ {
        return Err(format!("采样率 {rate} ≠ {MIC_SR_HZ}"));
    }
    if channels != 1 {
        return Err(format!("声道 {channels} ≠ 1（对话端点只收单声道）"));
    }
    if data.len() < 2 || data.len() % 2 != 0 {
        return Err("PCM 空或非整样本".into());
    }
    Ok(data
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect())
}

/// 一整轮（内存段）：ASR → 意图执行 → LLM → TTS → 播放等待 → 表情回落。
/// 由 cloud worker 在收 `Rr3CloudJob::Turn` 时调用（线程与 TLS 栈同 TTS）。
pub(crate) fn rr3_turn(bus: &Arc<Mutex<Bus>>, pcm: Vec<i16>) {
    let t0 = rr3_turn_open(bus);
    let asr = rr3_asr_stage(&pcm);
    rr3_turn_text(bus, t0, asr);
}

/// 一整轮（spool 段，无 PSRAM 路径）：从 flash 分块读并流式 ASR，随后释放记录。
/// 无论识别成败都释放——段已消费，`Done` 拒新段会让后续语音全部丢弃。
pub(crate) fn rr3_turn_spool(bus: &Arc<Mutex<Bus>>, info: rr_audio_mech::Rr3SegInfo) {
    let t0 = rr3_turn_open(bus);
    println!(
        "[dialog] spool 段 seq={} len={}B crc={:#010x} 上传",
        info.seq, info.len, info.crc32
    );
    if let Err(why) = rr3_spool_gate() {
        let _ = crate::spool::rr3_spool_clear();
        return rr3_turn_end(bus, &why);
    }
    let asr = rr3_asr_spool(info);
    if let Err(e) = crate::spool::rr3_spool_clear() {
        println!("[dialog] spool 段释放失败: {e}（后续语音段将被背压拒绝）");
    }
    rr3_turn_text(bus, t0, asr);
}

/// 上传前校验：CRC 不符或写线程异常都不送云端（损坏音频只会在云端得到错文本）。
fn rr3_spool_gate() -> Result<(), String> {
    if crate::spool::rr3_spool_verify()? {
        return Ok(());
    }
    Err("spool 段 CRC 不符，弃轮（记录已清理）".into())
}

/// 轮首公共段：计时起点、堆账、心跳、思考表情。
fn rr3_turn_open(bus: &Arc<Mutex<Bus>>) -> u64 {
    let t0 = now_ms();
    crate::mem::rr3_heap_mark("dialog", "start");
    rr3_beat_dialog();
    rr3_face(bus, "think");
    t0
}

/// ASR 之后的共用链（识别文本 → 唤醒门 → 意图 → LLM → TTS）。
fn rr3_turn_text(bus: &Arc<Mutex<Bus>>, t0: u64, asr: Result<String, String>) {
    let text = match asr {
        Ok(t) if !t.trim().is_empty() => t,
        Ok(_) => return rr3_turn_end(bus, "识别为空文本，弃轮"),
        Err(e) => return rr3_turn_fail(bus, &format!("ASR 失败，弃轮: {e}")),
    };
    crate::mem::rr3_heap_mark("dialog", "asr-ok");
    let Some(text) = rr3_wake_gate(bus, &text) else {
        return;
    };
    let actions = rr_intent_mech::rr3_extract_actions(&text);
    let acted = rr3_apply_actions(bus, &actions);
    let reply = match rr3_llm_stage(&text) {
        Ok(r) => rr3_cap_text(&r, MAX_REPLY),
        Err(e) => return rr3_turn_fail(bus, &format!("LLM 失败，弃轮: {e}")),
    };
    crate::mem::rr3_heap_mark("dialog", "llm-ok");
    let speak = rr3_speak_stage(bus, &reply);
    println!(
        "[dialog] 轮完成 {}ms asr='{text}' reply='{reply}' actions={acted} {speak}",
        now_ms().saturating_sub(t0)
    );
    crate::mem::rr3_heap_mark("dialog", "done");
}

/// 唤醒门控：ASR 文本 → 有效对话文本。
///
/// - `Ack`：播固定应答并返回 `None`（不进 LLM，不跑意图）；
/// - `Ignore`：窗外未命中 → 静默，返回 `None`（不 LLM、不 TTS）；
/// - `Command`：唤醒词已剥离，返回命令文本；
/// - `FollowUp`：跟进窗内，返回原文本。
fn rr3_wake_gate(bus: &Arc<Mutex<Bus>>, text: &str) -> Option<String> {
    let now = now_ms();
    let last = rr3_last_wake();
    match rr3_wake_decide(text, &S3_WAKE_CFG, now, last) {
        Rr3WakeAction::Ignore => {
            rr3_turn_end(bus, &format!("未命中唤醒词，静默（ASR='{text}'）"));
            None
        }
        Rr3WakeAction::Ack => {
            rr3_mark_wake(now);
            let ack = S3_WAKE_CFG.ack.clone();
            let speak = rr3_speak_stage(bus, &ack);
            println!("[dialog] 唤醒应答 asr='{text}' ack='{ack}' {speak}");
            None
        }
        Rr3WakeAction::Command(cmd) => {
            rr3_mark_wake(now);
            println!("[dialog] 唤醒命中，命令='{cmd}'");
            Some(cmd)
        }
        Rr3WakeAction::FollowUp => {
            rr3_mark_wake(now);
            Some(text.to_string())
        }
    }
}

/// 最近一次唤醒命中时刻（锁中毒不失效，同 `lock_bus` 纪律）。
fn rr3_last_wake() -> Option<u64> {
    *LAST_WAKE_MS.lock().unwrap_or_else(|p| p.into_inner())
}

/// 记一次唤醒命中（跟进窗起点）。
fn rr3_mark_wake(now: u64) {
    *LAST_WAKE_MS.lock().unwrap_or_else(|p| p.into_inner()) = Some(now);
}

/// 弃轮收尾：表情回落 idle 并打印点名（不 panic，板其余功能照常）。
fn rr3_turn_end(bus: &Arc<Mutex<Bus>>, why: &str) {
    rr3_face(bus, "idle");
    println!("[dialog] {why}");
}

/// 云端失败收尾：先点名，再尝试一句降级播报（契约 §4 不静默弃轮）。
fn rr3_turn_fail(bus: &Arc<Mutex<Bus>>, why: &str) {
    rr3_turn_end(bus, why);
    let speak = rr3_speak_stage(bus, RR3_DEGRADE_TEXT);
    println!("[dialog] 降级播报 {speak}");
}

/// ASR 段（内存）：PCM 切片 → 4 KB 边转边写 → 识别文本。
fn rr3_asr_stage(pcm: &[i16]) -> Result<String, String> {
    let mut src = crate::cloud::Rr3SliceSrc::rr3_new(pcm);
    rr3_asr_upload(pcm.len() * 2, &mut src)
}

/// ASR 段（spool）：按记录长度从 flash 分块读 → 同一份 multipart 上行。
fn rr3_asr_spool(info: rr_audio_mech::Rr3SegInfo) -> Result<String, String> {
    let mut src = crate::cloud::Rr3SpoolSrc::rr3_new(info.len);
    rr3_asr_upload(info.len as usize, &mut src)
}

/// ASR 公共段：同一份 multipart 计划 + 传输（源不同，字节契约一致）。
fn rr3_asr_upload(bytes: usize, src: &mut dyn crate::cloud::Rr3PcmSrc) -> Result<String, String> {
    let cfg = crate::cloud::rr3_dialog_cfg().map_err(|e| format!("云配置缺席: {e}"))?;
    let boundary = format!("rr3{:08x}", crate::state::esp_random_u32());
    let plan =
        rr_cloud_proto::rr3_asr_plan(&cfg.asr_model, bytes, MIC_SR_HZ, &boundary, Some("zh"));
    let ctype = format!("multipart/form-data; boundary={boundary}");
    let url = format!("{}/v1/audio/transcriptions", cfg.base.trim_end_matches('/'));
    let resp = crate::cloud::rr3_post_asr(
        "识别接口",
        &url,
        &cfg.key,
        &ctype,
        &plan,
        src,
        crate::cloud::MAX_JSON,
    )?;
    rr_cloud_proto::rr3_asr_text(&resp)
}

/// LLM 段：user_text → OpenAI 兼容补全 → 回复文本。
fn rr3_llm_stage(text: &str) -> Result<String, String> {
    let cfg = crate::cloud::rr3_dialog_cfg().map_err(|e| format!("云配置缺席: {e}"))?;
    let messages = [
        ("system".to_string(), RR3_SYSTEM_PROMPT.to_string()),
        ("user".to_string(), text.to_string()),
    ];
    let body = rr_cloud_proto::rr3_chat_body(&cfg.llm_model, &messages, RR3_LLM_MAX_TOKENS);
    let url = format!("{}/chat/completions", cfg.llm_base.trim_end_matches('/'));
    let resp = crate::cloud::rr3_post_retry(
        "大模型接口",
        &url,
        &cfg.key,
        &body,
        "application/json",
        crate::cloud::MAX_JSON,
    )?;
    rr_cloud_proto::rr3_reply_text(&resp).ok_or_else(|| "大模型应答形状不符".to_string())
}

/// TTS 段：回复文本 → 云 PCM 4 KB 流式 → 16 KB 播放环 → 有界等待播完 → 表情 idle。
fn rr3_speak_stage(bus: &Arc<Mutex<Bus>>, reply: &str) -> String {
    if reply.trim().is_empty() {
        rr3_face(bus, "idle");
        return "tts=skip(空回复)".to_string();
    }
    crate::mem::rr3_heap_mark("tts", "stream-start");
    let (played0, fail0) = {
        let b = lock_bus(bus);
        (b.st.tts_played, b.st.tts_fail)
    };
    rr3_face(bus, "talk");
    let (pcm, want_ms) = match crate::cloud::rr3_tts_stream(reply, Some(rr3_beat_dialog)) {
        Ok(v) => v,
        Err(e) => {
            rr3_face(bus, "idle");
            return format!("tts_fail={e}");
        }
    };
    crate::mem::rr3_heap_mark("tts", "stream-ok");
    rr3_wait_played(bus, played0, fail0, want_ms + PLAY_SLACK_MS);
    let failed = {
        let b = lock_bus(bus);
        b.st.tts_fail > fail0
    };
    rr3_face(bus, "idle");
    if failed {
        format!("tts_fail=播放中止 pcm={pcm}B")
    } else {
        format!("tts_ms={want_ms} pcm={pcm}B")
    }
}

/// 播放等待：按 `tts_played/tts_fail` 计数收敛，轮询期续 dialog 心跳。
fn rr3_wait_played(bus: &Arc<Mutex<Bus>>, played0: u64, fail0: u32, max_ms: u64) {
    let t0 = now_ms();
    while now_ms().saturating_sub(t0) < max_ms {
        rr3_beat_dialog();
        let (played, fail) = {
            let b = lock_bus(bus);
            (b.st.tts_played, b.st.tts_fail)
        };
        if played > played0 || fail > fail0 {
            return;
        }
        thread::sleep(Duration::from_millis(PLAY_POLL_MS));
    }
    println!("[dialog] 播放等待超窗 {max_ms}ms（tts_played 未推进）");
}

/// 意图动作执行：servo → 推 pan 移动作业；relay 在 S3 无执行面，如实点名跳过。
fn rr3_apply_actions(bus: &Arc<Mutex<Bus>>, actions: &[Rr3IntentAction]) -> usize {
    let mut acted = 0usize;
    for action in actions {
        let Some(deg) = action.rr3_angle_deg().and_then(|a| u32::try_from(a).ok()) else {
            println!(
                "[dialog] 意图动作跳过 topic={}（S3 无 relay 执行面或角非法）",
                action.rr3_topic()
            );
            continue;
        };
        lock_bus(bus).st.wander_on = false; // 外部点名优先，自主扫摆让位
        push_job(
            bus,
            Job::Move {
                pan: Some(deg),
                tilt: None,
            },
        );
        acted += 1;
    }
    acted
}

/// 表情切换（listen/think/talk/idle 四态；未知名由机制件兜底 idle）。
fn rr3_face(bus: &Arc<Mutex<Bus>>, expr: &str) {
    rr3_expr_set(bus, expr);
}

/// 一轮内的进度心跳（ASR/LLM/TTS 各段之间与播放等待）。
fn rr3_beat_dialog() {
    crate::autonomy::rr3_beat(Rr3Task::Dialog);
}

/// 文本上限（按 UTF-8 字符边界截断；TTS 输入与 `/say` 同门）。
fn rr3_cap_text(text: &str, max: usize) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if out.len() + ch.len_utf8() > max {
            break;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成最小 WAV（16 kHz mono 16 bit）。
    fn wav_mono(rate: u32, pcm: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        out.extend_from_slice(pcm);
        out
    }

    #[test]
    fn dialog_body_accepts_mono_wav_and_raw() {
        let pcm = [0x01, 0x00, 0xFE, 0xFF];
        assert_eq!(rr3_dialog_body(&wav_mono(16_000, &pcm)).unwrap(), [1, -2]);
        assert_eq!(rr3_dialog_body(&pcm).unwrap(), [1, -2]);
    }

    #[test]
    fn dialog_body_rejects_wrong_rate_channels_and_odd_bytes() {
        assert!(rr3_dialog_body(&wav_mono(8_000, &[0, 0]))
            .unwrap_err()
            .contains("采样率"));
        let mut stereo = wav_mono(16_000, &[0, 0, 0, 0]);
        stereo[22] = 2; // channels
        assert!(rr3_dialog_body(&stereo).unwrap_err().contains("声道"));
        assert!(rr3_dialog_body(&[0, 1, 2]).unwrap_err().contains("整样本"));
        assert!(rr3_dialog_body(&[]).unwrap_err().contains("整样本"));
    }

    #[test]
    fn cap_text_stops_on_char_boundary() {
        assert_eq!(rr3_cap_text("你好世界", 6), "你好");
        assert_eq!(rr3_cap_text("abc", 3), "abc");
        assert_eq!(rr3_cap_text("你好", 1), "");
    }
}
