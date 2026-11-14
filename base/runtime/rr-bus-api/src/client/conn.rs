//! 连接建立、注册、心跳线程与超时收尾。

use std::collections::{BTreeMap, HashMap};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};

/// 原子计数器型（task 序号 / JSON-RPC id 两处单调计数器的承载型，批N8 件三）：
/// 64 位平台走 AtomicU64；32 位目标（xtensa esp32 的 espidf 档 std 无 64 位原子）
/// 退 AtomicU32——两处都是进程内单调计数器，esp32 档生命周期内不触回绕面。
#[cfg(target_pointer_width = "64")]
type AtomicCounter = std::sync::atomic::AtomicU64;
#[cfg(not(target_pointer_width = "64"))]
type AtomicCounter = std::sync::atomic::AtomicU32;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rr_core::runtime::Rr3ModuleRuntime;
use rr_core::{rr3_frame_compose, RR3_TASK_RRD, RR3_TRACE_ID_DIRECT};
use rr_ipc::{
    opcode, rr3_body_encode, rr3_frame_write, Rr3Frame, Rr3PayloadType, RR3_BUS_PATH_ENV,
    RR3_FRAMING_PLEN_CAP_DEFAULT, RR3_UDS_PATH_DEFAULT,
};
use serde_json::{json, Value};

use super::parse::{rr3_entry_absolutize, rr3_manifest_ident, Rr3BusIdentity};
use super::sub::rr3_reader_loop;

/// 注册应答等待上界：注册是连接的第一件事，卡死在这里没有观察价值
const RR3_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// 心跳**连续失败拍数**上界（超限才退出交给 supervisor 裁定）。
///
/// 旧版单拍失败即 `process::exit(1)`：一次 5s RPC 超时（rrd 瞬时负载、GC/STW、
/// 调度抖动、对端短暂忙）就把整个模块杀掉重启——重启本身又要重注册，故障被
/// 放大成周期性抖动。心跳的语义是"我还活着"，短时失联应由下一拍自然修复；
/// 连丢 3 拍（≈3×心跳周期 + 3×5s 超时上界）才是真失联，此时退出仍由
/// restart_policy 接管。
const RR3_HEARTBEAT_MAX_MISSES: u32 = 3;

/// 收件箱（非应答帧）队列上界（帧数，0.0.8b 审计 B-6）。有界化把"主循环停摆的
/// 模块 OOM"折成"背压沿链上浮"：inbox 满 → reader 线程阻塞在 send → 不再读
/// socket → 内核缓冲满 → rrd 侧 writer 写超时（5s）→ B-5 慢消费者断投 →
/// supervisor 重启该模块。正常主循环每拍 `rr3_poll` 排空，256 帧对 125 帧/秒的
/// 音频流是 ~2s 余量；最坏驻留 256×~11KB ≈ 2.8MB/模块，可承受。
const RR3_INBOX_CAP: usize = 256;

/// 单调时基（0.0.5 批I A-2）：ADR-0018 裁定 3"单调时基"的 **std 驱动器侧供给**
/// ——`Instant` 锚点起的相对毫秒，墙钟回拨/跳变不影响拍点判定。旧实现用
/// `SystemTime` 却自称"单调"是误称，本型纠正；与 rrd 侧 watchdog 的 ms 语义
/// 同构（rrd 同样是进程内单调毫秒）。`rr3.heartbeat` 的 `timestamp_ns` 仍走
/// 墙钟（契约规定 CLOCK_REALTIME，见 [`rr3_now_ns`]）。
pub struct Rr3TimeBase {
    anchor: Instant,
}

impl Rr3TimeBase {
    /// 以当下为锚点新建
    pub fn rr3_time_base_new() -> Self {
        Self {
            anchor: Instant::now(),
        }
    }

    /// 锚点起的单调毫秒（机制核 [`Rr3ModuleRuntime`] 注入时钟的 std 供给；
    /// 模块业务线程的限速门/节拍同用此型，只看间隔不看绝对值）
    pub fn rr3_now_ms(&self) -> u64 {
        self.anchor.elapsed().as_millis() as u64
    }
}

