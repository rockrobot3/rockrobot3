//! spool 写线程执行面（自 `spool.rs` 拆出，控制单文件规模）：命令环、录制/
//! 消费状态机与失败语义。分区绑定、全局发送端与公开 API 仍在父模块。
//!
//! 契约：单线程独占 `Rr3Spool`；`Rec` 空闲 2s 自行弃段；提交成功才置 `Done`
//! 并交策略面，云端拒收即清理，不让背压永久占位。

use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use rr_audio_mech::Rr3Spool;

use super::{
    Rr3FlashSpool, Rr3RecCmd, Rr3SegFn, Rr3SpoolCmd, IDLE_MS, READ_MAX, RR3_SPOOL_BLK, SPOOL_STATE,
    ST_DONE, ST_IDLE,
};

/// 写线程主体：单线程串行执行命令；`Rec` 空闲超时自行弃段。
pub(super) fn rr3_spool_run(
    rx: Receiver<Rr3SpoolCmd>,
    mut sp: Rr3Spool<Rr3FlashSpool>,
    on_seg: Rr3SegFn,
) {
    println!(
        "[spool] 写线程进入命令环 cap={}B（等 Begin/Read/Clear）",
        sp.rr3_cap()
    );
    let mut rec = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(IDLE_MS)) {
            Ok(cmd) => rec = rr3_cmd(cmd, &mut sp, on_seg, rec),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if rec {
                    rec = false;
                    rr3_drop_rec(&mut sp, "空闲超时");
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                println!("[spool] 命令队列断开，写线程退出");
                return;
            }
        }
    }
}

/// 命令分派：录制面交 [`rr3_rec_cmd`]，消费面就地应答（不改变录制态）。
fn rr3_cmd(
    cmd: Rr3SpoolCmd,
    sp: &mut Rr3Spool<Rr3FlashSpool>,
    on_seg: Rr3SegFn,
    rec: bool,
) -> bool {
    match cmd {
        Rr3SpoolCmd::Rec(rc) => rr3_rec_cmd(rc, sp, on_seg, rec),
        Rr3SpoolCmd::Read { off, len, tx } => {
            let _ = tx.send(rr3_do_read(sp, off, len));
            rec
        }
        Rr3SpoolCmd::Verify(tx) => {
            let _ = tx.send(rr3_do_verify(sp));
            rec
        }
        Rr3SpoolCmd::Clear(tx) => {
            let _ = tx.send(rr3_do_clear(sp));
            false
        }
    }
}

/// 录制面状态机：Begin 开段、Block 累积、Finish 提交、Abort 弃段。
fn rr3_rec_cmd(
    cmd: Rr3RecCmd,
    sp: &mut Rr3Spool<Rr3FlashSpool>,
    on_seg: Rr3SegFn,
    rec: bool,
) -> bool {
    match cmd {
        Rr3RecCmd::Begin(seq) => rr3_do_begin(sp, seq),
        Rr3RecCmd::Block(mono) => rr3_block_if(sp, &mono, rec),
        Rr3RecCmd::Finish => rr3_finish_if(sp, on_seg, rec),
        Rr3RecCmd::Abort => {
            rr3_drop_if(sp, rec, "语音侧弃段");
            false
        }
    }
}

/// 未在录制中的块直接丢弃（Begin 失败/已 Finish 后的迟到块）。
fn rr3_block_if(sp: &mut Rr3Spool<Rr3FlashSpool>, mono: &[i16; RR3_SPOOL_BLK], rec: bool) -> bool {
    rec && rr3_do_block(sp, mono)
}

/// 只在录制中提交；提交后录制态归零。
fn rr3_finish_if(sp: &mut Rr3Spool<Rr3FlashSpool>, on_seg: Rr3SegFn, rec: bool) -> bool {
    if rec {
        rr3_do_finish(sp, on_seg);
    }
    false
}

/// 只在录制中弃段（未开录的 Abort 是幂等空操作）。
fn rr3_drop_if(sp: &mut Rr3Spool<Rr3FlashSpool>, rec: bool, why: &str) {
    if rec {
        rr3_drop_rec(sp, why);
    }
}

