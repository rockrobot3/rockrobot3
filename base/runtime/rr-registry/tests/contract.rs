//! 契约册形状的对拍用例：slots/roles/devices/topics/platform/wiring 的成员与判据。
//!
//! 判据源一律取仓内真册（`contracts/`、`profiles/`、`hardware/`），不手抄第二份；
//! 每个用例钉一类跨表判等（如 tier↔tier_scope、pin↔角色、平台词表），改了契约
//! 册或解析器都会在这里显形。

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR 已是 base/runtime/rr-registry -> runtime -> base -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn topics_toml() -> String {
    std::fs::read_to_string(repo_root().join("contracts/topics.toml"))
        .expect("contracts/topics.toml 必须存在（Task 4 产物）")
}

mod assemble;
mod depends;
mod failopen;
mod manifest;
mod profile_board;
mod profile_default;
mod profile_esp32;
mod profile_pi4b;
mod shape;
mod slots;
mod topics;

fn fixture_text() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rr3-sense.toml"),
    )
    .unwrap()
}

/// 把改写后的 fixture 放进带契约副本的临时根，返回 manifest 路径
fn staged(tag: &str, text: String) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rr3-mf-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("modules/rr3-sense")).unwrap();
    std::fs::create_dir_all(dir.join("contracts")).unwrap();
    std::fs::copy(
        repo_root().join("contracts/topics.toml"),
        dir.join("contracts/topics.toml"),
    )
    .unwrap();
    let m = dir.join("modules/rr3-sense/module.toml");
    std::fs::write(&m, text).unwrap();
    m
}

