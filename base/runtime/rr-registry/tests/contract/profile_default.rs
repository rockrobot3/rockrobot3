//! 默认/发行版档：器件词表形状、default 与 distro 两档的槽位绑定面。

use super::*;

/// 器件词表形状 pin（增补轮六批N1 §4.1b 的 CI 全集层，与 roles/slots pin 同判）：
/// 装载形状门（bus 取值域封闭、i2c ⇒ addrs 非空、baud_ok 非空、peak_ma > 0）+
/// 型号集与 mate 集封闭——加器件 = append 不红，改型号/删型号 = 契约级变更当场红
/// （wire.mate 对账⑦-d、id_reg 探测 L-14、peak_ma 互校⑨ 的判据源都在这一册）。
#[test]
fn devices_table_shape_and_mates_are_pinned() {
    let root = repo_root();
    let devices = rr_registry::rr3_dev_parse(
        &std::fs::read_to_string(root.join("contracts/devices.toml"))
            .expect("读 contracts/devices.toml"),
    );
    assert!(
        !devices.rr3_dev_is_empty(),
        "器件词表解析后为空（契约事故）"
    );
    let errs = devices.rr3_dev_check();
    assert!(errs.is_empty(), "devices.toml 形状越界: {errs:?}");
    assert_eq!(
        devices.rr3_dev_names(),
        vec![
            "ICM-42688".to_string(),
            "INA219".to_string(),
            "MAX7219".to_string(),
            "SG90".to_string(),
            "SHTC3".to_string(),
            "SSD1306".to_string(),
        ],
        "器件型号集漂移（加器件 = append 同批改本 pin；改/删型号 = 契约级变更；批N3 件八起 SHTC3/ICM-42688 演练行）"
    );
    let mates = devices.rr3_dev_mates();
    for mate in ["0.96in-OLED-FPC", "MAX7219-DIP24", "SG90-3P"] {
        assert!(
            mates.contains(mate),
            "mate {mate} 不在器件词表 mate 集（wire.mate 对账⑦-d 的判据面漂移）"
        );
    }
}

