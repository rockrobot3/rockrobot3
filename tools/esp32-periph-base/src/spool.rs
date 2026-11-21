//! 语音段 flash spool 的 ESP 分区后端与写线程（ADR-0031，W7-b1a/W7-b3b）。
//!
//! 机制（记录头 magic/len/CRC、顺序写、分块读、断电语义）在
//! [`rr_audio_mech::Rr3Spool`]；本件做两件策略事：
//! 1. **分区绑定**——按 label 找分区、校验几何、把 `esp_partition_*` 的
//!    `esp_err_t` 如实上报；分区缺位或几何不符时 boot 点名，不伪造容量。
//! 2. **写线程**——擦/写/读全部在独立线程串行执行，I2S 采集线程只做非阻塞投递；
//!    擦除是毫秒级操作，绝不允许在音频热路径上发生。
//!
//! 状态机（`SPOOL_STATE`，单写多读）：`Idle → Rec → Done → Idle`。`Rec` 期间
//! 只有写线程能落盘；`Done` 表示记录已提交、等待策略面（dialog）分块读走并
//! `rr3_spool_clear` 释放。`Done` 未释放前语音段无法开新段——状态即背压，
//! 不排队、不覆盖。
//!
//! 命令通道容量 16 块（320 ms）；满即拒（调用方弃段并计数），写线程空闲 2 s
//! 仍处 `Rec` 时自行弃段——防语音侧异常导致的记录悬挂。
//!
//! 跨重启：上电只清理残留记录（语音已过期，不做补传），并打印 seq/len/crc 供
//! 现场对账。分区真值见 crate 根 `partitions.csv`（label=spool，1MB，
//! data/undefined）。

