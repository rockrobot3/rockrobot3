//! 器件型号词表（增补轮六 §4.1b）：`contracts/devices.toml` 的集合视图。
//!
//! 生产真相只有 `contracts/devices.toml`。本文件实现它的装载与形状检查：
//! `wire.mate` 对账（校验⑦-d 的 mate 集判定源）、`peak_ma` 与件 demands 互校
//! （校验⑨，批N2）、器件级在位探测的 `id_reg` 数据源（§12.1/L-14）都查这张表。
//! 装载期不认具体器件型号（D10a）不与本章冲突：装载期只查 mate 外键闭合，
//! 型号能力（addrs/baud_ok）在探测期与出厂自检消费。空器件表是契约事故，
//! 不是放行许可证。
//!
//! 与 roles.rs 同型：解析失败给空表（`rr3_device_table_check` 点名"表为空"）；
//! 跨表判等不在此处（⑦-d 的另一侧真值是 rrd 装载链手里的 wiring 数据，
//! 由调用方携带判定）。

use std::collections::{BTreeMap, BTreeSet};
use toml::Value;

/// 合法 bus 词域（devices.toml 字段语义列，封闭集；I2S 不在其列——INMP441 等
/// 板侧拾音器件待 bus 词表扩（契约级变更）再收）
pub const RR3_DEVICE_BUSES: [&str; 6] = ["i2c", "spi", "vspi", "1wire", "adc", "pwm"];

/// 一条器件的登记面
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3DeviceEntry {
    bus: String,
    addrs: Vec<String>,
    baud_ok: Vec<i64>,
    /// 器件级在位探测寄存器 (地址, 期望读回值)；缺省 = 无 ID 寄存器，
    /// 探测面只允许 {reachable, absent}（不得冒充"在位"，devices.toml 注记）。
    /// 解析失败（缺 addr/expect 或类型不合）按缺省收——探测面退化到
    /// reachable/absent 是安全方向（永不假报"在位"），形状真相由 CI 契约
    /// 测试的 id_reg pin 钉住。
    id_reg: Option<(i64, i64)>,
    peak_ma: i64,
    mate: String,
}

/// 一册器件表（不可变快照）：型号名 → 登记面
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3DeviceTable {
    devices: BTreeMap<String, Rr3DeviceEntry>,
}

/// 解析一份器件表文本（生产契约与测试叠加同构，共用同一解析器，不写第二套）。
/// 解析失败给空表（`rr3_device_table_check` 会点名"表为空"，fail-closed 护栏同 roles）。
pub fn rr3_registry_device_table_from_text(text: &str) -> Rr3DeviceTable {
    let Ok(root) = toml::from_str::<Value>(text) else {
        return Rr3DeviceTable::default();
    };
    let mut table = Rr3DeviceTable::default();
    for d in root
        .get("device")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_table)
    {
        if let Some(model) = d.get("model").and_then(Value::as_str) {
            let id_reg = d.get("id_reg").and_then(Value::as_table).and_then(|t| {
                let addr = t.get("addr").and_then(Value::as_integer)?;
                let expect = t.get("expect").and_then(Value::as_integer)?;
                Some((addr, expect))
            });
            table.devices.insert(
                model.to_string(),
                Rr3DeviceEntry {
                    bus: d
                        .get("bus")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    addrs: d
                        .get("addrs")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                    baud_ok: d
                        .get("baud_ok")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_integer).collect())
                        .unwrap_or_default(),
                    id_reg,
                    peak_ma: d.get("peak_ma").and_then(Value::as_integer).unwrap_or(0),
                    mate: d
                        .get("mate")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                },
            );
        }
    }
    table
}

impl Rr3DeviceTable {
    pub fn rr3_device_table_is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    /// 查册：型号是否在册（门 o 的判定入口，§8 待建）
    pub fn rr3_device_table_contains(&self, model: &str) -> bool {
        self.devices.contains_key(model)
    }

    pub fn rr3_device_table_names(&self) -> Vec<String> {
        self.devices.keys().cloned().collect()
    }

    /// 器件峰值电流（校验⑨ 与件 demands.peak_ma 互校的分母侧事实源，批N2 消费）
    pub fn rr3_device_table_peak_ma(&self, model: &str) -> Option<i64> {
        self.devices.get(model).map(|e| e.peak_ma)
    }

    /// 全表 mate 集（校验⑦-d 的判定集）：wire.mate 必须落在其中，"none" 是
    /// 直连无对端的合法显式值（由调用方先剥）。空 mate（INA219 排针直连类
    /// 缺省）不进集——它不是可对账的对端型号。
    pub fn rr3_device_table_mates(&self) -> BTreeSet<String> {
        self.devices
            .iter()
            .filter(|(_, e)| !e.mate.is_empty())
            .map(|(_, e)| e.mate.clone())
            .collect()
    }

