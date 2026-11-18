//! rr3-audio-rec —— 批D③ 音频上行收流端（宿主；0.0.5 增补轮五 全案 §4 批D 判据①）。
//!
//! 链路：板面 INMP441 双麦（mic.rs 分片组帧，`rr3-sense/audio/pcm` Event @
//! trace_id=0 / src=2 / dst=1）→ UART2 1.5M（板固件 `board::UART2_BAUD_HZ` 同档；
//! 2026-10-30 自 921600 提档定档，音频 85KB/s 占线从 ~95% 降到 ~46%）→
//! 本件收流：`AUDIO ON` 行命令起流 → dst=1 门 → Event payload → topic 门 →
//! b64 解码 → 累积 PCM → `AUDIO OFF` → 按**实测采样率**写 wav（双通道 s16le）。
//!
//! 纪律：
//! - 帧语义/串口/b64 全部 rr-ipc 单点（`Rr3UartConn` + blanket impl
//!   `Rr3TransportConn` + `rr3_base64_decode`），零第二份帧层；
//! - 载荷形状事实源 modules/rr3-sense composite::`rr3_pcm_event_body`（seq/format/
//!   rate/channels/ms/pcm_b64），topic 键并入体（rr-bus `rr3_publish` 同形——板无
//!   rr-bus 客户端，按 `body["topic"]` 形状自行并入）；
//! - 开口 settle 纪律（批C 实锤）：open 后停 1.2s 让开口翻转的板复位走完，再
//!   `rr3_flush_input` 净化残留；握手先发**防御 AUDIO OFF**（上一会话看门狗/
//!   异常退出的板面残流会抢在 OK 应答前进线，d4 重跑实锤），停流冲净才 AUDIO ON。
//! - seq 连续性记账（wrapping u32）：断档即板侧背压丢片（`INFO` 的 `drop` 互为
//!   对账）；大步进（≥ 0x8000_0000）= 板侧重启记 `restarts`，不按 wrapping 补
//!   天文 gap。收流端只对账不补救（fail-honest：wav 是真实收到的流）。
//! - 三级防护（0.0.5 批D④ d4 600s 实锤；机制与策略分离；判据② 收流线程化——
//!   A/B 决定性实验定案：病灶是分相读节奏（128B 头→plen 载荷两段间让出），
//!   85KB/s 连续流下间歇 RX 溢出丢字节；python read(65536) 同款大块裸读 600s
//!   满速零污染。读线程只做大块裸读推裸字节通道，解码线程纯内存拆帧
//!   （[`FrameAssembler`]，头校验全数 rr-ipc [`Rr3Header::from_bytes`] 单点，
//!   零第二份帧语义））：
//!   ① 读超时——机制面 rr-ipc `rr3_read_timeout`（poll 等就绪，`Ok(0)`=此刻无
//!   数据非 EOF），读线程裸读/应答读全走超时档；阻塞读缺省语义保持为 rr-ipc
//!   其余用户（桥/哨兵）不动。
//!   ② 失步重同步——帧头 magic 线上字节 = "1ERA"（全 b64 字母表字符），失步后
//!   在载荷里撞上即"合法幻影帧头"+ 天文 payload_len。坏帧起点弃 4B magic 续扫
//!   （字节已在缓冲，弃扫零新 I/O——滑移不放大为风暴的关键）；连续
//!   [`ERR_STREAK_RESYNC`] 次拆帧失败记一次重同步账（日志对账用）。
//!   ③ 断流重连——读线程见过数据后连续 [`RESYNC_ZERO_BAIL`] 次零读（1.6s
//!   静默）= 线上断流，就地重开口 settle + AUDIO ON 续收（seq 重走/续走都如实
//!   记账：板复位 `restarts`，续走空洞入 `gaps`）；握手 [`HANDSHAKE_TRIES`] 次
//!   重试、每轮重开 fd（宿主侧 USB RX 冻结数分钟内自愈、板面四轮洗冤，重开 fd
//!   是宿主侧唯一捅法，骑行期看门狗解武装）。
//!   落盘走写线程（解码循环零磁盘 I/O，/dev/null 实验定案：逐片同步写刷是冻结
//!   放大器）；看门狗（武装后 [`STALL`] 无新分片字节 → 补 wav 恢复头 exit(3)）
//!   是最外层兜底；只管武装后的收流期，重连骑行期解武装不误触发。
//!
//! 用法：`rr3-audio-rec <device> [--out <path>] [--secs <n>] [--baud <n>] [--doa]`
//! 缺省 out=`rr3-audio-<unix秒>.wav`、secs=30、baud=1500000（板面 2026-10-30 定档
//! 实测档，与固件 `board::UART2_BAUD_HZ` 同值；
//! 波特率仍过 rr-ipc 档位表白名单，不预设动物园）。
//! 退出码：0=收兵；1=开口/握手/0 分片/磁盘写失败；2=参数错；3=看门狗停滞恢复；
//! 4=重连失败/收流线程亡（1/3/4 都先补恢复头保住已落盘数据再退）。

mod args;
mod decode;
mod event;
mod exit;
mod reader;
mod session;
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
