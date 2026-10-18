//! rrd 的平台无关部分：命令行参数、所有权表装载、manifest 运行期事实、在册表状态机与上行 RPC 编解码。
//!
//! 纯逻辑放在 lib 里，`tests/*.rs` 与后续 `rrd/src/bin/sim_peer.rs` 才能直接引用
//! （bin 里的模块对外不可见）。UDS、watchdog 与进程派生只在 `#[cfg(unix)]` 的
//! `conn`/`daemon`/`supervisor` 模块里（Task 7 落地监听、Task 11b 落地 spawn），
//! 非 Unix 平台它们根本不参与编译，
//! 所以 Windows 侧既能 `cargo check --all-targets` 也能跑到这里的全部测试——但那只证明
//! cfg 边界未破，`conn.rs`/`daemon.rs`/`supervisor.rs` 的每一行都要靠 Unix 侧（L2/L3）才有证据。

pub mod axis;
pub mod axis_write;
pub mod cli;
pub mod counters;
pub mod manifest;
pub mod probe;
pub mod profile;
pub mod registry;
pub mod rpc;
pub mod topics_load;
pub mod wiring;

#[cfg(unix)]
pub mod conn;
#[cfg(unix)]
pub mod daemon;
#[cfg(unix)]
pub mod supervisor;

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// 统一日志出口：stdout、行首 `rrd: `。集成测试的可观察面全靠它。
/// 走 `println!`：Rust stdout 是行缓冲，管道里也按行 flush，父进程逐行读得到。
///
/// ADR-0010 裁定 5：stdout 行为一字不动，同一行**追加**一份进固定容量日志环；
/// `rr3.ctl.logs` 读环的尾部副本。环在这里单点落——rr3_log 是全 daemon 唯一日志出口，
/// 在别的调用点各存一份迟早分叉。
pub fn rr3_log(msg: &str) {
    let line = format!("rrd: {msg}");
    println!("{line}");
    let mut ring = rr3_lock(rr3_log_ring());
    if ring.len() >= RR3_LOG_RING_CAP {
        ring.pop_front();
    }
    ring.push_back(line);
}

/// 日志环容量（行数）。512 行对"运维看刚才发生了什么"足够，内存成本有界；
/// 满了丢最旧（环形语义，不是"到顶即停"）。
const RR3_LOG_RING_CAP: usize = 512;

static RR3_LOG_RING: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn rr3_log_ring() -> &'static Mutex<VecDeque<String>> {
    RR3_LOG_RING.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// 日志环的尾部副本（`rr3.ctl.logs` 的数据源）。只有 `conn`（`#[cfg(unix)]`）读它，
/// 所以本函数同样 cfg——Windows 侧无人调用，不 cfg 就是 `dead_code` 警告。
#[cfg(unix)]
pub fn rr3_log_tail() -> Vec<String> {
    rr3_lock(rr3_log_ring()).iter().cloned().collect()
}

/// 锁中毒的唯一处理口径：中毒只可能来自 panic 的持锁者，继续用比弃连接/比整个守护停摆好。
/// **单点**原因：`conn.rs`（应答与收尾）、`daemon.rs`（watchdog 扫描与广播）与 `supervisor.rs`
/// （收尸与重启）共用同一本在册表锁，三处各写一份"怎么处理中毒"迟早分叉——与 Task 9 抽
/// `rr3_spawn_line_reader` 是同一条理由。
///
/// 原 `#[cfg(unix)]` 已随 F3（ADR-0010 裁定 5）摘除：`rr3_log` 的日志环让本函数在
/// 平台无关代码里也有了调用方，cfg 反而会在 Windows 上变成 `unused` 警告。
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
