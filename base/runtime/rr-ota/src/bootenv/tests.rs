// SPDX-License-Identifier: Apache-2.0
//! bootenv 单测（文件头：本文件只放测试，生产面在 `bootenv.rs`）。
//!
//! 拆分动因：`bootenv.rs` 543 行、内联测试自 364 行起，先按仓内同形搬出。

use super::*;

/// 构造一份合法 raw env（4B CRC + 段 + 收尾 + 补齐）
fn rr3_env_build(vars: &[(&str, &str)], size: usize) -> Vec<u8> {
    let mut env = Rr3BootEnv::default();
    for (k, v) in vars {
        env.rr3_env_set(k, v).expect("测试键值必须合法");
    }
    env.rr3_env_render(size).expect("测试容量必须够")
}

#[test]
fn env_raw_bytes() {
    let raw = rr3_env_build(
        &[("bootcmd", "run distro_bootcmd"), ("bootcount", "0")],
        128,
    );
    let env = Rr3BootEnv::rr3_env_parse(&raw).expect("自产 env 必须可解析");
    assert_eq!(env.rr3_env_get("bootcmd"), Some("run distro_bootcmd"));
    assert_eq!(env.rr3_env_get("bootcount"), Some("0"));
    assert_eq!(env.rr3_env_get("absent"), None);
    // 重渲染逐字节等值（CRC 与补齐全确定性）
    assert_eq!(env.rr3_env_render(128).unwrap(), raw);
}

#[test]
fn env_crc_rej() {
    let mut raw = rr3_env_build(&[("a", "1")], 64);
    raw[10] ^= 0x01;
    let err = Rr3BootEnv::rr3_env_parse(&raw).expect_err("坏 CRC 必须拒");
    assert!(err.contains("CRC"), "{err}");
}

#[test]
fn env_bad_shapes_named() {
    // 过短
    assert!(Rr3BootEnv::rr3_env_parse(&[0, 0, 0, 0]).is_err());
    // 载荷缺收尾 \0：手工造 CRC 正确但不带收尾的区
    let mut payload = b"a=1\0".to_vec();
    payload.resize(16, b'x');
    let mut raw = rr3_crc32(&payload).to_le_bytes().to_vec();
    raw.extend_from_slice(&payload);
    let err = Rr3BootEnv::rr3_env_parse(&raw).expect_err("无收尾必须拒");
    assert!(err.contains("收尾"), "{err}");
    // 段缺 '='
    let mut payload = b"nokv\0\0".to_vec();
    payload.resize(16, 0);
    let mut raw = rr3_crc32(&payload).to_le_bytes().to_vec();
    raw.extend_from_slice(&payload);
    let err = Rr3BootEnv::rr3_env_parse(&raw).expect_err("缺 = 必须拒");
    assert!(err.contains('='), "{err}");
}

#[test]
fn env_key_rej() {
    let mut payload = b"a=1\0a=2\0\0".to_vec();
    payload.resize(32, 0);
    let mut raw = rr3_crc32(&payload).to_le_bytes().to_vec();
    raw.extend_from_slice(&payload);
    let err = Rr3BootEnv::rr3_env_parse(&raw).expect_err("重复键必须拒");
    assert!(err.contains("重复"), "{err}");
}

#[test]
fn env_set_idempotent() {
    let mut env = Rr3BootEnv::default();
    env.rr3_env_set("k", "1").unwrap();
    env.rr3_env_set("k", "2").unwrap();
    assert_eq!(env.rr3_env_get("k"), Some("2"));
    env.rr3_env_unset("k");
    env.rr3_env_unset("k"); // 幂等
    assert_eq!(env.rr3_env_get("k"), None);
    assert!(env.rr3_env_set("", "x").is_err());
    assert!(env.rr3_env_set("a=b", "x").is_err());
    assert!(env.rr3_env_set("k", "a\0b").is_err());
}

#[test]
fn env_rej_small_area() {
    let mut env = Rr3BootEnv::default();
    env.rr3_env_set("bootcmd", "run distro_bootcmd").unwrap();
    let err = env.rr3_env_render(8).expect_err("容量不够必须拒");
    assert!(err.contains("容量"), "{err}");
}

