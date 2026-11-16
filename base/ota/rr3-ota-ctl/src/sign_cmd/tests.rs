//! 原子写与目录落盘的单测（sign_cmd 的文件边界）。
//!
//! 为什么单独成文件：`main.rs` 已贴着 600 行上限，文件边界新用例放这里，
//! 结构上随 `sign_cmd` 走（与 `rr-ota::sign/tests.rs` 同款分层）。

use super::*;

/// 同目录临时件 + rename 后内容可读回，父目录 fsync 成功。
#[test]
fn atomic_write_lands_and_dir_sync_passes() {
    let dir = std::env::temp_dir().join(format!("rr3-ota-dsync-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("out.txt");
    rr3_write_atomic(&path, b"hello\n").expect("原子写");
    assert_eq!(std::fs::read(&path).expect("读回"), b"hello\n");
    rr3_dir_sync(&dir).expect("目录 fsync 应成功");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 目录不存在时点名失败：不能把"无法确认落盘"静默成成功。
#[cfg(unix)]
#[test]
fn dir_sync_rejects_missing_dir() {
    let dir = std::env::temp_dir().join(format!("rr3-ota-nodir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (msg, code) = rr3_dir_sync(&dir).expect_err("缺目录必须拒");
    assert_eq!(code, ExitCode::from(1));
    assert!(msg.contains("打开目录"), "文案须点名目录 fsync: {msg}");
}