use std::sync::atomic::{AtomicU16, AtomicU8, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::Duration;

use esp_idf_sys::{
    esp_err_t, esp_partition_erase_range, esp_partition_find_first, esp_partition_read,
    esp_partition_subtype_t_ESP_PARTITION_SUBTYPE_DATA_UNDEFINED, esp_partition_t,
    esp_partition_type_t_ESP_PARTITION_TYPE_DATA, esp_partition_write, ESP_ERR_INVALID_ARG,
    ESP_ERR_INVALID_SIZE,
};
use rr_audio_mech::{Rr3SegInfo, Rr3Spool, Rr3SpoolIo, RR3_SPOOL_SECTOR};

/// 分区表 label（NUL 结尾，C 字符串字面形）。
const SPOOL_LABEL: &[u8] = b"spool\0";
/// 投递块长：20 ms @ 16 kHz 单声道（与 mic VAD 节拍同源）。
pub(crate) const RR3_SPOOL_BLK: usize = 320;
/// 命令队列容量（块；16 × 20 ms = 320 ms 缓冲）。
const SPOOL_Q_CAP: usize = 16;
/// 写线程栈：Flash 调用 + 格式化点名（不含 TLS）。
const SPOOL_STACK: usize = 6_144;
/// 写线程空闲判定：`Rec` 且 2 s 无命令即弃段（语音侧异常兜底）。
const IDLE_MS: u64 = 2_000;
/// 分块读单次上限与应答等待上限。
const READ_MAX: usize = 4_096;
const READ_WAIT_MS: u64 = 2_000;

/// spool 状态（`Done` 未释放前禁止开新段）。
const ST_IDLE: u8 = 0;
const ST_REC: u8 = 1;
const ST_DONE: u8 = 2;
static SPOOL_STATE: AtomicU8 = AtomicU8::new(ST_IDLE);
/// 段序号（wrapping；只用于点名对账）。
static SPOOL_SEQ: AtomicU16 = AtomicU16::new(0);

/// 录制面命令（音频热路径发；写线程串行执行）。
enum Rr3RecCmd {
    Begin(u16),
    Block([i16; RR3_SPOOL_BLK]),
    Finish,
    Abort,
}

/// 写线程命令：录制面与消费面（Read/Clear）共用一条串行通道——`Rr3Spool`
/// 始终只有一个线程持有，读也不需要跨线程锁。
enum Rr3SpoolCmd {
    Rec(Rr3RecCmd),
    Read {
        off: u32,
        len: u16,
        tx: SyncSender<Result<Vec<u8>, String>>,
    },
    /// 上传前整段 CRC 对账（true = 载荷与头一致）。
    Verify(SyncSender<Result<bool, String>>),
    Clear(SyncSender<Result<(), String>>),
}

/// 段提交回调（由装配面注入，如 `cloud::rr3_turn_spool`）——spool 不反向依赖
/// 策略层。返回 `Err` 即云端队列拒收：本段作废，不占着 `Done` 挡下一段。
pub(crate) type Rr3SegFn = fn(Rr3SegInfo) -> Result<(), &'static str>;

/// 写线程发送端（未装配 = None）。
static SPOOL_TX: Mutex<Option<SyncSender<Rr3SpoolCmd>>> = Mutex::new(None);

/// spool 分区后端：持有 IDF 分区表里的只读描述指针。
pub(crate) struct Rr3FlashSpool {
    part: *const esp_partition_t,
    cap: usize,
}

// SAFETY：`part` 指向 IDF 分区表的静态描述（进程生命周期只读，不随线程变化）；
// 擦/写/读都由 IDF 内部锁串行化，且本件只交给唯一 spool 写线程独占使用。
unsafe impl Send for Rr3FlashSpool {}

/// `esp_err_t` 归零即成功；其余原样上抛，不把失败映射成"空"。
fn rr3_ok(err: esp_err_t) -> Result<(), esp_err_t> {
    if err == 0 {
        Ok(())
    } else {
        Err(err)
    }
}

impl Rr3SpoolIo for Rr3FlashSpool {
    type Err = esp_err_t;

    fn rr3_cap(&self) -> usize {
        self.cap
    }

    fn rr3_erase(&mut self, off: usize, len: usize) -> Result<(), esp_err_t> {
        if off % RR3_SPOOL_SECTOR != 0 || len % RR3_SPOOL_SECTOR != 0 || off + len > self.cap {
            return Err(ESP_ERR_INVALID_ARG);
        }
        // SAFETY：`part` 来自 `esp_partition_find_first` 的非空返回（分区表静态
        // 生命周期）；off/len 已按擦除粒度与容量校验，IDF 只读这两个参数。
        rr3_ok(unsafe { esp_partition_erase_range(self.part, off, len) })
    }

    fn rr3_write(&mut self, off: usize, bytes: &[u8]) -> Result<(), esp_err_t> {
        if off + bytes.len() > self.cap {
            return Err(ESP_ERR_INVALID_SIZE);
        }
        // SAFETY：`part` 非空；bytes 的生存期覆盖调用；off/len 已做容量校验。
        rr3_ok(unsafe { esp_partition_write(self.part, off, bytes.as_ptr().cast(), bytes.len()) })
    }

    fn rr3_read(&self, off: usize, buf: &mut [u8]) -> Result<(), esp_err_t> {
        if off + buf.len() > self.cap {
            return Err(ESP_ERR_INVALID_SIZE);
        }
        // SAFETY：`part` 非空；buf 独占且长度与读取范围一致。
        rr3_ok(unsafe { esp_partition_read(self.part, off, buf.as_mut_ptr().cast(), buf.len()) })
    }
}

/// 查分区并校验几何（擦除粒度必须是 4KB、容量整数倍且放得下头扇区）。
fn rr3_open() -> Option<Rr3FlashSpool> {
    // SAFETY：label 以 NUL 结尾；`esp_partition_find_first` 只读分区表，返回
    // 指向 IDF 静态分区描述的指针（未找到为 null）。
    let part = unsafe {
        esp_partition_find_first(
            esp_partition_type_t_ESP_PARTITION_TYPE_DATA,
            esp_partition_subtype_t_ESP_PARTITION_SUBTYPE_DATA_UNDEFINED,
            SPOOL_LABEL.as_ptr().cast(),
        )
    };
    if part.is_null() {
        println!(
            "[spool] ✗ 分区 label=spool 缺位（查 partitions.csv 与烧录分区表）——语音段无法落存储"
        );
        return None;
    }
    // SAFETY：`part` 已判非空；启动期单线程读静态分区描述字段。
    let (size, erase) = unsafe { ((*part).size as usize, (*part).erase_size as usize) };
    if erase != RR3_SPOOL_SECTOR || size <= RR3_SPOOL_SECTOR || size % RR3_SPOOL_SECTOR != 0 {
        println!(
            "[spool] ✗ 分区几何不合法 size={size}B erase={erase}B（期望 {RR3_SPOOL_SECTOR}B 粒度，容量含头扇区）"
        );
        return None;
    }
    Some(Rr3FlashSpool { part, cap: size })
}

/// 起 spool 写线程（boot 在云 worker 之后、音频面之前调用）。
///
/// 启动期只读一次记录头：残留段点名后清理——语音跨重启已失效，不做补传。
/// 分区缺位/几何不符/线程起不来时保持"未装配"，语音侧据此显式降级。
pub(crate) fn rr3_start_spool(on_seg: Rr3SegFn) {
    let Some(dev) = rr3_open() else {
        return;
    };
    let mut sp = match Rr3Spool::rr3_new(dev) {
        Ok(sp) => sp,
        Err(e) => {
            println!("[spool] ✗ 布局不合法 err={e:?}（查 partitions.csv）");
            return;
        }
    };
    rr3_boot_sweep(&mut sp);
    rr3_spawn_writer(sp, on_seg);
}

/// 起写线程并发布发送端（失败保持未装配，语音显式降级）。
fn rr3_spawn_writer(sp: Rr3Spool<Rr3FlashSpool>, on_seg: Rr3SegFn) {
    let cap = sp.rr3_cap();
    let (tx, rx) = sync_channel(SPOOL_Q_CAP);
    let spawned = thread::Builder::new()
        .name("spool-w".into())
        .stack_size(SPOOL_STACK)
        .spawn(move || rr3_spool_run(rx, sp, on_seg));
    if let Err(e) = spawned {
        println!("[spool] ✗ 写线程起不来: {e:?}——语音段停用");
        return;
    }
    *SPOOL_TX.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
    println!("[spool] ✓ 写线程就绪 cap={cap}B（擦写读全在写线程，音频热路径零 Flash 调用）");
    crate::mem::rr3_heap_mark("spool", "ready");
}

/// 启动清扫：点名残留记录并清理（`Done` 也一并释放——跨重启不补传）。
fn rr3_boot_sweep(sp: &mut Rr3Spool<Rr3FlashSpool>) {
    let cap = sp.rr3_cap();
    match sp.rr3_load() {
        Ok(Some(info)) => {
            println!(
                "[spool] 残留段 seq={} len={}B crc={:#010x} → 清理（跨重启不补传）",
                info.seq, info.len, info.crc32
            );
            rr3_boot_clear(sp, "残留段");
        }
        Ok(None) => println!("[spool] cap={cap}B record=empty"),
        Err(e) => {
            println!("[spool] ✗ 记录头损坏 err={e:?} → 清理后继续");
            rr3_boot_clear(sp, "损坏头");
        }
    }
}

/// 启动期清理一次头扇区；失败点名（不阻断启动）。
fn rr3_boot_clear(sp: &mut Rr3Spool<Rr3FlashSpool>, what: &str) {
    if let Err(e) = sp.rr3_clear() {
        println!("[spool] ✗ {what}清理失败 err={e:?}（本段仍占位）");
    }
}

/// 写线程主体：单线程串行执行命令；`Rec` 空闲超时自行弃段。
fn rr3_spool_run(rx: Receiver<Rr3SpoolCmd>, mut sp: Rr3Spool<Rr3FlashSpool>, on_seg: Rr3SegFn) {
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

/// 非阻塞投递（音频热路径唯一入口；满/未装配都如实回错）。
fn rr3_send(cmd: Rr3SpoolCmd) -> Result<(), String> {
    let slot = SPOOL_TX.lock().unwrap_or_else(|p| p.into_inner());
    let Some(tx) = slot.as_ref() else {
        return Err("spool 写线程未装配".into());
    };
    tx.try_send(cmd).map_err(|e| match e {
        TrySendError::Full(_) => "spool 队列满".into(),
        TrySendError::Disconnected(_) => "spool 写线程已退出".into(),
    })
}

/// spool 是否可用（语音侧据此选流式落盘 vs 停用；只查装配面）。
pub(crate) fn rr3_spool_avail() -> bool {
    SPOOL_TX.lock().unwrap_or_else(|p| p.into_inner()).is_some()
}

/// 开段（非阻塞）：`Idle` 抢成功且命令投递成功才算开录。
pub(crate) fn rr3_spool_begin() -> bool {
    let seq = SPOOL_SEQ.fetch_add(1, Ordering::Relaxed);
    if SPOOL_STATE
        .compare_exchange(ST_IDLE, ST_REC, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return false;
    }
    match rr3_send(Rr3SpoolCmd::Rec(Rr3RecCmd::Begin(seq))) {
        Ok(()) => true,
        Err(e) => {
            SPOOL_STATE.store(ST_IDLE, Ordering::SeqCst);
            println!("[spool] ✗ seq={seq} 投递失败: {e}");
            false
        }
    }
}

/// 写一块（非阻塞）；`false` = 本段已不可用，调用方须立即弃段。
pub(crate) fn rr3_spool_block(mono: &[i16; RR3_SPOOL_BLK]) -> bool {
    rr3_send(Rr3SpoolCmd::Rec(Rr3RecCmd::Block(*mono))).is_ok()
}

/// 提交本段（非阻塞；提交结果由写线程点名）。
pub(crate) fn rr3_spool_finish() -> bool {
    rr3_send(Rr3SpoolCmd::Rec(Rr3RecCmd::Finish)).is_ok()
}

/// 弃段（非阻塞；写线程空闲超时是兜底）。
pub(crate) fn rr3_spool_abort() {
    if let Err(e) = rr3_send(Rr3SpoolCmd::Rec(Rr3RecCmd::Abort)) {
        println!("[spool] ✗ 弃段投递失败: {e}");
    }
}

/// 分块读（消费面；阻塞至写线程应答，超时回错）。
pub(crate) fn rr3_spool_read(off: u32, out: &mut [u8]) -> Result<usize, String> {
    let (tx, rx) = sync_channel(1);
    rr3_send(Rr3SpoolCmd::Read {
        off,
        len: out.len().min(READ_MAX) as u16,
        tx,
    })?;
    let data = rx
        .recv_timeout(Duration::from_millis(READ_WAIT_MS))
        .map_err(|e| format!("spool 读应答超时/断开: {e:?}"))??;
    out[..data.len()].copy_from_slice(&data);
    Ok(data.len())
}

/// 上传前 CRC 对账（消费面；阻塞至写线程应答）。
pub(crate) fn rr3_spool_verify() -> Result<bool, String> {
    let (tx, rx) = sync_channel(1);
    rr3_send(Rr3SpoolCmd::Verify(tx))?;
    rx.recv_timeout(Duration::from_millis(READ_WAIT_MS))
        .map_err(|e| format!("spool 校验应答超时/断开: {e:?}"))?
}

/// 释放记录（消费面；上传结束后必调，成功才回 `Idle`）。
pub(crate) fn rr3_spool_clear() -> Result<(), String> {
    let (tx, rx) = sync_channel(1);
    rr3_send(Rr3SpoolCmd::Clear(tx))?;
    rx.recv_timeout(Duration::from_millis(READ_WAIT_MS))
        .map_err(|e| format!("spool 释放应答超时/断开: {e:?}"))?
}