pub(super) struct Rr3Inner {
    pub(super) task: AtomicCounter,
    pub(super) name: String,
    /// 写半边（reader 线程持有读半边的另一个句柄）
    pub(super) sock: Mutex<UnixStream>,
    /// JSON-RPC id → 应答邮箱（先注册后写帧，杜绝"应答先到邮箱不存在"的竞态）
    pub(super) pending: Mutex<HashMap<u64, SyncSender<Result<Value, String>>>>,
    /// 非应答帧（Event / Notification / 点名 Request）的去处；`Arc` 使 poll 取 &self
    pub(super) inbox: Mutex<Receiver<Rr3Frame>>,
    /// 运维计数表（0.0.4 批B，Q1 裁定）：模块业务线程经 [`Rr3BusClient::rr3_counter_bump`]
    /// 自愿累加，心跳线程每拍发「自上一拍以来」的**增量**（b3c 2026-10-21 语义钉死：
    /// rrd 归并侧按增量累加，聚合表 = 生命周期累计——旧实现每拍发全量快照被 rrd
    /// 当增量再收一遍，pcm_frames 跑出 40 亿量级）——非空则心跳 params 带
    /// `{"counters": {...}}`，空则维持 `{}`（旧 rrd 兼容）。键是模块内部运维词汇，
    /// 归属表落各模块 README。
    pub(super) counters: Mutex<BTreeMap<String, u64>>,
    /// 已发水位镜像（与 `counters` 同型）：键 → 上一拍已上线的累计值。心跳线程独占
    /// 读写；增量 = 当前值 − 水位（水位只会 ≤ 当前值，模块进程内计数单调），发出后
    /// 水位推进到当前值。心跳失败 = 模块 exit（见文件头），不存在"已记账未上线"
    /// 的重发语义——丢的最多是临死一拍的量，运维账可接受。
    pub(super) counters_sent: Mutex<BTreeMap<String, u64>>,
    /// 机制核运行时状态机（批D₂）：心跳拍点真值——std 驱动器只做平台事
    /// （sleep 等拍点、socket 收发），连接→注册→心跳→分发→节拍的决策在
    /// rr-core（ADR-0018 裁定 1/3）。锁内即放，无嵌套锁。
    pub(super) rt: Mutex<Rr3ModuleRuntime>,
    next_id: AtomicCounter,
    pub(super) stop: AtomicBool,
}

impl Rr3Inner {
    pub(super) fn rr3_write_frame(&self, mut frame: Rr3Frame) -> Result<(), String> {
        let plen = frame.payload.len();
        // 总线读侧按 1MiB 策略帽拒收（rr3_frame_read_cap）：超限时若仍把帧写进
        // socket，对端会直接断开，模块只看到"总线已断开"而不知道根因。写侧
        // 先 fail-closed，把可诊断的错误留在本进程。
        if plen > RR3_FRAMING_PLEN_CAP_DEFAULT {
            return Err(format!(
                "载荷 {plen}B 超总线帧上界 {RR3_FRAMING_PLEN_CAP_DEFAULT}B（rrd 读侧会拒收并断链）"
            ));
        }
        frame.header.payload_len = plen as u32;
        rr3_frame_write(
            &mut *self.sock.lock().unwrap_or_else(|p| p.into_inner()),
            &frame,
        )
        .map_err(|e| format!("写帧失败: {e:?}"))
    }

