//! 原子写的文件边界单测。
//!
//! 为什么单独成文件：`main.rs` 已贴着 600 行上限，文件边界新用例放这里，
//! 结构上随 `sign_cmd` 走（与 `rr-ota::sign/tests.rs` 同款分层）。

use super::*;

/// 原子写走 rr-ipc 单一写序后：内容可读回、同目录临时件不残留。
#[test]
fn atomic_write_lands_without_tmp_residue() {
    let dir = std::env::temp_dir().join(format!("rr3-ota-dsync-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("out.txt");
    rr3_write_atomic(&path, b"hello\n").expect("原子写");
    assert_eq!(std::fs::read(&path).expect("读回"), b"hello\n");
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .expect("读目录")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".rr3-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "临时件不得残留: {leftovers:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
