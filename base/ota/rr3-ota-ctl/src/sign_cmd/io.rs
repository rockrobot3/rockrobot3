// SPDX-License-Identifier: Apache-2.0
//! 签名子命令的文件读写面：私钥种子/权限、上界读取与原子落盘。
//!
//! 机制单点：私钥形制真值在 `rr-ota::sign`，原子写序在 `rr_ipc::persist`
//! （同目录 tmp → fsync → rename → 父目录 fsync）；本文件只做"带判据的读写"。
//!
//! 拆分动因：`sign_cmd.rs` 485 行，读写助手与子命令编排分属两簇。

use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use rr_ota::sign;

/// 私钥文件：32 字节种子的 64 位十六进制，unix 上必须 0600。
pub(super) fn rr3_read_seed(path: &Path) -> Result<[u8; 32], (String, ExitCode)> {
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
pub(super) fn rr3_key_perm_check(path: &Path) -> Result<(), (String, ExitCode)> {
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
pub(super) fn rr3_key_perm_check(_path: &Path) -> Result<(), (String, ExitCode)> {
    Ok(())
}

/// 文本小文件上界：种子 64 字节、公钥 64 字节、签名 128 字节，4096 足够且防 OOM。
const RR3_TEXT_FILE_MAX: u64 = 4096;

pub(super) fn rr3_read_trimmed(path: &Path, what: &str) -> Result<String, (String, ExitCode)> {
    let bytes = rr3_read_capped(path, what, RR3_TEXT_FILE_MAX)?;
    String::from_utf8(bytes)
        .map(|s| s.trim().to_string())
        .map_err(|e| (format!("{what} 不是 UTF-8：{e}"), ExitCode::from(1)))
}

pub(super) fn rr3_read_bytes(path: &Path, what: &str) -> Result<Vec<u8>, (String, ExitCode)> {
    rr3_read_capped(path, what, sign::RR3_OTA_MANIFEST_MAX as u64)
}

pub(super) fn rr3_read_capped(
    path: &Path,
    what: &str,
    max: u64,
) -> Result<Vec<u8>, (String, ExitCode)> {
    // 用 take(max+1) 而不是先 metadata 再 read：文件可在两次调用之间被换大，
    // 读取上界必须由同一次读取本身保证。
    let mut file = std::fs::File::open(path).map_err(|e| {
        (
            format!("读{what} {} 失败：{e}", path.display()),
            ExitCode::from(1),
        )
    })?;
    let mut buf = Vec::new();
    file.by_ref()
        .take(max.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| {
            (
                format!("读{what} {} 失败：{e}", path.display()),
                ExitCode::from(1),
            )
        })?;
    if buf.len() as u64 > max {
        return Err((
            format!("{what} {} 过大（> 上限 {max} 字节）", path.display()),
            ExitCode::from(1),
        ));
    }
    Ok(buf)
}

/// 同目录临时件 + 原子换名（复用 rr-ipc 的单一写序：独占建 → fsync → rename
/// → 父目录 fsync）。失败清临时件，不留半成品。
pub(super) fn rr3_write_atomic(path: &Path, bytes: &[u8]) -> Result<(), (String, ExitCode)> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("out");
    let tmp = parent.join(format!(".{name}.{}.rr3-tmp", std::process::id()));
    rr_ipc::persist::rr3_atomic_store(path, &tmp, bytes).map_err(|e| (e, ExitCode::from(1)))
}