    /// 自身形状检查：非空 + bus 词域封闭 + I2C 类必带地址集 + baud_ok/peak_ma
    /// 必填（峰值 0 会让校验⑨ 恒过 = 假账）。mate 可缺省（直连类器件）。
    /// addrs/baud_ok 的**取值域**（地址是否合法、速率是否可达）是探测期与
    /// 出厂自检的判据（坑 7：装载期不认），本检查只管形状。
    pub fn rr3_device_table_check(&self) -> Vec<String> {
        if self.rr3_device_table_is_empty() {
            return vec!["器件表为空：必须至少登记一个 [[device]]（增补轮六 §4.1b）".to_string()];
        }
        let mut errs = Vec::new();
        for (model, e) in &self.devices {
            if model.is_empty() {
                errs.push("device 的 model 缺失或为空".to_string());
            }
            if !RR3_DEVICE_BUSES.contains(&e.bus.as_str()) {
                errs.push(format!(
                    "device {model} 的 bus 非法: {}（封闭词域 {RR3_DEVICE_BUSES:?}）",
                    e.bus
                ));
            }
            if e.bus == "i2c" && e.addrs.is_empty() {
                errs.push(format!(
                    "device {model} 是 I2C 类但 addrs 为空（合法地址集必填——装机写集外地址即红）"
                ));
            }
            for a in &e.addrs {
                if a.is_empty() {
                    errs.push(format!("device {model} 的 addrs 含空串"));
                }
            }
            if e.baud_ok.is_empty() {
                errs.push(format!(
                    "device {model} 的 baud_ok 为空（速率取值域必填——坑 7 的落点，不在册即拒 boot）"
                ));
            }
            if e.peak_ma <= 0 {
                errs.push(format!(
                    "device {model} 的 peak_ma 缺失或 ≤ 0（校验⑨ 预算分母的事实源，0 会让预算恒过）"
                ));
            }
        }
        errs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // include_str! 以本文件所在目录为基准：src → rr-registry → runtime → base → 仓库根
    const CONTRACT: &str = include_str!("../../../../contracts/devices.toml");

    /// 契约必须解析成 6 条器件，逐条点名断言（与 contract.rs 的形状 pin 同源、彼此独立）
    #[test]
    fn contract_text_parses_into_expected_shape() {
        let table = rr3_registry_device_table_from_text(CONTRACT);
        assert_eq!(
            table.devices.len(),
            6,
            "契约器件数（批N3 件八起 SHTC3/ICM-42688 演练行，现役六条）"
        );
        assert_eq!(
            table.rr3_device_table_names(),
            vec!["ICM-42688", "INA219", "MAX7219", "SG90", "SHTC3", "SSD1306"]
        );
        assert!(table.rr3_device_table_contains("SSD1306"));
        assert!(
            !table.rr3_device_table_contains("INMP441"),
            "bus 词表未扩，不在册"
        );
        // mate 集（⑦-d 判定集）：INA219 直连缺省不进集
        let mates = table.rr3_device_table_mates();
        assert_eq!(mates.len(), 3, "{mates:?}");
        assert!(mates.contains("SG90-3P"));
        assert!(mates.contains("0.96in-OLED-FPC"));
        assert!(mates.contains("MAX7219-DIP24"));
        assert!(!mates.contains("none"));
        // peak_ma 与 id_reg（⑨ 的事实源 / L-14 的数据源）
        assert_eq!(table.rr3_device_table_peak_ma("SG90"), Some(700));
        assert_eq!(table.rr3_device_table_peak_ma("INA219"), Some(1));
        // 契约形状自检：真契约必须全绿
        assert!(table.rr3_device_table_check().is_empty());
    }

    /// 解析失败给空表，空表是契约事故——check 点名不静默放行
    #[test]
    fn unparsable_text_yields_empty_table_and_check_is_fatal() {
        assert!(rr3_registry_device_table_from_text("not [ toml")
            .devices
            .is_empty());
        let errs = rr3_registry_device_table_from_text("").rr3_device_table_check();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("器件表为空"), "{errs:?}");
    }

    /// 形状反例：bus 越域 / I2C 缺地址集 / baud_ok 缺 / peak_ma ≤ 0 逐条点名
    #[test]
    fn shape_errors_are_named() {
        let table = rr3_registry_device_table_from_text(
            r#"
[[device]]
model = "X1"
bus = "i2s"
peak_ma = 10
baud_ok = [1]

[[device]]
model = "X2"
bus = "i2c"
baud_ok = [100000]
peak_ma = 10

[[device]]
model = "X3"
bus = "spi"
addrs = ["0x40"]
baud_ok = [1]
peak_ma = 0

[[device]]
model = "X4"
bus = "spi"
peak_ma = 10
"#,
        );
        let errs = table.rr3_device_table_check();
        assert_eq!(errs.len(), 4, "{errs:?}");
        assert!(errs
            .iter()
            .any(|e| e.contains("X1") && e.contains("bus 非法")));
        assert!(errs
            .iter()
            .any(|e| e.contains("X2") && e.contains("addrs 为空")));
        assert!(errs
            .iter()
            .any(|e| e.contains("X3") && e.contains("peak_ma")));
        assert!(errs
            .iter()
            .any(|e| e.contains("X4") && e.contains("baud_ok 为空")));
    }
}
