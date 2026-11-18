//! 读线程：会话主循环、拆帧记账、命令排水与心跳节拍。

use std::io::Write as _;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use rr_ipc::audio_session::{READ_BLOCK, READ_TIMEOUT_MS, RESYNC_ZERO_BAIL};
use rr_ipc::{FrameAssembler, Rr3FrameBuf, Rr3Header, Rr3TransportConn, Rr3UartConn};
use rr_periph_proto::cmd::Rr3PeriphCmd;

use super::cmd::Rr3AudioCmdConsumer;
use super::core::{
    rr3_esp32_reconnect, rr3_stat_tick, rr3_tee_open, rr3_tee_write, Rr3Esp32AudioCore,
    Rr3StatMark, RR3_BRIDGE_STAT_PERIOD, RR3_PING_PERIOD,
};
use super::evt::{rr3_err_log_now, rr3_esp32_classify, Rr3BoardFrameIn, Rr3Esp32AudioChunk};
use super::ring::rr3_s16le_decode;
use super::rr3_esp32_cmd_frame;

/// 命令旁路排水（每拍先排水再读）：逐条弹出 fire-and-forget 写出（板侧执行前
/// 回成功，回执由分类面记账）；io 失败 = 连接坏，命令回插队首不丢，调用方按
/// 读失败同路重连后补写（有界丢最旧兜底）。b3 冒烟修正：原实现把排水放在建连
/// 拍——会话中途入队的命令永远等不到写出口（队容 2 满丢最旧），板面收不到
/// servo 点名——b1 计划原文即「读循环每拍先排水再读」，此为落位笔误。
fn rr3_drain_cmds(
    c: &mut Rr3UartConn,
    core: &Rr3Esp32AudioCore,
    cmd_id: &mut u64,
) -> Result<(), String> {
    let Some(rx) = core.cmd.as_ref() else {
        return Ok(());
    };
    while let Some(cmd) = rx.rr3_cmd_pop() {
        rr3_drain_one(c, rx, cmd, cmd_id)?;
    }
    Ok(())
}

/// 写出一条命令：失败回插队首并交回错误（命令不丢，重连后补写）。
fn rr3_drain_one(
    c: &mut Rr3UartConn,
    rx: &Rr3AudioCmdConsumer,
    cmd: Rr3PeriphCmd,
    cmd_id: &mut u64,
) -> Result<(), String> {
    let frame = rr3_esp32_cmd_frame(*cmd_id, &cmd);
    if let Err(e) = c.rr3_send_buf(Rr3FrameBuf::from(&frame)) {
        rx.rr3_cmd_push_front(cmd);
        return Err(format!("命令写出失败（回插队首，连接报废转重连）: {e:?}"));
    }
    *cmd_id += 1;
    Ok(())
}

/// 读线程主体：会话内大块裸读 → 拆帧 → 四道门 → 入环；断流/错会话就地转
/// 重连。收兵（stop 旗）时板面 `AUDIO OFF`（防残流，conn 随作用域释放 →
/// flock 退账）。
/// 读线程跨会话持久状态（tee/记账/计时；重连不清零）。
struct Rr3ReaderState {
    tee: Option<std::fs::File>,
    once: Rr3IngestLog,
    cmd_id: u64,
    stat_at: Instant,
    stat_mark: Rr3StatMark,
    ping_at: Instant,
}

impl Rr3ReaderState {
    fn rr3_new() -> Self {
        Self {
            // 取证 tee（批N12 件二）：线程级开口一次，重连续写同一文件（见 [`rr3_tee_open`](super::core::rr3_tee_open)）
            tee: rr3_tee_open(),
            once: Rr3IngestLog::default(),
            // 命令旁路请求 id 计数（读线程局部；fire-and-forget 面只服务回执对账日志，
            // 无跨会话唯一性要求）
            cmd_id: 1,
            // 桥链周期统计计时（统计面与链路控制解耦：到点只打一行，不改任何状态）
            stat_at: Instant::now(),
            // 统计水位（上一窗累计值；本窗增量 = 现值 − 水位）
            stat_mark: Rr3StatMark::default(),
            // 宿主音频心跳计时（A9.3：板侧失联看门狗的时间源）
            ping_at: Instant::now(),
        }
    }
}

