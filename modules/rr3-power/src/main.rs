//! rr3-power 真入口（0.0.2 T4b）：battery/state 周期流 + battery/low 边沿事件 + charger/state。
//!
//! 启动序列与 rr3-act/rr3-sense/rr3-interact 同构：读 `RR3_MANIFEST`（rrd spawn 注入的
//! manifest 绝对路径，supervisor.rs）→ [`Rr3BusClient::rr3_connect`] → 读 config.toml
//! → 选 HAL 电源面并**开机探活**（Unsupported 即 exit(1)：能力缺失显式回错，不静默
//! 带病常驻）→ 主循环。本模块没有事务面（module.toml 无 [rpc].exposes），主循环只做
//! 三件事：
//! 1. 每 `sample_period_ms` 读一次 HAL 电池 → 发 `battery/state`（首拍即采）；
//! 2. 电压越过低电阈值的第一拍发 `battery/low`（Rr3LowEdge：同一次低电段恰一次，
//!    恢复再武装）——rrd 订阅它执行进程内兜底（Q4 裁定），brain 不在场也要能触发；
//! 3. 每 `state_period_ms` 发 `charger/state`（由最近一次电池读数的 charging 位派生）。
//!
//! 失败语义与 sense/interact/act 同一口径：本模块没有调用方，周期采样里**全部**失败
//! 都是传输性/设备级——HAL 读数失败、发布写不出去都 `eprintln` + `exit(1)`，不装
//! "活着"，死交给 supervisor 按 restart_policy=always 重启。订阅面为空是**设计**
//! （module.toml 同条注）：低压关断是本地决策，不依赖总线可达。
//!
//! cutoff 电压阈值本轮只解析不动作（0.0.2 方案 Q4：关机与充电策略归 rr3-power
//! 演进与 M6），装配时回显进启动日志。

mod battery;
mod charger;

use std::time::{Duration, Instant};

use rr_bus::Rr3BusClient;
use rr_hal::{backend::rr3_hal_power, rr3_block_on};

use battery::{Rr3BatteryCfg, Rr3LowEdge};
use charger::Rr3ChargerCfg;

/// rrd spawn 时注入的 manifest 绝对路径（supervisor.rs；rr-ipc 尚无常量，模块侧本地定义）
const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml）
const RR3_POWER_CONFIG_ENV: &str = "RR3_POWER_CONFIG";

/// 发布面三主题（module.toml [events].publishes 同源；topics.toml owner=rr3-power）
const RR3_BATTERY_STATE_TOPIC: &str = "rr3-power/battery/state";
const RR3_BATTERY_LOW_TOPIC: &str = "rr3-power/battery/low";
const RR3_CHARGER_STATE_TOPIC: &str = "rr3-power/charger/state";

/// 收帧等待上界兼循环节拍：广播全量扇出会送来与己无关的帧，排空即可，
/// 采样/充电拍点由 Instant 比较驱动，与帧到达无关
const RR3_POLL_WAIT: Duration = Duration::from_millis(100);

fn main() {
    if let Err(e) = rr3_run() {
        eprintln!("rr3-power 退出: {e}");
        std::process::exit(1);
    }
}