/// 门禁②（profile 侧）+ 裁定 3 的装配自洽：default 装配单的 bindings 逐格对表查册、
/// 绑到真实模块、与该模块的 manifest provides 对账；enabled_slots 与 provides 全集相等；
/// 全部非 base 格位恰好被绑定一次（default 机不留缺格）。
#[test]
fn default_profile_binds_module_slots_to_real_modules() {
    let root = repo_root();
    let table = rr_registry::rr3_slot_parse(&slots_toml());
    let profiles = root.join("profiles");
    let mut profile_files: Vec<String> = std::fs::read_dir(&profiles)
        .expect("profiles/ 目录必须存在（ADR-0012 裁定 3 产物）")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_str().unwrap().to_string())
        .filter(|n| n.ends_with(".toml"))
        .collect();
    profile_files.sort();
    assert_eq!(
        profile_files,
        vec![
            "default.toml".to_string(),
            "distro.toml".to_string(),
            "esp32-mini.toml".to_string(),
            "esp32s3-mini.toml".to_string(),
            "pi4b-board.toml".to_string(),
            "pi4b-host-esp32-classic.toml".to_string(),
            "pi4b-host-esp32s3.toml".to_string(),
            "pi4b-host.toml".to_string(),
        ],
        "profiles/ 机型集合漂移（新增机型须同批改本 pin 与该机型的形状测试，C2 批K）"
    );

    let text = std::fs::read_to_string(profiles.join("default.toml")).unwrap();
    let v: toml::Value = toml::from_str(&text).expect("default.toml 必须可解析");

    // [os] route 双轨取值（ADR-0013）
    let route = v
        .get("os")
        .and_then(|o| o.get("route"))
        .and_then(toml::Value::as_str)
        .expect("profile 缺 [os] route");
    assert!(
        route == "distro" || route == "buildroot",
        "route 越界: {route}"
    );

    // schema_version=5（0.0.5 增补轮六批N5 [runtime] 进场，F-30 环容量落数据）：
    // [platform]/[supplies]/[slots].present 逐键 pin——两份机型与 rrd 解析器同口径演进，
    // 漂移即红。
    assert_eq!(
        v.get("schema_version").and_then(toml::Value::as_integer),
        Some(5),
        "profile schema_version 漂移（[runtime] 随 5 落地）"
    );
    // [runtime].log_ring_cap（批N5 件二 F-30"装载期缺即红"）：三份现役装配单必须
    // 全部带正整数容量键——rrd parse 层同判据，这里 pin 的是"数据在册"这半边。
    let runtime = v
        .get("runtime")
        .and_then(toml::Value::as_table)
        .expect("default 缺 [runtime]（rrd 运行参数段，批N5 件二）");
    assert!(
        runtime
            .get("log_ring_cap")
            .and_then(toml::Value::as_integer)
            .is_some_and(|n| n > 0),
        "default 缺 [runtime].log_ring_cap 或非正整数（日志环容量，行数）"
    );
    let tier = v
        .get("platform")
        .and_then(|p| p.get("tier"))
        .and_then(toml::Value::as_str)
        .expect("profile 缺 [platform] tier");
    assert!(
        ["host", "rtos", "bare"].contains(&tier),
        "tier 越界: {tier}"
    );
    let supplies = supplies_keys(&v);
    // [supplies.budget]（schema 4，批N2 校验⑩ 分母）：host 档合法不适用显式写 true
    //（F-6 不许留空），数字预算键不得并存（与 rrd parse 层分档自洽同口径）。
    pin_budget(supplies, "host");
    assert_eq!(
        supplies
            .get("watchdog_present")
            .and_then(toml::Value::as_bool),
        Some(true),
        "host 档看门狗在场声明漂移"
    );
    let present: Vec<String> = v
        .get("slots")
        .and_then(|s| s.get("present"))
        .and_then(toml::Value::as_array)
        .expect("缺 [slots].present")
        .iter()
        .filter_map(toml::Value::as_str)
        .map(String::from)
        .collect();
    let mut expected_present = normal_slots(&table);
    expected_present.sort();
    let mut present_sorted = present.clone();
    present_sorted.sort();
    assert_eq!(
        present_sorted, expected_present,
        "[slots].present 与非 base 且非 placeholder/retired 格位全集漂移（参考机 normal 格全在位）"
    );
    assert_profile_pins_schema3(
        &v,
        &root,
        &[
            "act.axis.pwm",
            "act.led.gpio",
            "act.relay.gpio",
            "power.battery.meter",
            "power.charger.meter",
            "ui.face.spi",
        ],
    );

    let manifests: std::collections::BTreeMap<String, toml::Value> =
        module_manifest_values().into_iter().collect();

    // [bindings]：格位 → 件
    let bindings = v
        .get("bindings")
        .and_then(toml::Value::as_table)
        .expect("缺 [bindings]");
    let mut bound: Vec<String> = bindings_walk(bindings, &table, &manifests)
        .into_iter()
        .map(|(s, _)| s.to_string())
        .collect();

    // 全部 normal 格位恰好绑定一次（placeholder/retired 格不进生产装配面；配合 BTreeMap 键唯一即"不多不少"）
    bound.sort();
    let mut unbound: Vec<String> = normal_slots(&table)
        .into_iter()
        .filter(|n| !bound.contains(n))
        .collect();
    assert!(
        unbound.is_empty(),
        "default 机存在缺格（fail-closed，缺格即 Degraded）: {unbound:?}"
    );
    unbound.clear();

    // [enabled_slots]：模块 → 格位集合 == manifest provides 全集
    enabled_slots_check(&v, &manifests, &table);
}

