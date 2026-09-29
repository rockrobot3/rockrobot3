//! rrd 的平台无关部分：命令行参数、所有权表装载、manifest 运行期事实、在册表状态机与上行 RPC 编解码。
//!
//! 纯逻辑放在 lib 里，`tests/*.rs` 与后续 `rrd/src/bin/sim_peer.rs` 才能直接引用
//! （bin 里的模块对外不可见）。UDS、watchdog 与进程派生只在 `#[cfg(unix)]` 的
//! `conn`/`daemon`/`supervisor` 模块里（Task 7 落地监听、Task 11b 落地 spawn），
//! 非 Unix 平台它们根本不参与编译，
//! 所以 Windows 侧既能 `cargo check --all-targets` 也能跑到这里的全部测试——但那只证明
//! cfg 边界未破，`conn.rs`/`daemon.rs`/`supervisor.rs` 的每一行都要靠 Unix 侧（L2/L3）才有证据。

pub mod cli;
pub mod manifest;
pub mod registry;
pub mod rpc;
pub mod topics_load;

#[cfg(unix)]
pub mod conn;
#[cfg(unix)]
pub mod daemon;
#[cfg(unix)]
pub mod supervisor;

// 只被下面那条 `#[cfg(unix)]` 的 `rr3_lock` 用到，所以两处都必须带 cfg。实测（本轮在 Windows
// 上跑 `cargo check --all-targets -p rrd`）：import 不加 cfg 是 `unused_imports` 警告，
// `rr3_lock` 不加 cfg 是 `dead_code` 警告（"never used"，实测 1 条）——而本仓的门禁是零警告。
// 三个调用方（`conn`/`daemon`/`supervisor`）都是 `#[cfg(unix)]` 模块，所以 cfg 就是它们的编译面。
#[cfg(unix)]
use std::sync::{Mutex, MutexGuard};

/// 统一日志出口：stdout、行首 `rrd: `。集成测试的可观察面全靠它。
/// 走 `println!`：Rust stdout 是行缓冲，管道里也按行 flush，父进程逐行读得到。
pub fn rr3_log(msg: &str) {
    println!("rrd: {msg}");
}

/// 锁中毒的唯一处理口径：中毒只可能来自 panic 的持锁者，继续用比弃连接/比整个守护停摆好。
/// **单点**原因：`conn.rs`（应答与收尾）、`daemon.rs`（watchdog 扫描与广播）与 `supervisor.rs`
/// （收尸与重启）共用同一本在册表锁，三处各写一份"怎么处理中毒"迟早分叉——与 Task 9 抽
/// `rr3_spawn_line_reader` 是同一条理由。
///
/// `#[cfg(unix)]` 不是多余的装饰而是零警告的前提：它的三个调用方（`conn`/`daemon`/`supervisor`）
/// 都是 `#[cfg(unix)]` 模块，非 Unix 下无条件定义会立刻变成 `dead_code`（Windows ①b 的门禁是
/// 零警告）。若将来平台无关代码也要用这把锁，那时候再把这一行去掉。
#[cfg(unix)]
pub(crate) fn rr3_lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// 非 Unix 平台的唯一出口文案（Task 3 Step 6 的实证按它断言，别改字面量）
pub const RRD_UNSUPPORTED: &str =
    "rrd: 本平台不支持 UDS 总线（需 Unix；Windows 侧只编译协议层与平台无关逻辑）";

/// 启动横幅。主题数来自装载结果，不得写死 21。
pub fn rr3_banner(contract: u32, manifest: u32, bus: &str, topics: usize) -> String {
    format!(
        "rrd 0.0.1 (contracts schema v{contract}, manifest schema v{manifest}, bus {bus}, {topics} topics)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_names_both_schemas_and_topic_count() {
        let s = rr3_banner(1, 1, "/var/run/rock3-ipc.sock", 21);
        assert!(s.contains("contracts schema v1"), "{s}");
        assert!(s.contains("manifest schema v1"), "{s}");
        assert!(s.contains("rock3-ipc.sock"), "{s}");
        assert!(s.contains("21 topics"), "横幅必须打印在册主题数: {s}");
    }

    /// 主题数必须来自装载结果（把 21 写死进文案就抓不到叠加层生效与否）
    #[test]
    fn banner_topic_count_is_not_hardcoded() {
        assert!(rr3_banner(1, 1, "/tmp/x", 24).contains("24 topics"));
    }
}
