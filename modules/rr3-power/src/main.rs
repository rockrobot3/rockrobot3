//! rr3-power 入口（0.0.8 W-A 批1 拆分，ADR-0017 五层表装配层，≤80 行）。
//! 装配只做启动编排：SURFACE 自述 → [`rr3mod::bootstrap`]（收敛序列前四段）→
//! config/rail 两行回显（cutoff「声明未生效」文案逐字红线）→ HAL 探活（[`boot`]）→
//! Dispatcher 构造 → [`rr3mod::serve`] 主循环。机制在 [`dispatch`]（三节拍＋
//! ops/list 点名＋载荷组装面＋轨压判定核），原子件在 `atoms/` 原位不动。

#[path = "../atoms/charger/charger.rs"]
mod charger;
#[path = "../atoms/battery/battery.rs"]
mod battery;
mod boot;
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
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    eprintln!("rr3-power 0.0.1 (0.0.2 T4b 实装)");
    for line in battery::SURFACE
        .iter()
        .copied()
        .chain(charger::SURFACE.iter().copied())
    {
        eprintln!("  {line}");
    }

    // 总线接入 + config 读取解析（收敛序列前四段在 rr3mod bootstrap，文案逐字）
    let (bus, cfg) = bootstrap(Rr3ModConfig {
        config_env: Some(RR3_POWER_CONFIG_ENV),
        parse_config: boot::rr3_power_config_parse,
    })?;
    // cutoff 显式标注"声明未生效"（MC-8① 裁定 (c)）：回显 ≠ 读者，关断动作归 M6。
    eprintln!(
        "config 就绪: battery sample={}ms low={}mV charger state={}ms | cutoff={}mV（声明未生效：本版不关断，归 M6）",
        cfg.battery.sample_period_ms,
        cfg.battery.low_voltage_mv,
        cfg.charger.state_period_ms,
        cfg.battery.cutoff_voltage_mv,
    );
    // rail 段回显（批N2 件五）：独立一行，不动上面的 config 就绪行（逐字红线）
    if cfg.rail.up {
        eprintln!(
            "rail 监测就绪: domain={} mild={}mV severe={}mV max_age={}ms period={}ms",
            cfg.rail.domain,
            cfg.rail.mild_mv,
            cfg.rail.severe_mv,
            cfg.rail.max_age_ms,
            cfg.rail.sample_period_ms
        );
    } else {
        eprintln!("rail 未启用（up=false，合法不适用显式写）");
    }

    // HAL 电源面/轨压面探活（接入层编排，Unsupported 即致命——fail-closed 口径不变）
    let (dev, backend, first) = boot::rr3_power_probe()?;
    let (rail_dev, rail_backend) = boot::rr3_rail_probe(&cfg.rail)?;

    // 机制层注册面构造 + 主循环骨架（rr3mod serve：死亡探针/分帧/RPC/订阅/节拍）
    let mut dispatcher =
        dispatch::Rr3PowerDispatcher::new(dev, backend, rail_dev, rail_backend, cfg, first);
    serve(bus, &mut dispatcher, RR3_POLL_WAIT)
}
