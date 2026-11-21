//! OTA 制品签名与验签（ADR-0026）。
//!
//! 本模块只做三件事：严格解析/渲染行式 manifest、Ed25519 签名/验签、
//! 镜像摘要核对。文件读写边界在 `rr3-ota-ctl`，这里保持纯逻辑可对拍。
//!
//! manifest 是**签名对象本身**：签名覆盖 manifest 原始字节，image 由
//! `bytes`/`sha256` 两个字段约束。字段顺序固定、ASCII、LF 结尾；任何缺字段、
//! 重复、未知、CRLF、非 ASCII、超长都 fail-closed 拒。

use std::cmp::Ordering;
use std::fmt::Write as _;
use std::io::Read;
use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

/// manifest 首行（schema 1）。
pub const RR3_OTA_MANIFEST_HEADER: &str = "rr3-ota-manifest 1";

/// manifest 最大字节数：九个短字段的最坏长度远小于此值。
pub const RR3_OTA_MANIFEST_MAX: usize = 4096;

/// 本版只认整镜像 rootfs。
pub const RR3_OTA_KIND_ROOTFS: &str = "rootfs";

/// 严格解析后的 manifest 视图（字段借用源文本）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3OtaManifest<'a> {
    pub kind: &'a str,
    pub platform: &'a str,
    pub board: &'a str,
    pub version: &'a str,
    pub image: &'a str,
    pub bytes: u64,
    pub sha256: [u8; 32],
    pub key_id: [u8; 8],
}

/// 验签时的期望目标与降级策略。
#[derive(Debug, Clone, Copy, Default)]
pub struct Rr3OtaExpect<'a> {
    pub platform: Option<&'a str>,
    pub board: Option<&'a str>,
    pub current_version: Option<&'a str>,
    pub allow_downgrade: bool,
}

/// manifest 的五个字符串字段（顺序与文法绑定）。
struct Rr3Meta<'a> {
    kind: &'a str,
    platform: &'a str,
    board: &'a str,
    version: &'a str,
    image: &'a str,
}

/// 取五个字符串字段并做值域校验（kind 封闭词 / 词形 / 版本 / 文件名）。
fn rr3_meta_fields<'a>(lines: &[&'a str]) -> Result<Rr3Meta<'a>, String> {
    let kind = rr3_field(lines[1], "kind")?;
    let platform = rr3_field(lines[2], "platform")?;
    let board = rr3_field(lines[3], "board")?;
    let version = rr3_field(lines[4], "version")?;
    let image = rr3_field(lines[5], "image")?;
    rr3_manifest_meta(kind, platform, board, version, image)?;
    Ok(Rr3Meta {
        kind,
        platform,
        board,
        version,
        image,
    })
}

/// 解析 manifest：首行 + 八个固定字段，顺序不可变。
pub fn rr3_manifest_parse(bytes: &[u8]) -> Result<Rr3OtaManifest<'_>, String> {
    let lines = rr3_manifest_lines(bytes)?;
    let meta = rr3_meta_fields(&lines)?;
    let (bytes, sha256, key_id) = rr3_manifest_ids(
        rr3_field(lines[6], "bytes")?,
        rr3_field(lines[7], "sha256")?,
        rr3_field(lines[8], "key_id")?,
    )?;
    Ok(Rr3OtaManifest {
        kind: meta.kind,
        platform: meta.platform,
        board: meta.board,
        version: meta.version,
        image: meta.image,
        bytes,
        sha256,
        key_id,
    })
}

/// 数值/摘要字段：字节数、镜像 SHA-256、签发 key_id。
fn rr3_manifest_ids(
    bytes: &str,
    sha256: &str,
    key_id: &str,
) -> Result<(u64, [u8; 32], [u8; 8]), String> {
    Ok((
        rr3_u64_parse(bytes, "bytes")?,
        rr3_hex_decode(sha256, "sha256")?,
        rr3_hex_decode(key_id, "key_id")?,
    ))
}

/// 文本形制：只收 ASCII，且不得含 CR/NUL（统一 LF 结尾）。
fn rr3_text_check(text: &str) -> Result<(), String> {
    if !text.is_ascii() {
        return Err("manifest 只允许 ASCII".into());
    }
    if text.contains('\r') || text.contains('\0') {
        return Err("manifest 不得含 CR/NUL（统一 LF 结尾）".into());
    }
    Ok(())
}

