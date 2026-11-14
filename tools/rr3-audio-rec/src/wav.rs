//! wav 落盘面：头构造、建档、恢复头/真头的位置改写。

use std::fs::{File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::sync::atomic::Ordering;

use crate::watch::WatchState;

/// wav 头（44B PCM RIFF；s16le 双通道，rate 由调用方按**实测率**给定）。
pub(crate) fn wav_header(data_len: u32, channels: u16, rate: u32, bits: u16) -> [u8; 44] {
    let byte_rate = rate * channels as u32 * bits as u32 / 8;
    let block_align = channels * bits / 8;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    h[32..34].copy_from_slice(&block_align.to_le_bytes());
    h[34..36].copy_from_slice(&bits.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_len.to_le_bytes());
    h
}

/// 恢复头（data_len 取盘侧写透字节、rate 取本端实测率）。
pub(crate) fn rr3_recover_head(watch: &WatchState) -> [u8; 44] {
    let data_len = watch.disk_bytes.load(Ordering::Acquire) as u32;
    let rate = watch.rate.load(Ordering::Acquire);
    wav_header(data_len, 2, rate, 16)
}

/// 恢复头位置写（不动游标，与写线程 payload 写无交叠）。
pub(crate) fn rr3_head_patch(out: &std::path::Path, head: &[u8; 44]) -> std::io::Result<()> {
    OpenOptions::new()
        .write(true)
        .open(out)
        .and_then(|f| f.write_all_at(head, 0))
}

/// 占位头改写为实测真头（写失败即整进程退）。
pub(crate) fn rr3_head_write(out_path: &std::path::Path, data_len: u32, measured: u32) {
    OpenOptions::new()
        .write(true)
        .open(out_path)
        .and_then(|f| f.write_all_at(&wav_header(data_len, 2, measured, 16), 0))
        .unwrap_or_else(|e| {
            eprintln!("rr3-audio-rec: wav 补头失败 {}: {e}", out_path.display());
            std::process::exit(1)
        });
}

/// wav 建档（失败即退 1，文案逐字保留）。
pub(crate) fn rr3_wav_create(out_path: &std::path::Path) -> File {
    File::create(out_path).unwrap_or_else(|e| {
        eprintln!("rr3-audio-rec: wav 建档失败 {}: {e}", out_path.display());
        std::process::exit(1);
    })
}
