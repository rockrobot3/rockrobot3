// SPDX-License-Identifier: Apache-2.0
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
//! - 0.0.8 续轮十三：R26 的**执行面**也搬出心跳 reader 线程。原实现里窗口到点
//!   的心跳自己持全连接共享的 gate 锁做 tmp+fsync+rename——一次 eMMC/SD 抖动就把
//!   其余模块的心跳处理（含 `rr3_reg_beat` 判活记账）排在磁盘后面，慢过看门狗即
//!   误判死。现改为 reader 只投递"该落盘了"（`try_send` 非阻塞），快照与 fsync
//!   全在 `rrd-ctr-store` 单线程；`Rr3CountersGate` 增 `in_flight` 防重复排程，
//!   判活记账先于计数归并（心跳核心语义不被运维账推迟）。
//! - 0.0.8 续轮二十一：执行面的**存亡**也可读——`ctl.status.ctr_store` 三态
//!   （`off`/`on`/`dead`）。线程 panic 展开原本只有 panic hook 一行 stderr，
//!   运维面看不到"计数账本已经不再落盘"；现由收场守卫在展开时置位，起不来
//!   同样记 `dead`（启用过但执行面不在，不能读成"未启用"）。
//!
//! 旋钮（env，parse-or-default 口径同 rr-hal env 键——运维账旋钮不走 CLI fail-closed）：
//! - `RR3_COUNTERS_FLUSH_MS`：合并写窗口毫秒数（缺省 10_000——1s 心跳拍下落盘
//!   频次 60/min → ≤6/min，10× 削减）；
//! - `RR3_COUNTERS_FLUSH_MAX`：拍数兜底（缺省 64——高频心跳下 M 窗口拦不住的
//!   场景按拍数强制透一次）。
//!
//! 路径判定（[`rr3_path_with`]，纯函数便于测试）：
//! - env `RR3_DATA_DIR` 有值 → `$RR3_DATA_DIR/counters.json`（sim 侧注入口，
//!   装配单 / CI 显式给定）；
//! - env 无值 + cfg(linux) → 缺省 `/data/rr3/counters.json`（rr_data 分区挂载点，
//!   分区布局与OTA槽位.md 在册）；
//! - 其余（env 无值 + 非 linux）→ `None`：不持久化，启动日志一行点名，不报错。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use serde_json::{json, Value};

/// 进程级落盘互斥：心跳闸门（conn）与 SIGTERM flush 线程可并发写同一固定 tmp
/// 名——无互斥时 create/truncate/rename 交错会写出半截 JSON，下次 boot 起空账。
/// 原子写的"单写者"前提由本锁补上（其它写口各用独立 tmp，不受影响）。
#[cfg(unix)]
static RR3_COUNTERS_STORE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 异步落盘线程状态（`ctl.status.ctr_store`，续轮二十一）。三态词形与
/// `log_sink` 对齐：`off` = 未启用持久化或已随关停收场、`on` = 线程在位、
/// `dead` = 启用后执行面异常消失（起不来 / panic 展开）。没有这个位时，
/// "计数账本已经不再落盘"只能靠翻 panic 日志判断，运维面看不见。
static RR3_CTR_STORE_STATE: AtomicU8 = AtomicU8::new(RR3_CTR_OFF);

const RR3_CTR_OFF: u8 = 0;
const RR3_CTR_ON: u8 = 1;
const RR3_CTR_DEAD: u8 = 2;

fn rr3_ctr_store_mark(state: u8) {
    RR3_CTR_STORE_STATE.store(state, Ordering::Relaxed);
}

/// 状态读口（`ctl.status.ctr_store`；off/on/dead）
pub fn rr3_ctr_store_state() -> &'static str {
    match RR3_CTR_STORE_STATE.load(Ordering::Relaxed) {
        RR3_CTR_ON => "on",
        RR3_CTR_DEAD => "dead",
        _ => "off",
    }
}

/// `boots` 是 rrd 自有的计数键（启动自增，不走心跳），聚合表里的保留字面量
pub const RR3_COUNTERS_BOOTS_KEY: &str = "boots";

/// `conn_refused` 是 rrd 自有的计数键：并发槽满的累计拒连数。日志按 5s 窗口
/// 节流，这个键不节流——"现在有多少次拒连"必须能读到数，不能只看刷屏频率。
pub const RR3_COUNTERS_REFUSED_KEY: &str = "conn_refused";

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
    rr3_path_with(env.as_deref(), default).map(PathBuf::from)
}

