//! rr3-pack 单测（自入口同名单测模块原样抽出，判据不变）。

use super::*;

/// 源码仓身份零泄漏：缺省维护者必须是 RFC 2606 保留域（发版由环境变量
/// 注入真实身份，不在树内落任何个人/单位地址）。
#[test]
fn control_text_defaults_to_reserved_domain() {
    assert!(RR3_DEB_MAINTAINER.ends_with(".invalid>"));
    let text = rr3_control_text("arm64");
    assert!(text.contains("Maintainer: "));
    assert!(text.contains(RR3_DEB_MAINTAINER) || std::env::var(RR3_DEB_MAINTAINER_ENV).is_ok());
    assert!(text.ends_with('\n'));
}

/// 真仓装配单（基-25：打包预验走 rrd 解析器，夹具必须给合法装配单——空文本过不了
/// 解析器。include_str! 相对源文件 src/，到仓根四级）。distro chip=rk3566、
/// esp32-mini chip=esp32，平台事实表册位随之。
const DISTRO_PROFILE: &str = include_str!("../../../profiles/distro.toml");
const ESP32_MINI_PROFILE: &str = include_str!("../../../profiles/esp32-mini.toml");

/// 夹具根唯一序号：并行测试各占一个目录（ZST 指针恒同、pid 同批相同，都当不得唯一键）
fn fixture_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// 夹具：临时 payload + repo。
fn fixture() -> (PathBuf, PathBuf) {
    let base =
        std::env::temp_dir().join(format!("rr3-pack-{}-{}", std::process::id(), fixture_seq()));
    let _ = fs::remove_dir_all(&base);
    let payload = base.join("payload");
    let repo = base.join("repo");
    fs::create_dir_all(payload.join("rr3-modules/rr3-act")).unwrap();
    fs::create_dir_all(payload.join("rr3-modules/rr3-connect")).unwrap();
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(repo.join("contracts")).unwrap();
    fs::create_dir_all(repo.join("modules/rr3-act")).unwrap();
    fs::create_dir_all(repo.join("modules/rr3-connect")).unwrap();
    fs::write(payload.join("rrd"), "rrd-bin-bytes").unwrap();
    fs::write(payload.join("rr3-ctl"), "ctl-bin-bytes").unwrap();
    let manifest = "[manifest]\nschema_version = 1\nname = \"rr3-act\"\nversion = \"0.0.1\"\ntype = \"module\"\nlang = \"rust\"\nentry = \"rr3-act\" # 在仓相对\n\n[provides]\ncapabilities = [\"act.servo\"]\n\n[depends]\nrequired = [\"bus.publish\"]\n\n[events]\npublishes = []\nsubscribes = []\n\n[health]\nheartbeat_ms = 1000\nwatchdog_timeout_ms = 5000\n\n[resources]\nrestart_policy = \"always\"\nmem_limit_mb = 16\nstart_priority = 40\n";
    for m in ["rr3-act", "rr3-connect"] {
        fs::write(
            payload.join(format!("rr3-modules/{m}/manifest.toml")),
            manifest.replace("rr3-act", m),
        )
        .unwrap();
        fs::write(
            payload.join(format!("rr3-modules/{m}/bin")),
            format!("{m}-bin"),
        )
        .unwrap();
        fs::write(repo.join(format!("modules/{m}/config.toml")), "[x]\n").unwrap();
    }
    // python 模块（批F1）：pkg 树 + manifest（lang=python），无 bin——
    // entry 装机产物是打包器生成的 shim（PYTHONPATH 须装机绝对）
    fs::create_dir_all(payload.join("rr3-modules/rr3-brain/pkg/rr3_brain")).unwrap();
    fs::create_dir_all(repo.join("modules/rr3-brain")).unwrap();
    fs::write(
        payload.join("rr3-modules/rr3-brain/manifest.toml"),
        manifest
            .replace("rr3-act", "rr3-brain")
            .replace("lang = \"rust\"", "lang = \"python\""),
    )
    .unwrap();
    fs::write(
        payload.join("rr3-modules/rr3-brain/pkg/rr3_brain/__init__.py"),
        "__version__ = \"0.0.1\"\n",
    )
    .unwrap();
    fs::write(repo.join("modules/rr3-brain/config.toml"), "[rr3-brain]\n").unwrap();
    fs::write(repo.join("profiles/distro.toml"), DISTRO_PROFILE).unwrap();
    fs::write(repo.join("contracts/topics.toml"), "# topics\n").unwrap();
    fs::write(repo.join("contracts/slots.toml"), "# slots\n").unwrap();
    // boot 真值集（基-25 清单 files 成员）：roles/devices 契约 + 平台事实表
    //（按装配单 [platform].chip 选册）+ 机型接线档
    fs::write(repo.join("contracts/roles.toml"), "# roles\n").unwrap();
    fs::write(repo.join("contracts/devices.toml"), "# devices\n").unwrap();
    fs::create_dir_all(repo.join("contracts/platform")).unwrap();
    fs::write(
        repo.join("contracts/platform/rk3566.toml"),
        "# platform rk3566\n",
    )
    .unwrap();
    fs::create_dir_all(repo.join("hardware/distro")).unwrap();
    fs::write(repo.join("hardware/distro/wiring.toml"), "# wiring\n").unwrap();
    (payload, repo)
}

