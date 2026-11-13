//! ESP32 档：esp32-mini 桥接微型形态与 esp32s3-mini 线束差异面。

use super::*;

/// F-B 桥接微型形态（0.0.5 批K R3-D6 / H4+C2）：`profiles/esp32-mini.toml` 第二形态
/// 装配自洽 pin。证命题："裁剪 = 改数据"——件码、对外主题、slots.toml 零改动，本文件只加数据。
/// 三格微型形态：恰好 audio.capture/audio.vad → rr3-sense、act.servo → rr3-act；
/// 不启用 net.*（桥即链路）与 cognition.*（认知在 host，理由 = 模块设计索引 §10 F-B 行）；
/// tier = rtos 且全部引用格的 tier_scope 含 rtos（L-10 的 CI 全集层同判，rrd 装载门为运行期侧）；
/// [slots].present 升序且恰为三格；[hardware] 缺省（九键是宿主侧语义，板侧描述符归
/// 0.0.7 批G/批D1，进位同批改本断言）；看门狗在板且由板侧运行环喂（桥断仍守护）。
#[test]
fn esp32_mini_profile_binds_bridged_minimal_shape() {
    let root = repo_root();
    let table = rr_registry::rr3_slot_parse(&slots_toml());
    let manifests: std::collections::BTreeMap<String, toml::Value> =
        module_manifest_values().into_iter().collect();
    let text = std::fs::read_to_string(root.join("profiles/esp32-mini.toml")).unwrap();
    let v: toml::Value = toml::from_str(&text).expect("esp32-mini.toml 必须可解析");

    assert_eq!(
        v.get("schema_version").and_then(toml::Value::as_integer),
        Some(5),
        "esp32-mini schema_version 漂移（[runtime] 随 5 落地）"
    );
    let runtime = v
        .get("runtime")
        .and_then(toml::Value::as_table)
        .expect("esp32-mini 缺 [runtime]（rrd 运行参数段，批N5 件二）");
    assert!(
        runtime
            .get("log_ring_cap")
            .and_then(toml::Value::as_integer)
            .is_some_and(|n| n > 0),
        "esp32-mini 缺 [runtime].log_ring_cap 或非正整数（日志环容量，行数）"
    );
    assert_tier(&v, "rtos", "esp32-mini 档位漂移（F-B = rtos 经桥档）");
    // 板侧必须自己守住（模块设计索引 §10 F-B 行）：看门狗在板、由板侧运行环喂——
    // "经桥不等于把安全托付给网络"的硬判据落在装配单数据上
    let supplies = v
        .get("supplies")
        .and_then(toml::Value::as_table)
        .expect("esp32-mini 缺 [supplies]");
    assert_eq!(
        supplies
            .get("watchdog_present")
            .and_then(toml::Value::as_bool),
        Some(true),
        "F-B 看门狗必须在板（桥断仍守护）"
    );
    assert_eq!(
        supplies
            .get("watchdog_feeder")
            .and_then(toml::Value::as_str),
        Some("rr-core"),
        "F-B 看门狗由板侧运行环（rr-core::runtime）喂，不是 rrd"
    );
    // [supplies.budget]（schema 4，批N2 校验⑩ 分母）：rtos 档必填三数键，取值为
    // **声明预算位**——与 profiles/esp32-mini.toml 注释同源（board.rs 任务栈 +
    // S3 内部 SRAM 堆上限推算 + UART2_TX_RING 常驻缓冲 + audio_q 峰值 + 渲染 +
    // 板侧配置服务预留，批N4 件二 N-13 全账 16384→98816），对账归 0.0.7 真板，
    // 漂移即红（契约 append 同批更新全部形状 pin）。
    let budget = supplies
        .get("budget")
        .and_then(toml::Value::as_table)
        .expect("esp32-mini 缺 [supplies.budget]（校验⑩ 分母，批N2）");
    assert_eq!(
        budget.get("not_applicable").and_then(toml::Value::as_bool),
        Some(false),
        "rtos 档 budget 必须走数字预算键（合法不适用为假）"
    );
    for (k, expected) in [
        ("task_stack_budget_bytes", 65536),
        ("heap_total_bytes", 327680),
        ("heap_reserved_bytes", 98816),
    ] {
        assert_eq!(
            budget.get(k).and_then(toml::Value::as_integer),
            Some(expected),
            "esp32-mini budget {k} 漂移（声明预算位，对账归 0.0.7）"
        );
    }
    // [hardware] 板侧配置十一键（批N4 件七(b) 基-24 桥上配置腿三键 + 批N9 件五
    // MC-7 缺③进位补齐七键 + 批N17 件三 cred_store 键半边）：板侧裸寄存器/外设
    // 路径的引脚真值与非 GPIO 存储源标记——非 [pins] 派生面（无 role 对应，门 n
    // 物理键集不收），rtos 档 [hardware] 原样直给进 RR3_HAL_BINDINGS，经桥
    // rr3.set_bindings 注入板侧；注入前板侧消费面 fail-closed 拒（不回退硬编码）；
    // 主驱动七键消费语义 = 装配对账锚（注入到达时板侧逐键判等，不齐 ERROR 点名＋
    // 计数）；cred_store 消费语义 = 板侧读数面可见（GET /api/board/bindings 随键，
    // boot 读链不消费＝批N14 架构事实，非引脚不进对账锚）。取值与板侧 main.rs
    // 引脚真值段同源（SDA=21/SCL=22 = OLED I2C 总线；34 = 电池分压 ADC；UART2
    // TX=18/RX=19；LEDC pan=32/tilt=33 与 [[pin]] physical 同源同值；I2S
    // BCLK=4/SD=27/WS=26；cred_store = nvs:rr3 与板侧 cred namespace 单真值同源），
    // 漂移即红。主驱动具名引脚面（esp-idf-hal 结构约束）的装载仍归 0.0.7 批G/批D1。
    let hardware = v
        .get("hardware")
        .and_then(toml::Value::as_table)
        .expect("esp32-mini 缺 [hardware] 段（板侧配置十一键真值点，批N4 件七(b) + 批N9 件五 + 批N17 件三）");
    for (k, expected) in [
        ("i2c_sda", "21"),
        ("i2c_scl", "22"),
        ("adc_rail_pin", "34"),
        ("uart2_tx", "18"),
        ("uart2_rx", "19"),
        ("ledc_pan", "32"),
        ("ledc_tilt", "33"),
        ("i2s_bclk", "4"),
        ("i2s_sd", "27"),
        ("i2s_ws", "26"),
        ("cred_store", "nvs:rr3"),
    ] {
        assert_eq!(
            hardware.get(k).and_then(toml::Value::as_str),
            Some(expected),
            "esp32-mini hardware.{k} 漂移（板侧引脚真值，与板侧 main.rs 同源）"
        );
    }
    // schema 3 [pins]（增补轮六批N1）：板侧执行件的物理绑定真值点——rtos 档 rrd
    // 不派生（controller 词形 ledc<N> 是板侧词表），但校验①的形状照判。批N3 件五
    // 双轴收敛：pan/tilt 两枚同角色 pin（act.axis.pwm × 2，instance 消歧）——
    // 机型集 pin 变化同批改本 pin（文件头 C2 判据）。
    assert_profile_pins_schema3(&v, &root, &["act.axis.pwm", "act.axis.pwm"]);

    // [bindings] 恰好三格——微型形态的"裁剪"是显式集合，多绑少绑都算漂移
    let bindings = v
        .get("bindings")
        .and_then(toml::Value::as_table)
        .expect("缺 [bindings]");
    let bound: std::collections::BTreeMap<&str, &str> = bindings_walk(bindings, &table, &manifests)
        .into_iter()
        .collect();
    assert_eq!(
        bound,
        std::collections::BTreeMap::from([
            ("act.servo", "rr3-act"),
            ("audio.capture", "rr3-sense"),
            ("audio.vad", "rr3-sense"),
        ]),
        "F-B 三格绑定面漂移（裁剪是显式集合，模块设计索引 §10）"
    );

    // [enabled_slots]：恰两模块、格位是 provides 的子集（裁剪只能收窄不能自造）、
    // 不启用 net.*/cognition.*
    let enabled_slots = v
        .get("enabled_slots")
        .and_then(toml::Value::as_table)
        .expect("缺 [enabled_slots]");
    assert_eq!(
        enabled_slots.keys().map(|k| k.as_str()).collect::<Vec<_>>(),
        vec!["rr3-act", "rr3-sense"],
        "F-B 启用模块面漂移（不启用 connect/interact/power/brain）"
    );
    for (module, face) in enabled_slots {
        let slots: Vec<String> = face
            .as_array()
            .expect("enabled_slots 值必须是数组")
            .iter()
            .filter_map(toml::Value::as_str)
            .map(String::from)
            .collect();
        assert!(!slots.is_empty(), "{module} enabled_slots 不应为空");
        let caps = str_list(&manifests[module.as_str()], "provides.capabilities");
        for s in &slots {
            assert!(
                caps.iter().any(|c| c == s),
                "{module} enabled_slots 格位 {s} 不在其 manifest provides（裁剪只能收窄，不能自造）"
            );
            assert!(
                table.rr3_slot_contains(s),
                "[enabled_slots] 引用表外格位 {s}"
            );
            assert!(
                !s.starts_with("net.") && !s.starts_with("cognition."),
                "F-B 不启用 {s}（net.* 桥即链路、cognition.* 认知在 host——模块设计索引 §10）"
            );
        }
    }
    // 每条 binding 都被 enabled_slots 认领（装配单结构自洽，与 rrd 装载门同判）
    for (slot, module) in &bound {
        let claimed = enabled_slots
            .get(*module)
            .and_then(toml::Value::as_array)
            .is_some_and(|a| a.iter().filter_map(toml::Value::as_str).any(|s| s == *slot));
        assert!(claimed, "binding {slot}={module} 未被 enabled_slots 认领");
    }

    // [slots].present：恰为三格、升序（确定性）、全部在册非 base
    let present = present_of(&v);
    assert_eq!(
        present,
        vec![
            "act.servo".to_string(),
            "audio.capture".to_string(),
            "audio.vad".to_string(),
        ],
        "F-B 在位声明漂移（恰三格、升序）"
    );
    for s in &present {
        assert!(
            table.rr3_slot_contains(s) && table.rr3_slot_layer(s).as_deref() != Some("base"),
            "[slots].present 格位 {s} 必须在册且非 base"
        );
    }

    // L-10 tier↔tier_scope（CI 全集层同判）：F-B 引用的每一格其 tier_scope 必须含 rtos。
    // 三处引用（bindings / enabled_slots / present）合并去重，与 rrd 装载门同口径。
    let mut referenced: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for s in bound.keys() {
        referenced.insert((*s).to_string());
    }
    for face in enabled_slots.values() {
        let slots = face.as_array().expect("enabled_slots 值必须是数组");
        referenced.extend(
            slots
                .iter()
                .filter_map(toml::Value::as_str)
                .map(String::from),
        );
    }
    referenced.extend(present.iter().cloned());
    for slot in &referenced {
        let scope = table.rr3_slot_tier_scope(slot).unwrap_or_default();
        assert!(
            scope.iter().any(|t| t == "rtos"),
            "esp32-mini（rtos 档）引用格 {slot} 的 tier_scope = {scope:?} 不含 rtos（L-10 交叉校验）"
        );
    }
}

