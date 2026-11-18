//! [hardware] 段解读：可选的非引脚键，值只收字符串或整数标量。

use std::collections::BTreeMap;

/// [hardware]：可选段（纯 sim/无硬件格形态可以不给）。每键非空、值必须是
/// 字符串或整数标量（整数按十进制字符串化——rr-hal 描述符解析端全按字符串
/// 收）；布尔/浮点/数组/表逐条点名，一条不静默放行。
pub(super) fn rr3_hardware_parse(
    root: &toml::Value,
    errs: &mut Vec<String>,
) -> BTreeMap<String, String> {
    let mut hardware = BTreeMap::new();
    match root.get("hardware").and_then(toml::Value::as_table) {
        None => {}
        Some(t) => rr3_hardware_rows(t, &mut hardware, errs),
    }
    hardware
}

/// 逐键收册：空键点名，值只收字符串/整数标量（整数十进制字符串化）。
fn rr3_hardware_rows(
    t: &toml::Table,
    hardware: &mut BTreeMap<String, String>,
    errs: &mut Vec<String>,
) {
    for (k, v) in t {
        if k.is_empty() {
            errs.push("[hardware] 键不得为空串".to_string());
            continue;
        }
        match v {
            toml::Value::String(s) => {
                hardware.insert(k.clone(), s.clone());
            }
            toml::Value::Integer(i) => {
                hardware.insert(k.clone(), i.to_string());
            }
            other => errs.push(format!(
                "hardware.{k} 应为字符串或整数标量（物理绑定值），收到 {other:?}"
            )),
        }
    }
}