/// 从 topics.toml 文本解析出的 (name, owner, kind) 三元组
fn registry_triples(text: &str) -> Vec<(String, String, String)> {
    fn topic_str(v: &toml::Value, k: &str) -> String {
        v.get(k)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    let root: toml::Value = text.parse().expect("contracts/topics.toml 必须是合法 TOML");
    let arr = root
        .get("topic")
        .and_then(toml::Value::as_array)
        .expect("contracts/topics.toml 缺 [[topic]] 数组");
    arr.iter()
        .map(|e| {
            (
                topic_str(e, "name"),
                topic_str(e, "owner"),
                topic_str(e, "kind"),
            )
        })
        .collect()
}

/// C2：把 `contracts/topics.toml` 的**形状**钉死——owner 数、topic 数、kind 分布，
/// 以及全部 39 条 `(name, owner, kind)` 三元组逐条在册（且不多不少）。
///
/// 计数做成硬 pin 是**故意**的，不是"禁止 append"：契约只增不改，新增主题合法，
/// 但必须是一次**有意识的编辑**——同一次改动里既要改 `contracts/topics.toml`，
/// 也要更新这里的 `RR3_PINNED_TOPICS` / 四个计数期望，以及把这些形状当事实来写的
/// 文档（`docs/architecture/architecture.md` §6.3、`docs/architecture/naming.md` §3、`contracts/README.md`）。
/// 少改任何一处，本用例即红。
///（注：本数组在批N4 件六二/九二/九三只改了计数 pin、漏了随批补条——批N5 件二笔3
/// 补齐七条后重新与 topics.toml 等长，"不多不少"的等价自此恢复。）
///
/// 反过来说，删一条主题、改一条 kind、改一个 owner 数同样会红——评审证明这正是
/// 此前门禁的空洞（只有 owner 数与 fixture 引用的四条主题被钉住）。
/// 仲裁只吃 name / provides / depends.required / start_priority 四样事实，
/// 夹具因此只写这四样——其余字段的语法归单件校验，不在本节重复
fn depends_manifest(name: &str, provides: &[&str], requires: &[&str], prio: i64) -> String {
    let list = |v: &[&str]| {
        v.iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "[manifest]\nname = \"{name}\"\n\
         [provides]\ncapabilities = [{}]\n\
         [depends]\nrequired = [{}]\n\
         [resources]\nstart_priority = {prio}\n",
        list(provides),
        list(requires)
    )
}

fn facts_of(texts: &[String]) -> Vec<rr_registry::Rr3DependsFacts> {
    texts
        .iter()
        .map(|t| rr_registry::rr3_depends_parse(t).expect("夹具事实提取不该失败"))
        .collect()
}

// ---------- 能力格查册门（ADR-0012 裁定 5②）：manifest 与 profile 双侧对 slots.toml 查册 ----------

fn slots_toml() -> String {
    std::fs::read_to_string(repo_root().join("contracts/slots.toml"))
        .expect("contracts/slots.toml 必须存在（ADR-0012 裁定 1 产物）")
}

/// modules/ 六模块的 (模块名, manifest 根 Value)，按名字升序
fn module_manifest_values() -> Vec<(String, toml::Value)> {
    let modules = repo_root().join("modules");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&modules)
        .expect("读不到 modules/ 目录")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
    {
        let m = entry.join("module.toml");
        if m.is_file() {
            let v: toml::Value = toml::from_str(&std::fs::read_to_string(&m).unwrap())
                .expect("module.toml 必须可解析");
            let name = v
                .get("manifest")
                .and_then(|m| m.get("name"))
                .and_then(toml::Value::as_str)
                .expect("manifest 缺 name")
                .to_string();
            out.push((name, v));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn str_list(v: &toml::Value, path: &str) -> Vec<String> {
    let Some(node) = path.split('.').try_fold(v, |acc, seg| acc.get(seg)) else {
        return vec![];
    };
    node.as_array()
        .map(|a| {
            a.iter()
                .filter_map(toml::Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// [supplies] 表在册 + 六个安全供给键逐一在册（default/distro 装配单共用），带回表续用
fn supplies_keys(v: &toml::Value) -> &toml::Table {
    let supplies = v
        .get("supplies")
        .and_then(toml::Value::as_table)
        .expect("缺 [supplies]");
    for k in [
        "watchdog_present",
        "watchdog_timeout_ms",
        "watchdog_feeder",
        "tick_period_ms",
        "tick_jitter_ms",
        "safety_core_interval_ms",
    ] {
        assert!(supplies.contains_key(k), "缺 [supplies].{k}");
    }
    supplies
}

/// [supplies.budget]（校验⑩ 分母，批N2）全段 pin（host 机装配单共用）：
/// not_applicable 显式 true，数字预算键不得并存
fn pin_budget(supplies: &toml::Table, what: &str) {
    let budget = supplies
        .get("budget")
        .and_then(toml::Value::as_table)
        .expect("缺 [supplies.budget]（校验⑩ 分母，批N2）");
    assert_eq!(
        budget.get("not_applicable").and_then(toml::Value::as_bool),
        Some(true),
        "{what} 档 budget 合法不适用必须显式 true"
    );
    for num_key in [
        "task_stack_budget_bytes",
        "heap_total_bytes",
        "heap_reserved_bytes",
    ] {
        assert!(
            budget.get(num_key).is_none(),
            "host 档 not_applicable = true 与数字预算键 {num_key} 并存（自相矛盾）"
        );
    }
}

/// [enabled_slots] 对账（default/distro 装配单共用）：覆盖模块集合 == modules/ 在册，
/// 每模块格位集合 == manifest provides（normal 格），引用格位全部在表
fn enabled_slots_check(
    v: &toml::Value,
    manifests: &std::collections::BTreeMap<String, toml::Value>,
    table: &rr_registry::Rr3SlotTable,
) {
    let enabled_slots = v
        .get("enabled_slots")
        .and_then(toml::Value::as_table)
        .expect("缺 [enabled_slots]");
    let mut face_modules: Vec<&String> = enabled_slots.keys().collect();
    face_modules.sort();
    let mut manifest_modules: Vec<&String> = manifests.keys().collect();
    manifest_modules.sort();
    assert_eq!(
        face_modules, manifest_modules,
        "[enabled_slots] 覆盖的模块集合与 modules/ 漂移"
    );
    for (module, face) in enabled_slots {
        let mut slots: Vec<String> = face
            .as_array()
            .expect("enabled_slots 值必须是数组")
            .iter()
            .filter_map(toml::Value::as_str)
            .map(String::from)
            .collect();
        slots.sort();
        // manifest provides 可能含 placeholder/retired 格（模块"能提供"≠"装配单启用"），
        // 装配面只启用 normal 格；此处对 caps 做同口径过滤再判等。
        let mut caps: Vec<String> = str_list(&manifests[module], "provides.capabilities")
            .into_iter()
            .filter(|c| {
                let s = table.rr3_slot_status(c).unwrap_or_default();
                s != "placeholder" && s != "retired"
            })
            .collect();
        caps.sort();
        assert_eq!(
            slots, caps,
            "{module} 的 [enabled_slots] 与 manifest provides（normal 格）漂移"
        );
        for s in &slots {
            assert!(
                table.rr3_slot_contains(s),
                "[enabled_slots] 引用表外格位 {s}"
            );
        }
    }
}

/// platform.chip 断言（同板多机型共用）：档位所配 chip 必须与平台表登记一致
fn assert_chip(v: &toml::Value, chip: &str, what: &str) {
    assert_eq!(
        v.get("platform")
            .and_then(|p| p.get("chip"))
            .and_then(toml::Value::as_str),
        Some(chip),
        "{what} chip 漂移（平台表选 contracts/platform/{chip}.toml）"
    );
}

/// platform.tier 钉值断言（七处 profile 同款）：消息各自点名机型档位
fn assert_tier(v: &toml::Value, want: &str, msg: &str) {
    assert_eq!(
        v.get("platform")
            .and_then(|p| p.get("tier"))
            .and_then(toml::Value::as_str),
        Some(want),
        "{msg}"
    );
}

/// S3 板侧十一键真值（esp32s3-mini 与 pi4b-host-esp32s3 两处装配单同源：
/// 与 board/pins_s3.rs、main.rs 具名装配三方同值，漂移即红）
fn s3_hardware_keys(hardware: &toml::Table, what: &str, why: &str) {
    for (k, expected) in [
        ("i2c_sda", "8"),
        ("i2c_scl", "9"),
        ("adc_rail_pin", "1"),
        ("uart2_tx", "41"),
        ("uart2_rx", "42"),
        ("ledc_pan", "4"),
        ("ledc_tilt", "5"),
        ("i2s_bclk", "6"),
        ("i2s_sd", "15"),
        ("i2s_ws", "7"),
        ("cred_store", "nvs:rr3"),
    ] {
        assert_eq!(
            hardware.get(k).and_then(toml::Value::as_str),
            Some(expected),
            "{what} hardware.{k} 漂移（{why}）"
        );
    }
}

/// 生产格位全集：非 base 且非 placeholder/retired（[slots].present 对账与
/// 未绑格位检查两处同款 filter 链，一份判据一份真值）
fn normal_slots(table: &rr_registry::Rr3SlotTable) -> Vec<String> {
    table
        .rr3_slot_names()
        .into_iter()
        .filter(|n| table.rr3_slot_layer(n).as_deref() != Some("base"))
        .filter(|n| {
            let s = table.rr3_slot_status(n).unwrap_or_default();
            s != "placeholder" && s != "retired"
        })
        .collect()
}

/// 装配单 [slots].present 的字符串集（缺段/缺键 expect 点名，不静默放空）
fn present_of(v: &toml::Value) -> Vec<String> {
    v.get("slots")
        .and_then(|s| s.get("present"))
        .and_then(toml::Value::as_array)
        .expect("缺 [slots].present")
        .iter()
        .filter_map(toml::Value::as_str)
        .map(String::from)
        .collect()
}

/// [bindings] 逐项对账底座（default/distro 与 esp32-mini 两处装配单同款）：
/// provider 字符串化 + 格位在册 + 非基础层 + provider 确实 provides 该格位，
/// 返回 (格位, provider) 序列由调用方按各自形状对账
fn bindings_walk<'a>(
    bindings: &'a toml::Table,
    table: &rr_registry::Rr3SlotTable,
    manifests: &std::collections::BTreeMap<String, toml::Value>,
) -> Vec<(&'a str, &'a str)> {
    let mut bound = Vec::new();
    for (slot, provider) in bindings {
        let provider = provider
            .as_str()
            .unwrap_or_else(|| panic!("bindings.{slot} 非字符串"));
        assert!(
            table.rr3_slot_contains(slot),
            "bindings 引用表外格位 {slot}"
        );
        assert_ne!(
            table.rr3_slot_layer(slot).unwrap(),
            "base",
            "基础层格位 {slot} 由基座内建承载，不得绑定模块"
        );
        let m = manifests
            .get(provider)
            .unwrap_or_else(|| panic!("bindings.{slot} 绑到不在册模块 {provider}"));
        assert!(
            str_list(m, "provides.capabilities")
                .iter()
                .any(|c| c == slot),
            "{provider} 的 manifest 未 provides 被绑格位 {slot}"
        );
        bound.push((slot.as_str(), provider));
    }
    bound
}
/// schema 3 [pins] 与 roles.toml 的对账 pin（增补轮六批N1，§4.2 + 校验①的 CI 全集
/// 层同判）：[[pin]] 的 role 外键查册、direction 与角色一致、instance 有无与
/// per_instance 相符；[hardware] 不得出现八物理键（门 n 稳态判据的 CI 侧 pin——
/// 运行期判在 rrd 装载，两轴缺一即带病；spi_device 批N3 件六随 SPI 词形进表）。
fn assert_profile_pins_schema3(v: &toml::Value, root: &std::path::Path, expect_pin_roles: &[&str]) {
    let roles = rr_registry::rr3_role_parse(
        &std::fs::read_to_string(root.join("contracts/roles.toml"))
            .expect("读 contracts/roles.toml"),
    );
    let pins = v
        .get("pin")
        .and_then(toml::Value::as_array)
        .expect("缺 [[pin]]（schema 3 物理绑定真值点，§4.2）");
    let mut pin_roles: Vec<String> = pins
        .iter()
        .map(|p| {
            p.get("role")
                .and_then(toml::Value::as_str)
                .expect("pin 缺 role")
                .to_string()
        })
        .collect();
    pin_roles.sort();
    assert_eq!(
        pin_roles.iter().map(String::as_str).collect::<Vec<_>>(),
        expect_pin_roles,
        "[[pin]] 角色集漂移"
    );
    for p in pins {
        let role = p.get("role").and_then(toml::Value::as_str).unwrap();
        assert!(
            roles.rr3_role_contains(role),
            "[[pin]] 角色 {role} 未在 roles.toml 在册（校验①-a）"
        );
        if let (Some(pd), Some(rd)) = (
            p.get("direction").and_then(toml::Value::as_str),
            roles.rr3_role_direction(role),
        ) {
            assert_eq!(pd, rd, "pin {role} direction 与角色不一致（校验①）");
        }
        let per = roles.rr3_role_per_inst(role).unwrap_or(false);
        let has_inst = p
            .get("instance")
            .and_then(toml::Value::as_str)
            .is_some_and(|s| !s.is_empty());
        assert_eq!(
            has_inst, per,
            "pin {role} instance 有无与 per_instance = {per} 不符（校验①-b）"
        );
    }
    if let Some(hardware) = v.get("hardware").and_then(toml::Value::as_table) {
        for k in [
            "pwm_chip",
            "pwm_channel",
            "gpio_chip",
            "gpio_led_line",
            "gpio_relay_line",
            "i2c_bus",
            "ina219_addr",
            "spi_device",
        ] {
            assert!(
                !hardware.contains_key(k),
                "[hardware] 出现物理键 {k}（门 n：schema 3 起唯一来源 = [pins] 派生）"
            );
        }
    }
    // 域 pin（校验⑥ CI 侧静态面）：[[supplies.domain]] 域名非空不重复；
    // [pins].rail ∈ 域名集 ∪ {"none"}（⑥-a 在册 + ⑥-b 功率面挂真轨的名单面）
    let mut domains: Vec<String> = Vec::new();
    for d in v
        .get("supplies")
        .and_then(|s| s.get("domain"))
        .and_then(toml::Value::as_array)
        .expect("缺 [[supplies.domain]]（第七供给位，schema 3 起必填）")
    {
        let name = d
            .get("name")
            .and_then(toml::Value::as_str)
            .expect("域条目缺 name");
        assert!(!name.is_empty(), "域名不得为空");
        assert!(!domains.contains(&name.to_string()), "域名 {name} 重复声明");
        domains.push(name.to_string());
    }
    for p in pins {
        if let Some(rail) = p.get("rail").and_then(toml::Value::as_str) {
            if rail != "none" {
                assert!(
                    domains.contains(&rail.to_string()),
                    "pin {} rail = {rail} 未在 [[supplies.domain]] 在册（校验⑥-a）",
                    p.get("role").and_then(toml::Value::as_str).unwrap_or("?")
                );
            }
        }
    }
}
