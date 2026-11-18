//! rrd 各单测共享的夹具函数（仅测试面编入，`#[cfg(test)]`）。
//!
//! 抽取原则与 `tests/support/mod.rs` 同一条：同一段夹具代码出现第二次就该是个函数，
//! 否则两处的 EOF/路径/值面语义迟早分叉。目录名前缀由调用方给（各模块自留辨识段），
//! 唯一性由 pid + 纳秒保证（同 tag 并行不撞）。

use std::path::PathBuf;

/// pid+纳秒唯一的临时目录（axis.rs / axis_write.rs 单测共用一份口径）。
/// `prefix` 是目录名的辨识前缀（含尾连字符），`tag` 是用例短标。
pub(crate) fn tmpdir_ns(prefix: &str, tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "{prefix}{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// §4.4 示例档（轴档案）的参数化正例文本。值面 = 示例档（(center,lo,hi) =
/// (90,60,120)）；`hard_stop`/`calibrated` 由调用方组合：
/// axis.rs 的 sample_text = ("unknown", false)（解析正例基底）、
/// axis_write.rs 的 candidate_text = ("yes", true)（手工标定产物，写口放行面）。
/// 返回 `join("\n")` 不带尾换行，调用方按需补。
pub(crate) fn axis_sample_text(center: u16, hard_stop: &str, calibrated: bool) -> String {
    [
        "schema = 1",
        "pin = \"act.axis.pwm.pan\"",
        &format!("center_deg = {center}"),
        "lo_deg = 60",
        "hi_deg = 120",
        "dir_sign = 1",
        "slew_deg_s = 100",
        "settle_ms = 200",
        "hold = \"release\"",
        "current_ceil_ma = 750",
        "warmup_ms = 0",
        &format!("hard_stop = \"{hard_stop}\""),
        &format!("calibrated = {calibrated}"),
        "source = \"manual\"",
    ]
    .join("\n")
}
