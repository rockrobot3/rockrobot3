//! OTA 制品签名/验签子命令（ADR-0026）。
//!
//! 机制全在 `rr-ota::sign`；本文件只做参数形制、文件读写与原子落盘。
//! 五个子命令都不缺省碰任何路径：私钥、manifest、签名、镜像、公钥、目标都必须显式给。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rr_ota::sign;

use crate::args::{rr3_flag, rr3_opt_arg, rr3_use, rr3_val_arg};

/// 签名面子命令分派：keypub / manifest / sign / verify / install。
pub(crate) fn rr3_dispatch(cmd: &str, args: &[String]) -> Result<String, (String, ExitCode)> {
    match cmd {
        "keypub" => rr3_cmd_keypub(args),
        "manifest" => rr3_cmd_manifest(args),
        "sign" => rr3_cmd_sign(args),
        "verify" => rr3_cmd_verify(args),
        "install" => rr3_cmd_install(args),
        other => Err(rr3_use(format!("未知签名子命令 {other:?}"))),
    }
}

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

/// manifest 的参数面（六个必填；解析与执行分离，主流程只做生成与落盘）。
struct Rr3ManifestArgs {
    image: PathBuf,
    platform: String,
    board: String,
    version: String,
    key: PathBuf,
    out: PathBuf,
}

fn rr3_manifest_args(args: &[String]) -> Result<Rr3ManifestArgs, (String, ExitCode)> {
    let image =
        rr3_val_arg(args, "--image", "manifest 必须显式传 --image <镜像路径>").map_err(rr3_use)?;
    let platform =
        rr3_val_arg(args, "--platform", "manifest 必须显式传 --platform").map_err(rr3_use)?;
    let board = rr3_val_arg(args, "--board", "manifest 必须显式传 --board").map_err(rr3_use)?;
    let version =
        rr3_val_arg(args, "--version", "manifest 必须显式传 --version").map_err(rr3_use)?;
    let key = rr3_val_arg(args, "--key-file", "manifest 必须显式传 --key-file").map_err(rr3_use)?;
    let out =
        rr3_val_arg(args, "--out", "manifest 必须显式传 --out <manifest 路径>").map_err(rr3_use)?;
    Ok(Rr3ManifestArgs {
        image: PathBuf::from(image),
        platform,
        board,
        version,
        key: PathBuf::from(key),
        out: PathBuf::from(out),
    })
}

/// manifest：从镜像与元数据生成严格 manifest（sha256/bytes/key_id 由本命令算）。
pub(crate) fn rr3_cmd_manifest(args: &[String]) -> Result<String, (String, ExitCode)> {
    let a = rr3_manifest_args(args)?;
    let seed = rr3_read_seed(&a.key)?;
    let (sha256, bytes) = sign::rr3_image_digest(&a.image)
        .map_err(|e| (format!("拒绝生成 manifest：{e}"), ExitCode::from(1)))?;
    let image_name = rr3_image_name(&a.image)?;
    let m = sign::Rr3OtaManifest {
        kind: sign::RR3_OTA_KIND_ROOTFS,
        platform: &a.platform,
        board: &a.board,
        version: &a.version,
        image: &image_name,
        bytes,
        sha256,
        key_id: sign::rr3_key_id(&sign::rr3_pubkey(&seed)),
    };
    let text = sign::rr3_manifest_render(&m);
    sign::rr3_manifest_parse(text.as_bytes())
        .map_err(|e| (format!("拒绝生成 manifest：{e}"), ExitCode::from(1)))?;
    rr3_write_atomic(&a.out, text.as_bytes())?;
    Ok(format!(
        "manifest 已生成：{}（image={image_name} bytes={bytes} sha256={} key_id={}）",
        a.out.display(),
        sign::rr3_hex_encode(&sha256),
        sign::rr3_hex_encode(&m.key_id)
    ))
}

