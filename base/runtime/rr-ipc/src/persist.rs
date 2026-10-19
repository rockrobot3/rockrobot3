//! 本地落盘原子序（全仓唯一实现，批N4 件四 I-20 下沉共享）。
//!
//! 写序：tmp 写全量 → `sync_all()` → 同目录 `rename` → rename 失败清 tmp——
//! 断电卡在写中间时目标文件要么是旧全量、要么是新全量，不存在半截。
//!
//! 为什么住在 rr-ipc：裁定（全案 §19.2 Y-2 / interact I-20，索引 L-26）——
//! **轴档案、域阈值、`armed`、凭据的落盘一律复用同一份原子序，出现第二条写
//! 路径即红**（L-23"新加一层没迁旧一层"在写路径上的形态）。原实现住
//! `rrd/src/counters.rs`，但凭据面在 modules/rr3-interact（独立 workspace，
//! 不依赖 daemon crate rrd，也不许依赖——层级倒挂）；rr-ipc 是双方既有的
//! 最低公共依赖层（evidence 先例同径），故原样迁此，rrd 侧 re-export 保持
//! 既有调用点（counters/axis）零改动。
//!
//! 权限语义：tmp 经 `File::create` 落盘，最终权限 = umask 后的常规档
//! （0644 典型）；敏感面（secrets 0600）由调用方在 rename 后收权——敏感
//! 读面的真正护城河是**父目录 0700**（同机他用户进不了目录，tmp 窗口
//! 同样被挡），文件收权是第二道。调用方清单（改动须同批过本注释）：
//! - `rrd/src/counters.rs`：运维计数账（R26 合并写窗口尾 flush）
//! - `rrd/src/axis.rs`：执行件标定档案（§4.4"不许出现第二条写路径"）
//! - `modules/rr3-interact/src/panel/secrets.rs`：模型凭据（rename 后收 0600）

// no_std crate 的 std 显式恢复（evidence.rs 同径写法）：本模块是 std::fs 文件面，
// 只在 linux/macos 门内编译（lib.rs 模块声明同径）。
use std::format;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::string::String;

/// 原子写序：tmp 写全量 → fsync → rename（同目录原子覆盖）。任何一步失败都
/// 把错误交回调用方（daemon/conn 落日志点名，secrets 折 502 应答），不 panic
/// 不重试——重试在窗口判据再次成立时自然发生（R26 合并写）。
pub fn rr3_atomic_store(path: &Path, tmp: &Path, body: &[u8]) -> Result<(), String> {
    {
        let mut f = File::create(tmp).map_err(|e| format!("建 {} 失败: {e}", tmp.display()))?;
        f.write_all(body)
            .map_err(|e| format!("写 {} 失败: {e}", tmp.display()))?;
        f.sync_all()
            .map_err(|e| format!("fsync {} 失败: {e}", tmp.display()))?;
    }
    std::fs::rename(tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(tmp);
        format!("rename {} → {} 失败: {e}", tmp.display(), path.display())
    })
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
}