    /// 上行 Request 一次一答。先占邮箱再发帧：应答不可能落在邮箱就位前。
    pub(super) fn rr3_call_inner(
        &self,
        src_task: u64,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = self.rr3_next_id();
        let (tx, rx) = mpsc::sync_channel(1);
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, tx);
        let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let frame = rr3_frame_compose(
            opcode::SEND,
            0,
            RR3_TRACE_ID_DIRECT,
            rr3_now_ns(),
            src_task,
            RR3_TASK_RRD,
            rr3_body_encode(Rr3PayloadType::Request, body.to_string().as_bytes()),
        );
        if let Err(e) = self.rr3_write_frame(frame) {
            self.pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&id);
            return Err(e);
        }
        match rx.recv_timeout(timeout) {
            Ok(v) => v,
            Err(_) => {
                self.pending
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&id);
                Err(format!("RPC {method} 等应答超时（{timeout:?}）"))
            }
        }
    }

    // 64 位目标上 u64::from(u64) 是恒等转换，clippy 会报 useless_conversion；
    // 32 位（xtensa espidf，AtomicCounter=AtomicU32）上它是必需的出界面对齐。
    #[allow(clippy::useless_conversion)]
    fn rr3_next_id(&self) -> u64 {
        // 32 位目标（xtensa espidf）计数器退 AtomicU32，出界面对齐协议 u64——
        // 转换必须在本层做，调用方拿到的永远是宽口径。
        u64::from(self.next_id.fetch_add(1, Ordering::Relaxed)) + 1
    }

    /// 计数增量（心跳线程每拍调用）：对每键算「当前累计 − 已发水位」，零增量键
    /// 不上线（空闲模块的心跳维持 `{}` 旧形状）；发出后水位推进到当前值。两把锁
    /// 各自成段不嵌套——bump 只碰 `counters`，增量与水位只在心跳线程，竞态窗口
    /// 内的 bump 自然落到下一拍。
    fn rr3_counters_delta(&self) -> BTreeMap<String, u64> {
        let cur = self
            .counters
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let mut sent = self.counters_sent.lock().unwrap_or_else(|p| p.into_inner());
        let mut delta = BTreeMap::new();
        for (k, v) in &cur {
            let base = sent.get(k).copied().unwrap_or(0);
            // 模块进程内计数单调，水位只可能 ≤ 当前值；saturating 是防御形状
            let d = v.saturating_sub(base);
            if d > 0 {
                delta.insert(k.clone(), d);
            }
            sent.insert(k.clone(), *v);
        }
        delta
    }

    #[allow(clippy::useless_conversion)] // 同上：32 位目标必需
    pub(super) fn rr3_task(&self) -> u64 {
        u64::from(self.task.load(Ordering::Relaxed))
    }
}

/// 模块侧总线客户端。`Clone` 是同一连接的另一个句柄（Arc 共享）。
pub struct Rr3BusClient {
    pub(super) inner: Arc<Rr3Inner>,
}

impl Clone for Rr3BusClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Rr3BusClient {
    /// 一站式连接：读环境变量 → 取身份 → 绝对化 entry → 连接 → 注册 → 起心跳。
    /// 模块 main 的第一行就该是它；失败即返回 `Err`，由调用方决定退出。
    pub fn rr3_connect(manifest_text: &str) -> Result<Self, String> {
        let path =
            std::env::var(RR3_BUS_PATH_ENV).unwrap_or_else(|_| RR3_UDS_PATH_DEFAULT.to_string());
        let identity = rr3_manifest_ident(manifest_text)?;
        let exe = std::env::current_exe().map_err(|e| format!("取 current_exe 失败: {e}"))?;
        let wire = rr3_entry_absolutize(manifest_text, &exe)?;
        Self::rr3_connect_with(&path, identity, &wire)
    }

    /// 测试与特殊部署的注入口：路径、身份、线上 manifest 三者显式给定。
    pub(super) fn rr3_connect_with(
        path: &str,
        identity: Rr3BusIdentity,
        wire_manifest: &str,
    ) -> Result<Self, String> {
        Self::rr3_connect_timeout(path, identity, wire_manifest, RR3_CONNECT_TIMEOUT)
    }