/// 镜像文件名：manifest 只记单个路径分量，取不到合法文件名即拒。
fn rr3_image_name(image: &Path) -> Result<String, (String, ExitCode)> {
    image
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .ok_or_else(|| {
            (
                format!("镜像路径没有合法文件名：{image:?}"),
                ExitCode::from(1),
            )
        })
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

/// verify 的参数面（解析与校验分离，主流程只做验签与摘要核对）。
struct Rr3VerifyArgs {
    manifest: PathBuf,
    sig: PathBuf,
    pubkey: Option<PathBuf>,
    pub_dir: Option<PathBuf>,
    image: Option<PathBuf>,
    platform: Option<String>,
    board: Option<String>,
    current: Option<String>,
    allow_downgrade: bool,
}

fn rr3_verify_args(args: &[String]) -> Result<Rr3VerifyArgs, (String, ExitCode)> {
    let manifest = PathBuf::from(
        rr3_val_arg(args, "--manifest", "verify 必须显式传 --manifest").map_err(rr3_use)?,
    );
    let sig =
        PathBuf::from(rr3_val_arg(args, "--sig", "verify 必须显式传 --sig").map_err(rr3_use)?);
    let pubkey = rr3_opt_arg(args, "--pub")
        .map_err(rr3_use)?
        .map(PathBuf::from);
    let pub_dir = rr3_opt_arg(args, "--pub-dir")
        .map_err(rr3_use)?
        .map(PathBuf::from);
    if pubkey.is_some() == pub_dir.is_some() {
        return Err(rr3_use(
            "--pub 与 --pub-dir 必须二选一（单公钥文件，或按 key_id 查目录）".into(),
        ));
    }
    let image = rr3_opt_arg(args, "--image")
        .map_err(rr3_use)?
        .map(PathBuf::from);
    let platform = rr3_opt_arg(args, "--platform").map_err(rr3_use)?;
    let board = rr3_opt_arg(args, "--board").map_err(rr3_use)?;
    let current = rr3_opt_arg(args, "--current-version").map_err(rr3_use)?;
    let allow_downgrade = rr3_flag(args, "--allow-downgrade");
    if allow_downgrade && current.is_none() {
        return Err(rr3_use(
            "--allow-downgrade 只在同时给 --current-version 时有效".into(),
        ));
    }
    Ok(Rr3VerifyArgs {
        manifest,
        sig,
        pubkey,
        pub_dir,
        image,
        platform,
        board,
        current,
        allow_downgrade,
    })
}

/// 公钥路径：显式文件优先，否则按 manifest 的 key_id 在目录中查 `<key_id>.pub`。
fn rr3_pub_path(a: &Rr3VerifyArgs, m: &sign::Rr3OtaManifest<'_>) -> PathBuf {
    a.pubkey.clone().unwrap_or_else(|| {
        a.pub_dir
            .as_ref()
            .map(|d| d.join(format!("{}.pub", sign::rr3_hex_encode(&m.key_id))))
            .unwrap_or_default()
    })
}

/// 验签策略视图（verify / install 共用）。
fn rr3_expect(a: &Rr3VerifyArgs) -> sign::Rr3OtaExpect<'_> {
    sign::Rr3OtaExpect {
        platform: a.platform.as_deref(),
        board: a.board.as_deref(),
        current_version: a.current.as_deref(),
        allow_downgrade: a.allow_downgrade,
    }
}

/// 镜像路径：显式 `--image` 优先，否则按 manifest 同目录解析。
fn rr3_image_path(a: &Rr3VerifyArgs, m: &sign::Rr3OtaManifest<'_>) -> PathBuf {
    a.image.clone().unwrap_or_else(|| {
        a.manifest
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(m.image)
    })
}