pub(super) fn rr3_esp32_reader(core: Arc<Rr3Esp32AudioCore>, first: Rr3UartConn) {
    let mut st = Rr3ReaderState::rr3_new();
    let mut conn: Option<Rr3UartConn> = Some(first);
    while !core.stop.load(Ordering::Acquire) {
        rr3_reader_round(&core, &mut conn, &mut st);
    }
    // 收兵：板面停流（防御残流），conn 释放 → flock 退账
    rr3_reader_tail(conn);
}

/// 读循环一拍：无连接先重连；有连接跑一整个会话，坏链即摘除待下拍重连。
fn rr3_reader_round(
    core: &Rr3Esp32AudioCore,
    conn: &mut Option<Rr3UartConn>,
    st: &mut Rr3ReaderState,
) {
    let Some(c) = conn.as_mut() else {
        *conn = rr3_esp32_reconnect(core);
        return;
    };
    if rr3_esp32_stream(core, c, st) {
        *conn = None;
    }
}

/// 读线程收尾：残留连接上补发停流（conn 释放即 flock 退账）。
fn rr3_reader_tail(conn: Option<Rr3UartConn>) {
    if let Some(mut c) = conn {
        let _ = c.write_all(b"AUDIO OFF\n");
    }
}

/// 单会话读循环：`true` = 连接坏需重连，`false` = 收兵（stop 旗）。
fn rr3_esp32_stream(
    core: &Rr3Esp32AudioCore,
    c: &mut Rr3UartConn,
    st: &mut Rr3ReaderState,
) -> bool {
    // —— 会话内读循环（拆帧器/记账随会话重置：新会话是新流）——
    let mut asm = FrameAssembler::new();
    let mut raw = vec![0u8; READ_BLOCK];
    let mut zeros = 0u32;
    let mut seen = false;
    let mut err_streak = 0u64;
    // 累计坏帧（拆帧失败 + 未过门）：间歇性丢帧的唯一可见口径（见上注）
    let mut err_total = 0u64;
    loop {
        if core.stop.load(Ordering::Acquire) {
            return false;
        }
        if !rr3_stream_beat(core, c, st) {
            return true;
        }
        match rr3_read_step(c, &mut raw, &mut zeros, &mut seen) {
            Rr3ReadStep::Reconnect => return true,
            Rr3ReadStep::Data(0) => {}
            Rr3ReadStep::Data(n) => rr3_stream_data(
                core,
                &raw[..n],
                &mut asm,
                st,
                &mut err_streak,
                &mut err_total,
            ),
        }
    }
}

/// 每拍会话杂务：统计窗、宿主心跳、命令旁路排水；`false` = 连接坏需重连。
fn rr3_stream_beat(core: &Rr3Esp32AudioCore, c: &mut Rr3UartConn, st: &mut Rr3ReaderState) -> bool {
    rr3_stat_maybe(core, st);
    // —— 宿主心跳（A9.3）：与命令同线程串行写，5s 一个空行（非协议命令，
    // 任何版本固件都静默忽略；写失败即连接坏，与读失败同路重连）——
    if !rr3_ping(c, &mut st.ping_at) {
        return false;
    }
    // —— 命令旁路排水（每拍先排水再读）：逐条弹出 fire-and-forget 写出
    // （板侧执行前回成功，回执由分类面记账）；io 失败 = 连接坏，命令回插
    // 队首不丢，按读失败同路重连后补写（有界丢最旧兜底）。b3 冒烟修正：
    // 原实现把排水放在建连拍——会话中途入队的命令永远等不到写出口
    // （队容 2 满丢最旧），板面收不到 servo 点名——b1 计划原文即
    // 「读循环每拍先排水再读」，此为落位笔误。
    rr3_beat_drain(c, core, st)
}

/// 命令旁路排水一拍：io 失败点名后回 false（连接坏，命令回插队首不丢）。
fn rr3_beat_drain(c: &mut Rr3UartConn, core: &Rr3Esp32AudioCore, st: &mut Rr3ReaderState) -> bool {
    if let Err(e) = rr3_drain_cmds(c, core, &mut st.cmd_id) {
        eprintln!("rr-hal-esp32-audio: {e}");
        return false;
    }
    true
}

/// 统计窗到期即出窗（帧数/坏帧/重连增量打点，10min 一条）。
fn rr3_stat_maybe(core: &Rr3Esp32AudioCore, st: &mut Rr3ReaderState) {
    if st.stat_at.elapsed() >= RR3_BRIDGE_STAT_PERIOD {
        st.stat_at = Instant::now();
        rr3_stat_tick(core, &mut st.stat_mark);
    }
}

