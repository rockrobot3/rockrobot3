//! OTA 制品签名/验签子命令（ADR-0026）。
//!
//! 机制全在 `rr-ota::sign`；本文件只做参数形制、文件读写与原子落盘。
//! 四个子命令都不缺省碰任何路径：私钥、manifest、签名、镜像、公钥都必须显式给。

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rr_ota::sign;

use crate::args::{rr3_flag, rr3_opt_arg, rr3_use, rr3_val_arg};

/// keypub：从种子派生公钥与 key_id（私钥不进任何输出）。
pub(crate) fn rr3_cmd_keypub(args: &[String]) -> Result<String, (String, ExitCode)> {
    let key = PathBuf::from(
        rr3_val_arg(args, "--key-file", "keypub 必须显式传 --key-file").map_err(rr3_use)?,
    );
    let seed = rr3_read_seed(&key)?;
    let pubkey = sign::rr3_pubkey(&seed);
    let pub_hex = sign::rr3_hex_encode(&pubkey);
    let key_id = sign::rr3_hex_encode(&sign::rr3_key_id(&pubkey));
    let out = rr3_opt_arg(args, "--out-pub").map_err(rr3_use)?;
    if let Some(path) = out {
        let path = PathBuf::from(path);
        rr3_write_atomic(&path, format!("{pub_hex}\n").as_bytes())?;
        Ok(format!("公钥已写出：{}（key_id={key_id}）", path.display()))
    } else {
        Ok(format!("pubkey={pub_hex} key_id={key_id}"))
    }
}

/// manifest：从镜像与元数据生成严格 manifest（sha256/bytes/key_id 由本命令算）。
pub(crate) fn rr3_cmd_manifest(args: &[String]) -> Result<String, (String, ExitCode)> {
    let image = PathBuf::from(
        rr3_val_arg(args, "--image", "manifest 必须显式传 --image <镜像路径>").map_err(rr3_use)?,
    );
    let platform =
        rr3_val_arg(args, "--platform", "manifest 必须显式传 --platform").map_err(rr3_use)?;
    let board = rr3_val_arg(args, "--board", "manifest 必须显式传 --board").map_err(rr3_use)?;
    let version =
        rr3_val_arg(args, "--version", "manifest 必须显式传 --version").map_err(rr3_use)?;
    let key = PathBuf::from(
        rr3_val_arg(args, "--key-file", "manifest 必须显式传 --key-file").map_err(rr3_use)?,
    );
    let out = PathBuf::from(
        rr3_val_arg(args, "--out", "manifest 必须显式传 --out <manifest 路径>").map_err(rr3_use)?,
    );
    let seed = rr3_read_seed(&key)?;
    let (sha256, bytes) = sign::rr3_image_digest(&image)
        .map_err(|e| (format!("拒绝生成 manifest：{e}"), ExitCode::from(1)))?;
    let image_name = image
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            (
                format!("镜像路径没有合法文件名：{image:?}"),
                ExitCode::from(1),
            )
        })?
        .to_string();
    let m = sign::Rr3OtaManifest {
        kind: sign::RR3_OTA_KIND_ROOTFS,
        platform: &platform,
        board: &board,
        version: &version,
        image: &image_name,
        bytes,
        sha256,
        key_id: sign::rr3_key_id(&sign::rr3_pubkey(&seed)),
    };
    let text = sign::rr3_manifest_render(&m);
    sign::rr3_manifest_parse(text.as_bytes())
        .map_err(|e| (format!("拒绝生成 manifest：{e}"), ExitCode::from(1)))?;
    rr3_write_atomic(&out, text.as_bytes())?;
    Ok(format!(
        "manifest 已生成：{}（image={image_name} bytes={bytes} sha256={} key_id={}）",
        out.display(),
        sign::rr3_hex_encode(&sha256),
        sign::rr3_hex_encode(&m.key_id)
    ))
}

/// sign：对 manifest 原始字节签名；manifest 的 key_id 必须与私钥派生值一致。
pub(crate) fn rr3_cmd_sign(args: &[String]) -> Result<String, (String, ExitCode)> {
    let manifest_path = PathBuf::from(
        rr3_val_arg(args, "--manifest", "sign 必须显式传 --manifest").map_err(rr3_use)?,
    );
    let key = PathBuf::from(
        rr3_val_arg(args, "--key-file", "sign 必须显式传 --key-file").map_err(rr3_use)?,
    );
    let sig_out = PathBuf::from(
        rr3_val_arg(args, "--sig-out", "sign 必须显式传 --sig-out").map_err(rr3_use)?,
    );
    let bytes = rr3_read_bytes(&manifest_path, "manifest")?;
    let m = sign::rr3_manifest_parse(&bytes)
        .map_err(|e| (format!("拒绝签名：manifest 非法：{e}"), ExitCode::from(1)))?;
    let seed = rr3_read_seed(&key)?;
    let pubkey = sign::rr3_pubkey(&seed);
    let key_id = sign::rr3_key_id(&pubkey);
    if key_id != m.key_id {
        return Err((
            format!(
                "拒绝签名：manifest key_id={} 与私钥派生 key_id={} 不一致",
                sign::rr3_hex_encode(&m.key_id),
                sign::rr3_hex_encode(&key_id)
            ),
            ExitCode::from(1),
        ));
    }
    let sig = sign::rr3_sign(&bytes, &seed);
    let sig_hex = sign::rr3_hex_encode(&sig);
    rr3_write_atomic(&sig_out, format!("{sig_hex}\n").as_bytes())?;
    Ok(format!(
        "签名已写出：{}（key_id={}，manifest={}）",
        sig_out.display(),
        sign::rr3_hex_encode(&key_id),
        manifest_path.display()
    ))
}