/// verify：严格验签 + 目标兼容 + 可选镜像摘要核对；不写任何槽位。
pub(crate) fn rr3_cmd_verify(args: &[String]) -> Result<String, (String, ExitCode)> {
    let a = rr3_verify_args(args)?;
    let bytes = rr3_read_bytes(&a.manifest, "manifest")?;
    let sig = rr3_read_trimmed(&a.sig, "签名")?;
    let pre = sign::rr3_manifest_parse(&bytes)
        .map_err(|e| (format!("manifest 非法：{e}"), ExitCode::from(1)))?;
    let pub_path = rr3_pub_path(&a, &pre);
    let pub_hex = rr3_read_trimmed(&pub_path, "公钥")?;
    let m = sign::rr3_verify_signed(&bytes, &sig, &pub_hex)
        .map_err(|e| (format!("验签失败：{e}"), ExitCode::from(1)))?;
    sign::rr3_manifest_expect(&m, &rr3_expect(&a))
        .map_err(|e| (format!("目标/降级校验拒绝：{e}"), ExitCode::from(1)))?;
    let image = rr3_image_path(&a, &m);
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

/// install：验签通过后把镜像装到**普通文件** staging 目标；块设备写入归 M6。
pub(crate) fn rr3_cmd_install(args: &[String]) -> Result<String, (String, ExitCode)> {
    let a = rr3_verify_args(args)?;
    let target = PathBuf::from(
        rr3_val_arg(args, "--target", "install 必须显式传 --target").map_err(rr3_use)?,
    );
    if !rr3_flag(args, "--confirm") {
        return Err(rr3_use("install 会写目标文件，必须显式传 --confirm".into()));
    }
    let bytes = rr3_read_bytes(&a.manifest, "manifest")?;
    let sig = rr3_read_trimmed(&a.sig, "签名")?;
    let pre = sign::rr3_manifest_parse(&bytes)
        .map_err(|e| (format!("manifest 非法：{e}"), ExitCode::from(1)))?;
    let pub_path = rr3_pub_path(&a, &pre);
    let pub_hex = rr3_read_trimmed(&pub_path, "公钥")?;
    let m = sign::rr3_verify_signed(&bytes, &sig, &pub_hex)
        .map_err(|e| (format!("验签失败：{e}"), ExitCode::from(1)))?;
    sign::rr3_manifest_expect(&m, &rr3_expect(&a))
        .map_err(|e| (format!("目标/降级校验拒绝：{e}"), ExitCode::from(1)))?;
    let image = rr3_image_path(&a, &m);
    sign::rr3_image_check(&image, &m)
        .map_err(|e| (format!("镜像校验失败：{e}"), ExitCode::from(1)))?;
    rr3_install_file(&image, &target, &m)?;
    Ok(format!(
        "安装完成：{} → {}（version={} bytes={} key_id={}；目标为 staging 文件，块设备写入归 M6）",
        image.display(),
        target.display(),
        m.version,
        m.bytes,
        sign::rr3_hex_encode(&m.key_id)
    ))
}

/// 普通文件目标的原子落位：临时件 → 摘要复核 → fsync 临时件 → rename → fsync 父目录。
fn rr3_install_file(
    src: &Path,
    target: &Path,
    m: &sign::Rr3OtaManifest<'_>,
) -> Result<(), (String, ExitCode)> {
    if src == target {
        return Err(("拒绝安装：源镜像与目标路径相同".into(), ExitCode::from(1)));
    }
    if let Ok(md) = target.metadata() {
        if !md.file_type().is_file() {
            return Err((
                format!(
                    "拒绝安装：目标 {} 不是普通文件（块设备/槽位写入归 M6）",
                    target.display()
                ),
                ExitCode::from(1),
            ));
        }
    }
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err((
            format!("拒绝安装：目标目录不存在 {}", parent.display()),
            ExitCode::from(1),
        ));
    }
    let name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("target");
    let tmp = parent.join(format!(".{name}.{}.rr3-install", std::process::id()));
    let cleanup = |e: String| {
        let _ = std::fs::remove_file(&tmp);
        (e, ExitCode::from(1))
    };
    std::fs::copy(src, &tmp)
        .map_err(|e| cleanup(format!("复制到临时件 {} 失败：{e}", tmp.display())))?;
    sign::rr3_image_check(&tmp, m).map_err(|e| cleanup(format!("临时件摘要复核失败：{e}")))?;
    std::fs::File::open(&tmp)
        .and_then(|f| f.sync_all())
        .map_err(|e| cleanup(format!("fsync 临时件失败：{e}")))?;
    std::fs::rename(&tmp, target)
        .map_err(|e| cleanup(format!("安装落位 {} 失败：{e}", target.display())))?;
    // 目录项落盘走 rr-ipc 的单一机制；这里选**严格策略**：staging 的用途就是
    // 掉电后仍在，目录 fsync 失败就不能把"已安装"报成成功。
    rr_ipc::persist::rr3_sync_dir(target).map_err(|e| {
        (
            format!(
                "目录 fsync 失败（{}）：{e}——rename 已生效，但断电后目录项可能丢失",
                parent.display()
            ),
            ExitCode::from(1),
        )
    })
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

/// 文本小文件上界：种子 64 字节、公钥 64 字节、签名 128 字节，4096 足够且防 OOM。
const RR3_TEXT_FILE_MAX: u64 = 4096;

fn rr3_read_trimmed(path: &Path, what: &str) -> Result<String, (String, ExitCode)> {
    let bytes = rr3_read_capped(path, what, RR3_TEXT_FILE_MAX)?;
    String::from_utf8(bytes)
        .map(|s| s.trim().to_string())
        .map_err(|e| (format!("{what} 不是 UTF-8：{e}"), ExitCode::from(1)))
}

fn rr3_read_bytes(path: &Path, what: &str) -> Result<Vec<u8>, (String, ExitCode)> {
    rr3_read_capped(path, what, sign::RR3_OTA_MANIFEST_MAX as u64)
}

fn rr3_read_capped(path: &Path, what: &str, max: u64) -> Result<Vec<u8>, (String, ExitCode)> {
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
fn rr3_write_atomic(path: &Path, bytes: &[u8]) -> Result<(), (String, ExitCode)> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("out");
    let tmp = parent.join(format!(".{name}.{}.rr3-tmp", std::process::id()));
    rr_ipc::persist::rr3_atomic_store(path, &tmp, bytes).map_err(|e| (e, ExitCode::from(1)))
}

#[cfg(test)]
mod tests;