#[test]
fn rollback_ok_cycle() {
    let mut env = Rr3BootEnv::default();
    env.rr3_rollback_arm(3, "run rr3_altboot").unwrap();
    assert_eq!(env.rr3_env_get("upgrade_available"), Some("1"));
    assert_eq!(env.rr3_env_get("bootcount"), Some("0"));
    assert_eq!(env.rr3_env_get("bootlimit"), Some("3"));
    assert_eq!(env.rr3_env_get("altbootcmd"), Some("run rr3_altboot"));

    env.rr3_rollback_ok().unwrap();
    assert_eq!(env.rr3_env_get("upgrade_available"), Some("0"));
    assert_eq!(env.rr3_env_get("bootcount"), Some("0"));
    // 落点与上限保留（下次 arm 少写两笔、人工排障可见）
    assert_eq!(env.rr3_env_get("bootlimit"), Some("3"));
    assert_eq!(env.rr3_env_get("altbootcmd"), Some("run rr3_altboot"));

    // 非法 arm：空落点 / 零上限都拒
    assert!(env.rr3_rollback_arm(3, "  ").is_err());
    assert!(env.rr3_rollback_arm(0, "run x").is_err());
}

#[test]
fn env_keeps_entry() {
    // 覆盖已有键不得追加第二条（U-Boot 读到重复键行为未定义）
    let mut env = Rr3BootEnv::default();
    env.rr3_env_set("bootcount", "1").unwrap();
    env.rr3_rollback_arm(5, "run alt").unwrap();
    let raw = env.rr3_env_render(256).unwrap();
    let back = Rr3BootEnv::rr3_env_parse(&raw).unwrap();
    assert_eq!(back.rr3_env_get("bootcount"), Some("0"));
}

#[test]
fn cfg_ok_hex_comments() {
    let text = "# 板侧 env 布局\n/dev/mmcblk0 0x400000 0x8000\n\
                /dev/mtd1 0x0 0x2000 # 尾注释\n";
    let devs = rr3_env_cfg_parse(text).expect("合法 config");
    assert_eq!(devs.len(), 2);
    assert_eq!(devs[0].rr3_dev_path(), "/dev/mmcblk0");
    assert_eq!(devs[0].rr3_dev_offset(), 0x40_0000);
    assert_eq!(devs[0].rr3_dev_size(), 0x8000);
    assert_eq!(devs[1].rr3_dev_size(), 0x2000);
}

/// 区读写真路径（unix）：临时文件顶替块设备，验证定位偏移、整区覆盖、
/// 区外字节不动、读回逐键等值——不是"只编过"，是这一份 fs 代码真跑过。
#[cfg(unix)]
#[test]
fn env_dev_through_file() {
    use std::io::Write;
    let pid = std::process::id();
    let ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("rr3-env-{pid}-{ns}.bin"));
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0xAAu8; 16]).unwrap(); // 区前哨兵
        f.write_all(&[0u8; 128]).unwrap(); // env 区（全 0 = 坏载体）
        f.write_all(&[0xBBu8; 8]).unwrap(); // 区后哨兵
    }
    let dev = Rr3EnvDev {
        device: path.to_string_lossy().into_owned(),
        offset: 16,
        size: 128,
    };
    let err = rr3_env_read(&dev).expect_err("全 0 区必须拒读（CRC 不符）");
    assert!(err.contains("CRC") || err.contains("收尾"), "{err}");

    let mut env = Rr3BootEnv::default();
    env.rr3_rollback_arm(3, "run alt").unwrap();
    rr3_env_write(&dev, &env).unwrap();
    let back = rr3_env_read(&dev).unwrap();
    assert_eq!(back.rr3_env_get("altbootcmd"), Some("run alt"));
    assert_eq!(back.rr3_env_get("bootlimit"), Some("3"));

    let raw = std::fs::read(&path).unwrap();
    assert_eq!(raw.len(), 152);
    assert!(raw[..16].iter().all(|b| *b == 0xAA), "区前字节被改写");
    assert!(raw[144..].iter().all(|b| *b == 0xBB), "区后字节被改写");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn cfg_rej_bad_lines() {
    assert!(rr3_env_cfg_parse("").is_err(), "空册必须拒");
    assert!(rr3_env_cfg_parse("# only comments\n").is_err());
    assert!(rr3_env_cfg_parse("/dev/x 0\n").is_err(), "字段不足");
    assert!(rr3_env_cfg_parse("/dev/x zz 0x10\n").is_err(), "非数");
    assert!(rr3_env_cfg_parse("/dev/x 0 4\n").is_err(), "容量过小");
}