/// 字节层形制（自 [`rr3_manifest_lines`] 抽出）：上限、UTF-8/ASCII、LF 结尾、
/// 无空行；返回去尾 LF 的正文。
fn rr3_manifest_body(bytes: &[u8]) -> Result<&str, String> {
    if bytes.len() > RR3_OTA_MANIFEST_MAX {
        return Err(format!(
            "manifest 过长（{} 字节 > 上限 {RR3_OTA_MANIFEST_MAX}）",
            bytes.len()
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|e| format!("manifest 不是 UTF-8: {e}"))?;
    rr3_text_check(text)?;
    let body = text
        .strip_suffix('\n')
        .ok_or_else(|| "manifest 必须以 LF 结尾".to_string())?;
    if body.ends_with('\n') {
        return Err("manifest 末尾不得有空行".into());
    }
    Ok(body)
}

/// 形制面：九行与首行（正文由 [`rr3_manifest_body`] 定型）。
fn rr3_manifest_lines(bytes: &[u8]) -> Result<Vec<&str>, String> {
    let body = rr3_manifest_body(bytes)?;
    let lines: Vec<&str> = body.split('\n').collect();
    if lines.len() != 9 {
        return Err(format!(
            "manifest 行数应为 9（首行 + 8 字段），实得 {}",
            lines.len()
        ));
    }
    if lines[0] != RR3_OTA_MANIFEST_HEADER {
        return Err(format!(
            "manifest 首行非法（期望 {RR3_OTA_MANIFEST_HEADER:?}，实得 {:?}）",
            lines[0]
        ));
    }
    Ok(lines)
}

/// 字段值域：kind 封闭词、平台/板型词形、版本三段、image 单路径分量。
fn rr3_manifest_meta(
    kind: &str,
    platform: &str,
    board: &str,
    version: &str,
    image: &str,
) -> Result<(), String> {
    if kind != RR3_OTA_KIND_ROOTFS {
        return Err(format!(
            "kind={kind:?} 不在本版支持面（只认 {RR3_OTA_KIND_ROOTFS:?}）"
        ));
    }
    rr3_token_check(platform, 32, "platform")?;
    rr3_token_check(board, 32, "board")?;
    rr3_version_parse(version)?;
    rr3_image_name_check(image)
}

/// 按固定字段序渲染 manifest（以 LF 结尾，签名就签这份字节）。
pub fn rr3_manifest_render(m: &Rr3OtaManifest<'_>) -> String {
    let mut out = String::with_capacity(320);
    let _ = writeln!(out, "{RR3_OTA_MANIFEST_HEADER}");
    let _ = writeln!(out, "kind={}", m.kind);
    let _ = writeln!(out, "platform={}", m.platform);
    let _ = writeln!(out, "board={}", m.board);
    let _ = writeln!(out, "version={}", m.version);
    let _ = writeln!(out, "image={}", m.image);
    let _ = writeln!(out, "bytes={}", m.bytes);
    let _ = writeln!(out, "sha256={}", rr3_hex_encode(&m.sha256));
    let _ = writeln!(out, "key_id={}", rr3_hex_encode(&m.key_id));
    out
}

/// 公钥的 key_id：SHA-256 前 8 字节。
pub fn rr3_key_id(pubkey: &[u8; 32]) -> [u8; 8] {
    let d = Sha256::digest(pubkey);
    let mut id = [0u8; 8];
    id.copy_from_slice(&d[..8]);
    id
}

/// 从 32 字节种子派生 Ed25519 公钥。
pub fn rr3_pubkey(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

/// 对 manifest 原始字节签名。
pub fn rr3_sign(manifest: &[u8], seed: &[u8; 32]) -> [u8; 64] {
    SigningKey::from_bytes(seed).sign(manifest).to_bytes()
}

/// 验签：先严格解析 manifest，再核对 key_id，最后做 Ed25519 严格验签。
pub fn rr3_verify_signed<'a>(
    manifest: &'a [u8],
    sig_hex: &str,
    pubkey_hex: &str,
) -> Result<Rr3OtaManifest<'a>, String> {
    let m = rr3_manifest_parse(manifest)?;
    let sig: [u8; 64] = rr3_hex_decode(sig_hex, "signature")?;
    let pubkey: [u8; 32] = rr3_hex_decode(pubkey_hex, "pubkey")?;
    if rr3_key_id(&pubkey) != m.key_id {
        return Err(format!(
            "key_id 不匹配（manifest={}，公钥派生={}）",
            rr3_hex_encode(&m.key_id),
            rr3_hex_encode(&rr3_key_id(&pubkey))
        ));
    }
    let vk =
        VerifyingKey::from_bytes(&pubkey).map_err(|e| format!("公钥不是合法 Ed25519 点: {e}"))?;
    let sig = Signature::from_bytes(&sig);
    vk.verify_strict(manifest, &sig)
        .map_err(|e| format!("Ed25519 验签失败: {e}"))?;
    Ok(m)
}

/// 目标兼容与降级策略。
pub fn rr3_manifest_expect(m: &Rr3OtaManifest<'_>, e: &Rr3OtaExpect<'_>) -> Result<(), String> {
    rr3_expect_identity("platform", m.platform, e.platform)?;
    rr3_expect_identity("board", m.board, e.board)?;
    rr3_expect_version(m, e)?;
    Ok(())
}

/// 身份字段期望：缺席不校验，在场必须相等（文案按字段名逐字对位）。
fn rr3_expect_identity(field: &str, got: &str, want: Option<&str>) -> Result<(), String> {
    if let Some(w) = want {
        if w != got {
            return Err(format!("{field} 不匹配（制品={got}，期望={w}）"));
        }
    }
    Ok(())
}

/// 降级门：在当前版本可解析且未显式允许降级时拒绝 `new <= cur`。
fn rr3_expect_version(m: &Rr3OtaManifest<'_>, e: &Rr3OtaExpect<'_>) -> Result<(), String> {
    let Some(raw) = e.current_version else {
        return Ok(());
    };
    let cur = rr3_version_parse(raw)?;
    let new = rr3_version_parse(m.version)?;
    if new <= cur && !e.allow_downgrade {
        return Err(format!(
            "拒绝降级（制品 {} ≤ 当前 {}；如确需降级须显式 --allow-downgrade）",
            m.version,
            cur_text(cur)
        ));
    }
    Ok(())
}

/// 校验镜像：字节数与 SHA-256 必须同时等于 manifest 声明值。
pub fn rr3_image_check(path: &Path, m: &Rr3OtaManifest<'_>) -> Result<(), String> {
    let (got, total) = rr3_image_scan(path, Some(m.bytes))?;
    if total != m.bytes {
        return Err(format!("镜像字节数不符（声明 {}，实得 {total}）", m.bytes));
    }
    if got != m.sha256 {
        return Err(format!(
            "镜像 SHA-256 不符（声明 {}，实得 {}）",
            rr3_hex_encode(&m.sha256),
            rr3_hex_encode(&got)
        ));
    }
    Ok(())
}

/// 计算镜像摘要与字节数（manifest 生成面用）。
pub fn rr3_image_digest(path: &Path) -> Result<([u8; 32], u64), String> {
    rr3_image_scan(path, None)
}

/// 单次流式扫描：`cap` 存在时超限即停，避免坏包把磁盘读穿。
fn rr3_image_scan(path: &Path, cap: Option<u64>) -> Result<([u8; 32], u64), String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("打开镜像 {path:?} 失败: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("读镜像 {path:?} 失败: {e}"))?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or_else(|| "镜像字节数溢出".to_string())?;
        if let Some(cap) = cap {
            if total > cap {
                return Err(format!("镜像字节数不符（{path:?} 已超过声明值 {cap}）"));
            }
        }
        hasher.update(&buf[..n]);
    }
    Ok((hasher.finalize().into(), total))
}