fn rr3_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    println!("rr3-power 0.0.1 (0.0.2 T4b 实装)");
    for line in battery::SURFACE
        .iter()
        .copied()
        .chain(charger::SURFACE.iter().copied())
    {
        println!("  {line}");
    }

    // 总线接入：manifest 路径是 rrd spawn 的契约环境变量，直跑（开发调试）须自行设置
    let manifest_path = std::env::var(RR3_MANIFEST_ENV).map_err(|_| {
        format!("缺环境变量 {RR3_MANIFEST_ENV}（rrd spawn 注入；直跑请指向 module.toml）")
    })?;
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读 manifest {manifest_path} 失败: {e}"))?;
    let bus = Rr3BusClient::rr3_connect(&manifest_text)?;
    println!("总线就绪: name={} task={}", bus.rr3_name(), bus.rr3_task());

    let cfg = rr3_config_load(&manifest_path)?;
    println!(
        "config 就绪: battery sample={}ms low={}mV cutoff={}mV charger state={}ms",
        cfg.battery.sample_period_ms,
        cfg.battery.low_voltage_mv,
        cfg.battery.cutoff_voltage_mv,
        cfg.charger.state_period_ms
    );

    // HAL 电源面选择 + 开机探活：Unsupported 即致命——显式回错不静默退化
    // （rr-hal 占位件的契约），也不给"没有电池读数还常驻"的假活
    let (dev, backend) = rr3_hal_power();
    let first =
        rr3_block_on(dev.battery()).map_err(|e| format!("电源面探活失败({backend}): {e}"))?;
    println!(
        "电源面就绪: {backend} 电量 {}% {}mV charging={}",
        first.percent, first.voltage_mv, first.charging
    );

    // 主循环状态：两条流的 seq 各自计数、两个拍点、低电边沿机（首拍即武装）
    let mut battery_seq: u64 = 0;
    let mut charger_seq: u64 = 0;
    let mut low_seq: u64 = 0;
    let mut low_edge = Rr3LowEdge::rr3_low_edge_new(cfg.battery.low_voltage_mv);
    let mut next_battery = Instant::now(); // 首拍即采：兜底链路不等第一个周期
    let mut next_charger = Instant::now();
    let mut last = Some(first);

    loop {
        // 总线死亡探针：断开后 poll_wait 立即返回空 Vec，与"暂时没帧"不可区分——
        // 不探测，总线一死本循环就满转成僵尸。假了就走"不装活着"的退出语义。
        if !bus.rr3_connected() {
            return Err("总线已断开（reader EOF）：交 supervisor 按 restart_policy 裁定".into());
        }
        // 订阅面为空：广播全量扇出送到的帧（module/state 等）只排空不处理
        for frame in bus.rr3_poll_wait(RR3_POLL_WAIT) {
            let _ = frame;
        }

        // battery 拍：读数 → 周期流 → 低电边沿。HAL 读数失败 = 设备/传输级，exit(1)。
        let now = Instant::now();
        if now >= next_battery {
            let st = rr3_block_on(dev.battery())
                .map_err(|e| format!("电池读数失败（{backend}）: {e}"))?;
            battery_seq += 1;
            bus.rr3_publish(
                RR3_BATTERY_STATE_TOPIC,
                battery::rr3_battery_state_body(battery_seq, st),
            )?;
            if low_edge.rr3_low_edge_cross(st.voltage_mv) {
                low_seq += 1;
                bus.rr3_publish(
                    RR3_BATTERY_LOW_TOPIC,
                    battery::rr3_battery_low_body(low_seq, st),
                )?;
                println!(
                    "battery/low: {}% {}mV（越阈首拍，段内不重发）",
                    st.percent, st.voltage_mv
                );
            }
            last = Some(st);
            next_battery = now + Duration::from_millis(cfg.battery.sample_period_ms);
        }

        // charger 拍：由最近一次电池读数派生；首拍 battery 之前不发（读数还没着落）
        if now >= next_charger {
            if let Some(st) = last {
                charger_seq += 1;
                bus.rr3_publish(
                    RR3_CHARGER_STATE_TOPIC,
                    charger::rr3_charger_state_body(charger_seq, st.charging),
                )?;
            }
            next_charger = now + Duration::from_millis(cfg.charger.state_period_ms);
        }
    }
}

/// config.toml 解析：env 覆盖优先，缺省 manifest 同目录。
fn rr3_config_load(manifest_path: &str) -> Result<Rr3PowerCfg, String> {
    let path = std::env::var(RR3_POWER_CONFIG_ENV).unwrap_or_else(|_| {
        std::path::Path::new(manifest_path)
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("config.toml")
            .to_string_lossy()
            .into_owned()
    });
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读 config {path} 失败: {e}"))?;
    rr3_power_config_parse(&text)
}

