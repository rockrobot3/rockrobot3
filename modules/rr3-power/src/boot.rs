//! rr3-power 接入层（0.0.8 W-A 批1 拆分，ADR-0017 五层表接入层）。
//!
//! config schema 解析（自原 composite.rs 迁入：TOML 下钻、缺字段点名、周期钳制
//! 留痕——文案是装配观测面，逐字不变）＋ HAL 电源面/轨压面选择与开机探活编排
//! （Unsupported 即 Err 上抛：能力缺失显式回错，不静默带病常驻）＋两行就绪回显。
//! 本层零业务 match——策略量经 config 数据化注入（阈值/周期），rail 阈值倒挂
//! 执法是解析面策略合法性，不是业务分支。

use rr_hal::backend::{rr3_hal_power, rr3_hal_rail};
use rr_hal_api::{Rr3BatteryState, Rr3Power, Rr3Rail};

use crate::battery::Rr3BatteryCfg;
use crate::charger::Rr3ChargerCfg;

/// HAL 电源面选择 + 开机探活：首读即探活（Unsupported / 读数失败显式回错），
/// 「电源面就绪」回显归本编排（方案 §4.1 映射表字面）。返回三元——句柄半、
/// 后端名（"电池读数失败（{backend}）"文案复用）、首读（Dispatcher.last 种子）。
pub(crate) fn rr3_power_probe(
) -> Result<(Box<dyn Rr3Power>, &'static str, Rr3BatteryState), String> {
    let (dev, backend) = rr3_hal_power();
    let first = dev
        .battery()
        .map_err(|e| format!("电源面探活失败({backend}): {e}"))?;
    eprintln!(
        "电源面就绪: {backend} 电量 {}% {}mV charging={}",
        first.percent, first.voltage_mv, first.charging
    );
    Ok((dev, backend, first))
}

/// HAL 轨压面选择 + 开机探活（批N2 件五）：`rail.up = true` 时能力缺失显式回错，
/// 不静默带病常驻（与电源面同一 fail-closed 口径；今日全后端占位，宿主真件归
/// 0.0.7 敞口 12 行——up=false 是 host 档合法不适用，无测点不探活）。
pub(crate) fn rr3_rail_probe(cfg: &Rr3RailCfg) -> Result<(Box<dyn Rr3Rail>, &'static str), String> {
    let (rail_dev, rail_backend) = rr3_hal_rail();
    if cfg.up {
        rail_dev
            .rr3_read(&cfg.domain)
            .map_err(|e| format!("轨压面探活失败({rail_backend}): {e}"))?;
        eprintln!("轨压面就绪: {rail_backend} domain={}", cfg.domain);
    }
    Ok((rail_dev, rail_backend))
}

/// 本模块 config 的汇总形状（自原 composite.rs 迁入，字段形状零改动）
#[derive(Debug, Clone)]
pub(crate) struct Rr3PowerCfg {
    pub(crate) battery: Rr3BatteryCfg,
    pub(crate) charger: Rr3ChargerCfg,
    pub(crate) rail: Rr3RailCfg,
}

/// rail 段形状（批N2 件五）：段必填＋全部键必填——`up = false` 时其余键为惰性
/// 但显式写（F-6 精神：合法不适用也要写出来，统一解析路径无分支）；`max_age_ms
/// = 0` 合法（0 = 龄期恒超界 ⇒ 恒 Unknown，诚实语义）；`severe_mv < mild_mv`
/// 严格校验（相等也红——Mild 档不可达）。
#[derive(Debug, Clone)]
pub(crate) struct Rr3RailCfg {
    pub(crate) up: bool,
    pub(crate) domain: String,
    pub(crate) sample_period_ms: u64,
    pub(crate) mild_mv: u16,
    pub(crate) severe_mv: u16,
    pub(crate) max_age_ms: u32,
}

