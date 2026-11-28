// SPDX-License-Identifier: Apache-2.0
//! rr3-pack 单测（自入口同名单测模块原样抽出，判据不变）。

use super::*;

mod fixture;
mod profile;

use fixture::*;

/// 源码仓身份零泄漏：缺省维护者必须是 RFC 2606 保留域（发版由环境变量
/// 注入真实身份，不在树内落任何个人/单位地址）。
#[test]
fn control_text_domain() {
    assert!(RR3_DEB_MAINTAINER.ends_with(".invalid>"));
    let text = rr3_control_text("arm64");
    assert!(text.contains("Maintainer: "));
    assert!(text.contains(RR3_DEB_MAINTAINER) || std::env::var(RR3_DEB_MAINTAINER_ENV).is_ok());
    assert!(text.ends_with('\n'));
}

/// 三枚二进制注入位（usr/bin）。
fn assert_bins(stage: &std::path::Path) {
    assert_eq!(
        fs::read_to_string(stage.join("usr/bin/rrd")).unwrap(),
        "rrd-bin-bytes"
    );
    assert_eq!(
        fs::read_to_string(stage.join("usr/bin/rr3-ctl")).unwrap(),
        "ctl-bin-bytes"
    );
    assert_eq!(
        fs::read_to_string(stage.join("usr/bin/rr3-ota-ctl")).unwrap(),
        "ota-ctl-bin-bytes"
    );
}

/// 数据真值位零改写：装配单/契约、平台事实表按 chip 选册、接线档落装机位。
fn assert_truth(root: &std::path::Path) {
    assert_eq!(
        fs::read_to_string(root.join("profiles/distro.toml")).unwrap(),
        DISTRO_PROFILE
    );
    assert_eq!(
        fs::read_to_string(root.join("contracts/slots.toml")).unwrap(),
        "# slots\n"
    );
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
}

/// systemd 单元 + control + postinst（数据目录与 ctl 属组幂等建组）。
fn assert_service(stage: &std::path::Path) {
    assert_eq!(
        fs::read_to_string(stage.join("usr/lib/systemd/system/rrd.service")).unwrap(),
        RR3_SERVICE_UNIT
    );
    let control = fs::read_to_string(stage.join("DEBIAN/control")).unwrap();
    assert!(control.contains("Package: rockrobot3"));
    assert!(control.contains(&format!("Version: {}", RR3_PRODUCT_VERSION.trim())));
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
}

/// 模块位：entry 改写为装机绝对路径，其余字节零差异；bin 按 entry 落位；config 随包。
fn assert_modules(root: &std::path::Path, payload: &std::path::Path) {
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

#[test]
fn stage_out_deb_entry() {
    let (payload, repo) = fixture();
    let (cfg, out) = cfg_of(&payload, &repo);
    let stage = rr3_stage(&cfg).unwrap();
    assert_eq!(stage, out.with_file_name("rockrobot3.deb.stage"));
    let root = stage.join("usr/lib/rockrobot3");
    assert_bins(&stage);
    assert_truth(&root);
    assert_service(&stage);
    assert_modules(&root, &payload);
}

/// staging 复用必须先清空：上一次遗留的模块/配置不得进入本次包。
#[test]
fn stage_stale_files() {
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

/// 板级资产：boards/<profile>/ 存在即整树随装，postinst 按 profile 落 /etc
/// （systemd 段覆盖、其余只铺缺失件）。
#[test]
fn stage_board_postinst() {
    let (payload, repo) = fixture();
    let etc = repo.join("base/os/distro/boards/distro/etc");
    fs::create_dir_all(etc.join("systemd/system/rrd.service.d")).unwrap();
    fs::write(etc.join("asound.conf"), "pcm.x {}\n").unwrap();
    fs::write(
        etc.join("systemd/system/rrd.service.d/distro.conf"),
        "[Service]\nEnvironment=X=1\n",
    )
    .unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let stage = rr3_stage(&cfg).unwrap();
    let root = stage.join("usr/lib/rockrobot3");
    assert_eq!(
        fs::read_to_string(root.join("boards/distro/etc/asound.conf")).unwrap(),
        "pcm.x {}\n"
    );
    assert!(root
        .join("boards/distro/etc/systemd/system/rrd.service.d/distro.conf")
        .is_file());
    let post = fs::read_to_string(stage.join("DEBIAN/postinst")).unwrap();
    assert!(post.contains("/usr/lib/rockrobot3/boards/distro"), "{post}");
    assert!(post.contains("etc/systemd/*) install -D -m 644"), "{post}");
}

#[test]
fn stage_whole_tree_g4() {
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
fn miss_pieces_named() {
    let (payload, repo) = fixture();
    fs::remove_file(payload.join("rrd")).unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("rrd 缺席"), "{e}");
    // OTA CLI 缺件同样点名（签名/验签入口不能静默缺席）
    let (payload, repo) = fixture();
    fs::remove_file(payload.join("rr3-ota-ctl")).unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("rr3-ota-ctl 缺席"), "{e}");
    // 模块缺 bin
    let (payload, repo) = fixture();
    fs::remove_file(payload.join("rr3-modules/rr3-act/bin")).unwrap();
    let (cfg, _) = cfg_of(&payload, &repo);
    let e = rr3_stage(&cfg).unwrap_err();
    assert!(e.contains("rr3-act") && e.contains("bin"), "{e}");
    // 无模块（三个注入目录全清）
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
fn uninstallable_name() {
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
fn module_dir_mft_agree() {
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

#[test]
fn parse_knows_stage() {
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
    // 目标根出口：--install-root 与 --out 互斥，二者至少一个
    let cfg = rr3_pack_parse(&v(&[
        "--payload",
        "p",
        "--repo",
        "r",
        "--install-root",
        "t",
    ]))
    .unwrap();
    assert_eq!(cfg.install_root.as_deref(), Some(Path::new("t")));
    assert!(rr3_pack_parse(&v(&[
        "--payload",
        "p",
        "--repo",
        "r",
        "--out",
        "o.deb",
        "--install-root",
        "t",
    ]))
    .is_err());
}

/// 目标根装配：三件入口 + 模块树 + 真值集落位，DEBIAN/systemd 不出现。
#[test]
fn install_tree_lands_modules() {
    let (payload, repo) = fixture();
    let target = payload.parent().unwrap().join("target");
    let mut cfg = cfg_of(&payload, &repo).0;
    cfg.install_root = Some(target.clone());
    rr3_install_tree(&cfg).unwrap();
    let root = target.join("usr/lib/rockrobot3");
    assert!(target.join("usr/bin/rrd").is_file());
    assert!(root.join("modules/rr3-act/module.toml").is_file());
    assert!(root.join("modules/rr3-act/rr3-act").is_file());
    assert!(root.join("modules/rr3-act/config.toml").is_file());
    assert!(root
        .join("modules/rr3-brain/pkg/rr3_brain/__init__.py")
        .is_file());
    assert!(root.join("modules/rr3-brain/rr3-brain").is_file());
    assert!(root.join("profiles/distro.toml").is_file());
    assert!(root.join("contracts/topics.toml").is_file());
    assert!(root.join("hardware/distro/wiring.toml").is_file());
    assert!(!target.join("DEBIAN").exists());
    assert!(!target.join("usr/lib/systemd").exists());
}
