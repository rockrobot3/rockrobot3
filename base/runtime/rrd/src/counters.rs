//! 运维计数聚合的持久化（0.0.4 批B，Q1/Q2 裁定；0.0.5 增补轮四 R26 改判落盘时机）。
//!
//! 聚合表（`registry.rs` 的 `RrdRegistry` 计数字段）的**断电尽量不丢**落点。
//! 裁定演进（诚实记录，不覆盖旧裁定原文）：
//! - 0.0.4 批B Q2：write-through 逐次刷——每次聚合变化即整文件原子写（tmp → 写全量
//!   → fsync → rename，与 OTA 同款断电安全序），否决周期刷（"断电丢尾部"）；
//! - 0.0.5 增补轮四 R26（批W3）：真板 eMMC 上高频小写 = 磨损 + I/O 抖动实证敞口
//!   （0.0.6 批F 系真板实测教训），改判**心跳合并写**——M 毫秒窗口至多一次 +
//!   N 拍兜底 + SIGTERM/优雅关停强制 flush（`daemon.rs` 的 term 路径）。落盘仍走
//!   同一条原子写序，数据面零改动，只改 flush 时机；代价 = 断电丢 ≤M 秒运维账
//!   （计数是运维账不是账本事实，裁定接受该窗口）。
//!
//! 旋钮（env，parse-or-default 口径同 rr-hal env 键——运维账旋钮不走 CLI fail-closed）：
//! - `RR3_COUNTERS_FLUSH_MS`：合并写窗口毫秒数（缺省 10_000——1s 心跳拍下落盘
//!   频次 60/min → ≤6/min，10× 削减）；
//! - `RR3_COUNTERS_FLUSH_MAX`：拍数兜底（缺省 64——高频心跳下 M 窗口拦不住的
//!   场景按拍数强制透一次）。
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
/// 重试在窗口判据再次成立时自然发生（R26 合并写）。
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

// ── R26 心跳合并写（0.0.5 增补轮四 批W3）────────────────────────────────────

/// 合并写窗口缺省 10_000ms（1s 心跳拍下 60 次/min → ≤6 次/min）
const RR3_COUNTERS_FLUSH_MS_DEFAULT: u64 = 10_000;
/// 拍数兜底缺省 64（高频心跳下 M 窗口拦不住，按拍数强制透一次）
const RR3_COUNTERS_FLUSH_MAX_DEFAULT: u32 = 64;

/// 合并写窗口毫秒数（env `RR3_COUNTERS_FLUSH_MS`，非法值/未设回缺省）
pub fn rr3_counters_flush_interval_ms() -> u64 {
    std::env::var("RR3_COUNTERS_FLUSH_MS")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(RR3_COUNTERS_FLUSH_MS_DEFAULT)
}

/// 拍数兜底（env `RR3_COUNTERS_FLUSH_MAX`，非法值/未设回缺省）
pub fn rr3_counters_flush_max_pending() -> u32 {
    std::env::var("RR3_COUNTERS_FLUSH_MAX")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(RR3_COUNTERS_FLUSH_MAX_DEFAULT)
}

/// 合并写节流闸门（R26）：只裁"何时落盘"，不动数据面（归并/快照仍在
/// `RrdRegistry`，落盘仍走同一条 [`rr3_counters_store`] 原子写序）。
/// 挂在 `Rr3ConnCtx` 里 `Arc<Mutex<_>>` **全连接共享**——每条连接各持一份
/// 等于把窗口放大 N 连接倍。时钟单位与在册表同源（进程起点相对毫秒）。
#[derive(Debug)]
pub struct Rr3CountersGate {
    interval_ms: u64,
    max_pending: u32,
    last_flush_ms: Option<u64>,
    pending: u32,
}

impl Rr3CountersGate {
    /// daemon 启动解析一次 env（conn 每拍不碰 getenv）
    pub fn rr3_new(interval_ms: u64, max_pending: u32) -> Self {
        Self {
            interval_ms,
            max_pending,
            last_flush_ms: None,
            pending: 0,
        }
    }

