//! 异常退出收口：按盘侧真相补 wav 恢复头后再按码退出。

use crate::watch::WatchState;
use crate::wav::{rr3_head_patch, rr3_recover_head};

/// fail-honest 退出（重连失败路径）：已落盘数据补恢复头保住，exit(4) 点名。
pub(crate) fn bail_with_recovery(out_path: &std::path::Path, watch: &WatchState) -> ! {
    exit_with_recovery(out_path, watch, 4)
}

/// 收流异常统一收口：按盘侧真相补 wav 恢复头，再按 `code` 退出。data_len 用
/// [`WatchState::disk_bytes`]（写线程只在实际写透后推进，进程即退时 BufWriter
/// 未刷尾不虚计）；`FileExt::write_at` 位置写不动游标、与写线程 payload 写无
/// 交叠（44B 头 @0 vs ≥44 偏移的数据），活线程下也安全。三路（重连失败/看门狗
/// 停滞/磁盘写失败）共用——残片在任何退出路径都可解码。
pub(crate) fn exit_with_recovery(out: &std::path::Path, watch: &WatchState, code: i32) -> ! {
    let head = rr3_recover_head(watch);
    if let Err(e) = rr3_head_patch(out, &head) {
        eprintln!("[rec] 恢复头补写失败 {}: {e}", out.display());
    }
    std::process::exit(code);
}
