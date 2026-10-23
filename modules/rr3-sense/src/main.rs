//! rr3-sense 入口（0.0.8 W-A 批3 拆分，ADR-0017 五层表装配层，≤80 行）。
//! 装配只做启动编排：SURFACE 自述 → manifest 路径 → [`rr3mod::bootstrap`]（收敛
//! 序列前四段）→ config 就绪行 → 音频源探活（[`boot`]）→ Dispatcher 构造 →
//! [`rr3mod::serve`] 主循环。机制在 [`dispatch`]（采集拍＋门控/反射环/判向窗三
//! 切片＋三臂点名分发），协议面在 [`rpc`]（载荷组装＋dump/ops 快照体），原子件
//! 在 `atoms/` 原位不动（capture/env/imu/vad 四件）；三份清单功能守恒零改动。

#[path = "../atoms/capture/capture.rs"]
mod capture;
#[path = "../atoms/env/env.rs"]
mod env;
#[path = "../atoms/imu/imu.rs"]
mod imu;
#[path = "../atoms/vad/vad.rs"]
mod vad;
mod boot;
mod dispatch;
mod rpc;

use std::time::Duration;

use rr3mod::{bootstrap, rr3_config_path, serve, Rr3ModConfig};

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml；契约环境变量名不变）
const RR3_SENSE_CONFIG_ENV: &str = "RR3_SENSE_CONFIG";

/// 自 pacing 归一（0.0.8 方案 §2.6）：采集拍整拍在 on_tick 内（读帧→…→余量睡到
/// frame_ms），serve 收帧等待给 0——与原主循环时序逐字等价（两种后端同一形状）
const RR3_POLL_WAIT: Duration = Duration::ZERO;

fn main() {
    if let Err(e) = rr3_sense_run() {
        eprintln!("rr3-sense 退出: {e}");
        std::process::exit(1);
    }
}

/// 装配序列（自原 composite.rs `rr3_sense_run` 迁入，打印序列逐字不变）。
fn rr3_sense_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    eprintln!("rr3-sense 0.0.1 (M3 实装)");
    for line in capture::SURFACE
        .iter()
        .copied()
        .chain(vad::SURFACE.iter().copied())
        .chain(env::SURFACE.iter().copied())
        .chain(imu::SURFACE.iter().copied())
    {
        eprintln!("  {line}");
    }

    // manifest 路径（rrd spawn 契约环境变量；装配段先取一份供 config 来源回显
    // 投影，文案与 rr3mod bootstrap 同款逐字——bootstrap 内部自读同源）
    let manifest_path = std::env::var(rr3mod::RR3_MANIFEST_ENV).map_err(|_| {
        format!("缺环境变量 {}（rrd spawn 注入；直跑请指向 module.toml）", rr3mod::RR3_MANIFEST_ENV)
    })?;

    // 总线接入 + config 读取解析（收敛序列前四段在 rr3mod bootstrap，文案逐字；
    // 路径段 RR3_SENSE_CONFIG_ENV 覆盖 + manifest 同目录缺省归 load_config_text）
    let (bus, cfg) = bootstrap(Rr3ModConfig {
        config_env: Some(RR3_SENSE_CONFIG_ENV),
        parse_config: boot::rr3_sense_config_parse,
    })?;
    // config 就绪行（原 rr3_sense_config_load 文案逐字；来源路径经 rr3mod 投影
    // 单点——parse_config 是 fn 指针拿不到 path，投影是唯一不重复解析的通路）
    eprintln!(
        "config 就绪: 来源={} doa_bin_threshold_deg={} doa_window_ms={} doa_window_rms_min={} doa_mic_baseline_mm={}",
        rr3_config_path(Some(RR3_SENSE_CONFIG_ENV), &manifest_path),
        cfg.doa.bin_threshold_deg, cfg.doa.window_ms, cfg.doa.window_rms_min, cfg.doa.mic_baseline_mm
    );

    // HAL 音频源探活（接入层，打开失败 Err 上抛——fail-closed 显式回错，不静默
    // 带病常驻也不给"没有音频源还常驻"的假活）
    let (source, cmd_tx, backend, open_cfg) = boot::rr3_sense_probe(&cfg)?;

    // 机制层注册面构造 + 主循环骨架（rr3mod serve：死亡探针/分帧/RPC/订阅/节拍）
    let mut dispatcher = dispatch::Rr3SenseDispatcher::new(source, cmd_tx, backend, open_cfg, cfg);
    serve(bus, &mut dispatcher, RR3_POLL_WAIT)
}