    /// 连接本体：注册等待上界显式注入（生产走 [`RR3_CONNECT_TIMEOUT`]，单测
    /// 用短档钉"静默对端必须快速失败且 reader 线程被收掉"）。B9：连接 = 建面 →
    /// reader 上路 → 注册 → 心跳上路四段，段体各自成函数。
    pub(super) fn rr3_connect_timeout(
        path: &str,
        identity: Rr3BusIdentity,
        wire_manifest: &str,
        connect_timeout: Duration,
    ) -> Result<Self, String> {
        let stream = UnixStream::connect(path).map_err(|e| format!("连总线 {path} 失败: {e}"))?;
        let writer = stream
            .try_clone()
            .map_err(|e| format!("try_clone 失败: {e}"))?;
        let (tx, rx) = mpsc::sync_channel::<Rr3Frame>(RR3_INBOX_CAP);
        let inner = Arc::new(Rr3Inner {
            task: AtomicCounter::new(0),
            name: identity.name,
            sock: Mutex::new(writer),
            pending: Mutex::new(HashMap::new()),
            inbox: Mutex::new(rx),
            counters: Mutex::new(BTreeMap::new()),
            counters_sent: Mutex::new(BTreeMap::new()),
            rt: Mutex::new(Rr3ModuleRuntime::rr3_runtime_new(identity.heartbeat_ms)),
            next_id: AtomicCounter::new(0),
            stop: AtomicBool::new(false),
        });

        // reader 必须先于注册帧上路：注册应答也走 pending 路由
        let mut reader = Rr3ReaderGuard::rr3_start(stream, &inner, tx)?;
        let task = rr3_register(&inner, &mut reader, wire_manifest, connect_timeout)?;
        // 单调时基（批I A-2）：拍点全链（注册首拍 + 心跳判定）同一 Instant 锚点，
        // 墙钟回拨不再能扰动心跳节拍
        let tb = Rr3TimeBase::rr3_time_base_new();
        // 机制核状态机转移 Registering → Online，首拍 = 此刻 + heartbeat_ms
        inner
            .rt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_mark_registered(task, tb.rr3_now_ms());

        // 心跳线程：拍点真值在机制核状态机（相对节拍，逐拍 +hb）；std 驱动器
        // 只做平台事——sleep 等拍点、组装/发帧。起不来 = 这半套连接不可用：
        // 先收 reader（关 socket 解阻塞）再回错，不把调用进程挂在启动路径上。
        if let Err(e) = rr3_spawn_hb(&inner, tb) {
            reader.rr3_abort();
            return Err(format!("起心跳线程失败: {e}"));
        }
        Ok(Self { inner })
    }

    /// rrd 分配的本模块 task 号
    pub fn rr3_task(&self) -> u64 {
        self.inner.rr3_task()
    }
}

impl Drop for Rr3BusClient {
    fn drop(&mut self) {
        // 停心跳 + 关 socket 解阻塞 reader。不 join：Drop 不该等线程收尾，
        // reader 看到 EOF 自行退出并唤醒所有 pending。
        self.inner.stop.store(true, Ordering::Relaxed);
        let _ = self
            .inner
            .sock
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .shutdown(std::net::Shutdown::Both);
    }
}

/// 连接失败收尾：置 stop + shutdown 解 reader 的阻塞读，再 join。
/// 只置 stop（旧写法）不关 socket 时 reader 仍卡在 read 上，join 会把
/// "注册超时"拖成永久挂死——启动路径必须可失败、可观测。
/// reader 收尾守卫（B9）：连接建立期持有 reader 句柄，失败路径可显式收掉；
/// 成功路径直接析构 = 线程继续跑（Drop 不 join，语义与拆分前一致）。
struct Rr3ReaderGuard {
    inner: Arc<Rr3Inner>,
    handle: Option<JoinHandle<()>>,
}

impl Rr3ReaderGuard {
    fn rr3_start(
        stream: UnixStream,
        inner: &Arc<Rr3Inner>,
        tx: mpsc::SyncSender<Rr3Frame>,
    ) -> Result<Self, String> {
        let inner_c = inner.clone();
        let mut sock = stream;
        let handle = std::thread::Builder::new()
            .name("rr3-bus-reader".into())
            .spawn(move || rr3_reader_loop(&mut sock, inner_c, tx))
            .map_err(|e| format!("起 reader 线程失败: {e}"))?;
        Ok(Self {
            inner: inner.clone(),
            handle: Some(handle),
        })
    }

    fn rr3_abort(&mut self) {
        if let Some(h) = self.handle.take() {
            rr3_abort_reader(&self.inner, h);
        }
    }
}

/// 注册帧（B9 自连接体抽出）：注册前没有 task，按契约 src=0（"用户态不得自设"
/// 的客户端侧写法）；失败统一收 reader 并把错因上抛。
fn rr3_register(
    inner: &Arc<Rr3Inner>,
    reader: &mut Rr3ReaderGuard,
    wire_manifest: &str,
    timeout: Duration,
) -> Result<u64, String> {
    let reply = inner
        .rr3_call_inner(
            0,
            "rr3.register",
            json!({"name": inner.name, "manifest": wire_manifest}),
            timeout,
        )
        .map_err(|e| {
            reader.rr3_abort();
            format!("注册失败: {e}")
        })?;
    let task = reply
        .get("src_task")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            reader.rr3_abort();
            "注册应答缺 result.src_task".to_string()
        })?;
    inner.task.store(task as _, Ordering::Relaxed);
    Ok(task)
}

