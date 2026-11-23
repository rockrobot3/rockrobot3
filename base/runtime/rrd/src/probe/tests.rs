// SPDX-License-Identifier: Apache-2.0
//! probe 单测（文件头：本文件只放测试，生产面在 `probe.rs`）。
//!
//! 拆分动因：`probe.rs` 505 行、内联测试自 276 行起占近半，先按仓内同形搬出。

use super::*;

/// 唯一 temp 目录（pid + 纳秒后缀，批H5 教训：只 pid 会撞上一轮遗留目录）
fn rr3_test_probe_root(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("rr3-probe-{}-{nanos}-{tag}", std::process::id()))
}

/// 格位 → 面映射：硬件格位逐条归面，软件格/运行期格/基础层归 None。
#[test]
fn slot_probe_face_maps_hardware_and_software_slots() {
    assert_eq!(rr3_slot_probe_face("act.servo"), Some(Rr3ProbeFace::Pwm));
    assert_eq!(rr3_slot_probe_face("act.led"), Some(Rr3ProbeFace::Gpio));
    assert_eq!(rr3_slot_probe_face("act.relay"), Some(Rr3ProbeFace::Gpio));
    assert_eq!(rr3_slot_probe_face("ui.face"), Some(Rr3ProbeFace::Spi));
    for s in ["power.charger", "sensor.env", "sensor.imu"] {
        assert_eq!(rr3_slot_probe_face(s), Some(Rr3ProbeFace::I2c), "{s}");
    }
    for s in ["audio.capture", "audio.playback", "audio.vad"] {
        assert_eq!(rr3_slot_probe_face(s), Some(Rr3ProbeFace::Alsa), "{s}");
    }
    for s in [
        "cognition.llm",
        "cognition.asr",
        "net.wifi",
        "net.bt",
        "net.online",
        "power.battery",
        "bus.publish",
        "",
    ] {
        assert_eq!(rr3_slot_probe_face(s), None, "无硬件面: {s}");
    }
}

