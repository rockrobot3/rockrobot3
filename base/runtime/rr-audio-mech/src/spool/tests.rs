//! spool 记录机制的单测：记录头/CRC 对账、越界、IO 失败、写回不一致、掉电与
//! 头损坏。全部走内存后端（`Rr3SpoolIo`），不依赖 ESP-IDF。

use super::*;
use alloc::vec;
use alloc::vec::Vec;

/// 内存后端：擦 = 填 0xFF；可按偏移注入 IO 故障、可模拟头读回损坏。
#[derive(Clone)]
struct MemDev {
    bytes: Vec<u8>,
    fail_at: Option<usize>,
    corrupt_hdr: bool,
}

impl MemDev {
    fn new(cap: usize) -> Self {
        Self {
            bytes: vec![0xFF; cap],
            fail_at: None,
            corrupt_hdr: false,
        }
    }

    fn hit(&self, off: usize, len: usize) -> Result<(), ()> {
        if off + len > self.bytes.len() {
            return Err(());
        }
        if self.fail_at.is_some_and(|at| off <= at && at < off + len) {
            return Err(());
        }
        Ok(())
    }
}

impl Rr3SpoolIo for MemDev {
    type Err = ();
    fn rr3_cap(&self) -> usize {
        self.bytes.len()
    }
    fn rr3_erase(&mut self, off: usize, len: usize) -> Result<(), ()> {
        self.hit(off, len)?;
        self.bytes[off..off + len].fill(0xFF);
        Ok(())
    }
    fn rr3_write(&mut self, off: usize, src: &[u8]) -> Result<(), ()> {
        self.hit(off, src.len())?;
        self.bytes[off..off + src.len()].copy_from_slice(src);
        Ok(())
    }
    fn rr3_read(&self, off: usize, dst: &mut [u8]) -> Result<(), ()> {
        self.hit(off, dst.len())?;
        dst.copy_from_slice(&self.bytes[off..off + dst.len()]);
        if self.corrupt_hdr && off == 0 {
            dst[0] ^= 0xFF;
        }
        Ok(())
    }
}

const CAP: usize = RR3_SPOOL_DATA + RR3_SPOOL_SECTOR * 2;

fn open() -> Rr3Spool<MemDev> {
    Rr3Spool::rr3_new(MemDev::new(CAP)).expect("布局应合法")
}

#[test]
fn roundtrip_writes_header_and_payload() {
    let mut sp = open();
    assert_eq!(sp.rr3_cap(), RR3_SPOOL_SECTOR * 2);
    assert_eq!(sp.rr3_load(), Ok(None));
    sp.rr3_begin(7).unwrap();
    let seg = [1u8, 2, 3, 4, 5];
    sp.rr3_push(&seg).unwrap();
    sp.rr3_push(&[9u8; 4000]).unwrap();
    let info = sp.rr3_finish().unwrap();
    assert_eq!(info.seq, 7);
    assert_eq!(info.len, 4005);
    let mut buf = [0u8; 8];
    assert_eq!(sp.rr3_read(0, &mut buf).unwrap(), 8);
    assert_eq!(buf, [1, 2, 3, 4, 5, 9, 9, 9]);
    assert!(sp.rr3_crc_check(&mut [0u8; 512]).unwrap());
    assert_eq!(sp.rr3_load(), Ok(Some(info)));
}

#[test]
fn begin_invalidates_previous_record() {
    let mut sp = open();
    sp.rr3_begin(1).unwrap();
    sp.rr3_push(&[0xAA; 16]).unwrap();
    sp.rr3_finish().unwrap();
    sp.rr3_begin(2).unwrap();
    assert_eq!(sp.rr3_load(), Ok(None), "开新段后旧头必须失效");
}

#[test]
fn oversized_block_fails_without_header() {
    let mut sp = open();
    sp.rr3_begin(3).unwrap();
    let too_big = vec![0u8; RR3_SPOOL_SECTOR * 2 + 1];
    assert_eq!(sp.rr3_push(&too_big), Err(Rr3SpoolErr::TooLarge));
    assert_eq!(sp.rr3_push(&[0u8; 4]), Err(Rr3SpoolErr::State));
    assert_eq!(sp.rr3_load(), Ok(None));
}

#[test]
fn empty_finish_is_rejected() {
    let mut sp = open();
    sp.rr3_begin(4).unwrap();
    assert_eq!(sp.rr3_finish(), Err(Rr3SpoolErr::Empty));
    assert_eq!(sp.rr3_read(0, &mut [0u8; 4]), Err(Rr3SpoolErr::State));
}

#[test]
fn io_failure_is_reported_and_poisons_record() {
    let mut sp = open();
    sp.rr3_begin(5).unwrap();
    sp.dev.fail_at = Some(RR3_SPOOL_DATA);
    assert_eq!(sp.rr3_push(&[0u8; 4096]), Err(Rr3SpoolErr::Io(())));
    assert_eq!(sp.rr3_push(&[0u8; 1]), Err(Rr3SpoolErr::State));
}