fn cfg_of(payload: &Path, repo: &Path) -> (Rr3PackConfig, PathBuf) {
    let out = payload.parent().unwrap().join("rockrobot3.deb");
    (
        Rr3PackConfig {
            payload: payload.to_path_buf(),
            repo: repo.to_path_buf(),
            out: out.clone(),
            arch: "arm64".to_string(),
            profile: "distro".to_string(),
            stage_only: true,
        },
        out,
    )
}

#[test]
fn stage_tree_lays_out_deb_layout_and_rewrites_entry_only() {
    let (payload, repo) = fixture();
    let (cfg, out) = cfg_of(&payload, &repo);
    let stage = rr3_stage(&cfg).unwrap();
    assert_eq!(stage, out.with_file_name("rockrobot3.deb.stage"));
    let root = stage.join("usr/lib/rockrobot3");
    // 二进制注入位
    assert_eq!(
        fs::read_to_string(stage.join("usr/bin/rrd")).unwrap(),
        "rrd-bin-bytes"
    );
    assert_eq!(
        fs::read_to_string(stage.join("usr/bin/rr3-ctl")).unwrap(),
        "ctl-bin-bytes"
    );
    // 数据真值位：零改写
    assert_eq!(
        fs::read_to_string(root.join("profiles/distro.toml")).unwrap(),
        DISTRO_PROFILE
    );
    assert_eq!(
        fs::read_to_string(root.join("contracts/slots.toml")).unwrap(),
        "# slots\n"
    );
    // boot 真值集随包（基-25 清单 files 成员）：roles/devices 契约 + chip 选册的
    // 平台事实表 + 机型接线档；轴档案目录缺席（distro 无 axis）不造假数据
    assert_eq!(
        fs::read_to_string(root.join("contracts/roles.toml")).unwrap(),
        "# roles\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("contracts/devices.toml")).unwrap(),
        "# devices\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("contracts/platform/rk3566.toml")).unwrap(),
        "# platform rk3566\n",
        "平台事实表按装配单 [platform].chip 选册随包（基-25）"
    );
    assert_eq!(
        fs::read_to_string(root.join("hardware/distro/wiring.toml")).unwrap(),
        "# wiring\n",
        "接线档落装机位 hardware/<机型>/wiring.toml（装载推导同布局）"
    );
    assert!(!root.join("hardware/distro/axis").exists());
    assert_eq!(
        fs::read_to_string(stage.join("usr/lib/systemd/system/rrd.service")).unwrap(),
        RR3_SERVICE_UNIT
    );
    // control / postinst
    let control = fs::read_to_string(stage.join("DEBIAN/control")).unwrap();
    assert!(control.contains("Package: rockrobot3"));
    assert!(control.contains("Version: 0.0.6"));
    assert!(control.contains("Architecture: arm64"));
    let postinst = fs::read_to_string(stage.join("DEBIAN/postinst")).unwrap();
    assert!(postinst.starts_with("#!/bin/sh"));
    assert!(postinst.contains("systemctl enable rrd.service"));
    // 计数持久化目录（0.0.6 批E' R3）：postinst 建目录，与 unit 的 RR3_DATA_DIR 同路径
    assert!(postinst.contains("mkdir -p /var/lib/rockrobot3"));
    assert!(postinst.contains("chown root:root /var/lib/rockrobot3"));
    // ctl socket 属主组（0.0.6 批F5b R12）：幂等建组（组在位不报错），与
    // rrd daemon 的 chgrp 组名同字面
    assert!(postinst.contains("addgroup --system rockrobot3 2>/dev/null || true"));
    // 模块位：entry 改写为装机绝对路径，其余字节零差异；bin 按 entry 落位；config 随包
    let staged_manifest = fs::read_to_string(root.join("modules/rr3-act/module.toml")).unwrap();
    let facts = rr3_manifest_parse(&staged_manifest).unwrap();
    assert_eq!(facts.entry, "/usr/lib/rockrobot3/modules/rr3-act/rr3-act");
    let src = fs::read_to_string(payload.join("rr3-modules/rr3-act/manifest.toml")).unwrap();
    let expect = src.replace(
        "entry = \"rr3-act\"",
        "entry = \"/usr/lib/rockrobot3/modules/rr3-act/rr3-act\"",
    );
    assert_eq!(staged_manifest, expect, "除 entry 值外必须字节一致");
    assert_eq!(
        fs::read_to_string(root.join("modules/rr3-act/rr3-act")).unwrap(),
        "rr3-act-bin"
    );
    assert!(root.join("modules/rr3-act/config.toml").is_file());
    assert!(root.join("modules/rr3-connect/rr3-connect").is_file());
}

