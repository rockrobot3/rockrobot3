//! 运维计数聚合的持久化（0.0.4 批B，Q1/Q2 裁定）。
//!
//! 聚合表（`registry.rs` 的 `RrdRegistry` 计数字段）的**断电不丢**落点：
//! write-through 逐次刷——每次聚合变化即整文件原子写（tmp → 写全量 → fsync →
//! rename，与 OTA 同款断电安全序），计数低频（≤每秒几条），与 legacy NVS 逐次
//! commit 的"断电最多丢当前一条"语义等价。周期刷被 Q2 显式否决：断电丢尾部
//! 违背"断电不丢"。
//!
//! 路径判定（[`rr3_counters_path_with`]，纯函数便于测试）：
//! - env `RR3_DATA_DIR` 有值 → `$RR3_DATA_DIR/counters.json`（sim 侧注入口，
//!   装配单 / CI 显式给定）；
//! - env 无值 + cfg(linux) → 缺省 `/data/rr3/counters.json`（rr_data 分区挂载点，
//!   partition-layout.md 在册）；
//! - 其余（env 无值 + 非 linux）→ `None`：不持久化，启动日志一行点名，不报错。

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// `boots` 是 rrd 自有的计数键（启动自增，不走心跳），聚合表里的保留字面量
pub const RR3_COUNTERS_BOOTS_KEY: &str = "boots";

/// 持久化文件名（固定挂在数据目录下）
const COUNTERS_FILE: &str = "counters.json";
/// 原子写序的临时文件名（同目录，rename 原子覆盖）
const COUNTERS_TMP: &str = "counters.json.tmp";

/// 线上入口：读 env（cfg(linux) 缺省 `/data/rr3`），判定结果只有"路径"与"不持久化"两态
pub fn rr3_counters_path() -> Option<PathBuf> {
    let env = std::env::var("RR3_DATA_DIR").ok();
    #[cfg(target_os = "linux")]
    let default: Option<&str> = Some("/data/rr3");
    #[cfg(not(target_os = "linux"))]
    let default: Option<&str> = None;
    rr3_counters_path_with(env.as_deref(), default).map(PathBuf::from)
}

/// 路径判定的纯函数核心（env 与平台缺省都显式传入，测试不碰进程 env）。
/// 空串 env 视同未设置（与线上入口 `rr3_counters_path` 的 filter 同一口径）。
pub fn rr3_counters_path_with(env: Option<&str>, linux_default: Option<&str>) -> Option<String> {
    match env.filter(|s| !s.is_empty()) {
        Some(dir) => Some(
            Path::new(dir)
                .join(COUNTERS_FILE)
                .to_string_lossy()
                .into_owned(),
        ),
        // 非 linux（macOS sim 直跑）无 env 即不持久化；linux 缺省吃 rr_data 分区挂载点
        None => linux_default.map(|d| {
            Path::new(d)
                .join(COUNTERS_FILE)
                .to_string_lossy()
                .into_owned()
        }),
    }
}

/// 启动加载：读全表。文件不存在 = 首启（空表）；坏文件按空表处理并回显原因
/// （计数账丢了不致命，守护必须照常起来——运维账不许挡产品路）。
pub fn rr3_counters_load(path: &Path) -> (BTreeMap<String, u64>, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (BTreeMap::new(), None),
        Err(e) => {
            return (
                BTreeMap::new(),
                Some(format!("读 {COUNTERS_FILE} 失败: {e}")),
            )
        }
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => {
            let mut out = BTreeMap::new();
            for (k, v) in map {
                if let Some(n) = v.as_u64() {
                    out.insert(k, n);
                }
            }
            (out, None)
        }
        other => (
            BTreeMap::new(),
            Some(format!(
                "{COUNTERS_FILE} 不是 JSON 对象，按空表起账（读到 {} 字节）",
                {
                    let _ = other;
                    text.len()
                }
            )),
        ),
    }
}

