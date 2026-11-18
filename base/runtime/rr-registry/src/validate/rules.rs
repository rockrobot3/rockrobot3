//! 单条规则的词形/枚举判据与失配文案。

use crate::schema::rr3_rule_list;

const RR3_TOPIC_HEAD: &str = "rr3-";

/// 小写段（`[a-z]+`）：manifest.name 去前缀后的部分
fn rr3_lower_alpha(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase())
}

/// capability 段：小写字母 + 下划线，非空
fn rr3_lower_us(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// `^[a-z]+(\.[a-z_]+)+$`
fn rr3_capability_shape(s: &str) -> bool {
    let segs: Vec<&str> = s.split('.').collect();
    segs.len() >= 2 && segs.iter().all(|s| rr3_lower_us(s))
}

/// `rr3-<模块>/<域>/<事件>`，三段；首段带 rr3- 前缀
fn rr3_topic_shape_ok(name: &str) -> bool {
    let segs: Vec<&str> = name.split('/').collect();
    if !rr3_topic_head_ok(&segs) {
        return false;
    }
    rr3_topic_tail_ok(&segs)
}

/// 首段形制：三段且首段带 rr3- 前缀
fn rr3_topic_head_ok(segs: &[&str]) -> bool {
    segs.len() == 3 && segs[0].starts_with(RR3_TOPIC_HEAD)
}

/// 首段主干小写字母，域/事件段小写下划线
fn rr3_topic_tail_ok(segs: &[&str]) -> bool {
    let stem = &segs[0][RR3_TOPIC_HEAD.len()..];
    rr3_lower_alpha(stem) && segs[1..].iter().all(|s| rr3_lower_us(s))
}

/// 点分数字版本
fn rr3_semver_ok(s: &str) -> bool {
    s.split('.').count() >= 2 && s.split('.').all(rr3_semver_part)
}

/// 版本段：非空且全十进制数字
fn rr3_semver_part(p: &str) -> bool {
    !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())
}

/// 单条规则检查（上下文无关规则）。`own_topic` 需 manifest.name，由
/// `rr3_manifest_gate` 特判，不进这里。
pub fn rr3_registry_check(rule: &str, value: &str) -> Vec<String> {
    rr3_rule_pass(rule, value).map_or_else(
        || vec![format!("未知规则名（schema 与代码不同步）: {rule}")],
        |ok| {
            if ok {
                Vec::new()
            } else {
                vec![rr3_rule_err(rule, value)]
            }
        },
    )
}

/// 规则裁决总入口：词形类（[`rr3_rule_shape`]）先查，未命中再查枚举词表；
/// `None` = 规则名不在册（调用方点名）。
fn rr3_rule_pass(rule: &str, value: &str) -> Option<bool> {
    if let Some(ok) = rr3_rule_shape(rule, value) {
        return Some(ok);
    }
    rr3_rule_enum(rule, value)
}

/// module_name 词形：去 `rr3-` 前缀后小写字母
fn rr3_module_name_ok(value: &str) -> bool {
    rr3_lower_alpha(value.strip_prefix(RR3_TOPIC_HEAD).unwrap_or(""))
}

/// 词形类规则表项（规则名, 值判据）。
type Rr3ShapeRule = (&'static str, fn(&str) -> bool);

/// 词形类规则（值自身判据，与词表无关）。
fn rr3_rule_shape(rule: &str, value: &str) -> Option<bool> {
    // 原子件名（atom.schema.toml，0.0.5 批B①）：^[a-z][a-z_]*$，复用 capability 段规则
    let shapes: [Rr3ShapeRule; 5] = [
        ("module_name", rr3_module_name_ok),
        ("semver", rr3_semver_ok),
        ("capability", rr3_capability_shape),
        ("atom_name", rr3_lower_us),
        ("topic", rr3_topic_shape_ok),
    ];
    shapes
        .iter()
        .find(|(r, _)| *r == rule)
        .map(|(_, f)| (*f)(value))
}

/// 枚举类规则（`[rules]` 词表成员检查）；`none` 恒放行。
fn rr3_rule_enum(rule: &str, value: &str) -> Option<bool> {
    if rule == "none" {
        return Some(true);
    }
    if !["type", "lang", "restart_policy"].contains(&rule) {
        return None;
    }
    Some(
        rr3_rule_list(rule)
            .unwrap_or_default()
            .iter()
            .any(|a| a == value),
    )
}

/// 词形类规则的失配文案表（label + 追加提示，与判据同表单点）。
const RR3_SHAPE_ERRS: [(&str, &str, &str); 5] = [
    ("module_name", "manifest.name 非法", "要求 ^rr3-[a-z]+$"),
    ("semver", "semver 非法", "要求点分数字"),
    (
        "capability",
        "capability 非法",
        r"要求 ^[a-z]+(\.[a-z_]+)+$",
    ),
    ("atom_name", "atom.name 非法", "要求 ^[a-z][a-z_]*$"),
    ("topic", "topic 非法", "要求 rr3-<模块>/<域>/<事件>"),
];

/// 规则失配文案（一句一规则，与判据同表单点；新增规则两处同批）。
fn rr3_rule_err(rule: &str, value: &str) -> String {
    if let Some((_, label, hint)) = RR3_SHAPE_ERRS.iter().find(|(r, _, _)| *r == rule) {
        return format!("{label}: {value}（{hint}）");
    }
    if ["type", "lang", "restart_policy"].contains(&rule) {
        return format!(
            "{rule} 越界: {value}（允许 {:?}）",
            rr3_rule_list(rule).unwrap_or_default()
        );
    }
    format!("未知规则名（schema 与代码不同步）: {rule}")
}