/// staging 复用必须先清空：上一次遗留的模块/配置不得进入本次包。
#[test]
fn stage_reset_removes_stale_files() {
    let (payload, repo) = fixture();
    let (cfg, out) = cfg_of(&payload, &repo);
    let stage = out.with_file_name("rockrobot3.deb.stage");
    let stale = stage.join("usr/lib/rockrobot3/modules/rr3-gone/module.toml");
    fs::create_dir_all(stale.parent().unwrap()).unwrap();
    fs::write(&stale, "stale").unwrap();
    let got = rr3_stage(&cfg).unwrap();
    assert_eq!(got, stage);
    assert!(!stale.exists(), "旧 staging 的陈旧模块必须被清掉");
    assert!(stage.join("usr/bin/rrd").is_file(), "清空后必须重新装配");
}

#[test]
fn stage_copies_whole_declaration_tree_for_g4() {
    let (payload, repo) = fixture();
    // rr3-act 给齐声明树：组合件编排 + 两原子件（含子文件，证递归纯副本）
    let repo_act = repo.join("modules/rr3-act");
    fs::write(
        repo_act.join("composite.toml"),
        "[assemble]\natoms = [\"led\", \"servo\"]\n",
    )
    .unwrap();
    fs::create_dir_all(repo_act.join("atoms/servo")).unwrap();
    fs::create_dir_all(repo_act.join("atoms/led")).unwrap();
    fs::write(
        repo_act.join("atoms/servo/assemble.toml"),
        "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\n",
    )
    .unwrap();
    fs::write(repo_act.join("atoms/servo/servo.rs"), "// mech\n").unwrap();
    fs::write(
        repo_act.join("atoms/led/assemble.toml"),
        "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\n",
    )
    .unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let stage = rr3_stage(&cfg).unwrap();
    let root = stage.join("usr/lib/rockrobot3");
    assert_eq!(
        fs::read_to_string(root.join("modules/rr3-act/composite.toml")).unwrap(),
        "[assemble]\natoms = [\"led\", \"servo\"]\n",
        "组合件编排声明随包"
    );
    assert_eq!(
        fs::read_to_string(root.join("modules/rr3-act/atoms/servo/assemble.toml")).unwrap(),
        "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("modules/rr3-act/atoms/servo/servo.rs")).unwrap(),
        "// mech\n",
        "atoms 子树递归纯副本"
    );
    assert!(root
        .join("modules/rr3-act/atoms/led/assemble.toml")
        .is_file());
    // 未给声明树的模块不造假数据
    assert!(!root.join("modules/rr3-connect/atoms").exists());
    assert!(!root.join("modules/rr3-connect/composite.toml").exists());
}