/// write-through 落盘：tmp 写全量 → fsync → rename（同目录原子覆盖）。
/// 任何一步失败都把错误交回调用方（daemon/conn 落日志点名），不 panic 不重试——
/// 重试在下一拍心跳自然发生（write-through 的逐次性）。
pub fn rr3_counters_store(path: &Path, table: &BTreeMap<String, u64>) -> Result<(), String> {
    let body = json!(table).to_string();
    let tmp = path.with_file_name(COUNTERS_TMP);
    {
        let mut f = File::create(&tmp).map_err(|e| format!("建 {COUNTERS_TMP} 失败: {e}"))?;
        f.write_all(body.as_bytes())
            .map_err(|e| format!("写 {COUNTERS_TMP} 失败: {e}"))?;
        f.sync_all()
            .map_err(|e| format!("fsync {COUNTERS_TMP} 失败: {e}"))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename {COUNTERS_TMP} → {COUNTERS_FILE} 失败: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rrd-counters-t-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 路径判定三态：env 优先、linux 缺省、皆无不持久化
    #[test]
    fn path_decision_prefers_env_then_platform_default() {
        assert_eq!(
            rr3_counters_path_with(Some("/tmp/rr3data"), Some("/data/rr3")),
            Some("/tmp/rr3data/counters.json".to_string()),
            "env 有值时优先于平台缺省"
        );
        assert_eq!(
            rr3_counters_path_with(None, Some("/data/rr3")),
            Some("/data/rr3/counters.json".to_string()),
            "env 无值吃平台缺省（linux rr_data 分区）"
        );
        assert_eq!(rr3_counters_path_with(None, None), None, "皆无不持久化");
        // 空 env 等价于无 env（线上入口 filter 掉）
        assert_eq!(rr3_counters_path_with(Some(""), None), None);
    }

    /// 存取 round-trip：键序确定（BTreeMap）、值逐键一致
    #[test]
    fn store_then_load_round_trips() {
        let dir = tmpdir("round");
        let path = dir.join(COUNTERS_FILE);
        let mut t = BTreeMap::new();
        t.insert("rr3-brain/chat_rounds".to_string(), 7);
        t.insert("rr3-act/relay_on".to_string(), 3);
        t.insert(RR3_COUNTERS_BOOTS_KEY.to_string(), 2);
        rr3_counters_store(&path, &t).expect("写盘应成功");
        assert!(
            !dir.join(COUNTERS_TMP).exists(),
            "原子写序走完不得留 tmp 残留"
        );
        let (loaded, note) = rr3_counters_load(&path);
        assert_eq!(loaded, t, "round-trip 逐键一致");
        assert!(note.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 原子写序覆盖旧值：rename 之后读到的是新表，不存在半新半旧
    #[test]
    fn store_overwrites_atomically() {
        let dir = tmpdir("over");
        let path = dir.join(COUNTERS_FILE);
        let mut t1 = BTreeMap::new();
        t1.insert("a".to_string(), 1);
        t1.insert("b".to_string(), 2);
        rr3_counters_store(&path, &t1).unwrap();
        let mut t2 = BTreeMap::new();
        t2.insert("a".to_string(), 5);
        rr3_counters_store(&path, &t2).unwrap();
        let (loaded, _) = rr3_counters_load(&path);
        assert_eq!(loaded.len(), 1, "全量覆盖不是合并");
        assert_eq!(loaded["a"], 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 加载的容错：首启（不存在）= 空表无怨言；坏文件 = 空表 + 原因
    #[test]
    fn load_tolerates_missing_and_corrupt_files() {
        let dir = tmpdir("tol");
        let (t, note) = rr3_counters_load(&dir.join("nope.json"));
        assert!(t.is_empty() && note.is_none(), "首启静默起空账");
        let bad = dir.join(COUNTERS_FILE);
        std::fs::write(&bad, b"{not json").unwrap();
        let (t, note) = rr3_counters_load(&bad);
        assert!(t.is_empty());
        let note = note.expect("坏文件必须点原因");
        assert!(note.contains("不是 JSON 对象"), "{note}");
        // 非对象 JSON 同拒
        std::fs::write(&bad, b"[1,2]").unwrap();
        let (_, note) = rr3_counters_load(&bad);
        assert!(note.is_some());
        // 非整数值跳过，不毒化整表
        std::fs::write(&bad, br#"{"ok": 3, "bad": "x", "neg": -1}"#).unwrap();
        let (t, note) = rr3_counters_load(&bad);
        assert!(note.is_none());
        assert_eq!(t.len(), 1);
        assert_eq!(t["ok"], 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
