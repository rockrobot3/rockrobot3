// SPDX-License-Identifier: Apache-2.0
use super::*;

const SEED: [u8; 32] = [7u8; 32];
const IMAGE_BYTES: &[u8] = b"rr3-rootfs-test-image";

fn pubkey() -> [u8; 32] {
    rr3_pubkey(&SEED)
}

fn key_id() -> [u8; 8] {
    rr3_key_id(&pubkey())
}

fn manifest_text() -> String {
    let m = Rr3OtaManifest {
        kind: "rootfs",
        platform: "rk3566",
        board: "radxa-zero3",
        version: "0.0.8",
        image: "rr3-rootfs-rk3566-0.0.8.img",
        bytes: IMAGE_BYTES.len() as u64,
        sha256: Sha256::digest(IMAGE_BYTES).into(),
        key_id: key_id(),
    };
    rr3_manifest_render(&m)
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rr3-sign-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

#[test]
fn mft_byte_stable() {
    let text = manifest_text();
    let m = rr3_manifest_parse(text.as_bytes()).expect("合法 manifest");
    assert_eq!(rr3_manifest_render(&m), text);
    assert_eq!(m.key_id, key_id());
    assert_eq!(m.sha256, <[u8; 32]>::from(Sha256::digest(IMAGE_BYTES)));
}

#[test]
fn sign_then_roundtrip() {
    let text = manifest_text();
    let sig = rr3_sign(text.as_bytes(), &SEED);
    let sig_hex = rr3_hex_encode(&sig);
    let pub_hex = rr3_hex_encode(&pubkey());
    let m = rr3_verify_signed(text.as_bytes(), &sig_hex, &pub_hex).expect("验签应通过");
    assert_eq!(m.version, "0.0.8");
    assert_eq!(rr3_signature_parse(&sig_hex).unwrap(), sig);
    assert_eq!(rr3_pubkey_parse(&pub_hex).unwrap(), pubkey());
    assert_eq!(rr3_seed_parse(&rr3_hex_encode(&SEED)).unwrap(), SEED);
}

#[test]
fn tampered_mft_rej() {
    let text = manifest_text();
    let sig = rr3_hex_encode(&rr3_sign(text.as_bytes(), &SEED));
    let tampered = text.replace("version=0.0.8", "version=0.0.9");
    let e = rr3_verify_signed(tampered.as_bytes(), &sig, &rr3_hex_encode(&pubkey()))
        .expect_err("篡改 manifest 必须拒");
    assert!(e.contains("验签失败"), "{e}");
}

#[test]
fn wrong_key_key_id_rej() {
    let text = manifest_text();
    let sig = rr3_hex_encode(&rr3_sign(text.as_bytes(), &SEED));
    let other = rr3_pubkey(&[9u8; 32]);
    let e = rr3_verify_signed(text.as_bytes(), &sig, &rr3_hex_encode(&other))
        .expect_err("错误公钥必须拒");
    assert!(e.contains("key_id 不匹配"), "{e}");

    let mut m = rr3_manifest_parse(text.as_bytes()).unwrap();
    m.key_id = [0u8; 8];
    let text2 = rr3_manifest_render(&m);
    let sig2 = rr3_hex_encode(&rr3_sign(text2.as_bytes(), &SEED));
    let e = rr3_verify_signed(text2.as_bytes(), &sig2, &rr3_hex_encode(&pubkey()))
        .expect_err("manifest key_id 与公钥不符必须拒");
    assert!(e.contains("key_id 不匹配"), "{e}");
}

#[test]
fn bad_hex_rej() {
    let text = manifest_text();
    let pub_hex = rr3_hex_encode(&pubkey());
    for bad in ["", "00", &"0".repeat(126), &"G".repeat(128)] {
        let e = rr3_verify_signed(text.as_bytes(), bad, &pub_hex).expect_err("坏签名串必须拒");
        assert!(e.contains("signature"), "{e}");
    }
    let upper = "A".repeat(128);
    let e = rr3_verify_signed(text.as_bytes(), &upper, &pub_hex).expect_err("大写必须拒");
    assert!(e.contains("小写十六进制"), "{e}");
}

#[test]
fn strict_deformation() {
    let good = manifest_text();
    let cases = [
        good.replacen(RR3_OTA_MANIFEST_HEADER, "rr3-ota-manifest 2", 1),
        good.replace("kind=rootfs", "kind=module"),
        good.replace("platform=rk3566", "platform=RK3566"),
        good.replace("board=radxa-zero3", "board=radxa_zero3/x"),
        good.replace("version=0.0.8", "version=0.0"),
        good.replace("version=0.0.8", "version=00.0.8"),
        good.replace("image=rr3-rootfs-rk3566-0.0.8.img", "image=../evil.img"),
        good.replace("image=rr3-rootfs-rk3566-0.0.8.img", "image=a/b.img"),
        good.replace(
            &format!("bytes={}", IMAGE_BYTES.len()),
            &format!("bytes=0{}", IMAGE_BYTES.len()),
        ),
        good.replace("sha256=", "sha256=00"),
        good.replace("key_id=", "unknown="),
        good.replace("\nkey_id=", "\nkey_id=zz"),
        good.replace('\n', "\r\n"),
        format!("{good}\n"),
        good.replacen("board=", "boardx=", 1),
        good.replacen("kind=rootfs\n", "", 1),
        good.replacen("kind=rootfs", "kind=rootfs\nkind=rootfs", 1),
    ];
    for (i, text) in cases.iter().enumerate() {
        assert!(
            rr3_manifest_parse(text.as_bytes()).is_err(),
            "第 {i} 个变形必须拒: {text:?}"
        );
    }
    assert!(rr3_manifest_parse(&vec![b'a'; RR3_OTA_MANIFEST_MAX + 1]).is_err());
    assert!(rr3_manifest_parse(b"rr3-ota-manifest 1\n\xff\n").is_err());
}

#[test]
fn version_order_strict() {
    assert_eq!(
        rr3_version_parse("1.2.3").unwrap(),
        Rr3Version {
            major: 1,
            minor: 2,
            patch: 3
        }
    );
    assert_eq!(rr3_version_cmp("1.2.3", "1.2.3").unwrap(), Ordering::Equal);
    assert_eq!(rr3_version_cmp("1.2.3", "1.10.0").unwrap(), Ordering::Less);
    assert_eq!(
        rr3_version_cmp("2.0.0", "1.99.99").unwrap(),
        Ordering::Greater
    );
    for bad in [
        "1",
        "1.2",
        "1.2.3.4",
        "v1.2.3",
        "1.2.x",
        "01.2.3",
        "1.2.3-rc1",
    ] {
        assert!(rr3_version_parse(bad).is_err(), "{bad} 必须拒");
    }
}

fn expect_upgrade_ok(m: &Rr3OtaManifest) {
    rr3_manifest_expect(
        m,
        &Rr3OtaExpect {
            platform: Some("rk3566"),
            board: Some("radxa-zero3"),
            current_version: Some("0.0.7"),
            allow_downgrade: false,
        },
    )
    .expect("目标一致且升级");
}

fn expect_target_rejects(m: &Rr3OtaManifest) {
    let e = rr3_manifest_expect(
        m,
        &Rr3OtaExpect {
            platform: Some("raspberry-pi-4"),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(e.contains("platform 不匹配"), "{e}");

    let e = rr3_manifest_expect(
        m,
        &Rr3OtaExpect {
            board: Some("pi4b-board"),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(e.contains("board 不匹配"), "{e}");
}

fn expect_downgrade(m: &Rr3OtaManifest) {
    let e = rr3_manifest_expect(
        m,
        &Rr3OtaExpect {
            current_version: Some("0.0.8"),
            allow_downgrade: false,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(e.contains("拒绝降级"), "{e}");

    rr3_manifest_expect(
        m,
        &Rr3OtaExpect {
            current_version: Some("0.0.8"),
            allow_downgrade: true,
            ..Default::default()
        },
    )
    .expect("显式允许降级");
}

#[test]
fn expect_target_policy() {
    let text = manifest_text();
    let m = rr3_manifest_parse(text.as_bytes()).unwrap();
    expect_upgrade_ok(&m);
    expect_target_rejects(&m);
    expect_downgrade(&m);
}

#[test]
fn image_bound_mft() {
    let dir = tmp("image");
    let img = dir.join("rr3-rootfs-rk3566-0.0.8.img");
    std::fs::write(&img, IMAGE_BYTES).unwrap();
    let (sha, bytes) = rr3_image_digest(&img).unwrap();
    assert_eq!(bytes, IMAGE_BYTES.len() as u64);
    let m = Rr3OtaManifest {
        kind: "rootfs",
        platform: "rk3566",
        board: "radxa-zero3",
        version: "0.0.8",
        image: "rr3-rootfs-rk3566-0.0.8.img",
        bytes,
        sha256: sha,
        key_id: key_id(),
    };
    rr3_image_check(&img, &m).expect("摘要与字节数一致");

    let bad_size = Rr3OtaManifest {
        bytes: bytes + 1,
        ..m
    };
    let e = rr3_image_check(&img, &bad_size).unwrap_err();
    assert!(e.contains("字节数不符"), "{e}");

    let bad_sha = Rr3OtaManifest {
        sha256: [0u8; 32],
        ..m
    };
    let e = rr3_image_check(&img, &bad_sha).unwrap_err();
    assert!(e.contains("SHA-256 不符"), "{e}");

    let e = rr3_image_check(&dir.join("absent.img"), &m).unwrap_err();
    assert!(e.contains("打开镜像"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn key_id_stable() {
    assert_eq!(rr3_key_id(&pubkey()), key_id());
    assert_ne!(rr3_key_id(&pubkey()), rr3_key_id(&[9u8; 32]));
    assert_eq!(rr3_hex_encode(&key_id()).len(), 16);
}
