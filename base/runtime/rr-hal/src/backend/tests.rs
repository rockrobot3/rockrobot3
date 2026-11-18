//! backend 选择面与环境变量裁决的单元测试。

use super::selector::{rr3_backend_parse, rr3_desc_fail_note};
use super::*;

/// 解析函数的平台默认分支：未设环境变量时回落平台默认（本机 macOS = sim）。
/// 环境变量支无法在本测试里安全置位（并发测试读同一进程环境，std::env::set_var
/// 是 unsafe 且有竞态），故三态自洽：未设 → 钉平台缺省；显式合法值 → 白名单；
/// 显式非法值 → Err 点名（fail-closed）。显式取值路径另由模块侧 e2e 以真实
/// 进程环境覆盖（如 `--audio-backend mac` 起的 rr3-sim 全环）。
#[test]
fn default_backend_is_sim_off_linux_alsa() {
    let env_set = std::env::var_os(RR3_HAL_BACKEND_ENV).is_some();
    match rr3_backend_resolve() {
        Ok(resolved) => {
            assert!(
                [
                    RR3_BACKEND_SIM,
                    RR3_BACKEND_LINUX,
                    RR3_BACKEND_MAC,
                    RR3_BACKEND_PLACEHOLDER,
                ]
                .contains(&resolved),
                "解析结果必须是合法后端名: {resolved}"
            );
            if !env_set {
                if cfg!(all(target_os = "linux", feature = "alsa")) {
                    assert_eq!(resolved, RR3_BACKEND_LINUX);
                } else {
                    assert_eq!(resolved, RR3_BACKEND_SIM);
                }
            }
        }
        Err(e) => {
            assert!(env_set, "未设环境变量必须解析成功: {e}");
        }
    }
}

/// G5 收口（R3-D3）：描述符失败点名文案含面名与原因原文——"错误当场可见"
/// 的形状钉死（文案改动此处即红，防止退回无声吞错）。
#[test]
fn descriptor_failure_note_names_facet_and_reason() {
    let note = rr3_desc_fail_note(
        "音频采集",
        "描述符解析失败: 描述符键 gpio_led_line 取值不认: \"four\"",
    );
    assert!(note.contains("rr-hal:"), "{note}");
    assert!(note.contains("音频采集"), "{note}");
    assert!(note.contains("gpio_led_line"), "{note}");
    assert!(note.contains("Unsupported"), "{note}");
}

/// 执行面（舵机/数字输出）选择：平台默认下拿到的后端名必须合法，且对象可用
/// （sim 上探活读回成功；占位件留给使用时的 Unsupported——选择函数本身不触设备）。
#[test]
fn act_device_selectors_return_legal_backend_objects() {
    // linux 平台默认档：无 alsa feature 构建落 sim，带 alsa 构建落 linux
    // （舵机面在 linux 平台也可得 sysfs PWM 后端——构造不触设备，此处不探活）
    let mut legal = vec![RR3_BACKEND_SIM, RR3_BACKEND_PLACEHOLDER];
    if cfg!(target_os = "linux") {
        legal.push(RR3_BACKEND_LINUX);
    }
    if cfg!(target_os = "macos") {
        // mac 宿主档显示面 = sim 同款虚拟点阵（宿主无 LED 矩阵是平台事实）
        legal.push(RR3_BACKEND_MAC);
    }
    let (servo, servo_backend) = rr3_hal_servo();
    assert!(
        legal.contains(&servo_backend),
        "舵机后端名合法: {servo_backend}"
    );
    let (gpio, gpio_backend) = rr3_hal_gpio_out("led");
    assert!(
        legal.contains(&gpio_backend),
        "数字输出后端名合法: {gpio_backend}"
    );
    let (display, display_backend) = rr3_hal_display();
    assert!(
        legal.contains(&display_backend),
        "显示后端名合法: {display_backend}"
    );
    let (link, link_backend) = rr3_hal_link(std::time::Duration::from_secs(5));
    assert!(
        legal.contains(&link_backend),
        "链路后端名合法: {link_backend}"
    );
    // sim 档（本机 macOS 默认）下探活必须成功——执行件从断电态起步
    // （探活全是读面 &self，无需可变绑定；写面 &mut self 的使用在模块侧）
    if servo_backend == RR3_BACKEND_SIM {
        assert_eq!(servo.angle().unwrap(), 90, "sim 舵机初始中位");
        assert!(!gpio.level().unwrap(), "sim 通道初始断电");
    }
    if display_backend == RR3_BACKEND_SIM || display_backend == RR3_BACKEND_MAC {
        assert_eq!(display.frame().unwrap(), [0; 8], "宿主虚拟点阵初始黑屏");
    }
    if link_backend == RR3_BACKEND_SIM {
        let st = link.state().unwrap();
        if st.up {
            assert!(
                st.ssid.is_some() && st.rssi_dbm.is_some(),
                "sim 在链必带 SSID/RSSI"
            );
        } else {
            assert!(
                st.ssid.is_none() && st.rssi_dbm.is_none(),
                "sim 断链必无 SSID/RSSI"
            );
        }
    }
}

