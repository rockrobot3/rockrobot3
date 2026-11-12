//! 本地落盘原子序（全仓唯一实现，批N4 件四 I-20 下沉共享）。
//!
//! 写序：tmp 写全量 → `sync_all()` → 同目录 `rename` → 父目录 fsync（尽力而为）
//! → rename 失败清 tmp——断电卡在写中间时目标文件要么是旧全量、要么是新全量，
//! 不存在半截；目录 fsync 保证"新全量可见"在掉电后仍是事实（POSIX 崩溃一致
//! 性：只 fsync 文件不 fsync 目录，rename 可能在掉电后回旧态）。
//!
//! 为什么住在 rr-ipc：裁定（全案 §19.2 Y-2 / interact I-20，索引 L-26）——
//! **轴档案、域阈值、`armed`、凭据的落盘一律复用同一份原子序，出现第二条写
//! 路径即红**（L-23"新加一层没迁旧一层"在写路径上的形态）。原实现住
//! `rrd/src/counters.rs`，但凭据面在 modules/rr3-interact（独立 workspace，
//! 不依赖 daemon crate rrd，也不许依赖——层级倒挂）；rr-ipc 是双方既有的
//! 最低公共依赖层（evidence 先例同径），故原样迁此，rrd 侧 re-export 保持
//! 既有调用点（counters/axis）零改动。
//!
//! 权限语义：**tmp 建时即定权**（[`rr3_store_perm`]，unix `mode` 经
//! `OpenOptions::mode` 落位）——旧序"`File::create` 0644 建 tmp、rename 后
//! 再 chmod"在外部注入路径（父目录非 0700，如运维指定 `/data/rr3`）下留了
//! 同机他用户读 tmp 的窗口，崩溃残留时更是长期可读；敏感面（模型凭据 0600）
//! 由此不再依赖父目录护城河。非敏感面经 [`rr3_atomic_store`] 沿用 0644 档。
//!
//! 独占建语义：tmp 一律先清残留再 `create_new` 建（[`rr3_stage_excl`]）——
//! `create+truncate` 式开文件会跟着同机攻击者预置的符号链接写穿到链接目标
//! （root 侧写口 = 任意文件截断器）；独占建把"清→建"之间的换链竞态变成建失败
//! 而非写穿，崩溃残留也不再挡下一次写。
//!
//! 调用方清单（改动须同批过本注释）：
//! - `rrd/src/counters.rs`：运维计数账（R26 合并写窗口尾 flush）
//! - `rrd/src/axis.rs`：执行件标定档案（§4.4"不许出现第二条写路径"）
//! - `modules/rr3-brain/src/config_store.rs`：模型行改写（只存 api_key_ref，无密钥）
//! - `modules/rr3-interact/src/panel/secrets.rs`：模型凭据（建 tmp 即 0600）

// no_std crate 的 std 显式恢复（evidence.rs 同径写法）：本模块是 std::fs 文件面，
// 与 evidence 同门编译（linux/macos/windows；权限位在非 unix 忽略）。
use std::format;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::string::String;

/// 原子写序：tmp 写全量 → fsync → rename（同目录原子覆盖）。任何一步失败都
/// 把错误交回调用方（daemon/conn 落日志点名，secrets 折 502 应答），不 panic
/// 不重试——重试在窗口判据再次成立时自然发生（R26 合并写）。
pub fn rr3_atomic_store(path: &Path, tmp: &Path, body: &[u8]) -> Result<(), String> {
    rr3_store_perm(path, tmp, body, 0o644)
}

/// 独占建 tmp（机制单点）：先清上次崩溃残留，再 `create_new` 建——清与建之间
/// 被换成符号链接时建直接失败（`AlreadyExists`），绝不跟链写穿。建时即定权
/// （unix `mode`；非 unix 忽略），避免"建后 chmod"的读窗口。
fn rr3_tmp_open(tmp: &Path, mode: u32) -> std::io::Result<File> {
    let _ = std::fs::remove_file(tmp);
    #[cfg(not(unix))]
    let _ = mode;
    let mut opt = std::fs::OpenOptions::new();
    opt.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opt.mode(mode);
    }
    opt.open(tmp)
}

