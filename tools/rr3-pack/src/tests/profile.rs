//! 装配单参数化与打包预验单测（自 tests.rs 同判据抽出，判据不变）。
//!
//! 覆盖 0.0.5 批K 的 `--profile` 正/反例、基-25 真值集缺册反例，以及
//! “打包按装载同判据预验”的坏装配单拒绝面。

use std::fs;

use super::{cfg_of, fixture, DISTRO_PROFILE, ESP32_MINI_PROFILE};
use crate::rr3_stage;

// 指定机型装配单及其随装件的仓储侧铺料（--profile 正例前置）
fn seed_named_profile(repo: &std::path::Path) {
    fs::write(repo.join("profiles/esp32-mini.toml"), ESP32_MINI_PROFILE).unwrap();
    // chip 选册的平台事实表随 --profile 机型（基-25 清单成员）
    fs::write(
        repo.join("contracts/platform/esp32.toml"),
        "# platform esp32\n",
    )
    .unwrap();
    // 机型接线档随 --profile 同名落位（基-25 清单成员）
    fs::create_dir_all(repo.join("hardware/esp32-mini")).unwrap();
    fs::write(
        repo.join("hardware/esp32-mini/wiring.toml"),
        "# esp32-mini 接线档\n",
    )
    .unwrap();
    // 轴档案目录在则整目录随装（基-25 清单 dirs 成员，rtos 机型）
    fs::create_dir_all(repo.join("hardware/esp32-mini/axis")).unwrap();
    fs::write(repo.join("hardware/esp32-mini/axis/pan.toml"), "# pan\n").unwrap();
    fs::write(repo.join("hardware/esp32-mini/axis/tilt.toml"), "# tilt\n").unwrap();
}

// 指定机型装机位落位与 distro 不夹带的断言面（--profile 正例）
fn check_named_stage(stage: &std::path::Path) {
    assert_eq!(
        fs::read_to_string(stage.join("usr/lib/rockrobot3/profiles/esp32-mini.toml")).unwrap(),
        ESP32_MINI_PROFILE,
        "指定机型装配单零改写落位"
    );
    assert_eq!(
        fs::read_to_string(stage.join("usr/lib/rockrobot3/contracts/platform/esp32.toml")).unwrap(),
        "# platform esp32\n",
        "平台事实表按机型 chip 选册随包（基-25）"
    );
    assert_eq!(
        fs::read_to_string(stage.join("usr/lib/rockrobot3/hardware/esp32-mini/wiring.toml"))
            .unwrap(),
        "# esp32-mini 接线档\n",
        "指定机型接线档落同名装机位（装载推导同布局）"
    );
    assert_eq!(
        fs::read_to_string(stage.join("usr/lib/rockrobot3/hardware/esp32-mini/axis/pan.toml"))
            .unwrap(),
        "# pan\n",
        "轴档案目录在则整目录随装（基-25 清单 dirs 成员）"
    );
    assert!(stage
        .join("usr/lib/rockrobot3/hardware/esp32-mini/axis/tilt.toml")
        .is_file());
    // deb 只装 --profile 指的那一份装配单/接线档，不顺手带上 distro
    assert!(!stage
        .join("usr/lib/rockrobot3/profiles/distro.toml")
        .exists());
    assert!(!stage.join("usr/lib/rockrobot3/hardware/distro").exists());
    // rrd.service ExecStart 的装机位随 --profile 同步改写（深挖收口 P1）：
    // 非 distro 机型出包，单元不得再指向包里不存在的 distro.toml
    let staged_unit = fs::read_to_string(stage.join("usr/lib/systemd/system/rrd.service")).unwrap();
    assert!(
        staged_unit.contains("--profile /usr/lib/rockrobot3/profiles/esp32-mini.toml"),
        "ExecStart 装机位须随 --profile 改写"
    );
    assert!(
        !staged_unit.contains("profiles/distro.toml"),
        "非 distro 机型的单元不得残留 distro 装机位"
    );
}

/// H4/C2（0.0.5 批K）：--profile 参数化——出包链不再只认 distro 一份装配单，
/// "换板只改装配单"在装机路径真走通（staging 按 --profile 名零改写落位；
/// 缺席点名到具体机型文件，不静默回落 distro）。
#[test]
fn profile_flag_stages_named_profile_and_names_it_when_absent() {
    // 正例：--profile esp32-mini → 装机位 profiles/esp32-mini.toml，字节零改写
    //（真仓文本：rtos 档装配单过打包预验 = 过装载同判据，chip=esp32）
    let (payload, repo) = fixture();
    seed_named_profile(&repo);
    let (mut cfg, _) = cfg_of(&payload, &repo);
    cfg.profile = "esp32-mini".to_string();
    let stage = rr3_stage(&cfg).unwrap();
    check_named_stage(&stage);
    // 反例：指定机型缺席 → 点名到具体文件
    let (payload, repo) = fixture();
    let (mut cfg, _) = cfg_of(&payload, &repo);
    cfg.profile = "esp32-mini".to_string();
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("esp32-mini.toml"), "{e}");
}

/// 基-25 清单判等反例面：平台事实表缺席 → 打包拒出包并点名册位（清单真值 =
/// rrd rr3_truth_set，打包面不自带第二份清单）。
#[test]
fn stage_refuses_when_truth_set_file_missing() {
    let (payload, repo) = fixture();
    fs::remove_file(repo.join("contracts/platform/rk3566.toml")).unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("真值集缺册"), "{e}");
    assert!(e.contains("contracts/platform/rk3566.toml"), "{e}");
}

/// 基-25 打包预验判据（与装载同判据单点）：装配单坏形 → 拒出包，出包时红早于
/// 装机 boot 红（块-15：装载必需件缺席而装机验证全绿才是最贵的洞）。
#[test]
fn stage_rejects_malformed_profile_by_loader_criteria() {
    let (payload, repo) = fixture();
    let bad = DISTRO_PROFILE.replace(
        &format!("schema_version = {}", 5),
        &format!("schema_version = {}", 99),
    );
    assert_ne!(bad, DISTRO_PROFILE, "替换须命中（schema_version 行在册）");
    fs::write(repo.join("profiles/distro.toml"), bad).unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("预验失败"), "{e}");
    assert!(e.contains("schema_version"), "{e}");
}