/// 路径判定的纯函数核心（env 与平台缺省都显式传入，测试不碰进程 env）。
/// 空串 env 视同未设置（与线上入口 `rr3_counters_path` 的 filter 同一口径）。
pub fn rr3_path_with(env: Option<&str>, linux_default: Option<&str>) -> Option<String> {
    match env.filter(|s| !s.is_empty()) {
        Some(dir) => Some(rr3_ctr_path(dir)),
        // 非 linux（macOS sim 直跑）无 env 即不持久化；linux 缺省吃 rr_data 分区挂载点
        None => linux_default.map(rr3_ctr_path),
    }
}

/// `dir/counters.json`（lossy、owned）。
fn rr3_ctr_path(dir: &str) -> String {
    Path::new(dir)
        .join(COUNTERS_FILE)
        .to_string_lossy()
        .into_owned()
}

/// 启动加载：读全表。文件不存在 = 首启（空表）；坏文件按空表处理并回显原因
/// （计数账丢了不致命，守护必须照常起来——运维账不许挡产品路）。
pub fn rr3_counters_load(path: &Path) -> (BTreeMap<String, u64>, Option<String>) {
    let text = match rr3_counters_text(path) {
        Ok(t) => t,
        Err(reason) => return (BTreeMap::new(), reason),
    };
    rr3_counters_parse(&text)
}

/// 读计数档：不存在 = 首启（无原因），其余读错带回显原因。
fn rr3_counters_text(path: &Path) -> Result<String, Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(None),
        Err(e) => Err(Some(format!("读 {COUNTERS_FILE} 失败: {e}"))),
    }
}

/// 解析计数档：非对象/坏 JSON 按空表起账并回显原因。
fn rr3_counters_parse(text: &str) -> (BTreeMap<String, u64>, Option<String>) {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => (rr3_ctr_rows(map), None),
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

fn rr3_ctr_rows(map: serde_json::Map<String, Value>) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for (k, v) in map {
        if let Some(n) = v.as_u64() {
            out.insert(k, n);
        }
    }
    out
}

/// 原子写序共享件（批N4 件四 I-20 下沉）：实现原样迁 `rr-ipc/src/persist.rs`
/// （凭据面 modules/rr3-interact 不依赖 daemon crate，共享件须住双方公共最低层）。
/// 本 re-export 保持 axis.rs 既有 `use crate::counters::rr3_atomic_store` 路径
/// 零改动；全仓仍只有这一份实现——任何落盘写路径出现第二条同形序即红（I-20）。
/// `#[cfg(unix)]` 随 rr-ipc persist 模块门同径（批N9 件一 Windows 复跑时补钉：
/// 此前漏门被 rr-bus 红以"依赖失败 skip 不自报"掩盖，承载面契约化后浮出）。
#[cfg(unix)]
pub use rr_ipc::persist::rr3_atomic_store;

/// write-through 落盘：tmp 写全量 → fsync → rename（同目录原子覆盖），序本体见
/// [`rr3_atomic_store`]。调用面（conn/daemon flush 路径）整体 `#[cfg(unix)]`，
/// 本函数随写盘面同门。
#[cfg(unix)]
pub fn rr3_counters_store(path: &Path, table: &BTreeMap<String, u64>) -> Result<(), String> {
    let _one_writer = RR3_COUNTERS_STORE.lock().unwrap_or_else(|p| p.into_inner());
    let body = json!(table).to_string();
    let tmp = path.with_file_name(COUNTERS_TMP);
    rr3_atomic_store(path, &tmp, body.as_bytes())
        .map_err(|e| e.replace(&tmp.display().to_string(), COUNTERS_TMP))
        .map_err(|e| e.replace(&path.display().to_string(), COUNTERS_FILE))
}

// ── R26 心跳合并写（0.0.5 增补轮四 批W3）────────────────────────────────────

/// 合并写窗口缺省 10_000ms（1s 心跳拍下 60 次/min → ≤6 次/min）
const RR3_COUNTERS_FLUSH_MS_DEFAULT: u64 = 10_000;
/// 拍数兜底缺省 64（高频心跳下 M 窗口拦不住，按拍数强制透一次）
const RR3_COUNTERS_FLUSH_MAX_DEFAULT: u32 = 64;

