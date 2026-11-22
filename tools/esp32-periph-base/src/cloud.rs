//! 板侧云 TTS/对话 worker（W4-2b 起，W7-b2 改流式）：有界作业队列 → NVS 配置 →
//! 4 KB 流式收 PCM → 16 KB 播放环；ASR/LLM 仍走整段小 JSON。
//!
//! 契约（docs/process/dev/单板全链云API契约-2026-11-19.md）：
//! - 只请求 `response_format=pcm`、16 kHz；不做 MP3/AAC 解码；
//! - 单次 8 s 超时、最多 2 次尝试、500 ms 间隔；429/5xx/传输失败可重试；
//!   流式 TTS 一旦开始播不再重试（重放音频比失败更糟）；
//! - key 只存 NVS、永不出日志/回显（零回显墙同 `cred`）；
//! - 传输层用 IDF `esp_http_client` + mbedTLS 证书束，不引 ureq/rustls。
//!
//! 线程模型：一条常驻 worker（24 KB 栈——TLS 握手栈需求高于普通 IO 线程），
//! 有界作业队列（容量 2）串行处理 TTS 朗读 / 对话整轮 / 已收全 PCM 播放。
//! 播放面负责半双工门与 `audio-out` 心跳；本模块只管取数与错误点名。
//!
//! 凭据/配置读写拆在 [`cfg`]（NVS 与形制门），HTTP 在 [`http`]，TTS 流式接收在
//! [`tts`]；本文件只留队列与编排。

use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::state::Bus;

mod cfg;
mod http;
mod tts;
pub(crate) use cfg::{rr3_cloud_write, rr3_dialog_cfg, rr3_say_body};
pub(crate) use http::{
    rr3_post_asr, rr3_post_retry, Rr3PcmSrc, Rr3SliceSrc, Rr3SpoolSrc, MAX_JSON,
};
pub(crate) use tts::rr3_tts_stream;

/// TTS 流式接收上限：512 KB ≈ 16 s @16k mono s16（2026-11-20 真板实锤：LLM
/// 45 字回复的 CosyVoice PCM ≈9 s/288 KB）。流式路径不再整段驻留，此值只做
/// 防御性封顶（超限中止并点名）。
const MAX_PCM: usize = 524_288;
/// 作业队列容量（条；满即拒，不阻塞 HTTP 任务）。
const TTS_Q_CAP: usize = 2;
/// worker 栈：mbedTLS 握手 + esp_http_client 缓冲。
const TTS_STACK: usize = 24_576;
/// worker 空转周期（只为及时退出，不承担心跳——dialog 归 W6）。
const IDLE_MS: u64 = 2_000;

/// 云端 worker 作业（TTS 朗读 / 对话整轮 / 已收全 PCM 播放；同一条 TLS 线程，
/// 省一份 24 KB 栈）。
pub(crate) enum Rr3CloudJob {
    Say(String),
    Turn(Vec<i16>),
    /// 已提交到 flash spool 的语音段：worker 从 spool 分块读并流式 ASR（无 PSRAM 路径）。
    Spool(rr_audio_mech::Rr3SegInfo),
    /// `/api/board/play` 的已收全 PCM：入队后由 worker 喂 16 KB 环，HTTP 回调不阻塞。
    Play(Vec<u8>),
}

/// 云端 worker 发送端（未装配 = None）。
static TTS_TX: Mutex<Option<SyncSender<Rr3CloudJob>>> = Mutex::new(None);

/// 对话整轮策略入口（由装配面注入，cloud 只持机制：队列/传输/线程）。
pub(crate) type Rr3TurnFn = fn(&Arc<Mutex<Bus>>, Vec<i16>);
/// spool 段整轮策略入口（同注入纪律）。
pub(crate) type Rr3SpoolFn = fn(&Arc<Mutex<Bus>>, rr_audio_mech::Rr3SegInfo);