/// 音频面选择（A 批 c3）：源/播两选择函数拿到的后端名必须合法；**播面
/// 永不落 esp32**（板无扬声器，fail-closed 显式档）。构造不触设备
/// （esp32 源 open 才真开 UART），环境变量缺席时本机缺省档合法即可。
#[test]
fn audio_selectors_return_legal_backend_objects() {
    let (src, src_backend) = rr3_audio_source();
    assert!(
        [
            RR3_BACKEND_SIM,
            RR3_BACKEND_LINUX,
            RR3_BACKEND_MAC,
            RR3_BACKEND_ESP32,
            RR3_BACKEND_PLACEHOLDER,
        ]
        .contains(&src_backend),
        "音频源后端名合法: {src_backend}"
    );
    let (sink, sink_backend) = rr3_hal_audio_sink();
    assert!(
        [
            RR3_BACKEND_SIM,
            RR3_BACKEND_LINUX,
            RR3_BACKEND_MAC,
            RR3_BACKEND_PLACEHOLDER,
        ]
        .contains(&sink_backend),
        "音频播后端名合法: {sink_backend}"
    );
    assert_ne!(
        sink_backend, RR3_BACKEND_ESP32,
        "板无扬声器，播面永不落 esp32"
    );
    let _ = (src, sink); // 对象可用性归使用面（探活），选择面不触设备
}

/// GPIO 角色 → 应急 env 键拼装（R8 预算表 GPIO4/17 真值已迁 profiles
/// [hardware]，批D④ 后无代码缺省）：键拼装是通用机制（任意字母数字 role
/// 都拼得出键），**绑定白名单在选择面**（rr3_hal_gpio_out 只认 led/relay，
/// 白名单外 fail-closed 显式回错）；空串/怪字符不拼键。
#[test]
fn gpio_role_env_key_matches_whitelist() {
    assert_eq!(
        rr3_gpio_line_key("led").as_deref(),
        Some("RR3_HAL_GPIO_LED_LINE")
    );
    assert_eq!(
        rr3_gpio_line_key("relay").as_deref(),
        Some("RR3_HAL_GPIO_RELAY_LINE")
    );
    assert_eq!(rr3_gpio_line_key(""), None, "空串不拼键");
    assert_eq!(rr3_gpio_line_key("led-1"), None, "怪字符不拼键");
    assert_eq!(
        rr3_gpio_line_key("fan").as_deref(),
        Some("RR3_HAL_GPIO_FAN_LINE"),
        "键拼装通用；fan 不在选择面白名单，无绑定 fail-closed"
    );
}

/// linux PWM 芯片/通道号键的取值裁决（应急覆盖与描述符共用）：十进制非负
/// 整数才收，负数/垃圾/空不认（真值 pwmchip0/pwm0 = GPIO12 在 profiles
/// [hardware]，批D④ 后代码不再持有缺省）。
#[test]
fn pwm_index_env_values_parse_or_default() {
    assert_eq!(rr3_pwm_index_parse("0"), Some(0));
    assert_eq!(rr3_pwm_index_parse("1"), Some(1));
    assert_eq!(rr3_pwm_index_parse(" 2 "), Some(2));
    for bad in ["", "-1", "3.5", "abc"] {
        assert_eq!(rr3_pwm_index_parse(bad), None, "{bad}");
    }
}

/// 后端白名单裁决（纯函数）：允许集全收、平台门落占位（linux 于非 linux、
/// mac 于非 macOS）、垃圾点名**所查 env 键**——全局键与音频能力面覆盖键
/// 共用本裁决，防两键各写一套白名单静默分叉。
#[test]
fn backend_parse_whitelist_platform_doors_and_key_naming() {
    assert_eq!(rr3_backend_parse("K", "sim").unwrap(), RR3_BACKEND_SIM);
    assert_eq!(rr3_backend_parse("K", "esp32").unwrap(), RR3_BACKEND_ESP32);
    assert_eq!(
        rr3_backend_parse("K", "placeholder").unwrap(),
        RR3_BACKEND_PLACEHOLDER
    );
    let linux = rr3_backend_parse("K", "linux").unwrap();
    if cfg!(target_os = "linux") {
        assert_eq!(linux, RR3_BACKEND_LINUX);
    } else {
        assert_eq!(
            linux, RR3_BACKEND_PLACEHOLDER,
            "非 linux 平台显式 linux 落占位（使用时 Unsupported）"
        );
    }
    let mac = rr3_backend_parse("K", "mac").unwrap();
    if cfg!(target_os = "macos") {
        assert_eq!(mac, RR3_BACKEND_MAC);
    } else {
        assert_eq!(
            mac, RR3_BACKEND_PLACEHOLDER,
            "非 macOS 平台显式 mac 落占位（使用时 Unsupported）"
        );
    }
    let e = rr3_backend_parse(RR3_HAL_AUDIO_BACKEND_ENV, "bogus").unwrap_err();
    assert!(e.contains(RR3_HAL_AUDIO_BACKEND_ENV), "错误点名所查键: {e}");
    assert!(e.contains("bogus"), "错误含入参: {e}");
}