/// verify：严格验签 + 目标兼容 + 可选镜像摘要核对；不写任何槽位。
pub(crate) fn rr3_cmd_verify(args: &[String]) -> Result<String, (String, ExitCode)> {
    let manifest_path = PathBuf::from(
        rr3_val_arg(args, "--manifest", "verify 必须显式传 --manifest").map_err(rr3_use)?,
    );
    let sig_path =
        PathBuf::from(rr3_val_arg(args, "--sig", "verify 必须显式传 --sig").map_err(rr3_use)?);
    let pub_path =
        PathBuf::from(rr3_val_arg(args, "--pub", "verify 必须显式传 --pub").map_err(rr3_use)?);
    let image_arg = rr3_opt_arg(args, "--image").map_err(rr3_use)?;
    let platform = rr3_opt_arg(args, "--platform").map_err(rr3_use)?;
    let board = rr3_opt_arg(args, "--board").map_err(rr3_use)?;
    let current = rr3_opt_arg(args, "--current-version").map_err(rr3_use)?;
    let allow_downgrade = rr3_flag(args, "--allow-downgrade");
    if allow_downgrade && current.is_none() {
        return Err(rr3_use(
            "--allow-downgrade 只在同时给 --current-version 时有效".into(),
        ));
    }

    let bytes = rr3_read_bytes(&manifest_path, "manifest")?;
    let sig = rr3_read_trimmed(&sig_path, "签名")?;
    let pub_hex = rr3_read_trimmed(&pub_path, "公钥")?;
    let m = sign::rr3_verify_signed(&bytes, &sig, &pub_hex)
        .map_err(|e| (format!("验签失败：{e}"), ExitCode::from(1)))?;
    let expect = sign::Rr3OtaExpect {
        platform: platform.as_deref(),
        board: board.as_deref(),
        current_version: current.as_deref(),
        allow_downgrade,
    };
    sign::rr3_manifest_expect(&m, &expect)
        .map_err(|e| (format!("目标/降级校验拒绝：{e}"), ExitCode::from(1)))?;

    let image = image_arg.map(PathBuf::from).unwrap_or_else(|| {
        manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(m.image)
    });
    sign::rr3_image_check(&image, &m)
        .map_err(|e| (format!("镜像校验失败：{e}"), ExitCode::from(1)))?;
    Ok(format!(
        "验签通过：kind={} platform={} board={} version={} image={} bytes={} key_id={}（镜像摘要一致）",
        m.kind,
        m.platform,
        m.board,
        m.version,
        image.display(),
        m.bytes,
        sign::rr3_hex_encode(&m.key_id)
    ))
}

/// 私钥文件：32 字节种子的 64 位十六进制，unix 上必须 0600。
fn rr3_read_seed(path: &Path) -> Result<[u8; 32], (String, ExitCode)> {
    rr3_key_perm_check(path)?;
    let text = rr3_read_trimmed(path, "私钥")?;
    sign::rr3_seed_parse(&text).map_err(|e| {
        (
            format!("拒绝私钥 {}：{e}", path.display()),
            ExitCode::from(1),
        )
    })
}

#[cfg(unix)]
fn rr3_key_perm_check(path: &Path) -> Result<(), (String, ExitCode)> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
        .map_err(|e| {
            (
                format!("读私钥权限 {} 失败：{e}", path.display()),
                ExitCode::from(1),
            )
        })?
        .permissions()
        .mode()
        & 0o777;
    if mode & 0o077 != 0 {
        return Err((
            format!(
                "拒绝私钥 {}：权限 {mode:o} 过宽（必须 0600，组/其他不可读写）",
                path.display()
            ),
            ExitCode::from(1),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn rr3_key_perm_check(_path: &Path) -> Result<(), (String, ExitCode)> {
    Ok(())
}

fn rr3_read_trimmed(path: &Path, what: &str) -> Result<String, (String, ExitCode)> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        (
            format!("读{what} {} 失败：{e}", path.display()),
            ExitCode::from(1),
        )
    })?;
    Ok(text.trim().to_string())
}

fn rr3_read_bytes(path: &Path, what: &str) -> Result<Vec<u8>, (String, ExitCode)> {
    std::fs::read(path).map_err(|e| {
        (
            format!("读{what} {} 失败：{e}", path.display()),
            ExitCode::from(1),
        )
    })
}

/// 同目录临时文件 + rename；失败清临时件，不留半成品。
fn rr3_write_atomic(path: &Path, bytes: &[u8]) -> Result<(), (String, ExitCode)> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("out");
    let tmp = parent.join(format!(".{name}.{}.rr3-tmp", std::process::id()));
    let fail = |e: std::io::Error| {
        let _ = std::fs::remove_file(&tmp);
        (
            format!("写 {} 失败：{e}", path.display()),
            ExitCode::from(1),
        )
    };
    std::fs::write(&tmp, bytes).map_err(fail)?;
    std::fs::rename(&tmp, path).map_err(fail)
}