/// 起云端 worker（httpd 之前调用，避免 `/say` 与 `/dialog` 撞"未装配"窗）。
/// 两个整轮入口由调用方注入（`dialog::rr3_turn` / `dialog::rr3_turn_spool`）——
/// cloud 不反向依赖策略层。
pub(crate) fn spawn_tts(bus: Arc<Mutex<Bus>>, turn: Rr3TurnFn, spool: Rr3SpoolFn) {
    let (tx, rx) = sync_channel(TTS_Q_CAP);
    let _ = TTS_TX.lock().map(|mut slot| *slot = Some(tx));
    println!("[cloud] 云端 worker 起（TTS 流式 + 对话整轮 + spool 段；单条 TLS 栈）");
    let _ = thread::Builder::new()
        .name("cloud-dialog".into())
        .stack_size(TTS_STACK)
        .spawn(move || cloud_run(rx, bus, turn, spool))
        .map_err(|e| println!("[cloud] 起云端线程失败: {e:?}"));
}

/// 入队一条朗读请求（满/未装配/线程退出都如实回错，绝不阻塞 HTTP 任务）。
pub(crate) fn rr3_say(text: String) -> Result<(), &'static str> {
    rr3_enqueue(Rr3CloudJob::Say(text))
}

/// 入队一整轮对话（mic 切段 / `/api/board/dialog` 共用；不阻塞调用方）。
pub(crate) fn rr3_turn(pcm: Vec<i16>) -> Result<(), &'static str> {
    rr3_enqueue(Rr3CloudJob::Turn(pcm))
}

/// 入队一段已提交的 spool 语音段（spool 写线程提交后调用）。
pub(crate) fn rr3_turn_spool(info: rr_audio_mech::Rr3SegInfo) -> Result<(), &'static str> {
    rr3_enqueue(Rr3CloudJob::Spool(info))
}

/// 入队一段已收全 PCM（`/api/board/play`）：worker 串行喂播放环，HTTP 不阻塞。
pub(crate) fn rr3_play_async(pcm: Vec<u8>) -> Result<(), &'static str> {
    rr3_enqueue(Rr3CloudJob::Play(pcm))
}

/// 公共入队（错误文案按 job 面口径统一）。
fn rr3_enqueue(job: Rr3CloudJob) -> Result<(), &'static str> {
    let Ok(slot) = TTS_TX.lock() else {
        return Err("云线程锁中毒");
    };
    let Some(tx) = slot.as_ref() else {
        return Err("云线程未装配");
    };
    tx.try_send(job).map_err(|e| {
        if matches!(e, TrySendError::Full(_)) {
            "云队列满"
        } else {
            "云线程已退出"
        }
    })
}

/// worker 主体：Say → 流式 TTS → 播放环；Turn/Spool → 注入的整轮策略；
/// Play → 整段喂环；空转续 dialog 心跳。
fn cloud_run(rx: Receiver<Rr3CloudJob>, bus: Arc<Mutex<Bus>>, turn: Rr3TurnFn, spool: Rr3SpoolFn) {
    loop {
        match rx.recv_timeout(Duration::from_millis(IDLE_MS)) {
            Ok(Rr3CloudJob::Say(text)) => tts_say(&text),
            Ok(Rr3CloudJob::Turn(pcm)) => {
                crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Dialog);
                turn(&bus, pcm);
            }
            Ok(Rr3CloudJob::Spool(info)) => {
                crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Dialog);
                spool(&bus, info);
            }
            Ok(Rr3CloudJob::Play(pcm)) => rr3_play(&pcm),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Dialog);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                println!("[cloud] 作业队列断开，worker 退出");
                return;
            }
        }
    }
}

/// 整段喂播放环（HTTP 回调不阻塞；失败点名不弃线程）。
fn rr3_play(pcm: &[u8]) {
    if let Err(e) = crate::playout::rr3_play_stream(pcm) {
        println!("[cloud] 播放失败: {e}");
    }
}

/// 朗读一条：云端 TTS 4 KB 流式收包 → 16 KB 播放环（半双工门由播放面收尾）。
fn tts_say(text: &str) {
    let chars = text.chars().count();
    match rr3_tts_stream(text, None) {
        Ok((pcm, want_ms)) => {
            println!("[cloud] TTS流 ok chars={chars} pcm={pcm}B 应播={want_ms}ms（16KB 环）")
        }
        Err(e) => println!("[cloud] TTS 失败 chars={chars}: {e}"),
    }
}