    /// 归并**有变化**的一拍调用：登记待落盘，回"此刻是否 flush"。
    /// 规则（R26）：①首拍即刷（冷启动第一笔变化不等窗口——既有 e2e
    /// "第一拍心跳落盘"的观测面由此保持）；②距上次落盘 ≥ 窗口即刷；
    /// ③待落盘拍数 ≥ 兜底即刷。
    pub fn rr3_on_change(&mut self, now_ms: u64) -> bool {
        self.pending = self.pending.saturating_add(1);
        match self.last_flush_ms {
            None => true,
            Some(last) => {
                now_ms.saturating_sub(last) >= self.interval_ms
                    || self.pending >= self.max_pending
            }
        }
    }

    /// 落盘成功后记账；失败**不**记账——窗口判据下一拍仍成立，自然重试
    pub fn rr3_on_flushed(&mut self, now_ms: u64) {
        self.last_flush_ms = Some(now_ms);
        self.pending = 0;
    }
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

    /// R26 裁定判据（逐字）：1s 心跳拍 1000 拍、窗口 M=10s → 落盘次数
    /// ≤ ceil(1000×间隔/M)+1 = 101（首拍即刷是那个 +1）。合并写的量化收益
    /// 就钉在这条上：1000 → 100 次 fsync 整文件写。
    #[test]
    fn gate_respects_window_across_thousand_heartbeats() {
        let mut g = Rr3CountersGate::rr3_new(10_000, 64);
        let mut flushes = 0;
        for beat in 0..1000u64 {
            if g.rr3_on_change(beat * 1000) {
                flushes += 1;
                g.rr3_on_flushed(beat * 1000);
            }
        }
        assert_eq!(flushes, 100, "10s 窗口 1000s 跨度应恰 100 次");
        assert!(
            flushes <= (1000 * 1000 + 10_000 - 1) / 10_000 + 1,
            "裁定判据上界 ceil(1000×间隔/M)+1"
        );
    }

    /// N 拍兜底：10ms 高频拍下 M 窗口（10s）拦不住，pending 攒到 64 强制透一次
    /// （首拍 + 每 64 拍一次 = ceil 语义；兜底路径与窗口路径独立成立）。
    #[test]
    fn gate_flushes_by_pending_floor_under_burst() {
        let mut g = Rr3CountersGate::rr3_new(10_000, 64);
        let mut flushes = 0;
        for beat in 0..1000u64 {
            if g.rr3_on_change(beat * 10) {
                flushes += 1;
                g.rr3_on_flushed(beat * 10);
            }
        }
        // 首拍（last=None）+ 此后每攒满 64 拍一次：(1000-1)/64 = 15 段 → 共 16
        assert_eq!(flushes, 16, "64 拍兜底应把 10ms 拍 × 1000 压到 16 次");
    }

    /// 落盘失败不记账：窗口判据下一拍仍成立（自然重试），成功才推进 last_flush
    #[test]
    fn gate_failed_flush_keeps_retrying_next_beat() {
        let mut g = Rr3CountersGate::rr3_new(10_000, 64);
        assert!(g.rr3_on_change(0), "首拍必刷");
        // 模拟 store 失败：不调 rr3_on_flushed → 下一拍仍在窗口内也照刷
        assert!(g.rr3_on_change(100), "失败后下一拍重试");
        g.rr3_on_flushed(100); // 这次成功
        assert!(!g.rr3_on_change(10_099), "窗口内不刷（last=100，窗口到 10_100）");
        assert!(g.rr3_on_change(10_100), "窗口到照刷");
        g.rr3_on_flushed(10_100);
        assert!(!g.rr3_on_change(20_099), "第二个窗口内不刷");
        assert!(g.rr3_on_change(20_100), "第二个窗口到照刷");
    }
}