/// 合并写窗口毫秒数（env `RR3_COUNTERS_FLUSH_MS`，非法值/未设回缺省）
pub fn rr3_flush_ms() -> u64 {
    std::env::var("RR3_COUNTERS_FLUSH_MS")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(RR3_COUNTERS_FLUSH_MS_DEFAULT)
}

/// 拍数兜底（env `RR3_COUNTERS_FLUSH_MAX`，非法值/未设回缺省）
pub fn rr3_flush_max() -> u32 {
    std::env::var("RR3_COUNTERS_FLUSH_MAX")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(RR3_COUNTERS_FLUSH_MAX_DEFAULT)
}

/// 合并写节流闸门（R26）：只裁"何时落盘"，不动数据面（归并/快照仍在
/// `RrdRegistry`，落盘仍走同一条 [`rr3_counters_store`] 原子写序）。
/// 挂在 `Rr3ConnCtx` 里 `Arc<Mutex<_>>` **全连接共享**——每条连接各持一份
/// 等于把窗口放大 N 连接倍。时钟单位与在册表同源（进程起点相对毫秒）。
///
/// 续轮十三起多一个 `in_flight`：落盘执行面搬到 `rrd-ctr-store` 线程后，
/// reader 侧的窗口判定与执行完成之间存在时间差；没有在飞位，落盘期间到达的
/// 每一拍都会再投一次"该落盘了"，连续 fsync 把节流打回逐拍写。
#[derive(Debug)]
pub struct Rr3CountersGate {
    interval_ms: u64,
    max_pending: u32,
    last_flush_ms: Option<u64>,
    pending: u32,
    /// 已投递、尚未回执（成功/失败）的一轮；在飞期间不再排新的一轮
    in_flight: bool,
}

impl Rr3CountersGate {
    /// daemon 启动解析一次 env（conn 每拍不碰 getenv）
    pub fn rr3_new(interval_ms: u64, max_pending: u32) -> Self {
        Self {
            interval_ms,
            max_pending,
            last_flush_ms: None,
            pending: 0,
            in_flight: false,
        }
    }

    /// 归并**有变化**的一拍调用：登记待落盘，回"此刻是否该排一轮落盘"。
    /// 规则（R26）：①首拍即刷（冷启动第一笔变化不等窗口——既有 e2e
    /// "第一拍心跳落盘"的观测面由此保持）；②距上次落盘 ≥ 窗口即刷；
    /// ③待落盘拍数 ≥ 兜底即刷。在飞的一轮没回执前一律不排（合并进同一轮）。
    pub fn rr3_on_change(&mut self, now_ms: u64) -> bool {
        self.pending = self.pending.saturating_add(1);
        !self.in_flight && self.rr3_flush_due(now_ms)
    }

    /// 投递成功后的排程记账：从此刻起至回执前不再排新的一轮。
    pub fn rr3_mark_inflight(&mut self) {
        self.in_flight = true;
    }

    /// flush 判据（R26 ②③）：首拍即刷，否则距上次落盘满窗口或兜底拍数到点。
    fn rr3_flush_due(&self, now_ms: u64) -> bool {
        match self.last_flush_ms {
            None => true,
            Some(last) => {
                now_ms.saturating_sub(last) >= self.interval_ms || self.pending >= self.max_pending
            }
        }
    }

    /// 落盘成功后记账；失败**不**记账——窗口判据下一拍仍成立，自然重试
    pub fn rr3_on_flushed(&mut self, now_ms: u64) {
        self.last_flush_ms = Some(now_ms);
        self.pending = 0;
        self.in_flight = false;
    }

    /// 落盘失败回执：清在飞位但**不**推进窗口（下一拍重试），pending 保留
    /// （兜底拍数已攒够的语义不因一次失败回退）。
    pub fn rr3_on_failed(&mut self) {
        self.in_flight = false;
    }
}

