//! rr3-sense 入口（0.0.8 W-A 批3 拆分，ADR-0017 五层表装配层，≤80 行）。
//! 装配只做启动编排：SURFACE 自述 → manifest 路径 → [`rr3mod::bootstrap`]（收敛
//! 序列前四段）→ config 就绪行 → 音频源探活（[`boot`]）→ Dispatcher 构造 →
//! [`rr3mod::serve`] 主循环。机制在 [`dispatch`]（采集拍编排＋反射环＋三臂点名
//! 分发），门控与判向窗两个纯机制切片各自成件（[`tts_gate`]/[`doa_ring`]），
//! 协议面在 [`rpc`]（载荷组装＋dump/ops 快照体），原子件在 `atoms/` 原位不动
//! （capture/env/imu/vad 四件）；三份清单功能守恒零改动。

mod boot;
#[path = "../atoms/capture/capture.rs"]
mod capture;
mod dispatch;
mod doa_ring;
#[path = "../atoms/env/env.rs"]
mod env;
#[path = "../atoms/imu/imu.rs"]
mod imu;
mod rpc;
mod tts_gate;
#[path = "../atoms/vad/vad.rs"]
mod vad;

use std::time::Duration;

use rr3mod::{bootstrap, rr3_config_path, serve, Rr3ModConfig};

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml；契约环境变量名不变）
const RR3_SENSE_CONFIG_ENV: &str = "RR3_SENSE_CONFIG";

/// 自 pacing 归一（0.0.8 方案 §2.6）：采集拍整拍在 on_tick 内，serve 收帧等待给 0
const RR3_POLL_WAIT: Duration = Duration::ZERO;

fn main() {
    if let Err(e) = rr3_sense_run() {
        eprintln!("rr3-sense 退出: {e}");
        std::process::exit(1);
    }
}

/// 装配序列（自原 composite.rs `rr3_sense_run` 迁入，打印序列逐字不变）。
fn rr3_sense_run() -> Result<(), String> {
    boot::rr3_sense_surface();

    let manifest_path = std::env::var(rr3mod::RR3_MANIFEST_ENV).map_err(|_| {
        format!(
            "缺环境变量 {}（rrd spawn 注入；直跑请指向 module.toml）",
            rr3mod::RR3_MANIFEST_ENV
        )
    })?;

    let (bus, cfg) = bootstrap(Rr3ModConfig {
        config_env: Some(RR3_SENSE_CONFIG_ENV),
        parse_config: boot::rr3_sense_cfg_parse,
    })?;
    eprintln!(
        "config 就绪: 来源={} doa_bin_threshold_deg={} doa_window_ms={} doa_window_rms_min={} doa_mic_baseline_mm={}",
        rr3_config_path(Some(RR3_SENSE_CONFIG_ENV), &manifest_path),
        cfg.doa.bin_threshold_deg, cfg.doa.window_ms, cfg.doa.window_rms_min, cfg.doa.mic_baseline_mm
    );

    // HAL 音频源探活：打开失败 Err 上抛——fail-closed
    let (source, cmd_tx, backend, open_cfg) = boot::rr3_sense_probe(&cfg)?;

    let mut dispatcher = dispatch::Rr3SenseDispatcher::new(source, cmd_tx, backend, open_cfg, cfg);
    serve(bus, &mut dispatcher, RR3_POLL_WAIT)
}
