// SPDX-License-Identifier: Apache-2.0
//! 生成物写盘/对账（`--check` 只比对；漂移/缺席/写失败均退出 1）。

use std::fs;
use std::path::Path;

/// 写盘或对账一份生成物。
pub(crate) fn rr3_emit(path: &Path, text: String, check_only: bool) {
    if check_only {
        rr3_emit_check(path, &text);
    } else {
        fs::write(path, text)
            .unwrap_or_else(|e| crate::die(&[format!("写 {} 失败: {e}", path.display())]));
        println!("rr3-codegen: 已写 {}", path.display());
    }
}

/// 只比对不写：一致打行，漂移/缺席点名退 1。
fn rr3_emit_check(path: &Path, text: &str) {
    let existing = fs::read_to_string(path).unwrap_or_else(|e| {
        crate::die(&[format!(
            "生成文件缺席（先跑一次不带 --check 的生成器落盘）: {e}"
        )])
    });
    if existing == text {
        println!(
            "rr3-codegen: --check 一致（{} 与真值点同步）",
            path.display()
        );
    } else {
        eprintln!(
            "rr3-codegen: 生成物与真值点漂移——{} 与重跑结果不一致；\
             在仓根跑 `cargo run -p rr3-codegen` 重生成并审阅（门 f 拟 ⑨-d）",
            path.display()
        );
        std::process::exit(1);
    }
}
