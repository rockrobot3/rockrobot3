//! 语音段 flash spool 的 ESP 分区后端（ADR-0031 裁定 1，W7-b1a）。
//!
//! 机制（记录头 magic/len/CRC、顺序写、分块读、断电语义）在
//! [`rr_audio_mech::Rr3Spool`]；本件只做**分区绑定**——按 label 找分区、校验
//! 几何、把 `esp_partition_*` 的 `esp_err_t` 如实上报。何时开录/收段/清理由
//! 策略面（mic/dialog）决定，本件不猜、不静默降级。
//!
//! 擦除代价：ESP32 flash 4KB 扇区擦除是毫秒级，禁止在 I2S 音频热路径上调用；
//! spool 机制按 4KB 边界惰性擦除（写入前擦目标扇区），W7-b3 由独立写线程承接。
//!
//! 分区真值见 crate 根 `partitions.csv`（label=spool，1MB，data/undefined）；
//! 分区缺位或几何不符时 boot 点名，不伪造容量。

use esp_idf_sys::{
    esp_err_t, esp_partition_erase_range, esp_partition_find_first, esp_partition_read,
    esp_partition_subtype_t_ESP_PARTITION_SUBTYPE_DATA_UNDEFINED, esp_partition_t,
    esp_partition_type_t_ESP_PARTITION_TYPE_DATA, esp_partition_write, ESP_ERR_INVALID_ARG,
    ESP_ERR_INVALID_SIZE,
};
use rr_audio_mech::{Rr3Spool, Rr3SpoolIo, RR3_SPOOL_SECTOR};

/// 分区表 label（NUL 结尾，C 字符串字面形）。
const SPOOL_LABEL: &[u8] = b"spool\0";

/// spool 分区后端：持有 IDF 分区表里的只读描述指针。
pub(crate) struct Rr3FlashSpool {
    part: *const esp_partition_t,
    cap: usize,
}

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
    println!("[spool] partition=spool size={size}B erase={erase}B");
    Some(Rr3FlashSpool { part, cap: size })
}

/// boot 探针：绑定分区 → 读记录头 → 点名"有无待处理语音段"。只读一个扇区，
/// 不在启动路径写/擦（写寿命与启动时延都不允许）。
pub(crate) fn rr3_spool_probe() {
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
    let cap = sp.rr3_cap();
    match sp.rr3_load() {
        Ok(Some(info)) => println!(
            "[spool] ✓ cap={cap}B record=valid seq={} len={}B crc={:#010x}（待上传或由策略清理）",
            info.seq, info.len, info.crc32
        ),
        Ok(None) => println!("[spool] ✓ cap={cap}B record=empty"),
        Err(e) => println!("[spool] ✗ 记录头读取失败 err={e:?}（分区保留，不猜测内容）"),
    }
    crate::mem::rr3_heap_mark("spool", "ready");
}