/// 32 字节种子 / 公钥的十六进制解析（必须 64 位小写十六进制）。
pub fn rr3_seed_parse(hex: &str) -> Result<[u8; 32], String> {
    rr3_hex_decode(hex, "seed")
}

/// 64 字节签名的十六进制解析（必须 128 位小写十六进制）。
pub fn rr3_signature_parse(hex: &str) -> Result<[u8; 64], String> {
    rr3_hex_decode(hex, "signature")
}

/// 公钥十六进制解析。
pub fn rr3_pubkey_parse(hex: &str) -> Result<[u8; 32], String> {
    rr3_hex_decode(hex, "pubkey")
}

/// 小写十六进制编码。
pub fn rr3_hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn rr3_field<'a>(line: &'a str, want: &str) -> Result<&'a str, String> {
    let (k, v) = line
        .split_once('=')
        .ok_or_else(|| format!("字段行缺 '='：{line:?}"))?;
    if k != want {
        return Err(format!(
            "字段顺序/名称不符：期望 {want:?}，实得 {k:?}（缺字段、重复或未知字段）"
        ));
    }
    if v.is_empty() {
        return Err(format!("字段 {want} 不得为空"));
    }
    if v.contains('=') {
        return Err(format!("字段 {want} 含多余 '='"));
    }
    Ok(v)
}