/// 异步落盘执行面的入口（续轮十三）：返回投递端；未启用持久化时回 None
/// （不落盘也就不需要线程）。线程起不来同样回 None 并点名——reader 侧不做
/// 任何伪装的同步写回退（那正是本轮要拆掉的阻塞面），关停路径的 SIGTERM
/// flush 仍在，持久化窗口退化为"关停一次"，点名让运维看得见。
/// 状态位（续轮二十一）在起线程成功/失败处分别定格 `on`/`dead`。
#[cfg(unix)]
pub fn rr3_ctr_store_spawn(
    shared: std::sync::Arc<std::sync::Mutex<crate::registry::RrdRegistry>>,
    gate: std::sync::Arc<std::sync::Mutex<Rr3CountersGate>>,
    path: PathBuf,
    t0: std::time::Instant,
) -> Option<std::sync::mpsc::SyncSender<()>> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<()>(1);
    let spawned = std::thread::Builder::new()
        .name("rrd-ctr-store".into())
        .spawn(move || rr3_ctr_store_loop(rx, &shared, &gate, &path, t0));
    match spawned {
        Ok(_) => {
            rr3_ctr_store_mark(RR3_CTR_ON);
            Some(tx)
        }
        Err(e) => {
            rr3_ctr_store_mark(RR3_CTR_DEAD);
            crate::rr3_log(&format!(
                "counters 异步落盘线程起不来（{e}）——本次仅内存账，关停 flush 仍兜底"
            ));
            None
        }
    }
}

/// 落盘线程主体：收到一次"该落盘了" → 取**当下**快照 → 原子写 → 回执 gate。
/// 快照克隆是短内存操作（≤16 模块 × 键数），fsync 在锁外——这正是把执行面
/// 搬出 reader 线程的意义。通道容量 1 + 发送侧 `try_send`：落盘期间到达的
/// 变化合并成一轮，不排队、不堆积。
#[cfg(unix)]
fn rr3_ctr_store_loop(
    rx: std::sync::mpsc::Receiver<()>,
    shared: &std::sync::Mutex<crate::registry::RrdRegistry>,
    gate: &std::sync::Mutex<Rr3CountersGate>,
    path: &Path,
    t0: std::time::Instant,
) {
    let mut run = Rr3CtrStoreRun { disarmed: false };
    for _ in rx.iter() {
        rr3_ctr_round_guard(gate, || rr3_ctr_store_round(shared, gate, path, t0));
    }
    run.disarmed = true;
    rr3_ctr_store_mark(RR3_CTR_OFF);
}

/// 单轮隔离（续轮二十二）：一轮 panic 不许带走整个执行面。panic 文案由 daemon 的
/// panic hook 落日志；这里只做恢复——释放在飞位（不释放的话 reader 侧永远不再排
/// 新一轮，账本静默停写）并让线程续跑下一轮。
#[cfg(unix)]
fn rr3_ctr_round_guard<F: FnOnce()>(gate: &std::sync::Mutex<Rr3CountersGate>, round: F) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(round)).is_err() {
        crate::rr3_lock(gate).rr3_on_failed();
        crate::rr3_log("counters 落盘一轮 panic（已隔离，线程续跑；原因见 panic hook 行）");
    }
}

/// 单轮落盘：取**当下**快照 → 原子写 → 回执 gate。快照克隆是短内存操作
/// （≤16 模块 × 键数），fsync 在锁外——这正是把执行面搬出 reader 线程的意义。
#[cfg(unix)]
fn rr3_ctr_store_round(
    shared: &std::sync::Mutex<crate::registry::RrdRegistry>,
    gate: &std::sync::Mutex<Rr3CountersGate>,
    path: &Path,
    t0: std::time::Instant,
) {
    let snap = crate::rr3_lock(shared).rr3_reg_counters();
    match rr3_counters_store(path, &snap) {
        Ok(()) => {
            let now = t0.elapsed().as_millis() as u64;
            crate::rr3_lock(gate).rr3_on_flushed(now);
        }
        Err(e) => {
            crate::rr3_lock(gate).rr3_on_failed();
            crate::rr3_log(&format!("counters 异步落盘失败（下一窗口自然重试）: {e}"));
        }
    }
}

/// 落盘线程收场守卫：正常关停（通道 EOF，即全连接 ctx 已释放）由主体 disarm
/// 后置 `off`；未 disarm 的收场只有 panic 展开一条路，置 `dead`。展开期间不做
/// 任何日志/加锁（可能二次 panic），panic 文案由 daemon 的 panic hook 负责。
struct Rr3CtrStoreRun {
    disarmed: bool,
}

impl Drop for Rr3CtrStoreRun {
    fn drop(&mut self) {
        if !self.disarmed {
            rr3_ctr_store_mark(RR3_CTR_DEAD);
        }
    }
}

#[cfg(test)]
mod tests;