#[test]
fn missing_payload_pieces_are_named() {
    let (payload, repo) = fixture();
    fs::remove_file(payload.join("rrd")).unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("rrd 缺席"), "{e}");
    // 模块缺 bin
    let (payload, repo) = fixture();
    fs::remove_file(payload.join("rr3-modules/rr3-act/bin")).unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("rr3-act") && e.contains("bin"), "{e}");
    // 无模块（三个注入目录全清，含 python 的 rr3-brain）
    let (payload, repo) = fixture();
    for m in ["rr3-act", "rr3-connect", "rr3-brain"] {
        fs::remove_dir_all(payload.join(format!("rr3-modules/{m}"))).unwrap();
    }
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("无模块"), "{e}");
    // repo 缺装配单
    let (payload, repo) = fixture();
    fs::remove_file(repo.join("profiles/distro.toml")).unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("profiles/distro.toml"), "{e}");
    // repo 缺机型接线档（boot 真值集第五册，Z-4：deb 缺它 rrd 装载即拒 boot）
    let (payload, repo) = fixture();
    fs::remove_file(repo.join("hardware/distro/wiring.toml")).unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("hardware/distro/wiring.toml"), "{e}");
    // 缺 config（模块开机即读，缺件必重启环）
    let (payload, repo) = fixture();
    fs::remove_file(repo.join("modules/rr3-act/config.toml")).unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("config.toml"), "{e}");
}

#[test]
fn uninstallable_entries_are_rejected_by_name() {
    let (payload, repo) = fixture();
    let manifest = payload.join("rr3-modules/rr3-act/manifest.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)
            .unwrap()
            .replace("entry = \"rr3-act\"", "entry = \"/usr/bin/rr3-act\""),
    )
    .unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("绝对路径"), "{e}");
    // `..` 逃逸段
    let (payload, repo) = fixture();
    let manifest = payload.join("rr3-modules/rr3-act/manifest.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)
            .unwrap()
            .replace("entry = \"rr3-act\"", "entry = \"../rr3-act\""),
    )
    .unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains(".."), "{e}");
}

#[test]
fn module_dir_and_manifest_name_must_agree() {
    let (payload, repo) = fixture();
    fs::rename(
        payload.join("rr3-modules/rr3-act"),
        payload.join("rr3-modules/rr3-actx"),
    )
    .unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("rr3-actx") && e.contains("不一致"), "{e}");
}