/// INA219 从地址键的取值裁决：只认 `0x` 前缀十六进制（裸写有十进制歧义
/// 不收），7-bit 可寻址域 0x08..=0x77 外（含 0x00 广播、0x78+ 保留段）与
/// 垃圾不认。
#[test]
fn ina219_addr_parse_accepts_hex_within_7bit() {
    assert_eq!(rr3_ina219_parse("0x40"), Some(0x40), "BOM #5 定案档");
    assert_eq!(rr3_ina219_parse("0X48"), Some(0x48), "大写前缀");
    assert_eq!(rr3_ina219_parse(" 0x48 "), Some(0x48), "trim");
    for bad in ["0x00", "0x07", "0x78", "0xFF", "16", "40", "", "zz"] {
        assert_eq!(rr3_ina219_parse(bad), None, "{bad}");
    }
}

/// sim 电池两个 env 键的取值裁决：合法档、越界/垃圾全点名。e2e 从 rrd 继承
/// 环境注入低电量的正门靠这两个裁决，取值不认一律落缺省（80% / 未充电）。
#[test]
fn sim_battery_env_values_parse_or_default() {
    assert_eq!(rr3_sim_batt_parse("0"), Some(0));
    assert_eq!(rr3_sim_batt_parse("100"), Some(100));
    assert_eq!(rr3_sim_batt_parse(" 5 "), Some(5));
    assert_eq!(rr3_sim_batt_parse("101"), None, "越界不认");
    assert_eq!(rr3_sim_batt_parse("-1"), None, "u8 负数不认");
    assert_eq!(rr3_sim_batt_parse("abc"), None, "垃圾不认");
    assert_eq!(rr3_sim_chg_parse("1"), Some(true));
    assert_eq!(rr3_sim_chg_parse("0"), Some(false));
    for bad in ["", "true", "2", "01x"] {
        assert_eq!(rr3_sim_chg_parse(bad), None, "{bad}");
    }
}

/// sim 链路两个 env 键的取值裁决：在线态只认 1/0，rssi 只认 -100..=-1；
/// 0 与正值、越界、垃圾全点名。e2e 注入断链场景（RR3_HAL_SIM_LINK_UP=0）
/// 的正门靠这两个裁决，取值不认一律落缺省（在线 / -50）。
#[test]
fn sim_link_env_values_parse_or_default() {
    assert_eq!(rr3_sim_up_parse("1"), Some(true));
    assert_eq!(rr3_sim_up_parse(" 0 "), Some(false));
    for bad in ["", "true", "2", "on"] {
        assert_eq!(rr3_sim_up_parse(bad), None, "{bad}");
    }
    assert_eq!(rr3_sim_rssi_parse("-50"), Some(-50));
    assert_eq!(rr3_sim_rssi_parse("-100"), Some(-100), "下界在域内");
    assert_eq!(rr3_sim_rssi_parse("-1"), Some(-1), "上界在域内");
    assert_eq!(rr3_sim_rssi_parse("0"), None, "0 dBm 不是合法读数");
    assert_eq!(rr3_sim_rssi_parse("-101"), None, "越下界不认");
    assert_eq!(rr3_sim_rssi_parse("abc"), None, "垃圾不认");
}

/// sim 方位角 env 键的取值裁决：±90 闭区间内的有限小数才收，越界/垃圾不认
/// （落"不注入"）。批F2 判向 e2e（RR3_HAL_SIM_DOA_DEG=-60）的正门靠它。
#[test]
fn sim_doa_deg_env_values_parse_or_default() {
    assert_eq!(rr3_sim_doa_parse("-60"), Some(-60.0));
    assert_eq!(rr3_sim_doa_parse(" 45.5 "), Some(45.5));
    assert_eq!(rr3_sim_doa_parse("-90"), Some(-90.0), "下界在域内");
    assert_eq!(rr3_sim_doa_parse("90"), Some(90.0), "上界在域内");
    assert_eq!(rr3_sim_doa_parse("0"), Some(0.0));
    assert_eq!(rr3_sim_doa_parse("90.1"), None, "越上界不认");
    assert_eq!(rr3_sim_doa_parse("-90.5"), None, "越下界不认");
    assert_eq!(rr3_sim_doa_parse("abc"), None, "垃圾不认");
    assert_eq!(rr3_sim_doa_parse(""), None, "空串不认");
    assert_eq!(rr3_sim_doa_parse("nan"), None, "NaN 不认");
    assert_eq!(rr3_sim_doa_parse("inf"), None, "inf 不认");
}
