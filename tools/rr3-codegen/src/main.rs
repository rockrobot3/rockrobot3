// SPDX-License-Identifier: Apache-2.0
//! 板侧常量表与探针帧布局生成器（全案 0.0.5 增补轮六 §4.5 生成物表，批N1 n1-8；
//! 批N3 件五扩行程表；第十三批扩探针帧布局）。
//!
//! 真值点 → 副本方向固定：契约/档案文件是唯一事实源，本生成器把消费方需要的
//! 常量写成生成文件；消费方只读生成物，不再手写第二份。判据 = 重跑生成器
//! `git diff` 为空（门 f 拟 ⑨-d，`tools/ci/check.sh` 同批接线）。
//!
//! 生成物三份（各域一格，互不串读写）：
//! - `tools/esp32-periph-base/src/topics_gen.rs`：主题串 + 表情白名单
//!   ← `contracts/topics.toml` / `contracts/face-expressions.toml`（board 域）
//! - `tools/esp32-periph-base/src/axis_gen.rs`：轴行程常量表
//!   ← `hardware/esp32-mini/axis/*.toml`（axis 域）
//! - `tools/pi-periph-probe/envelope_gen.py`：A-IPC 帧头 magic/偏移/宽度
//!   ← `contracts/envelope.schema.json`（probe 域，探针零手抄）
//!
//! 用法：`cargo run -p rr3-codegen`（仓根执行，写生成文件）；
//! `cargo run -p rr3-codegen -- --check`（只比对不写，漂移退出 1——门禁用）。

mod axis;
mod board;
mod emit;
mod probe;

use std::fs;
use std::path::{Path, PathBuf};

/// 仓根（Cargo.toml 的 tools/rr3-codegen 上两级）。
fn repo_root() -> PathBuf {
    let raw = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // canonicalize 只为消掉 `..`；失败（异常挂载/权限）就退回原路径，读写会在
    // 具体文件处点名失败，不让路径解析 panic 抢走归因。
    raw.canonicalize().unwrap_or(raw)
}

/// fail-closed 收口：任何一条错误都不写生成文件（宁可不生成，不许带病落盘）。
pub(crate) fn die(errs: &[String]) -> ! {
    for e in errs {
        eprintln!("rr3-codegen: {e}");
    }
    std::process::exit(1);
}

/// 读真值文件（缺件/不可读直接收口——生成器没有"缺件继续"的语义）。
pub(crate) fn rr3_read(root: &Path, rel: &str) -> String {
    let path = root.join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| die(&[format!("读 {} 失败: {e}", path.display())]))
}

fn main() {
    let check_only = std::env::args().any(|a| a == "--check");
    let root = repo_root();
    let mut errs: Vec<String> = Vec::new();

    let board = board::rr3_board_gen(&root, &mut errs);
    let axes = axis::rr3_axis_gen(&root, &mut errs);
    let probe = probe::rr3_probe_gen(&root, &mut errs);
    if !errs.is_empty() {
        die(&errs);
    }

    // 三份生成物同批对账：任一份漂移都退出 1——门 f 拟 ⑨-d 对三表一视同仁。
    emit::rr3_emit(&root.join(board::OUT), board, check_only);
    emit::rr3_emit(&root.join(axis::OUT), axes, check_only);
    emit::rr3_emit(&root.join(probe::OUT), probe, check_only);
}
