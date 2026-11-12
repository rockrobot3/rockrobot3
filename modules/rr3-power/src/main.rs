//! rr3-power 入口（0.0.8 W-A 批1 拆分，ADR-0017 五层表装配层，≤80 行）。
//! 装配只做启动编排：SURFACE 自述 → [`rr3mod::bootstrap`]（收敛序列前四段）→
//! config/rail 两行回显（cutoff「声明未生效」文案逐字红线）→ HAL 探活（[`boot`]）→
//! Dispatcher 构造 → [`rr3mod::serve`] 主循环。机制在 [`dispatch`]（三节拍＋
//! ops/list 点名＋载荷组装面＋轨压判定核），原子件在 `atoms/` 原位不动。

#[path = "../atoms/battery/battery.rs"]
mod battery;
mod boot;
#[path = "../atoms/charger/charger.rs"]
mod charger;
mod dispatch;

use std::time::Duration;

use rr3mod::{bootstrap, serve, Rr3ModConfig};

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml；契约环境变量名不变）
const RR3_POWER_CONFIG_ENV: &str = "RR3_POWER_CONFIG";

/// 收帧等待上界兼循环节拍：广播全量扇出会送来与己无关的帧，排空即可，
/// 采样/充电拍点由 Instant 比较驱动，与帧到达无关
const RR3_POLL_WAIT: Duration = Duration::from_millis(100);

fn main() {
    if let Err(e) = rr3_power_run() {
        eprintln!("rr3-power 退出: {e}");
        std::process::exit(1);
    }
}

/// 装配序列（自原 composite.rs `rr3_power_run` 迁入，打印序列逐字不变）。
fn rr3_power_run() -> Result<(), String> {
    boot::rr3_power_banner();

    // 总线接入 + config 读取解析（收敛序列前四段在 rr3mod bootstrap，文案逐字）
    let (bus, cfg) = bootstrap(Rr3ModConfig {
        config_env: Some(RR3_POWER_CONFIG_ENV),
        parse_config: boot::rr3_power_cfg_parse,
    })?;
    boot::rr3_power_echo(&cfg);

    // HAL 电源面/轨压面探活（接入层编排，Unsupported 即致命——fail-closed 口径不变）
    let (dev, backend, first) = boot::rr3_power_probe()?;
    let (rail_dev, rail_backend) = boot::rr3_rail_probe(&cfg.rail)?;

    // 机制层注册面构造 + 主循环骨架（rr3mod serve：死亡探针/分帧/RPC/订阅/节拍）
    let mut dispatcher =
        dispatch::Rr3PowerDispatcher::new(dev, backend, rail_dev, rail_backend, cfg, first);
    serve(bus, &mut dispatcher, RR3_POLL_WAIT)
}
