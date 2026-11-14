//! [capabilities] 段：能力集四键与 input-only 集。

use super::*;

/// `[capabilities]` 段：能力集四键（段缺即红；空集合法但必须显式写键）。
pub(super) fn rr3_plat_caps(t: &toml::Table, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    let Some(v) = t.get("capabilities") else {
        errs.push("平台表缺 [capabilities] 段（⑤-a/⑤-b 判据前件，缺段即红，裁定①）".into());
        return;
    };
    let Value::Table(caps) = v else {
        errs.push(format!(
            "平台表 capabilities 应为表（[capabilities]），收到 {v:?}"
        ));
        return;
    };
    rr3_caps_keys(caps, errs);
    rr3_caps_set(caps, table, errs);
}

/// 能力集四键封闭：未知键逐条点名。
fn rr3_caps_keys(caps: &toml::Table, errs: &mut Vec<String>) {
    for k in caps.keys() {
        rr3_caps_key_err(k, errs);
    }
}

/// 单个能力集键裁决（四键外点名）。
fn rr3_caps_key_err(k: &str, errs: &mut Vec<String>) {
    if ![
        "adc_pins",
        "input_only_pins",
        "no_open_drain",
        "output_capable_pins",
    ]
    .contains(&k)
    {
        errs.push(format!(
            "[capabilities] 未知键 {k}（只认 adc_pins / input_only_pins / no_open_drain / output_capable_pins）"
        ));
    }
}

/// 能力四键取值：三个引脚集 + output_capable_pins 规则集（缺键分别点名）。
fn rr3_caps_set(caps: &toml::Table, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    table.capabilities.adc_pins = rr3_caps_pins(
        caps,
        "adc_pins",
        "[capabilities] 缺 adc_pins（无 ADC 须显式写 [] 并带行级注记，裁定②）",
        errs,
    );
    table.capabilities.input_only_pins = rr3_caps_pins(
        caps,
        "input_only_pins",
        "[capabilities] 缺 input_only_pins（无此类脚须显式写 []）",
        errs,
    );
    table.capabilities.no_open_drain = rr3_caps_pins(
        caps,
        "no_open_drain",
        "[capabilities] 缺 no_open_drain（无此类脚须显式写 []）",
        errs,
    );
    rr3_caps_output(caps, table, errs);
}

/// 单个引脚集键：有值解析，缺键点名并回空集。
fn rr3_caps_pins(
    caps: &toml::Table,
    key: &str,
    missing: &str,
    errs: &mut Vec<String>,
) -> BTreeSet<i64> {
    let Some(v) = caps.get(key) else {
        errs.push(missing.to_string());
        return BTreeSet::new();
    };
    rr3_pin_set_from(v, key, errs)
}

/// output_capable_pins：在册规则写入，越域/缺键分别点名。
fn rr3_caps_output(caps: &toml::Table, table: &mut Rr3PlatformTable, errs: &mut Vec<String>) {
    let Some(r) = caps.get("output_capable_pins").and_then(Value::as_str) else {
        errs.push(
            "[capabilities] 缺 output_capable_pins（取值规则集，§4.6：写规则不写枚举）".into(),
        );
        return;
    };
    if !RR3_PLATFORM_OUTPUT_RULES.contains(&r) {
        errs.push(format!(
            "[capabilities].output_capable_pins \"{r}\" 不在册（在册规则 {RR3_PLATFORM_OUTPUT_RULES:?}，扩规则走契约级评审）"
        ));
        return;
    }
    table.capabilities.output_rule = r.to_string();
}