/// 心跳线程上路（B9 自连接体抽出）：线程体归 [`rr3_hb_loop`]，此处只做 spawn。
fn rr3_spawn_hb(inner: &Arc<Rr3Inner>, tb: Rr3TimeBase) -> std::io::Result<JoinHandle<()>> {
    let hb_inner = inner.clone();
    std::thread::Builder::new()
        .name("rr3-bus-heartbeat".into())
        .spawn(move || rr3_hb_loop(hb_inner, tb))
}

/// 心跳主循环（B9 自连接体抽出，语义逐字未动）：每拍先取计数快照——非空才带
/// `counters` 字段（可选 append，旧 rrd 忽略未知字段，双向兼容；空映射不带，
/// 保持与无上报模块同一条线上形状）。被拒/超时 = 连续失败到上界才退出，交给
/// supervisor 裁定（单拍抖动由下一拍自愈，见 [`RR3_HEARTBEAT_MAX_MISSES`]）。
fn rr3_hb_loop(hb_inner: Arc<Rr3Inner>, tb: Rr3TimeBase) {
    let mut misses: u32 = 0;
    loop {
        let Some(wait_ms) = hb_inner
            .rt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_beat_wait_ms(tb.rr3_now_ms())
        else {
            return; // 断开（Down）即停拍——"不装活着"同一裁定
        };
        std::thread::sleep(Duration::from_millis(wait_ms));
        if hb_inner.stop.load(Ordering::Relaxed) {
            return;
        }
        let now_ms = tb.rr3_now_ms();
        let due = hb_inner
            .rt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_heartbeat_due(now_ms);
        if !due {
            continue; // 提前醒（信号打断等），回睡
        }
        let delta = hb_inner.rr3_counters_delta();
        let params = if delta.is_empty() {
            json!({})
        } else {
            json!({ "counters": delta })
        };
        if hb_inner
            .rr3_call_inner(
                hb_inner.rr3_task(),
                "rr3.heartbeat",
                params,
                Duration::from_secs(5),
            )
            .is_err()
            && !hb_inner.stop.load(Ordering::Relaxed)
        {
            // 宽限：连续失败到上界才退出（见 RR3_HEARTBEAT_MAX_MISSES）。
            // 不记 `rr3_heartbeat_sent` → 下一拍立即到期重试，间隔由 RPC
            // 超时（5s）兜住，不空转。
            misses += 1;
            if misses >= RR3_HEARTBEAT_MAX_MISSES {
                eprintln!(
                    "rr3-bus: 心跳连续 {misses} 拍失败，标记总线死亡（supervisor 按 restart_policy 裁定）"
                );
                // 库内不 process::exit：设 stop 让宿主 serve 循环的
                // rr3_connected 死亡探针翻假，由宿主自行以 exit code 1
                // 退出（库不得杀宿主进程）。关 socket 解阻塞 reader
                // 与 pending 等待者，与 Drop 同形。
                hb_inner.stop.store(true, Ordering::Relaxed);
                let _ = hb_inner
                    .sock
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .shutdown(std::net::Shutdown::Both);
                break;
            }
            eprintln!(
                "rr3-bus: 心跳失败 第 {misses}/{RR3_HEARTBEAT_MAX_MISSES} 拍（不退出，下拍重试）"
            );
            continue;
        }
        // 成功即清零宽限计数
        misses = 0;
        hb_inner
            .rt
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rr3_heartbeat_sent(now_ms);
    }
}

fn rr3_abort_reader(inner: &Rr3Inner, reader: JoinHandle<()>) {
    inner.stop.store(true, Ordering::Relaxed);
    let _ = inner
        .sock
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .shutdown(std::net::Shutdown::Both);
    let _ = reader.join();
}

pub(super) fn rr3_now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}