/// 门禁②（profile 侧）+ 0.0.2 T5 的 distro 装配单对账：快路机型的绑定面**恰好是
/// 身体层**（非 base 未绑格位 == cognition.\* 四格，即 rr3-brain 的 provides 全集），
/// enabled_slots 仍覆盖全部在册模块且与 provides 全集相等——"enabled_slots 保留声明、格位不绑定"
/// 正是快路 payload 缺 Python brain 的诚实表达（P2：缺格点名降级，不拒 boot；
/// CI distro-smoke 的断言锚点与这份装配单同源）。
#[test]
fn distro_profile_binds_exactly_the_body_layer() {
    let root = repo_root();
    let table = rr_registry::rr3_slot_parse(&slots_toml());
    let manifests: std::collections::BTreeMap<String, toml::Value> =
        module_manifest_values().into_iter().collect();
    let text = std::fs::read_to_string(root.join("profiles/distro.toml")).unwrap();
    let v: toml::Value = toml::from_str(&text).expect("distro.toml 必须可解析");

    let route = v
        .get("os")
        .and_then(|o| o.get("route"))
        .and_then(toml::Value::as_str)
        .expect("profile 缺 [os] route");
    assert_eq!(route, "distro", "distro 机 route 必须是快路");

    // schema_version=5 三新段 + [pins] + [supplies.budget] + [runtime]（批N5 件二）：
    // 与 default 同一块参考机，[platform]/[supplies]/[slots] 逐值一致（present 为
    // normal 格全集——placeholder/retired 格不进生产装配面）。
    assert_eq!(
        v.get("schema_version").and_then(toml::Value::as_integer),
        Some(5),
        "profile schema_version 漂移"
    );
    let runtime = v
        .get("runtime")
        .and_then(toml::Value::as_table)
        .expect("distro 缺 [runtime]（rrd 运行参数段，批N5 件二）");
    assert!(
        runtime
            .get("log_ring_cap")
            .and_then(toml::Value::as_integer)
            .is_some_and(|n| n > 0),
        "distro 缺 [runtime].log_ring_cap 或非正整数（日志环容量，行数）"
    );
    assert_tier(&v, "host", "distro 机档位漂移");
    let supplies = supplies_keys(&v);
    // [supplies.budget]（schema 4，批N2 校验⑩ 分母）：与 default 同板同值——host 档
    // 合法不适用显式写 true，数字预算键不得并存。
    pin_budget(supplies, "distro 机 host");
    let present = present_of(&v);
    let mut expected_present = normal_slots(&table);
    expected_present.sort();
    let mut present_sorted = present;
    present_sorted.sort();
    assert_eq!(
        present_sorted, expected_present,
        "[slots].present 与非 base 且非 placeholder/retired 格位全集漂移"
    );
    assert_profile_pins_schema3(
        &v,
        &root,
        &[
            "act.axis.pwm",
            "act.led.gpio",
            "act.relay.gpio",
            "power.battery.meter",
            "power.charger.meter",
            "ui.face.spi",
        ],
    );

    let bindings = v
        .get("bindings")
        .and_then(toml::Value::as_table)
        .expect("缺 [bindings]");
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
    }

    // 未绑定的 normal 格位必须为空（placeholder/retired 格不进生产装配面；
    // 0.0.6 批F1：brain 随 deb 装机、cognition 四格绑定兑现——快路满配 = 身体层 +
    // 认知层 normal 格一格不缺。将来新增格位而 distro 不绑时，此钉强制显式重审）
    let unbound: Vec<String> = normal_slots(&table)
        .into_iter()
        .filter(|n| !bindings.contains_key(n))
        .collect();
    assert!(
        unbound.is_empty(),
        "distro 机快路满配后仍有未绑 normal 格位 {unbound:?}（装配单漂移，须显式绑定或重审边界）"
    );

    // enabled_slots 仍覆盖全部在册模块（含 brain——enabled_slots 决定起谁，brain 现在实际进 boot 队列），且与 provides 全集相等
    enabled_slots_check(&v, &manifests, &table);
}