/// 词形字节：小写字母/数字/'-'/'_'。
fn rr3_token_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
}

fn rr3_token_check(s: &str, max: usize, what: &str) -> Result<(), String> {
    if s.is_empty() {
        return Err(format!("{what} 不得为空"));
    }
    if s.len() > max {
        return Err(format!("{what} 过长（{} > {max}）", s.len()));
    }
    let first = s.as_bytes()[0];
    let last = s.as_bytes()[s.len() - 1];
    if !first.is_ascii_alphanumeric() || !last.is_ascii_alphanumeric() {
        return Err(format!("{what} 必须以字母/数字开头和结尾: {s:?}"));
    }
    if !s.bytes().all(rr3_token_byte) {
        return Err(format!("{what} 只允许小写字母/数字/'-'/'_'（实得 {s:?}）"));
    }
    Ok(())
}

/// 镜像名允许字节：字母/数字/'.'/'-'/'_'（单一路径分量）。
fn rr3_image_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')
}

fn rr3_image_name_check(s: &str) -> Result<(), String> {
    if s.is_empty() {
        return Err("image 不得为空".into());
    }
    if s.len() > 128 {
        return Err(format!("image 名过长（{} > 128）", s.len()));
    }
    let first = s.as_bytes()[0];
    if !first.is_ascii_alphanumeric() {
        return Err(format!("image 必须以字母/数字开头: {s:?}"));
    }
    if s == "." || s.contains("..") {
        return Err(format!("image 不得含 '..'（路径穿越）: {s:?}"));
    }
    if !s.bytes().all(rr3_image_byte) {
        return Err(format!(
            "image 只允许单一路径分量（字母/数字/'.'/'-'/'_'）: {s:?}"
        ));
    }
    Ok(())
}

fn rr3_u64_parse(s: &str, what: &str) -> Result<u64, String> {
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("{what} 只收十进制无符号整数（实得 {s:?}）"));
    }
    if s.len() > 1 && s.starts_with('0') {
        return Err(format!("{what} 不得有前导零（实得 {s:?}）"));
    }
    s.parse::<u64>()
        .map_err(|e| format!("{what} 解析失败（{s:?}）: {e}"))
}

fn rr3_hex_decode<const N: usize>(s: &str, what: &str) -> Result<[u8; N], String> {
    if s.len() != N * 2 {
        return Err(format!(
            "{what} 必须是 {} 位小写十六进制（实得 {} 位）",
            N * 2,
            s.len()
        ));
    }
    let mut out = [0u8; N];
    for (i, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
        let hi = rr3_hex_nibble(chunk[0], what)?;
        let lo = rr3_hex_nibble(chunk[1], what)?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn rr3_hex_nibble(b: u8, what: &str) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        _ => Err(format!("{what} 只收小写十六进制（非法字节 0x{b:02x}）")),
    }
}

/// 严格三段式版本：`major.minor.patch`，无前导零、无预发布/构建元数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rr3Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

/// 解析版本；失败点名段位，不猜。
pub fn rr3_version_parse(s: &str) -> Result<Rr3Version, String> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return Err(format!("version 必须是 major.minor.patch（实得 {s:?}）"));
    }
    let mut nums = [0u64; 3];
    for (i, p) in parts.iter().enumerate() {
        nums[i] = rr3_u64_parse(p, ["major", "minor", "patch"][i])?;
    }
    Ok(Rr3Version {
        major: nums[0],
        minor: nums[1],
        patch: nums[2],
    })
}

fn cur_text(v: Rr3Version) -> String {
    format!("{}.{}.{}", v.major, v.minor, v.patch)
}

/// 版本比较（供调用方与测试共用）。
pub fn rr3_version_cmp(a: &str, b: &str) -> Result<Ordering, String> {
    Ok(rr3_version_parse(a)?.cmp(&rr3_version_parse(b)?))
}

#[cfg(test)]
mod tests;