/// 本模块 config 的汇总形状
#[derive(Debug, Clone, Copy)]
struct Rr3PowerCfg {
    battery: Rr3BatteryCfg,
    charger: Rr3ChargerCfg,
}

/// TOML 文本 → Rr3PowerCfg。手工下钻不引 serde derive：字段个位数，
/// 点名缺字段的文案比派生宏的默认报错可读（rr3-act 同款）。
fn rr3_power_config_parse(text: &str) -> Result<Rr3PowerCfg, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("config TOML 解析失败: {e}"))?;
    let sect = |name: &str| -> Result<&toml::Value, String> {
        v.get("rr3-power")
            .and_then(|s| s.get(name))
            .ok_or_else(|| format!("config 缺 [rr3-power.{name}] 段"))
    };
    let u16_of = |sect: &toml::Value, key: &str, sect_name: &str| -> Result<u16, String> {
        sect.get(key)
            .and_then(toml::Value::as_integer)
            .filter(|n| *n >= 0 && *n <= u16::MAX as i64)
            .map(|n| n as u16)
            .ok_or_else(|| format!("config [rr3-power.{sect_name}] 缺 {key}（0–65535 整数）"))
    };
    let u64_of = |sect: &toml::Value, key: &str, sect_name: &str| -> Result<u64, String> {
        sect.get(key)
            .and_then(toml::Value::as_integer)
            .filter(|n| *n >= 0)
            .map(|n| n as u64)
            .ok_or_else(|| format!("config [rr3-power.{sect_name}] 缺 {key}（非负整数）"))
    };
    let battery_sect = sect("battery")?.clone();
    let charger_sect = sect("charger")?.clone();
    Ok(Rr3PowerCfg {
        battery: Rr3BatteryCfg {
            sample_period_ms: u64_of(&battery_sect, "sample_period_ms", "battery")?.max(1),
            low_voltage_mv: u16_of(&battery_sect, "low_voltage_mv", "battery")?,
            cutoff_voltage_mv: u16_of(&battery_sect, "cutoff_voltage_mv", "battery")?,
        },
        charger: Rr3ChargerCfg {
            state_period_ms: u64_of(&charger_sect, "state_period_ms", "charger")?.max(1),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红
    #[test]
    fn config_parse_covers_repo_shape() {
        let text = r#"
[rr3-power]
log_level = "info"

[rr3-power.battery]
sample_period_ms = 5000
low_voltage_mv = 3300
cutoff_voltage_mv = 3150

[rr3-power.charger]
state_period_ms = 2000
"#;
        let cfg = rr3_power_config_parse(text).unwrap();
        assert_eq!(cfg.battery.sample_period_ms, 5000);
        assert_eq!(cfg.battery.low_voltage_mv, 3300);
        assert_eq!(cfg.battery.cutoff_voltage_mv, 3150);
        assert_eq!(cfg.charger.state_period_ms, 2000);
    }

    #[test]
    fn config_parse_rejects_missing_sections_and_fields() {
        let e = rr3_power_config_parse("[rr3-power]\nlog_level = \"info\"\n").unwrap_err();
        assert!(e.contains("[rr3-power.battery]"), "缺段点名: {e}");
        let e = rr3_power_config_parse(
            "[rr3-power.battery]\nsample_period_ms = 5000\nlow_voltage_mv = 3300\n[rr3-power.charger]\nstate_period_ms = 2000\n",
        )
        .unwrap_err();
        assert!(e.contains("cutoff_voltage_mv"), "缺字段点名: {e}");
        let e = rr3_power_config_parse("garbage =\n").unwrap_err();
        assert!(e.contains("TOML"), "坏文本点名: {e}");
    }
}