#[test]
fn header_mismatch_is_detected() {
    let mut sp = open();
    sp.rr3_begin(6).unwrap();
    sp.rr3_push(&[0x55; 32]).unwrap();
    sp.dev.corrupt_hdr = true;
    assert_eq!(sp.rr3_finish(), Err(Rr3SpoolErr::HdrMismatch));
    assert_eq!(sp.rr3_load(), Ok(None), "写回不一致的记录不得被接受");
}

#[test]
fn crc_detects_corrupted_payload() {
    let mut sp = open();
    sp.rr3_begin(8).unwrap();
    sp.rr3_push(&[0x33; 64]).unwrap();
    sp.rr3_finish().unwrap();
    sp.dev.bytes[RR3_SPOOL_DATA + 1] ^= 0x01;
    assert!(!sp.rr3_crc_check(&mut [0u8; 32]).unwrap());
}

#[test]
fn power_loss_mid_record_leaves_no_segment() {
    let mut sp = open();
    sp.rr3_begin(9).unwrap();
    sp.rr3_push(&[0x77; 128]).unwrap();
    let lost = sp.dev.clone();
    drop(sp);
    let mut again = Rr3Spool::rr3_new(lost).expect("布局合法");
    assert_eq!(again.rr3_load(), Ok(None), "未 finish 不得出现有效记录");
}

#[test]
fn read_bounds_and_clear() {
    let mut sp = open();
    sp.rr3_begin(10).unwrap();
    sp.rr3_push(&[0x11; 100]).unwrap();
    sp.rr3_finish().unwrap();
    assert_eq!(sp.rr3_read(100, &mut [0u8; 8]), Ok(0));
    assert_eq!(sp.rr3_read(101, &mut [0u8; 8]), Err(Rr3SpoolErr::Range));
    sp.rr3_clear().unwrap();
    assert_eq!(sp.rr3_load(), Ok(None));
}

#[test]
fn abort_resets_recording_without_touching_committed() {
    let mut sp = open();
    sp.rr3_abort().unwrap();
    sp.rr3_begin(11).unwrap();
    sp.rr3_push(&[0x22; 64]).unwrap();
    let erased_hdr = sp.dev.bytes[0..RR3_SPOOL_SECTOR].to_vec();
    sp.rr3_abort().unwrap();
    assert_eq!(
        &sp.dev.bytes[0..RR3_SPOOL_SECTOR],
        &erased_hdr[..],
        "abort 不得再擦头（begin 已擦）"
    );
    assert_eq!(sp.rr3_load(), Ok(None), "放弃的段不得出现有效记录");
    assert_eq!(sp.rr3_finish(), Err(Rr3SpoolErr::State));
    sp.rr3_begin(12).unwrap();
    sp.rr3_push(&[0x33; 8]).unwrap();
    let info = sp.rr3_finish().unwrap();
    assert_eq!(sp.rr3_abort(), Err(Rr3SpoolErr::State), "已提交只能 clear");
    assert_eq!(sp.rr3_load(), Ok(Some(info)));
}

#[test]
fn corrupt_header_is_named_not_empty() {
    let mut sp = open();
    let mut raw = rr3_hdr_encode(&Rr3SegInfo {
        seq: 1,
        len: 4,
        crc32: 0,
    });
    raw[4] = 9;
    sp.dev.bytes[0..RR3_SPOOL_HDR].copy_from_slice(&raw);
    assert_eq!(
        sp.rr3_load(),
        Err(Rr3SpoolErr::BadHdr(Rr3HdrErr::Version)),
        "magic 可信但版本损坏必须点名"
    );
    let raw = rr3_hdr_encode(&Rr3SegInfo {
        seq: 1,
        len: (RR3_SPOOL_SECTOR * 2 + 1) as u32,
        crc32: 0,
    });
    sp.dev.bytes[0..RR3_SPOOL_HDR].copy_from_slice(&raw);
    assert_eq!(sp.rr3_load(), Err(Rr3SpoolErr::BadHdr(Rr3HdrErr::Len)));
}

#[test]
fn bad_layout_is_rejected() {
    assert_eq!(
        Rr3Spool::rr3_new(MemDev::new(RR3_SPOOL_DATA)).err(),
        Some(Rr3SpoolErr::Layout)
    );
    assert_eq!(
        Rr3Spool::rr3_new(MemDev::new(RR3_SPOOL_DATA + 100)).err(),
        Some(Rr3SpoolErr::Layout)
    );
}

#[test]
fn hdr_decode_rejects_untrusted_fields() {
    let good = rr3_hdr_encode(&Rr3SegInfo {
        seq: 1,
        len: 2,
        crc32: 3,
    });
    assert_eq!(
        rr3_hdr_decode(&good),
        Ok(Rr3SegInfo {
            seq: 1,
            len: 2,
            crc32: 3
        })
    );
    let mut bad = good;
    bad[0] = b'X';
    assert_eq!(rr3_hdr_decode(&bad), Err(Rr3HdrErr::Magic));
    let mut bad = good;
    bad[4] = 9;
    assert_eq!(rr3_hdr_decode(&bad), Err(Rr3HdrErr::Version));
    let mut bad = good;
    bad[8..12].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(rr3_hdr_decode(&bad), Err(Rr3HdrErr::Empty));
}