/// 一拍新数据：原始流 tee → 拆帧 → 逐帧落账。
fn rr3_stream_data(
    core: &Rr3Esp32AudioCore,
    bytes: &[u8],
    asm: &mut FrameAssembler,
    st: &mut Rr3ReaderState,
    err_streak: &mut u64,
    err_total: &mut u64,
) {
    // 批N12 件二：宿主读面原始字节 tee（拆帧前镜像——批N10 件二
    // 病灶"帧内尾随残渣"的取证层，tee 流与消费流逐字节一致）
    rr3_tee_write(&mut st.tee, bytes);
    asm.feed(bytes);
    rr3_esp32_frames(core, asm, &mut st.once, err_streak, err_total);
}

/// 宿主心跳到点一拍（A9.3）：`true` = 本拍无心跳或写成功；`false` = 写失败
/// （已点名，连接不可用）。空行不是协议命令，新旧固件都静默忽略。
fn rr3_ping(c: &mut Rr3UartConn, at: &mut Instant) -> bool {
    if at.elapsed() < RR3_PING_PERIOD {
        return true;
    }
    *at = Instant::now();
    rr3_ping_send(c)
}

/// 写出一个空行心跳；失败点名并回 false（连接不可用）。
fn rr3_ping_send(c: &mut Rr3UartConn) -> bool {
    if let Err(e) = c.write_all(b"\n") {
        eprintln!("rr-hal-esp32-audio: 心跳写出失败: {e} → 重连");
        return false;
    }
    true
}

/// 读一拍裁决：`Data(n)` = 读到 n 字节（n=0 即本拍无数据）；`Reconnect` =
/// 读失败或见过数据后连续静默达阈值（均已点名，调用方报废连接重开）。
enum Rr3ReadStep {
    Data(usize),
    Reconnect,
}

/// 会话内读一拍：超时窗口裸读 + 零读计数/断流判据（计数与 `seen` 跨拍保留）。
fn rr3_read_step(
    c: &mut Rr3UartConn,
    raw: &mut [u8],
    zeros: &mut u32,
    seen: &mut bool,
) -> Rr3ReadStep {
    match c.rr3_read_timeout(raw, READ_TIMEOUT_MS) {
        Err(e) => {
            eprintln!("rr-hal-esp32-audio: 读失败: {e} → 重连");
            Rr3ReadStep::Reconnect
        }
        Ok(0) => rr3_read_zero(zeros, *seen),
        Ok(n) => {
            *zeros = 0;
            *seen = true;
            Rr3ReadStep::Data(n)
        }
    }
}

/// 零读一拍的裁决：见过数据后连续静默达阈值即判断流。
fn rr3_read_zero(zeros: &mut u32, seen: bool) -> Rr3ReadStep {
    // 此刻无数据（EOF 形状）；见过数据后连续零读 = 断流
    if seen {
        *zeros += 1;
        if *zeros >= RESYNC_ZERO_BAIL {
            eprintln!(
                "rr-hal-esp32-audio: 断流（连续 {RESYNC_ZERO_BAIL}×{READ_TIMEOUT_MS}ms 静默）→ 重连"
            );
            return Rr3ReadStep::Reconnect;
        }
    }
    Rr3ReadStep::Data(0)
}

/// 会话内「只打一次」观测旗标（跨重连保留：首帧就绪/首次溢丢/首条回执各打一行）。
#[derive(Default)]
struct Rr3IngestLog {
    ready: bool,
    overflow: bool,
    ack: bool,
}

/// 会话内帧落账：把拆帧器已喂入的字节逐帧弹出 → 分类 → 入环/记账。
/// 断流与读失败留在读循环（连接级），本函数只处理"已到手的完整帧"；
/// `err_streak/err_total` 是会话内计数（重连即新流），调用方持有。
fn rr3_esp32_frames(
    core: &Rr3Esp32AudioCore,
    asm: &mut FrameAssembler,
    once: &mut Rr3IngestLog,
    err_streak: &mut u64,
    err_total: &mut u64,
) {
    loop {
        match asm.pull() {
            Ok(Some((hdr, body))) => rr3_esp32_frame(core, once, err_streak, err_total, hdr, body),
            Ok(None) => break,
            Err((discarded, why)) => rr3_esp32_lost(core, err_streak, err_total, discarded, &why),
        }
    }
}

