// SPDX-License-Identifier: Apache-2.0
//! rr3-audio-rec —— 音频上行收流端（宿主）：板面双麦 PCM → UART → wav。
//!
//! 链路：板 `mic` 分片（`rr3-sense/audio/pcm` Event，dst=1）→ UART2 1.5M → 本件
//! `AUDIO ON` 起流 → dst/topic 双门 → b64 解码 → 累积 PCM → `AUDIO OFF` 后按实测采样率
//! 写双通道 s16le wav。帧语义/串口/b64 全走 rr-ipc 单点。
//! 三级防护：① 读超时（`rr3_read_timeout`，`Ok(0)` = 暂无数据非 EOF）；② 失步重同步
//! （坏帧弃 4B magic 续扫，连续 [`ERR_STREAK_RESYNC`] 次记一次）；③ 断流重连（见过数据
//! 后连续 [`RESYNC_ZERO_BAIL`] 次零读 = 断流，重开口 + settle + `AUDIO ON`）。
//! 线程分工：读线程裸读、解码线程纯内存拆帧（[`FrameAssembler`]）、写线程落盘；看门狗
//! （[`STALL`] 无新分片）补 wav 恢复头并 exit 3，重连期解武装。开口纪律：open 后停 1.2s
//! 等板复位，`rr3_flush_input` 净化；握手先发防御 `AUDIO OFF`（旧会话残流会抢在 OK 前），
//! 停流冲净才 `AUDIO ON`。seq 只对账不补救：断档即背压丢片。
//! 用法：`<device> [--out <path>] [--secs <n>] [--baud <n>] [--doa]`；退出码 0 收兵 /
//! 1 开口握手或磁盘失败 / 2 参数错 / 3 看门狗停滞 / 4 重连失败（1/3/4 先补恢复头）。

mod args;
mod decode;
mod event;
mod exit;
mod reader;
mod session;
mod state;
mod track;
mod watch;
mod wav;
mod writer;

#[cfg(test)]
mod tests;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rr_ipc::FrameAssembler;

use args::parse_args;
use decode::{decode_loop, DecodeState};
use session::{finish_and_report, open_handshake, rec_spawn_threads};
use watch::rr3_watch_state;
use wav::rr3_wav_create;

// ---------------------------------------------------------------------------
// 两端帧头约定（板不依赖 rr-core，板/收流端常量自证——board::RR3_*_TASK 同纪律；
// 与宿主 rr-hal::esp32_uart 的同值锚定测试同一防静默分叉口径）
// ---------------------------------------------------------------------------

/// 宿主 task 位形（帧头 dst 门：只收发给宿主的 Event）。
pub(crate) const RR3_HOST_TASK: u64 = 1;
/// 板面外设执行面 task 位形（帧头 src，诊断显示用）。**仅测试构造帧用**——本件
/// 只收流落盘、不发布帧，故生产路径无消费者。
#[cfg(test)]
pub(crate) const RR3_PERIPH_TASK: u64 = 2;
/// 采样率兜底（Hz）：只在流内**未声明** `rate` 时用（板面 `mic::MIC_SR_HZ` 缺省）。
/// 声明的值才是真值——硬编码会让板面换采样档后宿主静默写错 wav 头。
pub(crate) const NOMINAL_RATE_FALLBACK_HZ: u32 = 16000;
/// 音频 PCM 上行主题（在册 stream L1：contracts/topics.toml）。
pub(crate) const RR3_AUDIO_TOPIC: &str = "rr3-sense/audio/pcm";
/// 停滞死线（看门狗）：**武装后的收流期**内无新分片字节即判停滞。分片节拍
/// 128ms/片，10s ≈ 78 片空窗——正常流不可达；重连握手重试期（≈90s 骑行窗）
/// 与重同步窗口（1.6s）看门狗解武装不计停滞，否则长骑行必被误杀；触发即实锤
/// "武装收流中，分片字节 10s 未确认入队"。
pub(crate) const STALL: Duration = Duration::from_secs(10);
// 开口 settle/应答超时/断流判据/大块裸读窗/重连预算七常量与握手三函数已上提
// rr-ipc 单点（`rr_ipc::audio_session`，A 批 c2 随 rr-hal esp32 音频源后端共用；
// 本件 import 接线，不再持第二份会话机制）。

/// 连续拆帧失败达此数记一次失步重同步账（日志对账；坏帧弃扫本身由
/// [`FrameAssembler`] 在内存里即时完成，无独立扫描动作——1-2 次可能是单帧毛刺，
/// 3 次 = 帧界成片损坏，值得一条日志点名）。
pub(crate) const ERR_STREAK_RESYNC: u64 = 3;
/// 解码线程等裸字节的醒来节拍（查收兵死线；断流/重连由读线程自治，解码面
/// 只管出帧与记账）。
pub(crate) const DECODE_TICK: Duration = Duration::from_millis(200);

// ---------------------------------------------------------------------------

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("rr3-audio-rec: {e}\n\n用法: rr3-audio-rec <device> [--out <path>] [--secs <n>] [--baud <n>] [--doa]\n缺省 out=rr3-audio-<unix秒>.wav secs=30 baud=1500000（板面 2026-10-30 定档）；--doa 逐 1s 窗 ILD 出角（批D⑤ 冒烟）");
            std::process::exit(2);
        }
    };
    let conn = open_handshake(&args.device, args.baud);

    // 收流面线程化（判据② A/B 定案）：读线程只做大块裸读推裸字节通道；解码
    // 主循环纯内存拆帧 → 四道门 → 逐片交写线程落盘 + seq 对账。落盘线程化
    // （d4 /dev/null 定案）：解码循环零磁盘 I/O——逐片同步写刷的延迟尖峰把
    // 冻结概率放大 ~7 倍，通道解耦后尖峰不再进收流面；看门狗保数据前提不变。
    let out_path = args.out.clone();
    let wav_file = rr3_wav_create(&out_path);
    let watch = rr3_watch_state();
    let threads = rec_spawn_threads(
        conn,
        wav_file,
        Arc::clone(&watch),
        &out_path,
        &args.device,
        args.baud,
    );
    watch.armed.store(true, Ordering::Release);

    let mut st = DecodeState::new(args.doa);
    let deadline = Instant::now() + Duration::from_secs(args.secs);
    let mut asm = FrameAssembler::new();
    println!(
        "[rec] streaming {}s @ {} ({})…",
        args.secs, args.baud, args.device
    );
    decode_loop(
        &threads.byte_rx,
        &mut asm,
        &mut st,
        deadline,
        &watch,
        &threads.tx,
        &out_path,
    );
    finish_and_report(threads, &watch, st, &out_path, args.doa);
}
