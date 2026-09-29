//! rr3-sim —— x86 全链路联调入口（骨架）。
//!
//! 规划（architecture.md §3）：mock HAL + 假模块，让整条链路在无板设备上跑起来。
//! v0.0.1 只做一件事：用 `rr-hal` 的真实类型产出一段模拟拾音流并报告时长，
//! 证明"sim 与 linux 后端交出同一种流"这条不变量有落点。总线注入与假模块随 M2。

mod devices;

use rr_hal::backend::RR3_BACKEND_SIM;
use rr_ipc::{RR3_BUS_PATH_ENV, RR3_UDS_PATH_DEFAULT};

fn main() {
    let frames = std::env::var("RR3_SIM_FRAMES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1600);
    let stream = devices::rr3_sim_silence(frames);
    let bus = std::env::var(RR3_BUS_PATH_ENV).unwrap_or_else(|_| RR3_UDS_PATH_DEFAULT.to_string());
    println!(
        "rr3-sim: backend={RR3_BACKEND_SIM}, {frames} 帧 -> {}ms, 目标总线 {bus}（注入与假模块随 M2）",
        stream.rr3_pcm_duration_ms()
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn device_module_is_linked() {
        // 入口与设备源的连接点：main 不调用它就无法验证类型一致
        assert_eq!(crate::devices::rr3_sim_silence(160).rr3_pcm_duration_ms(), 10);
    }
}