/// 一帧的分类落账：好帧入环/记账，坏帧按门故障计数。
fn rr3_esp32_frame(
    core: &Rr3Esp32AudioCore,
    once: &mut Rr3IngestLog,
    err_streak: &mut u64,
    err_total: &mut u64,
    hdr: Rr3Header,
    body: Vec<u8>,
) {
    match rr3_esp32_classify(&hdr, &body) {
        Ok(frame) => rr3_esp32_take(core, once, err_streak, frame),
        Err(e) => rr3_esp32_miss(core, err_streak, err_total, &e),
    }
}

/// 过门好帧的两类落账：音频入环并清连续坏帧计数，回执按成败记账。
fn rr3_esp32_take(
    core: &Rr3Esp32AudioCore,
    once: &mut Rr3IngestLog,
    streak: &mut u64,
    frame: Rr3BoardFrameIn,
) {
    match frame {
        Rr3BoardFrameIn::Audio(chunk) => {
            *streak = 0;
            rr3_esp32_take_audio(core, once, chunk);
        }
        Rr3BoardFrameIn::Reply { id, err } => rr3_esp32_take_reply(once, id, err),
    }
}

/// 音频帧入环：首帧就绪与首次溢丢各打一行（会话内只打一次）。
fn rr3_esp32_take_audio(
    core: &Rr3Esp32AudioCore,
    once: &mut Rr3IngestLog,
    chunk: Rr3Esp32AudioChunk,
) {
    core.chunks.fetch_add(1, Ordering::Relaxed);
    rr3_audio_ready_once(once, chunk.rate);
    let samples = rr3_s16le_decode(&chunk.samples);
    let mut ring = core.ring.lock().unwrap_or_else(|p| p.into_inner());
    ring.rr3_ring_push(&samples, core.channels as usize);
    rr3_overflow_once(once, ring.rr3_ring_dropped());
}

/// 首帧就绪打点（会话内只打一次）。
fn rr3_audio_ready_once(once: &mut Rr3IngestLog, rate: u32) {
    if once.ready {
        return;
    }
    once.ready = true;
    eprintln!("rr-hal-esp32-audio: 板面流就绪（实测 rate={rate} ch=2 s16le）");
}

/// 首次溢丢打点（会话内只打一次：稳态钟滑移损耗不刷屏）。
fn rr3_overflow_once(once: &mut Rr3IngestLog, dropped: u64) {
    if dropped == 0 || once.overflow {
        return;
    }
    once.overflow = true;
    eprintln!("rr-hal-esp32-audio: 环形缓冲开始溢丢最旧（板-宿主钟滑移稳态损耗，VAD/ASR 可容忍）");
}

/// 命令回执：业务失败逐条点名；成功回执只打首条（高频命令面不刷屏，首条
/// 已证明回执通道活着）。
fn rr3_esp32_take_reply(once: &mut Rr3IngestLog, id: u64, err: Option<String>) {
    match err {
        Some(e) => eprintln!("rr-hal-esp32-audio: 板面应答 id={id} 业务失败: {e}"),
        None => rr3_take_ack_log(once, id),
    }
}

/// 成功回执只打首条（首条已证明回执通道活着，高频面不刷屏）。
fn rr3_take_ack_log(once: &mut Rr3IngestLog, id: u64) {
    if once.ack {
        return;
    }
    once.ack = true;
    eprintln!("rr-hal-esp32-audio: 板面命令回执就绪（id={id} ok；后续成功回执不逐条打印）");
}

/// 四道门未过（帧界损坏/载荷 CRC 不符）：连续与累计双计数，首条与每千条点名。
fn rr3_esp32_miss(core: &Rr3Esp32AudioCore, streak: &mut u64, total: &mut u64, why: &str) {
    *streak += 1;
    *total += 1;
    core.errs.fetch_add(1, Ordering::Relaxed);
    if rr3_err_log_now(*total) {
        eprintln!(
            "rr-hal-esp32-audio: 累计 {total} 帧未过门（连续 {streak}；帧界损坏/载荷 CRC 不符？）: {why}"
        );
    }
}

/// 拆帧器弃字节重扫：与门故障同族计数，文案点明弃字节数（线面坏段可见）。
fn rr3_esp32_lost(
    core: &Rr3Esp32AudioCore,
    streak: &mut u64,
    total: &mut u64,
    discarded: u64,
    why: &str,
) {
    *streak += 1;
    *total += 1;
    core.errs.fetch_add(1, Ordering::Relaxed);
    if rr3_err_log_now(*total) {
        eprintln!(
            "rr-hal-esp32-audio: 累计 {total} 次拆帧失败（连续 {streak}，已弃 {discarded}B 重扫）: {why}"
        );
    }
}
