//! 装配链硬件探测面（0.0.2 方案 D2：sim 后端先行；linux 探测骨架挂 cfg 门）。
//!
//! 探测 fail-closed（方案 §4）：探测不到与探测自身故障同判——回 `false` 即缺格，
//! 装配链对点名模块走"绝不带病起"。探测在 boot 前消费一次（ADR-0012 裁定 4：
//! 运行期不重排），不阻塞启动横幅（sim 后端零 I/O；板上探测批次的实现自守时长上限）。

use std::sync::Arc;

/// 一台机器的"格位在位吗"探测面。装配链每 boot 构造一次、逐格询问。
pub trait Rr3Probe: Send + Sync {
    fn rr3_probe_slot(&self, slot: &str) -> bool;
}

/// sim 后端：全在位。sim 机无硬件可探，缺格只由装配单缺 binding 表达
/// （0.0.2 方案 D2 "sim 后端先行"的口径；`profiles/default.toml` 基础层本就不绑模块）。
pub struct Rr3SimProbe;

impl Rr3Probe for Rr3SimProbe {
    fn rr3_probe_slot(&self, _slot: &str) -> bool {
        true
    }
}

/// linux 探测骨架（cfg 门，方案 D2 原文"linux 探测骨架挂 cfg 门"）：板上探测件
/// （NetworkManager / sysfs 类读取）属后置批次，当前对一切格位回 `false` = "探测不到"
/// （fail-closed，绝不带病起）。**未接线**：`rr3_probe_default` 仍选 sim，板上批次实装后
/// 在选择点切换。cfg 门让它参与编译校验（trait 形状漂移编译期可见），但 Linux 上不被选中——
/// 远端 CI 的 Linux runner 若被它接管会让 profile boot 全体缺格，那不是本批要的行为。
#[cfg(target_os = "linux")]
pub struct Rr3LinuxProbe;

#[cfg(target_os = "linux")]
impl Rr3Probe for Rr3LinuxProbe {
    fn rr3_probe_slot(&self, _slot: &str) -> bool {
        false
    }
}

/// 探测后端选择（0.0.2：sim 先行）。返回 trait 对象，调用方不感知具体后端。
pub fn rr3_probe_default() -> Arc<dyn Rr3Probe> {
    Arc::new(Rr3SimProbe)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// sim 后端对任何格位回"在位"——缺格判定因此只由装配单缺 binding 表达（0.0.2 口径）
    #[test]
    fn sim_backend_reports_every_slot_present() {
        let probe = rr3_probe_default();
        for slot in ["act.servo", "sensor.env", "cognition.llm", ""] {
            assert!(probe.rr3_probe_slot(slot), "sim 后端必须全在位: {slot:?}");
        }
    }
}