/// TOML 文本 → Rr3PowerCfg。手工下钻不引 serde derive：字段个位数，
/// 点名缺字段的文案比派生宏的默认报错可读（rr3-act 同款）。
pub(crate) fn rr3_power_config_parse(text: &str) -> Result<Rr3PowerCfg, String> {
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
    let u32_of = |sect: &toml::Value, key: &str, sect_name: &str| -> Result<u32, String> {
        sect.get(key)
            .and_then(toml::Value::as_integer)
            .filter(|n| *n >= 0 && *n <= u32::MAX as i64)
            .map(|n| n as u32)
            .ok_or_else(|| format!("config [rr3-power.{sect_name}] 缺 {key}（0–4294967295 整数）"))
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
    // 周期键钳制留痕（0.0.5 批I P-2）：0 无节拍意义，收敛 1ms 并打一行
    // （哪个键、原值、钳后值）——钳位不再静默
    let period_of = |sect: &toml::Value, key: &str, sect_name: &str| -> Result<u64, String> {
        let raw = u64_of(sect, key, sect_name)?;
        if raw < 1 {
            eprintln!("config [rr3-power.{sect_name}] {key}={raw} 无节拍意义，钳到 1ms");
            Ok(1)
        } else {
            Ok(raw)
        }
    };
    // battery/charger 先解析（既有红例的次序契约：缺字段点名先于 rail 段点名），
    // rail 段（批N2 件五）殿后——段必填＋全键必填，up=false 惰性但显式写
    let battery = Rr3BatteryCfg {
        sample_period_ms: period_of(&battery_sect, "sample_period_ms", "battery")?,
        low_voltage_mv: u16_of(&battery_sect, "low_voltage_mv", "battery")?,
        cutoff_voltage_mv: u16_of(&battery_sect, "cutoff_voltage_mv", "battery")?,
    };
    let charger = Rr3ChargerCfg {
        state_period_ms: period_of(&charger_sect, "state_period_ms", "charger")?,
    };
    let rail_sect = sect("rail")?.clone();
    let rail = Rr3RailCfg {
        up: rail_sect
            .get("up")
            .and_then(toml::Value::as_bool)
            .ok_or_else(|| "config [rr3-power.rail] 缺 up（布尔）".to_string())?,
        domain: rail_sect
            .get("domain")
            .and_then(toml::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .ok_or_else(|| "config [rr3-power.rail] 缺 domain（非空字符串）".to_string())?,
        sample_period_ms: period_of(&rail_sect, "sample_period_ms", "rail")?,
        mild_mv: u16_of(&rail_sect, "mild_mv", "rail")?,
        severe_mv: u16_of(&rail_sect, "severe_mv", "rail")?,
        max_age_ms: u32_of(&rail_sect, "max_age_ms", "rail")?,
    };
    if rail.severe_mv >= rail.mild_mv {
        return Err(format!(
            "config [rr3-power.rail] 阈值倒挂: severe={}mV 必须 < mild={}mV（相等则 Mild 档不可达）",
            rail.severe_mv, rail.mild_mv
        ));
    }
    Ok(Rr3PowerCfg {
        battery,
        charger,
        rail,
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

[rr3-power.rail]
up = false
domain = "rail5v"
sample_period_ms = 1000
mild_mv = 4600
severe_mv = 4400
max_age_ms = 0
"#;
        let cfg = rr3_power_config_parse(text).unwrap();
        assert_eq!(cfg.battery.sample_period_ms, 5000);
        assert_eq!(cfg.battery.low_voltage_mv, 3300);
        assert_eq!(cfg.battery.cutoff_voltage_mv, 3150);
        assert_eq!(cfg.charger.state_period_ms, 2000);
        // rail 段逐键（批N2 件五）：up=false 全键惰性但显式写，max_age_ms=0 合法
        assert!(!cfg.rail.up);
        assert_eq!(cfg.rail.domain, "rail5v");
        assert_eq!(cfg.rail.sample_period_ms, 1000);
        assert_eq!(cfg.rail.mild_mv, 4600);
        assert_eq!(cfg.rail.severe_mv, 4400);
        assert_eq!(cfg.rail.max_age_ms, 0);
    }

    #[test]
    fn config_parse_rejects_missing_sections_and_fields() {
        let e = rr3_power_config_parse("[rr3-power]\nlog_level = \"info\"\n").unwrap_err();
        assert!(e.contains("[rr3-power.battery]"), "缺段点名（battery 先解析）: {e}");
        let e = rr3_power_config_parse(
            "[rr3-power.battery]\nsample_period_ms = 5000\nlow_voltage_mv = 3300\n[rr3-power.charger]\nstate_period_ms = 2000\n",
        )
        .unwrap_err();
        assert!(e.contains("cutoff_voltage_mv"), "缺字段点名: {e}");
        // rail 段缺段点名（批N2 件五：段必填）＋缺 up 键点名（全键必填）
        let e = rr3_power_config_parse(
            "[rr3-power.battery]\nsample_period_ms = 5000\nlow_voltage_mv = 3300\ncutoff_voltage_mv = 3150\n[rr3-power.charger]\nstate_period_ms = 2000\n",
        )
        .unwrap_err();
        assert!(e.contains("[rr3-power.rail]"), "rail 缺段点名: {e}");
        let e = rr3_power_config_parse(
            "[rr3-power.battery]\nsample_period_ms = 5000\nlow_voltage_mv = 3300\ncutoff_voltage_mv = 3150\n[rr3-power.charger]\nstate_period_ms = 2000\n[rr3-power.rail]\ndomain = \"rail5v\"\nsample_period_ms = 1000\nmild_mv = 4600\nsevere_mv = 4400\nmax_age_ms = 0\n",
        )
        .unwrap_err();
        assert!(e.contains("缺 up"), "rail 缺 up 键点名: {e}");
        let e = rr3_power_config_parse("garbage =\n").unwrap_err();
        assert!(e.contains("TOML"), "坏文本点名: {e}");
    }

    /// 阈值倒挂执法（批N2 件五）：severe >= mild 即红——相等也红（Mild 档不可达）。
    /// up=false 不豁免：解析路径统一无分支，策略量合法性不看开关。
    #[test]
    fn config_parse_rejects_inverted_rail_thresholds() {
        let base = "\ncutoff_voltage_mv = 3150\n[rr3-power.charger]\nstate_period_ms = 2000\n[rr3-power.rail]\nup = false\ndomain = \"rail5v\"\nsample_period_ms = 1000\n";
        let e = rr3_power_config_parse(
            &format!("[rr3-power.battery]\nsample_period_ms = 5000\nlow_voltage_mv = 3300\n{base}mild_mv = 4400\nsevere_mv = 4400\nmax_age_ms = 0\n"),
        )
        .unwrap_err();
        assert!(e.contains("阈值倒挂"), "相等也红: {e}");
        let e = rr3_power_config_parse(
            &format!("[rr3-power.battery]\nsample_period_ms = 5000\nlow_voltage_mv = 3300\n{base}mild_mv = 4400\nsevere_mv = 4500\nmax_age_ms = 0\n"),
        )
        .unwrap_err();
        assert!(e.contains("阈值倒挂"), "severe > mild 红: {e}");
    }

    /// 钳制留痕（0.0.5 批I P-2）：0 周期收敛 1 不拒 boot——采样拍不能自毁；
    /// eprintln 留痕在解析期打（键名/原值/钳后值），此处钉收敛结果
    #[test]
    fn config_parse_clamps_zero_period_to_one() {
        let text = r#"
[rr3-power.battery]
sample_period_ms = 0
low_voltage_mv = 3300
cutoff_voltage_mv = 3150

[rr3-power.charger]
state_period_ms = 0

[rr3-power.rail]
up = false
domain = "rail5v"
sample_period_ms = 0
mild_mv = 4600
severe_mv = 4400
max_age_ms = 0
"#;
        let cfg = rr3_power_config_parse(text).unwrap();
        assert_eq!(cfg.battery.sample_period_ms, 1, "0 周期钳到 1ms");
        assert_eq!(cfg.charger.state_period_ms, 1, "0 周期钳到 1ms");
        assert_eq!(cfg.rail.sample_period_ms, 1, "rail 0 周期同钳 1ms");
    }
}
