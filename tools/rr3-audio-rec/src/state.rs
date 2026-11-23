// SPDX-License-Identifier: Apache-2.0
//! 收流共享账（自 watch.rs 抽出）：跨线程原子计数与武装标志的数据形状。
//!
//! 独立成件让退出收口（exit）、wav 恢复头（wav）与看门狗（watch）共同依赖
//! 数据层，而不是彼此引用——模块依赖保持单向。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};

/// 看门狗共享账（解码线程推进 `ingested`/`rate`，写线程推进 `disk_bytes`，读
/// 线程推进 `reconnects`/`armed`，看门狗线程只读其余）：停滞判据 = 已确认入队
/// 字节不再增长（原始字节到达≠有分片——失步垃圾流也算死流，只认过了事件门的
/// 分片）。落盘线程化（d4 /dev/null 实验定案：解码循环零磁盘 I/O）后账拆**流侧/
/// 盘侧**双真相——`ingested` 给看门狗判停滞，`disk_bytes` 给恢复头报实际写透
/// 字节数（写线程 flush 后推进，未刷尾不虚计）。
pub(crate) struct WatchState {
    /// 解码线程已确认交给写线程的 PCM 字节（流侧真相；看门狗停滞判据）。
    pub(crate) ingested: AtomicU64,
    /// 写线程已写透落盘的 PCM 字节（盘侧真相；恢复头 data_len 事实源）。
    pub(crate) disk_bytes: AtomicU64,
    /// 读线程重连记账（骑行一次 +1；收兵汇总显示用）。
    pub(crate) reconnects: AtomicU64,
    /// 实测采样率（逐片刷新：frames/elapsed；不足 0.5s 测时标称）。
    pub(crate) rate: AtomicU32,
    /// AUDIO ON 应答后武装（武装前不计时——开口 settle/复位 banner 阶段静默合法）。
    pub(crate) armed: AtomicBool,
}