/// python 模块（批F1）：entry 装机产物 = 打包器生成的 shim（PYTHONPATH 装机绝对），
/// pkg 树落位、entry 改写为装机绝对路径、config 随包、shim 可执行。
#[test]
fn python_module_stages_generated_shim_and_pkg_tree() {
    let (payload, repo) = fixture();
    let (cfg, _) = cfg_of(&payload, &repo);
    let stage = rr3_stage(&cfg).unwrap();
    let dir = stage.join("usr/lib/rockrobot3/modules/rr3-brain");
    // pkg 树落位（逐文件，非空目录即过）
    assert_eq!(
        fs::read_to_string(dir.join("pkg/rr3_brain/__init__.py")).unwrap(),
        "__version__ = \"0.0.1\"\n"
    );
    // entry 改写为装机绝对路径，lang 事实保留
    let facts = rr3_manifest_parse(&fs::read_to_string(dir.join("module.toml")).unwrap()).unwrap();
    assert_eq!(facts.lang, "python");
    assert_eq!(
        facts.entry,
        "/usr/lib/rockrobot3/modules/rr3-brain/rr3-brain"
    );
    // shim 形状：RR3_BRAIN_CONFIG + PYTHONPATH 装机绝对 + python3 -m rr3_brain
    // （目录名连字符转下划线；config 注入是装机布局修正——brain 回退按"包上一级"
    // 寻址，pkg/ 嵌套下会指到 pkg/config.toml 虚位，见批F1 真板验证）
    assert_eq!(
        fs::read_to_string(dir.join("rr3-brain")).unwrap(),
        "#!/bin/sh\nRR3_BRAIN_CONFIG=/usr/lib/rockrobot3/modules/rr3-brain/config.toml \
             PYTHONPATH=/usr/lib/rockrobot3/modules/rr3-brain/pkg exec python3 -m rr3_brain\n"
    );
    assert!(dir.join("config.toml").is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.join("rr3-brain"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0, "shim 必须可执行（rrd 直接 exec entry）");
    }
}

/// python 模块缺 pkg 树 → 点名拒绝（不是含糊的 "bin 缺席"）
#[test]
fn python_module_without_pkg_is_rejected_by_name() {
    let (payload, repo) = fixture();
    fs::remove_dir_all(payload.join("rr3-modules/rr3-brain/pkg")).unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("rr3-brain") && e.contains("pkg"), "{e}");
}

/// H4/C2（0.0.5 批K）：--profile 参数化——出包链不再只认 distro 一份装配单，
/// "换板只改装配单"在装机路径真走通（staging 按 --profile 名零改写落位；
/// 缺席点名到具体机型文件，不静默回落 distro）。
#[test]
fn profile_flag_stages_named_profile_and_names_it_when_absent() {
    // 正例：--profile esp32-mini → 装机位 profiles/esp32-mini.toml，字节零改写
    //（真仓文本：rtos 档装配单过打包预验 = 过装载同判据，chip=esp32）
    let (payload, repo) = fixture();
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
    let (mut cfg, _) = cfg_of(&payload, &repo);
    cfg.profile = "esp32-mini".to_string();
    let stage = rr3_stage(&cfg).unwrap();
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

#[test]
fn parse_requires_flags_and_knows_stage_only() {
    assert!(rr3_pack_parse(&[]).is_err());
    let v = |s: &[&str]| -> Vec<String> { s.iter().map(|x| x.to_string()).collect() };
    let cfg = rr3_pack_parse(&v(&[
        "--payload",
        "p",
        "--repo",
        "r",
        "--out",
        "o.deb",
        "--stage-only",
    ]))
    .unwrap();
    assert!(cfg.stage_only);
    assert_eq!(cfg.arch, "arm64");
    assert_eq!(
        cfg.profile, "distro",
        "缺省机型 = distro（既有调用方零改动）"
    );
    let cfg = rr3_pack_parse(&v(&[
        "--payload",
        "p",
        "--repo",
        "r",
        "--out",
        "o.deb",
        "--profile",
        "esp32-mini",
    ]))
    .unwrap();
    assert_eq!(cfg.profile, "esp32-mini");
    assert!(rr3_pack_parse(&v(&["--payload", "p"])).is_err());
    assert!(rr3_pack_parse(&v(&["--payload", "p", "--repo", "r"])).is_err());
    assert!(rr3_pack_parse(&v(&["--bogus"])).is_err());
}
