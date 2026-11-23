// SPDX-License-Identifier: Apache-2.0
//! rr3-ota-ctl 单测（文件头：本文件只放测试，生产面在 `main.rs`）。

use super::*;
use crate::bootconf_cmd::*;
use crate::sign_cmd::*;
use rr_ota::bootenv;
use std::path::{Path, PathBuf};
/// 打印的标签必须与 rr-ota 常量逐一对应（否则文档与代码就分家了）
#[test]
fn slot_labels_come_from_rr_ota_constants() {
    assert_eq!(
        ALL_SLOTS.map(|s| s.rr3_slot_label()),
        ["rr_a", "rr_b", "rr_factory"]
    );
    assert_eq!(rr_ota::RR3_OTA_SLOT_NAMES, ["rr_a", "rr_b"]);
}
/// 写侧实装后，"恒拒"面由"无效目标拒"取代：不在册标签在用法层拒
/// （exit 2）；业务规则（同槽 / factory 目标 / 形状漂移）归 rr-ota::bootconf
#[test]
fn slot_arg_maps_known_labels_and_rejects_unknown_ones() {
    assert_eq!(rr3_slot_arg("rr_a"), Ok(Rr3Slot::SlotA));
    assert_eq!(rr3_slot_arg("rr_b"), Ok(Rr3Slot::SlotB));
    assert_eq!(rr3_slot_arg("rr_factory"), Ok(Rr3Slot::Factory));
    let e = rr3_slot_arg("nonsense").unwrap_err();
    assert!(e.contains("未知槽位标签"), "不在册标签必须拒: {e}");
}
/// CLI 壳端到端：临时文件 读 → 改写 → 原子写回；业务拒（同槽）透传 rr-ota 文案
#[test]
fn switch_command_rewrites_file_and_rejects_same_slot() {
    let dir = std::env::temp_dir().join(format!("rr3-ota-ctl-{}-e2e", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("extlinux.conf");
    std::fs::write(
        &path,
        "label rockrobot3\n    append root=LABEL=rr_a rootwait\n",
    )
    .unwrap();
    let msg = rr3_cmd_switch(Rr3Slot::SlotB, &path).unwrap();
    assert!(msg.contains("rr_b"), "成功文案须点名新槽: {msg}");
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("root=LABEL=rr_b"),
        "文件必须真的落了新标签"
    );
    // 同槽再切：业务拒（退出码 1），文件不被重写
    let err = rr3_cmd_switch(Rr3Slot::SlotB, &path).unwrap_err();
    assert_eq!(err.1, ExitCode::from(1), "业务拒退出码是 1");
    assert!(err.0.contains("拒绝切换"), "拒因须点名: {}", err.0);
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("root=LABEL=rr_b"),
        "被拒的请求不得改动文件"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
/// --bootconf 是硬门槛：缺参必须在用法层拒，不得缺省碰任何路径
#[test]
fn bootconf_arg_is_mandatory_and_never_defaulted() {
    let args: Vec<String> = vec!["switch".into(), "rr_b".into()];
    assert!(rr3_bootconf_arg(&args).is_err(), "缺 --bootconf 必须拒");
    let args: Vec<String> = vec!["switch".into(), "rr_b".into(), "--bootconf".into()];
    assert!(
        rr3_bootconf_arg(&args).is_err(),
        "--bootconf 缺路径值必须拒"
    );
    let args: Vec<String> = vec![
        "switch".into(),
        "rr_b".into(),
        "--bootconf".into(),
        "/tmp/x.conf".into(),
    ];
    assert_eq!(
        rr3_bootconf_arg(&args).unwrap(),
        PathBuf::from("/tmp/x.conf")
    );
}
/// 回滚链夹具：合法 raw env（一条 bootcmd）+ 单行 fw_env.config。
/// 普通文件顶替块设备（同 rr-ota::bootenv 测试口径），返回 (config 路径, 目录)。
#[cfg(unix)]
fn rb_fixture(tag: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("rr3-ota-rb-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dev = dir.join("env.bin");
    let mut env = bootenv::Rr3BootEnv::default();
    env.rr3_env_set("bootcmd", "run distro_bootcmd").unwrap();
    std::fs::write(&dev, env.rr3_env_render(128).unwrap()).unwrap();
    let cfg = dir.join("fw_env.config");
    std::fs::write(&cfg, format!("{} 0x0 0x80\n", dev.display())).unwrap();
    (cfg, dir)
}
/// arm 写四变量、ok 写两变量：都经真实 fs 读改写，读回逐键等值。
#[cfg(unix)]
#[test]
fn rollback_arm_then_ok_cycles_through_env_file() {
    let (cfg, dir) = rb_fixture("cycle");
    let envp = vec!["--env".into(), cfg.to_string_lossy().into_owned()];
    let mut arm = vec!["--limit".into(), "3".into(), "--altcmd".into()];
    arm.push("run rr3_alt".into());
    arm.extend(envp.iter().cloned());
    let msg = rr3_cmd_rb_arm(&arm).unwrap();
    assert!(msg.contains("bootlimit=3"), "成功文案须点名上限: {msg}");
    assert!(
        msg.contains("upgrade_available=1"),
        "成功文案须点名上膛: {msg}"
    );
    let dev = rr3_env_dev(&cfg).unwrap().0;
    let env = bootenv::rr3_env_read(&dev).unwrap();
    assert_eq!(env.rr3_env_get("upgrade_available"), Some("1"));
    assert_eq!(env.rr3_env_get("bootcount"), Some("0"));
    assert_eq!(env.rr3_env_get("bootlimit"), Some("3"));
    assert_eq!(env.rr3_env_get("altbootcmd"), Some("run rr3_alt"));
    let msg = rr3_cmd_rb_ok(&envp).unwrap();
    assert!(msg.contains("upgrade_available=0"), "{msg}");
    let env = bootenv::rr3_env_read(&dev).unwrap();
    assert_eq!(env.rr3_env_get("upgrade_available"), Some("0"));
    assert_eq!(env.rr3_env_get("bootcount"), Some("0"));
    // 落点与上限保留（排障可见、下次上膛少写两笔）
    assert_eq!(env.rr3_env_get("bootlimit"), Some("3"));
    assert_eq!(env.rr3_env_get("altbootcmd"), Some("run rr3_alt"));
    let _ = std::fs::remove_dir_all(&dir);
}
/// 三条业务拒（altcmd 空 / limit=0 / env 缺位）+ 两条用法错（缺参 / 非数）
/// 各自独立成立：文案点名对象，退出码分 1（业务）与 2（用法）。
#[cfg(unix)]
#[test]
fn rollback_error_faces_are_named() {
    let (cfg, dir) = rb_fixture("err");
    let envp = cfg.to_string_lossy().into_owned();
    let mk = |limit: &str, alt: &str| -> Vec<String> {
        vec![
            "--limit".into(),
            limit.into(),
            "--altcmd".into(),
            alt.into(),
            "--env".into(),
            envp.clone(),
        ]
    };
    let (m, c) = rr3_cmd_rb_arm(&mk("3", "  ")).unwrap_err();
    assert_eq!(c, ExitCode::from(1), "空落点是业务拒");
    assert!(m.contains("拒绝上膛") && m.contains("altbootcmd"), "{m}");
    let (m, c) = rr3_cmd_rb_arm(&mk("0", "run x")).unwrap_err();
    assert_eq!(c, ExitCode::from(1), "零上限是业务拒");
    assert!(m.contains("bootlimit"), "{m}");
    let (m, c) = rr3_cmd_rb_arm(&mk("x", "run x")).unwrap_err();
    assert_eq!(c, ExitCode::from(2), "非数是用法错");
    assert!(m.contains("--limit") && m.contains("无符号整数"), "{m}");
    let no_env: Vec<String> = mk("3", "run x")[..4].to_vec();
    let (m, c) = rr3_cmd_rb_arm(&no_env).unwrap_err();
    assert_eq!(c, ExitCode::from(2), "缺 --env 是用法错");
    assert!(m.contains("--env"), "{m}");
    let missing = vec![
        "--limit".into(),
        "3".into(),
        "--altcmd".into(),
        "run x".into(),
        "--env".into(),
        dir.join("absent.config").to_string_lossy().into_owned(),
    ];
    let (m, c) = rr3_cmd_rb_arm(&missing).unwrap_err();
    assert_eq!(c, ExitCode::from(1), "配置读不到是业务面");
    assert!(m.contains("读 env 配置"), "{m}");
    let _ = std::fs::remove_dir_all(&dir);
}
// ---- OTA 签名/验签子命令（ADR-0026） ----
use rr_ota::sign;
const IMAGE: &[u8] = b"rr3-rootfs-test-image";
const SEED: [u8; 32] = [7u8; 32];
struct OtaFixture {
    dir: PathBuf,
    image: PathBuf,
    key: PathBuf,
    pubkey: PathBuf,
    manifest: PathBuf,
    sig: PathBuf,
}
impl OtaFixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rr3-ota-sign-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let image = dir.join("rr3-rootfs-rk3566-0.0.8.img");
        std::fs::write(&image, IMAGE).unwrap();
        let key = write_key(&dir.join("sign.key"), &SEED);
        Self {
            pubkey: dir.join("sign.pub"),
            manifest: dir.join("rr3-ota.manifest"),
            sig: dir.join("rr3-ota.sig"),
            dir,
            image,
            key,
        }
    }
    fn prepare(&self) {
        let key = self.key.to_string_lossy().into_owned();
        rr3_cmd_keypub(&[
            "--key-file".into(),
            key.clone(),
            "--out-pub".into(),
            self.pubkey.to_string_lossy().into_owned(),
        ])
        .expect("keypub");
        rr3_cmd_manifest(&[
            "--image".into(),
            self.image.to_string_lossy().into_owned(),
            "--platform".into(),
            "rk3566".into(),
            "--board".into(),
            "radxa-zero3".into(),
            "--version".into(),
            "0.0.8".into(),
            "--key-file".into(),
            key.clone(),
            "--out".into(),
            self.manifest.to_string_lossy().into_owned(),
        ])
        .expect("manifest");
        rr3_cmd_sign(&[
            "--manifest".into(),
            self.manifest.to_string_lossy().into_owned(),
            "--key-file".into(),
            key,
            "--sig-out".into(),
            self.sig.to_string_lossy().into_owned(),
        ])
        .expect("sign");
    }
    fn install_args(&self, target: &Path) -> Vec<String> {
        let mut args = self.verify_args();
        args.extend([
            "--target".into(),
            target.to_string_lossy().into_owned(),
            "--confirm".into(),
        ]);
        args
    }
    fn verify_args_pub_dir(&self, dir: &Path) -> Vec<String> {
        vec![
            "--manifest".into(),
            self.manifest.to_string_lossy().into_owned(),
            "--sig".into(),
            self.sig.to_string_lossy().into_owned(),
            "--pub-dir".into(),
            dir.to_string_lossy().into_owned(),
            "--platform".into(),
            "rk3566".into(),
            "--board".into(),
            "radxa-zero3".into(),
        ]
    }
    fn verify_args(&self) -> Vec<String> {
        vec![
            "--manifest".into(),
            self.manifest.to_string_lossy().into_owned(),
            "--sig".into(),
            self.sig.to_string_lossy().into_owned(),
            "--pub".into(),
            self.pubkey.to_string_lossy().into_owned(),
            "--platform".into(),
            "rk3566".into(),
            "--board".into(),
            "radxa-zero3".into(),
        ]
    }
}
impl Drop for OtaFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn write_key(path: &Path, seed: &[u8; 32]) -> PathBuf {
    std::fs::write(path, format!("{}\n", sign::rr3_hex_encode(seed))).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    path.to_path_buf()
}
/// 生成 → 签名 → 验签全链；镜像篡改必须拒。
#[test]
fn ota_sign_verify_flow_is_end_to_end() {
    let f = OtaFixture::new("flow");
    f.prepare();
    let mut args = f.verify_args();
    args.extend(["--current-version".into(), "0.0.7".into()]);
    let msg = rr3_cmd_verify(&args).expect("验签应通过");
    assert!(msg.contains("验签通过") && msg.contains("rk3566"), "{msg}");
    let text = std::fs::read(&f.manifest).unwrap();
    let m = sign::rr3_manifest_parse(&text).unwrap();
    assert_eq!(m.bytes, IMAGE.len() as u64);
    assert_eq!(m.key_id, sign::rr3_key_id(&sign::rr3_pubkey(&SEED)));
    std::fs::write(&f.image, [IMAGE, b"x"].concat()).unwrap();
    let (m, c) = rr3_cmd_verify(&args).unwrap_err();
    assert_eq!(c, ExitCode::from(1), "篡改镜像属校验失败");
    assert!(m.contains("镜像校验失败"), "{m}");
}
/// 用错私钥签名必须拒；私钥权限过宽必须拒。
#[test]
fn sign_rejects_wrong_key_and_loose_permissions() {
    let f = OtaFixture::new("wrong-key");
    f.prepare();
    let other = write_key(&f.dir.join("other.key"), &[9u8; 32]);
    let e = rr3_cmd_sign(&[
        "--manifest".into(),
        f.manifest.to_string_lossy().into_owned(),
        "--key-file".into(),
        other.to_string_lossy().into_owned(),
        "--sig-out".into(),
        f.dir.join("other.sig").to_string_lossy().into_owned(),
    ])
    .unwrap_err();
    assert_eq!(e.1, ExitCode::from(1));
    assert!(e.0.contains("key_id"), "{:?}", e.0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f.key, std::fs::Permissions::from_mode(0o644)).unwrap();
        let e = rr3_cmd_keypub(&["--key-file".into(), f.key.to_string_lossy().into_owned()])
            .unwrap_err();
        assert_eq!(e.1, ExitCode::from(1));
        assert!(e.0.contains("0600"), "{:?}", e.0);
    }
}
/// 降级门与开关互斥；platform/board 不匹配必须拒。
#[test]
fn verify_enforces_downgrade_and_target_policy() {
    let f = OtaFixture::new("policy");
    f.prepare();
    let mut args = f.verify_args();
    args.extend(["--current-version".into(), "0.0.8".into()]);
    let (m, c) = rr3_cmd_verify(&args).unwrap_err();
    assert_eq!(c, ExitCode::from(1));
    assert!(m.contains("拒绝降级"), "{m}");
    args.push("--allow-downgrade".into());
    rr3_cmd_verify(&args).expect("显式允许降级应通过");
    let e = rr3_cmd_verify(&[
        "--manifest".into(),
        f.manifest.to_string_lossy().into_owned(),
        "--sig".into(),
        f.sig.to_string_lossy().into_owned(),
        "--pub".into(),
        f.pubkey.to_string_lossy().into_owned(),
        "--allow-downgrade".into(),
    ])
    .unwrap_err();
    assert_eq!(e.1, ExitCode::from(2), "无 current 的降级开关是用法错");
    assert!(e.0.contains("--current-version"), "{:?}", e.0);
    let mut wrong = f.verify_args();
    wrong[7] = "raspberry-pi-4".into();
    wrong.extend(["--current-version".into(), "0.0.7".into()]);
    let (m, c) = rr3_cmd_verify(&wrong).unwrap_err();
    assert_eq!(c, ExitCode::from(1));
    assert!(m.contains("platform 不匹配"), "{m}");
}
/// --pub-dir 按 key_id 查公钥；缺键点名实际查找路径。
#[test]
fn pub_dir_resolves_key_id_and_names_missing_key() {
    let f = OtaFixture::new("pub-dir");
    f.prepare();
    let keys = f.dir.join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    let key_id = sign::rr3_hex_encode(&sign::rr3_key_id(&sign::rr3_pubkey(&SEED)));
    std::fs::copy(&f.pubkey, keys.join(format!("{key_id}.pub"))).unwrap();
    let mut args = f.verify_args_pub_dir(&keys);
    args.extend(["--current-version".into(), "0.0.7".into()]);
    rr3_cmd_verify(&args).expect("按 key_id 查公钥");
    let empty = f.dir.join("empty-keys");
    std::fs::create_dir_all(&empty).unwrap();
    let e = rr3_cmd_verify(&f.verify_args_pub_dir(&empty)).unwrap_err();
    assert_eq!(e.1, ExitCode::from(1));
    assert!(e.0.contains(&format!("{key_id}.pub")), "{:?}", e.0);
}
/// install：验签通过才落 staging 文件；缺 confirm、目录目标、篡改镜像都拒。
#[test]
fn install_writes_only_verified_staging_file() {
    let f = OtaFixture::new("install");
    f.prepare();
    let target = f.dir.join("staging.img");
    let mut args = f.install_args(&target);
    args.extend(["--current-version".into(), "0.0.7".into()]);
    let msg = rr3_cmd_install(&args).expect("验签后安装");
    assert!(msg.contains("安装完成"), "{msg}");
    assert_eq!(
        std::fs::read(&target).unwrap(),
        IMAGE,
        "目标字节必须等于镜像"
    );
    let mut no_confirm = f.verify_args();
    no_confirm.extend(["--target".into(), target.to_string_lossy().into_owned()]);
    let e = rr3_cmd_install(&no_confirm).unwrap_err();
    assert_eq!(e.1, ExitCode::from(2), "缺 --confirm 是用法错");
    assert!(e.0.contains("--confirm"), "{:?}", e.0);
    let dir_target = f.dir.join("target-dir");
    std::fs::create_dir_all(&dir_target).unwrap();
    let e = rr3_cmd_install(&f.install_args(&dir_target)).unwrap_err();
    assert_eq!(e.1, ExitCode::from(1));
    assert!(e.0.contains("不是普通文件"), "{:?}", e.0);
    std::fs::write(&f.image, [IMAGE, b"x"].concat()).unwrap();
    let bad_target = f.dir.join("bad-staging.img");
    let e = rr3_cmd_install(&f.install_args(&bad_target)).unwrap_err();
    assert_eq!(e.1, ExitCode::from(1));
    assert!(e.0.contains("镜像校验失败"), "{:?}", e.0);
    assert!(!bad_target.exists(), "验签失败不得落目标");
}
/// 文本输入有上界：超大私钥/manifest 在读取前拒，不把内存读穿。
#[test]
fn oversized_text_inputs_are_rejected() {
    let dir = std::env::temp_dir().join(format!("rr3-ota-sign-{}-big", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let big_key = dir.join("big.key");
    std::fs::write(&big_key, "a".repeat(8192)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&big_key, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let e =
        rr3_cmd_keypub(&["--key-file".into(), big_key.to_string_lossy().into_owned()]).unwrap_err();
    assert_eq!(e.1, ExitCode::from(1));
    assert!(e.0.contains("过大"), "{:?}", e.0);
    let big_manifest = dir.join("big.manifest");
    std::fs::write(&big_manifest, "a".repeat(8192)).unwrap();
    let e = rr3_cmd_sign(&[
        "--manifest".into(),
        big_manifest.to_string_lossy().into_owned(),
        "--key-file".into(),
        big_key.to_string_lossy().into_owned(),
        "--sig-out".into(),
        dir.join("x.sig").to_string_lossy().into_owned(),
    ])
    .unwrap_err();
    assert_eq!(e.1, ExitCode::from(1));
    assert!(e.0.contains("过大"), "{:?}", e.0);
    let _ = std::fs::remove_dir_all(&dir);
}
/// manifest 元数据不合法时在生成面拒，不产生半成品。
#[test]
fn manifest_rejects_bad_metadata() {
    let f = OtaFixture::new("metadata");
    let mk = |version: &str, platform: &str| -> Vec<String> {
        vec![
            "--image".into(),
            f.image.to_string_lossy().into_owned(),
            "--platform".into(),
            platform.into(),
            "--board".into(),
            "radxa-zero3".into(),
            "--version".into(),
            version.into(),
            "--key-file".into(),
            f.key.to_string_lossy().into_owned(),
            "--out".into(),
            f.manifest.to_string_lossy().into_owned(),
        ]
    };
    for (version, platform) in [("0.0", "rk3566"), ("0.0.8", "RK3566")] {
        let (m, c) = rr3_cmd_manifest(&mk(version, platform)).unwrap_err();
        assert_eq!(c, ExitCode::from(1), "非法元数据属业务拒");
        assert!(m.contains("拒绝生成 manifest"), "{m}");
        assert!(!f.manifest.exists(), "被拒不得留下 manifest");
    }
}