/// Begin：开新段；失败即回 Idle（语音侧按弃段处理）。
fn rr3_do_begin(sp: &mut Rr3Spool<Rr3FlashSpool>, seq: u16) -> bool {
    match sp.rr3_begin(seq) {
        Ok(()) => {
            println!("[spool] rec seq={seq} 起");
            true
        }
        Err(e) => {
            SPOOL_STATE.store(ST_IDLE, Ordering::SeqCst);
            println!("[spool] ✗ seq={seq} 开段失败 err={e:?}");
            false
        }
    }
}

/// Block：单声道 s16le 顺序落盘；失败即作废本段（不留下半截记录）。
fn rr3_do_block(sp: &mut Rr3Spool<Rr3FlashSpool>, mono: &[i16; RR3_SPOOL_BLK]) -> bool {
    let mut raw = [0u8; RR3_SPOOL_BLK * 2];
    for (dst, s) in raw.chunks_exact_mut(2).zip(mono) {
        dst.copy_from_slice(&s.to_le_bytes());
    }
    match sp.rr3_push(&raw) {
        Ok(()) => true,
        Err(e) => {
            SPOOL_STATE.store(ST_IDLE, Ordering::SeqCst);
            println!("[spool] ✗ 写块失败 err={e:?}——本段作废");
            false
        }
    }
}

/// Finish：提交并交策略面；云端拒收即清理，不让 `Done` 永久占位。
fn rr3_do_finish(sp: &mut Rr3Spool<Rr3FlashSpool>, on_seg: Rr3SegFn) {
    let info = match sp.rr3_finish() {
        Ok(info) => info,
        Err(e) => {
            SPOOL_STATE.store(ST_IDLE, Ordering::SeqCst);
            println!("[spool] ✗ 提交失败 err={e:?}——本段作废");
            return;
        }
    };
    SPOOL_STATE.store(ST_DONE, Ordering::SeqCst);
    println!(
        "[spool] ✓ seq={} len={}B crc={:#010x} → 云队列",
        info.seq, info.len, info.crc32
    );
    if let Err(e) = on_seg(info) {
        println!("[spool] ✗ seq={} 入云队列失败: {e}——本段作废", info.seq);
        let _ = rr3_do_clear(sp);
    }
}

/// 弃掉进行中的段（擦头由下次 `begin` 惰性完成；这里只复位状态）。
fn rr3_drop_rec(sp: &mut Rr3Spool<Rr3FlashSpool>, why: &str) {
    SPOOL_STATE.store(ST_IDLE, Ordering::SeqCst);
    match sp.rr3_abort() {
        Ok(()) => println!("[spool] 弃段（{why}）"),
        Err(e) => println!("[spool] 弃段失败（{why}）err={e:?}"),
    }
}

/// 分块读：长度夹在 `READ_MAX` 内；读失败/越界显式回错。
fn rr3_do_read(sp: &mut Rr3Spool<Rr3FlashSpool>, off: u32, len: u16) -> Result<Vec<u8>, String> {
    let n = (len as usize).min(READ_MAX);
    let mut buf = vec![0u8; n];
    let got = sp
        .rr3_read(off, &mut buf)
        .map_err(|e| format!("spool 读失败 off={off}: {e:?}"))?;
    buf.truncate(got);
    Ok(buf)
}

/// 整段 CRC 对账：顺序读全段续算，与头里的 crc 比对；非 `Done` 态显式回错。
fn rr3_do_verify(sp: &mut Rr3Spool<Rr3FlashSpool>) -> Result<bool, String> {
    let mut buf = [0u8; READ_MAX];
    sp.rr3_crc_check(&mut buf)
        .map_err(|e| format!("spool CRC 对账失败: {e:?}"))
}

/// 释放记录（只擦头扇区）并回 Idle。
fn rr3_do_clear(sp: &mut Rr3Spool<Rr3FlashSpool>) -> Result<(), String> {
    sp.rr3_clear()
        .map_err(|e| format!("spool 释放失败: {e:?}"))?;
    SPOOL_STATE.store(ST_IDLE, Ordering::SeqCst);
    println!("[spool] 段已释放");
    Ok(())
}