/// S3 微型形态（W1/T2）：`profiles/esp32s3-mini.toml` 与 esp32-mini 同形态、异线束。
/// 本测试只钉**差异面**（chip / [hardware] 十一键 / 双轴物理脚 / 绑定面），形态与
/// 预算的完整形状由 esp32-mini 测试和 rrd 装载门共同覆盖；线束漂移即红。
#[test]
fn esp32s3_mini_profile_pins_s3_harness() {
    let root = repo_root();
    let text = std::fs::read_to_string(root.join("profiles/esp32s3-mini.toml"))
        .expect("profiles/esp32s3-mini.toml 必须存在（W1/T2）");
    let v: toml::Value = toml::from_str(&text).expect("esp32s3-mini.toml 必须可解析");

    assert_eq!(
        v.get("schema_version").and_then(toml::Value::as_integer),
        Some(5),
        "esp32s3-mini schema_version 漂移"
    );
    assert_tier(&v, "rtos", "esp32s3-mini 档位漂移（F-B = rtos 经桥档）");
    assert_eq!(
        v.get("platform")
            .and_then(|p| p.get("chip"))
            .and_then(toml::Value::as_str),
        Some("esp32s3"),
        "esp32s3-mini chip 漂移（平台表选 contracts/platform/esp32s3.toml）"
    );

    // [hardware] 十一键：与 board/pins_s3.rs、main.rs 具名装配三方同源。
    let hardware = v
        .get("hardware")
        .and_then(toml::Value::as_table)
        .expect("esp32s3-mini 缺 [hardware] 段（板侧配置十一键真值点）");
    s3_hardware_keys(hardware, "esp32s3-mini", "pins_s3.rs 同源");

    // 形态面：三格绑定 + 三格在位，与 esp32-mini 同源同值。
    let bindings = v
        .get("bindings")
        .and_then(toml::Value::as_table)
        .expect("esp32s3-mini 缺 [bindings]");
    for (slot, provider) in [
        ("act.servo", "rr3-act"),
        ("audio.capture", "rr3-sense"),
        ("audio.vad", "rr3-sense"),
    ] {
        assert_eq!(
            bindings.get(slot).and_then(toml::Value::as_str),
            Some(provider),
            "esp32s3-mini bindings.{slot} 漂移"
        );
    }
    let present = v
        .get("slots")
        .and_then(|s| s.get("present"))
        .and_then(toml::Value::as_array)
        .expect("esp32s3-mini 缺 [slots].present");
    assert_eq!(present.len(), 3, "esp32s3-mini 在位声明应恰三格");

    // 双轴 [[pin]]：role / 通道 / 物理脚（S3 线束 4/5）。
    assert_profile_pins_schema3(&v, &root, &["act.axis.pwm", "act.axis.pwm"]);
    let pins = v
        .get("pin")
        .and_then(toml::Value::as_array)
        .expect("esp32s3-mini 缺 [[pin]]");
    let mut physicals: Vec<i64> = Vec::new();
    for pin in pins {
        let arr = pin
            .get("physical")
            .and_then(toml::Value::as_array)
            .expect("[[pin]].physical 必须是数组");
        physicals.extend(arr.iter().filter_map(toml::Value::as_integer));
    }
    assert_eq!(
        physicals,
        vec![4, 5],
        "esp32s3-mini 双轴物理脚漂移（应为 S3 LEDC pan=4 / tilt=5）"
    );
}
