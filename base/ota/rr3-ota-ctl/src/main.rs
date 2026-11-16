//! rr3-ota-ctl —— OTA 槽位查看/切换、回滚链与制品签名 CLI。
//!
//! 子命令：
//!   slots                                打印在册槽位与对合关系（读侧，不碰文件）
//!   switch <rr_a|rr_b> --bootconf P      改写引导配置 root=LABEL 做持久 A/B 切换
//!   factory            --bootconf P      根定位切 rr_factory 做一次性出厂引导
//!   rollback arm --limit N --altcmd C --env P
//!                                        U-Boot 回滚链上膛（四变量）
//!   rollback ok        --env P           回滚链解除（两变量）
//!
//! 判定与改写逻辑全在 `rr-ota`（`bootconf` / `bootenv`，base/runtime workspace 内，
//! 随七步门 ③ 全量跑测试）；本 CLI 只做参数解析与落盘调用，且**不缺省碰任何
//! 路径**——四个写命令必须显式给 `--bootconf` / `--env`。退出码：0 成功 /
//! 1 业务拒或 IO 失败 / 2 用法错。
//!
//! `sign` / `verify` / `manifest` / `keypub` 的机制在 `rr-ota::sign`（ADR-0026），
//! 本文件只保留分派与 usage；参数面在 `args`，子命令面在 `bootconf_cmd` /
//! `sign_cmd`。

mod args;
mod bootconf_cmd;
mod sign_cmd;

use std::process::ExitCode;

use rr_ota::{Rr3Slot, RR3_OTA_STAGING_PARTITION};

const ALL_SLOTS: [Rr3Slot; 3] = [Rr3Slot::SlotA, Rr3Slot::SlotB, Rr3Slot::Factory];

/// slots：只读打印，不碰任何路径。
fn rr3_print_slots() {
    for slot in ALL_SLOTS {
        println!(
            "{}  peer={}",
            slot.rr3_slot_label(),
            slot.rr3_slot_peer().rr3_slot_label()
        );
    }
    println!("{RR3_OTA_STAGING_PARTITION}  (模块级暂存，无对侧)");
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match rr3_dispatch(&argv) {
        Ok(msg) => {
            if !msg.is_empty() {
                println!("{msg}");
            }
            ExitCode::SUCCESS
        }
        Err((msg, code)) => {
            eprintln!("{msg}");
            code
        }
    }
}

/// 两级分派：槽位/引导面进 bootconf_cmd，制品签名面进 sign_cmd；未知子命令一次点名。
fn rr3_dispatch(argv: &[String]) -> Result<String, (String, ExitCode)> {
    match argv.first().map(String::as_str) {
        Some("slots") => {
            rr3_print_slots();
            Ok(String::new())
        }
        Some(cmd @ ("switch" | "factory" | "rollback")) => {
            bootconf_cmd::rr3_dispatch(cmd, &argv[1..])
        }
        Some(cmd @ ("keypub" | "manifest" | "sign" | "verify")) => {
            sign_cmd::rr3_dispatch(cmd, &argv[1..])
        }
        Some(other) => Err(args::rr3_use(format!(
            "未知子命令 {other:?}（在册：slots / switch / factory / rollback / keypub / manifest / sign / verify）"
        ))),
        None => Err(args::rr3_use(
            "缺子命令（在册：slots / switch / factory / rollback / keypub / manifest / sign / verify）"
                .into(),
        )),
    }
}

#[cfg(test)]
mod tests {
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
            let dir =
                std::env::temp_dir().join(format!("rr3-ota-sign-{}-{tag}", std::process::id()));
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
        let e = rr3_cmd_keypub(&["--key-file".into(), big_key.to_string_lossy().into_owned()])
            .unwrap_err();
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
}