/// 两段式写的候选落盘（独占建 + 写全量，不 fsync 不 rename）：调用方要
/// "先落候选 → 过全判据 → 再原子换"时用（域写口的装载链预核、轴档案候选）。
/// 与 [`rr3_store_perm`] 同一独占建语义，候选档同样不给符号链接写穿面。
pub fn rr3_stage_excl(tmp: &Path, body: &[u8], mode: u32) -> Result<(), String> {
    let mut f = rr3_tmp_open(tmp, mode).map_err(|e| format!("建 {} 失败: {e}", tmp.display()))?;
    f.write_all(body)
        .map_err(|e| format!("写 {} 失败: {e}", tmp.display()))?;
    f.flush()
        .map_err(|e| format!("刷 {} 失败: {e}", tmp.display()))
}

/// 原子写序（显式权限版）：tmp **建时即**以 `mode` 落盘（unix；非 unix 忽略
/// `mode`），rename 原子替换后目标继承 tmp 权限——敏感面无须再二次 chmod。
pub fn rr3_store_perm(path: &Path, tmp: &Path, body: &[u8], mode: u32) -> Result<(), String> {
    {
        let mut f =
            rr3_tmp_open(tmp, mode).map_err(|e| format!("建 {} 失败: {e}", tmp.display()))?;
        f.write_all(body)
            .map_err(|e| format!("写 {} 失败: {e}", tmp.display()))?;
        f.sync_all()
            .map_err(|e| format!("fsync {} 失败: {e}", tmp.display()))?;
    }
    std::fs::rename(tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(tmp);
        format!("rename {} → {} 失败: {e}", tmp.display(), path.display())
    })?;
    // 目录 fsync（尽力而为）：POSIX 只保证文件 fsync 后内容在掉电后可见，
    // rename 的目录项持久化要父目录 fsync 才成立——不刷则掉电可能静默回旧值
    // （凭据/标定/计数全受影响）。失败不致命（tmpfs 等不支持时 EINVAL）：
    // 文件级保证仍在，比没有强；调用方无需感知。
    rr3_dir_sync(path);
    Ok(())
}

/// 父目录 fsync（尽力而为，失败静默）。
fn rr3_dir_sync(path: &Path) {
    if let Some(parent) = path.parent() {
        if let Ok(d) = File::open(parent) {
            let _ = d.sync_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_is_atomic_and_cleans_tmp_on_success() {
        let dir = std::env::temp_dir().join(format!("rr3-persist-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("t.json");
        let tmp = dir.join("t.json.tmp");
        rr3_atomic_store(&path, &tmp, b"{\"v\":1}").unwrap();
        // rename 成功 = tmp 不残留
        assert!(!tmp.exists());
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"v\":1}");
        // 覆盖写：目标先旧后新，无半截态可观察
        rr3_atomic_store(&path, &tmp, b"{\"v\":2}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"v\":2}");
        assert!(!tmp.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// tmp 位预置符号链接不许写穿：清残留后独占建新档，受害文件原封——root 侧
    /// 写口不得因同机可写目录里的链接变成任意文件截断器。
    #[cfg(unix)]
    #[test]
    fn store_does_not_follow_tmp_symlink() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("rr3-persist-link-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, "untouched").unwrap();
        let path = dir.join("t.json");
        let tmp = dir.join("t.json.tmp");
        symlink(&victim, &tmp).unwrap();
        rr3_atomic_store(&path, &tmp, b"{\"v\":1}").expect("独占建应换掉链接而不是写穿");
        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            "untouched",
            "链接目标不许被写"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"v\":1}");
        assert!(!tmp.exists(), "rename 后 tmp 不残留");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 两段式候选落盘：独占建 + 定权（0600），残留链接同样被换掉不写穿。
    #[cfg(unix)]
    #[test]
    fn stage_excl_is_symlink_safe_and_mode_restricted() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = std::env::temp_dir().join(format!("rr3-persist-stage-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, "untouched").unwrap();
        let link = dir.join("candidate.toml");
        symlink(&victim, &link).unwrap();
        rr3_stage_excl(&link, b"body", 0o600).expect("独占建必成");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "untouched");
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "body");
        let mode = std::fs::metadata(&link).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "候选档只许属主读写");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 显式权限版：目标文件（rename 自 tmp）落在 0600——敏感面的写窗口与崩溃
    /// 残留都不给同机他用户读的机会（umask 只会收紧不会放宽）。
    #[cfg(unix)]
    #[test]
    fn perm_store_lands_restricted_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rr3-persist-perm-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("s.json");
        let tmp = dir.join("s.json.tmp");
        rr3_store_perm(&path, &tmp, b"{\"k\":\"v\"}", 0o600).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "敏感落盘须 0600");
        assert!(!tmp.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