/// 空假根：硬件格位全缺（fail-closed），软件/运行期格位在位（无反证）。
#[test]
fn linux_probe_on_empty_root_reports_hardware_absent() {
    let root = rr3_test_probe_root("empty");
    std::fs::create_dir_all(&root).unwrap();
    let probe = Rr3LinuxProbe::rr3_linux_probe_new(&root);
    for s in [
        "act.servo",
        "act.led",
        "ui.face",
        "power.charger",
        "audio.capture",
    ] {
        assert!(!probe.rr3_probe_slot(s), "空根下硬件格位必须缺: {s}");
    }
    for s in ["cognition.llm", "net.wifi", "net.online", ""] {
        assert!(probe.rr3_probe_slot(s), "无硬件面格位在位: {s}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// 假根摆上五面设备节点 → 对应格位全在位；未摆的面仍缺。这条是"真值接线"
/// 的主判据（探测确实读了设备面，而不是恒真/恒假）。
#[test]
fn linux_probe_reads_device_faces_from_root() {
    let root = rr3_test_probe_root("faces");
    std::fs::create_dir_all(root.join("sys/class/pwm/pwmchip0")).unwrap();
    std::fs::create_dir_all(root.join("sys/class/gpio/gpiochip0")).unwrap();
    std::fs::create_dir_all(root.join("dev")).unwrap();
    std::fs::write(root.join("dev/i2c-1"), b"").unwrap();
    std::fs::create_dir_all(root.join("proc/asound")).unwrap();
    std::fs::write(root.join("proc/asound/cards"), b" 0 [Headphones ]: ...\n").unwrap();
    let probe = Rr3LinuxProbe::rr3_linux_probe_new(&root);

    assert!(probe.rr3_probe_slot("act.servo"), "pwmchip 在位");
    assert!(probe.rr3_probe_slot("act.led"), "gpiochip 在位");
    assert!(probe.rr3_probe_slot("power.charger"), "i2c 设备在位");
    assert!(probe.rr3_probe_slot("audio.capture"), "声卡表非空");
    assert!(
        !probe.rr3_probe_slot("ui.face"),
        "spidev 未摆 → ui.face 缺（fail-closed，不冒充在位）"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 空 ALSA 卡表（文件在但 0 字节）= 无卡注册 → 缺（"拿不到答案不当作在位"）。
#[test]
fn linux_probe_treats_empty_alsa_cards_as_absent() {
    let root = rr3_test_probe_root("alsa-empty");
    std::fs::create_dir_all(root.join("proc/asound")).unwrap();
    std::fs::write(root.join("proc/asound/cards"), b"").unwrap();
    let probe = Rr3LinuxProbe::rr3_linux_probe_new(&root);
    assert!(
        !probe.rr3_probe_slot("audio.playback"),
        "空卡表 = 无卡注册 → 缺"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// L-14 主判据（这条是缺陷的判据身）：总线节点在而 sysfs 从机节点不在 =
/// "总线在、器件不在" → 缺。改造前这一格恒判在位（只看 /dev/i2c-* 存在），
/// 器件没接也绿——缺格降级环在 I2C 三格上不转的真缺陷。
#[test]
fn linux_probe_i2c_requires_device_node_not_just_bus() {
    let root = rr3_test_probe_root("dev-only");
    std::fs::create_dir_all(root.join("dev")).unwrap();
    std::fs::write(root.join("dev/i2c-1"), b"").unwrap();
    let probe = Rr3LinuxProbe::rr3_linux_probe_new(&root);
    // 总线级判据（前置条件）单看是绿的——正是 L-14 的病：它不是在位证据
    assert!(probe.rr3_probe_slot("power.charger"), "旧判据下总线在即绿");
    // 器件级判据：sysfs 从机节点不在 → 缺
    assert!(
        !probe.rr3_probe_slot_bound("power.charger", Some(1), Some("0x40")),
        "总线在、器件不在必须缺（L-14）"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 器件级正例：总线节点 + sysfs 从机节点都在 → 在位；从机节点摆错总线号 → 缺。
#[test]
fn linux_probe_i2c_device_node_satisfies_and_is_bus_scoped() {
    let root = rr3_test_probe_root("dev-node");
    std::fs::create_dir_all(root.join("dev")).unwrap();
    std::fs::write(root.join("dev/i2c-1"), b"").unwrap();
    std::fs::create_dir_all(root.join("sys/bus/i2c/devices/1-0040")).unwrap();
    let probe = Rr3LinuxProbe::rr3_linux_probe_new(&root);
    assert!(
        probe.rr3_probe_slot_bound("power.charger", Some(1), Some("0x40")),
        "1-0040 在 → 在位"
    );
    assert!(
        !probe.rr3_probe_slot_bound("power.charger", Some(0), Some("0x40")),
        "节点在总线 1，问总线 0 → 缺（收窄到具体总线，不回退任一总线即绿）"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// addr 词形坏（无 0x 前缀 / 越出 7 位地址域）= 拿不到答案 = 缺
/// （fail-closed，不回落总线级冒充在位）。
#[test]
fn linux_probe_i2c_bad_addr_word_is_absent() {
    let root = rr3_test_probe_root("bad-addr");
    std::fs::create_dir_all(root.join("dev")).unwrap();
    std::fs::write(root.join("dev/i2c-1"), b"").unwrap();
    std::fs::create_dir_all(root.join("sys/bus/i2c/devices/1-0040")).unwrap();
    let probe = Rr3LinuxProbe::rr3_linux_probe_new(&root);
    for bad in ["40", "0xzz", "0x80", "0x", "none"] {
        assert!(
            !probe.rr3_probe_slot_bound("sensor.env", Some(1), Some(bad)),
            "addr 词形 {bad:?} 必须缺"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// 兼容面：装配单未声明 [pins] / 该格无 I2C 绑定（bus/addr 任一缺席）→
/// 回落五面总线级判据原样——无 [pins] 形态的行为逐位不变（e2e 夹具即此形）。
#[test]
fn linux_probe_i2c_without_binding_falls_back_to_bus_level() {
    let root = rr3_test_probe_root("fallback");
    std::fs::create_dir_all(root.join("dev")).unwrap();
    std::fs::write(root.join("dev/i2c-1"), b"").unwrap();
    let probe = Rr3LinuxProbe::rr3_linux_probe_new(&root);
    for (bus, addr) in [(None, None), (Some(1), None), (None, Some("0x40"))] {
        assert!(
            probe.rr3_probe_slot_bound("sensor.imu", bus, addr),
            "bus/addr 缺席（{bus:?}, {addr:?}）必须回落总线级"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// sim 显式档：任何格位回"在位"（装配单缺 binding 才是它表达的缺格）。
#[test]
fn sim_probe_reports_every_slot_present() {
    let probe = Rr3SimProbe;
    for slot in ["act.servo", "sensor.env", "cognition.llm", ""] {
        assert!(probe.rr3_probe_slot(slot), "sim 档必须全在位: {slot:?}");
    }
}

/// 恒缺档（非 linux 生产面）：有硬件面的格位全缺，无硬件面可反证的格位在位
/// （与真值探测同判——"没有反证"不是"拿不到答案"）。
#[test]
fn absent_probe_reports_hardware_faces_missing_software_present() {
    let probe = Rr3AbsentProbe;
    for slot in [
        "act.servo",
        "act.led",
        "ui.face",
        "power.charger",
        "audio.capture",
    ] {
        assert!(!probe.rr3_probe_slot(slot), "有硬件面格位必须缺: {slot}");
    }
    for slot in ["cognition.llm", "net.wifi", "net.bt", "net.online", ""] {
        assert!(probe.rr3_probe_slot(slot), "无硬件面格位在位: {slot}");
    }
}

/// 台-1 清零观察式：显式档优先 + 平台默认分流（用**纯选择函数**断言，不读 env——
/// 单测进程 env 共享，读 env 会串别的用例；env 读取本身无分支逻辑）。
#[test]
fn probe_select_prefers_explicit_then_dispatches_by_target() {
    // 显式 sim：即便在 linux 上也选 sim（e2e/开发机的唯一出口，不再当缺省）
    assert!(rr3_probe_select(Some("sim")).rr3_probe_slot("act.servo"));
    // 显式 absent：全缺
    assert!(!rr3_probe_select(Some("absent")).rr3_probe_slot("act.servo"));
    // 显式 linux：linux 上真值探测（真机值不断言），非 linux 恒缺（fail-closed）
    let l = rr3_probe_select(Some("linux"));
    if !cfg!(target_os = "linux") {
        assert!(
            !l.rr3_probe_slot("act.servo"),
            "非 linux 显式 linux → 拿不到答案 → 恒缺"
        );
    }
    // 取值不认（含空串）：与"未给"同判 → 平台默认
    assert!(
        rr3_probe_select(Some("bogus")).rr3_probe_slot("act.servo"),
        "test 面回落 sim"
    );
    // 未给：test 面默认 sim；非 test 且非 linux → 恒缺（分流观察式）
    let d = rr3_probe_select(None);
    if cfg!(test) {
        assert!(d.rr3_probe_slot("act.servo"), "test 面默认 sim");
    } else if !cfg!(target_os = "linux") {
        assert!(!d.rr3_probe_slot("act.servo"), "非 linux 生产面恒缺");
    }
}
