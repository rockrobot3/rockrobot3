//! 模块侧同步总线客户端（M3，architecture.md §6）。
//!
//! rr-bus 的文档声明落点（lib.rs 头注）：总线的**常量层 + 同步客户端**。客户端是
//! 模块与 rrd 之间唯一的标准说话方式：连接 → 注册（manifest 全文，entry 绝对化）
//! → 心跳线程 → 发布 / 点名调用 / 收帧。
//!
//! 并发形状（与 rrd 的 conn.rs 同构、方向相反）：reader 线程独占读；写半边由
//! `Mutex<UnixStream>` 保护（一帧一次 `write_all`，整帧原子）。**应答与广播在同一
//! socket 上的相对次序不保证**（conn.rs 写面注释的同一条事实），所以 Response 按
//! JSON-RPC id 路由到 pending 表，其余帧进 inbox 由模块 `rr3_poll` 轮询——客户端
//! 永远不假设帧序。
//!
//! 心跳失败 = stderr 一行 + `exit(1)`：模块不装"活着"，把死交给 supervisor 按
//! restart_policy 裁定（rrd 的进程语义）。
//!
//! `rr3.call`（[`RR3_METHOD_CALL`])是 M3 定型的第 4 个上行 RPC 方法（trace_id
//! 语义：0 = 非代理；代理请求的 trace_id = 调用方 task，见 contracts/README.md
//! 演进登记）——被调模块用 [`Rr3BusClient::rr3_reply_call`] 顺原路回话。

use std::collections::{BTreeMap, HashMap};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame, Rr3Header,
    Rr3PayloadType, RR3_BUS_PATH_ENV, RR3_UDS_PATH_DEFAULT,
};
use serde_json::{json, Value};

use crate::RR3_TASK_RRD;

/// 模块间请求路由的上行 RPC 方法名（rr-bus 是唯一真值点，rrd 的 rpc.rs 引用它）
pub const RR3_METHOD_CALL: &str = "rr3.call";

/// 注册应答等待上界：注册是连接的第一件事，卡死在这里没有观察价值
const RR3_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// 从 manifest 文本里取模块身份（name + heartbeat_ms）。
/// 只取客户端自己需要的两个字段：**校验是 rrd 的事**（rr-registry 的校验器 +
/// conn.rs 的注册门禁），客户端不重复也不抢先——manifest 非法时注册会被 rrd
/// 拒掉，错误从那边回来才是唯一真值。
pub struct Rr3BusIdentity {
    pub name: String,
    pub heartbeat_ms: u64,
}

pub fn rr3_manifest_identity(text: &str) -> Result<Rr3BusIdentity, String> {
    let root: toml::Value = text
        .parse()
        .map_err(|e| format!("manifest 不是合法 TOML: {e}"))?;
    let name = root
        .get("manifest")
        .and_then(|m| m.get("name"))
        .and_then(toml::Value::as_str)
        .ok_or("manifest 缺 [manifest].name")?
        .to_string();
    let heartbeat_ms = root
        .get("health")
        .and_then(|h| h.get("heartbeat_ms"))
        .and_then(toml::Value::as_integer)
        .and_then(|v| u64::try_from(v).ok())
        .ok_or("manifest 缺 [health].heartbeat_ms（须为非负整数）")?;
    Ok(Rr3BusIdentity { name, heartbeat_ms })
}

/// 注册门禁要求 `manifest.entry` 是绝对路径（conn.rs），而在仓的 module.toml 是
/// 契约工件、entry 由装机安装步骤写入。模块直跑（测试 / 开发）时走本助手：把
/// entry 改写成进程自己的可执行文件路径再注册——对应"装机时写绝对 entry"的
/// 生产路径的等价物。
pub fn rr3_manifest_entry_absolutize(text: &str, exe: &Path) -> Result<String, String> {
    let exe = exe
        .to_str()
        .ok_or_else(|| "可执行文件路径不是合法 UTF-8".to_string())?
        .to_string();
    if !Path::new(&exe).is_absolute() {
        return Err(format!("entry 绝对化失败：{exe} 不是绝对路径"));
    }
    let mut root: toml::Value = text
        .parse()
        .map_err(|e| format!("manifest 不是合法 TOML: {e}"))?;
    root.get_mut("manifest")
        .ok_or("manifest 缺 [manifest] 表")?
        .as_table_mut()
        .ok_or("[manifest] 不是 TOML 表")?
        .insert("entry".into(), toml::Value::String(exe));
    toml::to_string(&root).map_err(|e| format!("manifest 序列化失败: {e}"))
}

fn rr3_now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

struct Rr3Inner {
    task: AtomicU64,
    name: String,
    /// 写半边（reader 线程持有读半边的另一个句柄）
    sock: Mutex<UnixStream>,
    /// JSON-RPC id → 应答邮箱（先注册后写帧，杜绝"应答先到邮箱不存在"的竞态）
    pending: Mutex<HashMap<u64, SyncSender<Result<Value, String>>>>,
    /// 非应答帧（Event / Notification / 点名 Request）的去处；`Arc` 使 poll 取 &self
    inbox: Mutex<Receiver<Rr3Frame>>,
    /// 运维计数表（0.0.4 批B，Q1 裁定）：模块业务线程经 [`Rr3BusClient::rr3_counter_bump`]
    /// 自愿累加，心跳线程每拍快照——非空则心跳 params 带 `{"counters": {...}}`，
    /// 空则维持 `{}`（旧 rrd 兼容）。键是模块内部运维词汇，归属表落各模块 README。
    counters: Mutex<BTreeMap<String, u64>>,
    next_id: AtomicU64,
    stop: AtomicBool,
}

impl Rr3Inner {
    fn rr3_write_frame(&self, mut frame: Rr3Frame) -> Result<(), String> {
        frame.header.payload_len = frame.payload.len() as u32;
        rr3_frame_write(
            &mut *self.sock.lock().unwrap_or_else(|p| p.into_inner()),
            &frame,
        )
        .map_err(|e| format!("写帧失败: {e:?}"))
    }

    /// 上行 Request 一次一答。先占邮箱再发帧：应答不可能落在邮箱就位前。
    fn rr3_call_inner(
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
        let frame = Rr3Frame {
            header: Rr3Header {
                opcode: opcode::SEND,
                flags: 0,
                trace_id: 0,
                timestamp_ns: rr3_now_ns(),
                src_task,
                dst_task: RR3_TASK_RRD,
                payload_len: 0,
            },
            payload: rr3_body_encode(Rr3PayloadType::Request, body.to_string().as_bytes()),
        };
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

    fn rr3_next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// 计数快照（心跳线程每拍调用）：克隆全表，锁内即放
    fn rr3_counters_snapshot(&self) -> BTreeMap<String, u64> {
        self.counters
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn rr3_task(&self) -> u64 {
        self.task.load(Ordering::Relaxed)
    }
}

/// 模块侧总线客户端。`Clone` 是同一连接的另一个句柄（Arc 共享）。
pub struct Rr3BusClient {
    inner: Arc<Rr3Inner>,
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
        let identity = rr3_manifest_identity(manifest_text)?;
        let exe = std::env::current_exe().map_err(|e| format!("取 current_exe 失败: {e}"))?;
        let wire = rr3_manifest_entry_absolutize(manifest_text, &exe)?;
        Self::rr3_connect_with(&path, identity, &wire)
    }

    /// 测试与特殊部署的注入口：路径、身份、线上 manifest 三者显式给定。
    fn rr3_connect_with(
        path: &str,
        identity: Rr3BusIdentity,
        wire_manifest: &str,
    ) -> Result<Self, String> {
        let stream = UnixStream::connect(path).map_err(|e| format!("连总线 {path} 失败: {e}"))?;
        let writer = stream
            .try_clone()
            .map_err(|e| format!("try_clone 失败: {e}"))?;
        let (tx, rx) = mpsc::channel::<Rr3Frame>();
        let inner = Arc::new(Rr3Inner {
            task: AtomicU64::new(0),
            name: identity.name,
            sock: Mutex::new(writer),
            pending: Mutex::new(HashMap::new()),
            inbox: Mutex::new(rx),
            counters: Mutex::new(BTreeMap::new()),
            next_id: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        });

        // reader 必须先于注册帧上路：注册应答也走 pending 路由
        let reader_inner = inner.clone();
        let mut reader_sock = stream;
        let reader: JoinHandle<()> = std::thread::Builder::new()
            .name("rr3-bus-reader".into())
            .spawn(move || rr3_reader_loop(&mut reader_sock, reader_inner, tx))
            .map_err(|e| format!("起 reader 线程失败: {e}"))?;

        // 注册：注册前没有 task，按契约 src=0（"用户态不得自设"的客户端侧写法）
        let reply = inner.rr3_call_inner(
            0,
            "rr3.register",
            json!({"name": inner.name, "manifest": wire_manifest}),
            RR3_CONNECT_TIMEOUT,
        );
        let reply = match reply {
            Ok(v) => v,
            Err(e) => {
                inner.stop.store(true, Ordering::Relaxed);
                let _ = reader.join();
                return Err(format!("注册失败: {e}"));
            }
        };
        let task = reply
            .get("src_task")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                inner.stop.store(true, Ordering::Relaxed);
                "注册应答缺 result.src_task".to_string()
            });
        let task = match task {
            Ok(t) => t,
            Err(e) => {
                let _ = reader.join();
                return Err(e);
            }
        };
        inner.task.store(task, Ordering::Relaxed);

        // 心跳线程：按 manifest 间隔喂狗；被拒/超时 = 进程退出，交给 supervisor 裁定。
        // 每拍先取计数快照：非空才带 `counters` 字段（可选 append，旧 rrd 忽略未知字段，
        // 双向兼容；空映射不带，保持与无上报模块同一条线上形状）。
        let hb_inner = inner.clone();
        let hb_ms = identity.heartbeat_ms.max(1);
        std::thread::Builder::new()
            .name("rr3-bus-heartbeat".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(hb_ms));
                if hb_inner.stop.load(Ordering::Relaxed) {
                    return;
                }
                let snap = hb_inner.rr3_counters_snapshot();
                let params = if snap.is_empty() {
                    json!({})
                } else {
                    json!({ "counters": snap })
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
                    eprintln!("rr3-bus: 心跳失败，退出（supervisor 按 restart_policy 裁定）");
                    std::process::exit(1);
                }
            })
            .map_err(|e| format!("起心跳线程失败: {e}"))?;

        Ok(Self { inner })
    }

    /// rrd 分配的本模块 task 号
    pub fn rr3_task(&self) -> u64 {
        self.inner.rr3_task()
    }

    /// 运维计数自报（0.0.4 批B，Q1 裁定）：给键累加 `by`，心跳线程每拍把全表
    /// 快照塞进 `rr3.heartbeat` 的可选 `counters` 字段。键是模块内部运维词汇
    /// （不逐键 pin，归属表落各模块 README）；无上报的模块字段缺省，rrd 侧
    /// 未知字段忽略——旧端 / 新端双向兼容。
    pub fn rr3_counter_bump(&self, key: &str, by: u64) {
        let mut t = self
            .inner
            .counters
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        *t.entry(key.to_string()).or_insert(0) += by;
    }

    /// 总线还活着吗（reader 线程在 = 活；reader 断开 = 死）。轮询型主循环的死亡探针：
    /// [`Rr3BusClient::rr3_poll_wait`] 在断开时**立即**返回空 `Vec`，与"暂时没帧"不可
    /// 区分——不探测的话，总线一死主循环就满转成僵尸。每圈探一次，假了就走
    /// "模块不装活着"的退出语义（与本文件头的心跳失败同一裁定）。
    pub fn rr3_connected(&self) -> bool {
        !self.inner.stop.load(Ordering::Relaxed)
    }

    pub fn rr3_name(&self) -> &str {
        &self.inner.name
    }

    /// 上行 RPC 一次一答（rr3.heartbeat 由心跳线程用，模块业务也可用）
    pub fn rr3_call(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        self.inner
            .rr3_call_inner(self.inner.rr3_task(), method, params, timeout)
    }

    /// 模块间请求：`rr3.call`（第 4 个上行 RPC 方法）。`args` 原样进 params.args，
    /// 由 rrd 路由到 exposes 该 topic 的在册模块，trace_id = 调用方 task。
    pub fn rr3_call_topic(
        &self,
        topic: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        self.rr3_call(
            RR3_METHOD_CALL,
            json!({"topic": topic, "args": args}),
            timeout,
        )
    }

    /// 业务发布：Event 帧，topic 并入体（conn.rs 按 body["topic"] 归属检查）。
    /// `body` 必须是 JSON 对象。
    pub fn rr3_publish(&self, topic: &str, body: Value) -> Result<(), String> {
        let mut body = match body {
            Value::Object(map) => Value::Object(map),
            other => return Err(format!("Event 体必须是 JSON 对象，收到 {other}")),
        };
        body["topic"] = json!(topic);
        self.inner
            .rr3_write_frame(Rr3Frame {
                header: Rr3Header {
                    opcode: opcode::SEND,
                    flags: 0,
                    trace_id: 0,
                    timestamp_ns: rr3_now_ns(),
                    src_task: self.inner.rr3_task(),
                    dst_task: RR3_TASK_RRD,
                    payload_len: 0,
                },
                payload: rr3_body_encode(Rr3PayloadType::Event, body.to_string().as_bytes()),
            })
            .map_err(|e| format!("发布 {topic} 失败: {e}"))
    }

    /// 应答一条点名到自己的 `rr3.call`：Response 上行，trace_id = 调用方 task，
    /// rrd 据此把应答改道回调用方（conn.rs 的 rr3_route_response）。
    /// `result` 是调用方在 `rr3_call_topic` 返回值里看到的对象。
    pub fn rr3_reply_call(&self, trace_id: u64, id: u64, result: Value) -> Result<(), String> {
        if trace_id == 0 {
            return Err("trace_id=0 的请求没有回路，无从应答".into());
        }
        let body = json!({"jsonrpc": "2.0", "id": id, "result": result});
        self.inner
            .rr3_write_frame(Rr3Frame {
                header: Rr3Header {
                    opcode: opcode::SEND,
                    flags: 0,
                    trace_id,
                    timestamp_ns: rr3_now_ns(),
                    src_task: self.inner.rr3_task(),
                    dst_task: RR3_TASK_RRD,
                    payload_len: 0,
                },
                payload: rr3_body_encode(Rr3PayloadType::Response, body.to_string().as_bytes()),
            })
            .map_err(|e| format!("应答 rr3.call 失败: {e}"))
    }

    /// 非阻塞收帧：取走当前积压的全部非应答帧（Event / Notification / 点名 Request）。
    /// 模块主循环的标准姿势：`poll` → 逐帧分派 → `sleep`。
    pub fn rr3_poll(&self) -> Vec<Rr3Frame> {
        let rx = self.inner.inbox.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            out.push(frame);
        }
        out
    }

    /// 带等待的收帧：先等第一帧（至多 `timeout`），再非阻塞排空余量。
    /// reader 断开（总线没了）时返回空 `Vec`，模块靠 `rr3_call` 的错误发现死活。
    pub fn rr3_poll_wait(&self, timeout: Duration) -> Vec<Rr3Frame> {
        let rx = self.inner.inbox.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = Vec::new();
        match rx.recv_timeout(timeout) {
            Ok(frame) => out.push(frame),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return out,
        }
        while let Ok(frame) = rx.try_recv() {
            out.push(frame);
        }
        out
    }

    /// 解出帧的 payload 类型与 JSON 体；非 JSON 体返回 `Err`。
    /// 模块分派循环的第一步（payload 前缀 + 体的形状在这里一并落地）。
    pub fn rr3_frame_body(frame: &Rr3Frame) -> Result<(Rr3PayloadType, Value), String> {
        let (ty, body) =
            rr3_body_split(&frame.payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
        let v: Value = serde_json::from_slice(&body).map_err(|e| format!("帧体不是 JSON: {e}"))?;
        Ok((ty, v))
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

/// reader 主循环：Response 按 id 投邮箱，其余进 inbox；连接死亡时唤醒所有等待者。
fn rr3_reader_loop(sock: &mut UnixStream, inner: Arc<Rr3Inner>, inbox_tx: mpsc::Sender<Rr3Frame>) {
    // EOF / 超时 / 坏帧：统一按断开处理
    while let Ok(frame) = rr3_frame_read(&mut *sock) {
        if inner.stop.load(Ordering::Relaxed) {
            break;
        }
        let (ty, body) = match rr3_body_split(&frame.payload) {
            Ok(v) => v,
            Err(_) => break,
        };
        if ty == Rr3PayloadType::Response {
            // 按 JSON-RPC id 挑自己的应答（序不可依赖的同一条事实，读侧的答案）
            let v: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => break,
            };
            let id = v.get("id").and_then(Value::as_u64).unwrap_or(0);
            let sender = inner
                .pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&id);
            if let Some(tx) = sender {
                let payload = match v.get("result") {
                    Some(r) => Ok(r.clone()),
                    None => Err(format!(
                        "RPC 被拒: {}",
                        v.get("error").cloned().unwrap_or(Value::Null)
                    )),
                };
                let _ = tx.send(payload);
            }
            // 无主应答：超时已被调用方记账，静默丢弃
            continue;
        }
        if inbox_tx.send(frame).is_err() {
            break; // inbox 没了：客户端已死
        }
    }
    // 收尾：唤醒所有还在等应答的调用方，让它们拿到确定性错误而不是吃满超时
    let mut pending = inner.pending.lock().unwrap_or_else(|p| p.into_inner());
    for (_, tx) in pending.drain() {
        let _ = tx.send(Err("总线已断开".into()));
    }
    inner.stop.store(true, Ordering::Relaxed);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;

    const MANIFEST: &str = "[manifest]\nschema_version = 1\nname = \"rr3-bus-t\"\nversion = \"0.0.1\"\ntype = \"module\"\nlang = \"rust\"\nentry = \"/bin/true\"\n\n[events]\npublishes = []\nsubscribes = []\n\n[health]\nheartbeat_ms = 50\n";

    /// 假 rrd：收注册回 src_task=7；心跳回 accepted；Event 记账并回播一条广播；
    /// 点名 Request 原样记账。行为写死（测试夹具，不是通用服务器）。
    fn fake_rrd(listener: UnixListener, log: Arc<Mutex<Vec<(String, Value)>>>) {
        let (mut stream, _) = listener.accept().expect("假 rrd 应接受到连接");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        loop {
            let Some(frame) = rr3_frame_read(&mut stream).ok() else {
                return;
            };
            let Ok((ty, body)) = rr3_body_split(&frame.payload) else {
                return;
            };
            let Ok(v) = serde_json::from_slice::<Value>(&body) else {
                return;
            };
            match ty {
                Rr3PayloadType::Request => {
                    let method = v["method"].as_str().unwrap_or("").to_string();
                    let id = v["id"].as_u64().unwrap_or(0);
                    let (result, ok): (Value, bool) = match method.as_str() {
                        "rr3.register" => (json!({"src_task": 7}), true),
                        "rr3.heartbeat" => (json!({"beat": "accepted"}), true),
                        _ => (json!({"noop": true}), true),
                    };
                    log.lock()
                        .unwrap()
                        .push((method.clone(), v["params"].clone()));
                    if method != RR3_METHOD_CALL {
                        let reply = if ok {
                            json!({"jsonrpc": "2.0", "id": id, "result": result})
                        } else {
                            v.clone()
                        };
                        let payload =
                            rr3_body_encode(Rr3PayloadType::Response, reply.to_string().as_bytes());
                        let out = Rr3Frame {
                            header: Rr3Header {
                                opcode: opcode::SEND,
                                flags: 0,
                                trace_id: 0,
                                timestamp_ns: rr3_now_ns(),
                                src_task: RR3_TASK_RRD,
                                dst_task: 7,
                                payload_len: payload.len() as u32,
                            },
                            payload,
                        };
                        if rr3_frame_write(&mut stream, &out).is_err() {
                            return;
                        }
                    }
                }
                Rr3PayloadType::Event => {
                    log.lock().unwrap().push(("EVENT".into(), v.clone()));
                    let payload = rr3_body_encode(
                        Rr3PayloadType::Event,
                        json!({"topic": "rr3-fake/echo", "echo": true})
                            .to_string()
                            .as_bytes(),
                    );
                    let out = Rr3Frame {
                        header: Rr3Header {
                            opcode: opcode::SEND,
                            flags: 0,
                            trace_id: 0,
                            timestamp_ns: rr3_now_ns(),
                            src_task: RR3_TASK_RRD,
                            dst_task: 7,
                            payload_len: payload.len() as u32,
                        },
                        payload,
                    };
                    if rr3_frame_write(&mut stream, &out).is_err() {
                        return;
                    }
                }
                _ => return,
            }
        }
    }

    fn tmp_bus_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "rr3-bus-t-{tag}-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        p
    }

    fn connect_on(tag: &str, log: Arc<Mutex<Vec<(String, Value)>>>) -> (Rr3BusClient, PathBuf) {
        let path = tmp_bus_path(tag);
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || fake_rrd(listener, log));
        let client = Rr3BusClient::rr3_connect_with(
            path.to_str().unwrap(),
            rr3_manifest_identity(MANIFEST).unwrap(),
            MANIFEST,
        )
        .expect("连假 rrd 应成功");
        (client, path)
    }

    #[test]
    fn manifest_identity_takes_name_and_heartbeat() {
        let id = rr3_manifest_identity(MANIFEST).unwrap();
        assert_eq!(id.name, "rr3-bus-t");
        assert_eq!(id.heartbeat_ms, 50);
        assert!(rr3_manifest_identity("no-toml-here [").is_err());
        assert!(
            rr3_manifest_identity("[manifest]\nname=\"x\"\n").is_err(),
            "缺 heartbeat_ms 必须报错"
        );
    }

    #[test]
    fn entry_absolutize_rewrites_entry_and_keeps_rest() {
        let out = rr3_manifest_entry_absolutize(MANIFEST, Path::new("/opt/rr3/rr3-sense")).unwrap();
        let root: toml::Value = out.parse().unwrap();
        assert_eq!(
            root["manifest"]["entry"].as_str(),
            Some("/opt/rr3/rr3-sense")
        );
        assert_eq!(root["manifest"]["name"].as_str(), Some("rr3-bus-t"));
        assert_eq!(root["health"]["heartbeat_ms"].as_integer(), Some(50));
        assert!(rr3_manifest_entry_absolutize(MANIFEST, Path::new("rel/bin")).is_err());
    }

    #[test]
    fn connect_registers_and_call_round_trips() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (client, path) = connect_on("call", log.clone());
        assert_eq!(client.rr3_task(), 7);
        assert_eq!(client.rr3_name(), "rr3-bus-t");
        let r = client
            .rr3_call("rr3.heartbeat", json!({}), Duration::from_secs(2))
            .unwrap();
        assert_eq!(r["beat"], "accepted");
        let log = log.lock().unwrap();
        assert!(
            log.iter().any(|(m, _)| m == "rr3.register"),
            "注册帧必须到过假 rrd"
        );
        assert!(log.iter().any(|(m, _)| m == "rr3.heartbeat"));
        drop(client);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn publish_reaches_bus_and_broadcast_lands_in_inbox() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (client, path) = connect_on("pub", log.clone());
        client.rr3_publish("rr3-t/pcm", json!({"seq": 1})).unwrap();
        assert!(
            client.rr3_publish("bad", json!([])).is_err(),
            "非对象体必须被拒"
        );
        let frames = client.rr3_poll_wait(Duration::from_secs(2));
        assert_eq!(frames.len(), 1, "回播的广播应落进 inbox");
        let (ty, v) = Rr3BusClient::rr3_frame_body(&frames[0]).unwrap();
        assert_eq!(ty, Rr3PayloadType::Event);
        assert_eq!(v["topic"], "rr3-fake/echo");
        let log = log.lock().unwrap();
        let pubed = log
            .iter()
            .find(|(m, _)| *m == "EVENT")
            .expect("发布应到达假 rrd");
        assert_eq!(pubed.1["topic"], "rr3-t/pcm");
        assert_eq!(pubed.1["seq"], 1);
        drop(client);
        let _ = std::fs::remove_file(&path);
    }

    /// 计数自报 → 心跳载荷（0.0.4 批B，Q1 裁定的双端兼容面）：
    /// 无上报的拍 params 维持 `{}`；bump 后的拍 params 带非空 `counters`，
    /// 值是累加结果。有界轮询不猜时刻（hb=50ms，2s 上界足够）。
    #[test]
    fn heartbeat_carries_counters_only_after_bump() {
        use std::time::Instant;
        let log = Arc::new(Mutex::new(Vec::new()));
        let (client, path) = connect_on("cnt", log.clone());
        // 第一段：等一个不带 counters 的心跳（空表 → params = {}）
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let has_empty = log
                .lock()
                .unwrap()
                .iter()
                .any(|(m, p)| m == "rr3.heartbeat" && p.get("counters").is_none());
            assert!(
                has_empty || Instant::now() < deadline,
                "bump 前应出现过 params={{}} 的心跳"
            );
            if has_empty {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // bump 两次同键（累加）+ 一次异键
        client.rr3_counter_bump("relay_on", 2);
        client.rr3_counter_bump("relay_on", 1);
        client.rr3_counter_bump("chat_rounds", 1);
        // 第二段：等带 counters 的心跳，值逐键核对
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let hit = {
                let log = log.lock().unwrap();
                log.iter()
                    .find(|(m, p)| m == "rr3.heartbeat" && p.get("counters").is_some())
                    .map(|(_, p)| p["counters"].clone())
            };
            if let Some(c) = hit {
                assert_eq!(c["relay_on"], 3, "同键累加");
                assert_eq!(c["chat_rounds"], 1);
                break;
            }
            assert!(Instant::now() < deadline, "bump 后应出现带 counters 的心跳");
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(client);
        let _ = std::fs::remove_file(&path);
    }

    /// topic 参数的写入形状钉死：rrd 的 rr3.call 解析器按 params.topic/args 取值
    #[test]
    fn call_topic_sends_method_and_params_shape() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (client, path) = connect_on("ct", log.clone());
        // 假 rrd 对 rr3.call 不回标准应答（走 _ 分支回了 {"noop":true}）——够用来核对线上形状
        let _ = client.rr3_call_topic("rr3-t/say", json!({"text": "hi"}), Duration::from_secs(2));
        let log = log.lock().unwrap();
        let (_, params) = log
            .iter()
            .find(|(m, _)| m == RR3_METHOD_CALL)
            .expect("rr3.call 应到达");
        assert_eq!(params["topic"], "rr3-t/say");
        assert_eq!(params["args"]["text"], "hi");
        drop(client);
        let _ = std::fs::remove_file(&path);
    }

    /// 总线死亡探针：对端在注册后掉线 → reader EOF → `rr3_connected` 翻假、
    /// `rr3_poll_wait` 立即返回空。心跳用大间隔（3600s）：否则心跳线程先撞上
    /// 断连走 `exit(1)`，测试进程自己先死，探针根本轮不到被观察。
    #[test]
    fn disconnect_flips_connected_false_and_poll_wait_returns_promptly() {
        use std::time::Instant;
        let path = tmp_bus_path("dead");
        let listener = UnixListener::bind(&path).unwrap();
        // 假 rrd：完成注册握手后**挂起等信号**，测试先观察"活着"，再 drop 发送端
        // 让 stream 掉线（FIN）——直接退场会和 connect 返回赛跑，探针的初值就成了赌运气
        let (close_tx, close_rx) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            let Ok(mut stream) = listener.accept().map(|(s, _)| s) else {
                return;
            };
            if rr3_frame_read(&mut stream).is_err() {
                return;
            }
            let payload = rr3_body_encode(
                Rr3PayloadType::Response,
                json!({"jsonrpc": "2.0", "id": 1, "result": {"src_task": 7}})
                    .to_string()
                    .as_bytes(),
            );
            let out = Rr3Frame {
                header: Rr3Header {
                    opcode: opcode::SEND,
                    flags: 0,
                    trace_id: 0,
                    timestamp_ns: rr3_now_ns(),
                    src_task: RR3_TASK_RRD,
                    dst_task: 7,
                    payload_len: payload.len() as u32,
                },
                payload,
            };
            if rr3_frame_write(&mut stream, &out).is_err() {
                return;
            }
            let _ = close_rx.recv(); // 挂到测试发令再退场（drop stream → 对端 EOF）
        });
        // 心跳用大间隔：否则心跳线程先撞上断连走 exit(1)，测试进程自己先死
        let manifest = MANIFEST.replace("heartbeat_ms = 50", "heartbeat_ms = 3600000");
        let client = Rr3BusClient::rr3_connect_with(
            path.to_str().unwrap(),
            rr3_manifest_identity(&manifest).unwrap(),
            &manifest,
        )
        .expect("注册握手应成功");
        assert_eq!(client.rr3_task(), 7);
        assert!(client.rr3_connected(), "握手刚完成时必须活着");

        // 发令掉线：对端 stream drop → 客户端 reader 撞上 EOF
        drop(close_tx);
        // 有界轮询，不猜时刻
        let deadline = Instant::now() + Duration::from_secs(2);
        while client.rr3_connected() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!client.rr3_connected(), "对端掉线后探针必须翻假");
        // 断开后的 poll_wait 立即返回空（满转僵尸的根源，这里钉住它是"可检测的"）
        let started = Instant::now();
        assert!(client.rr3_poll_wait(Duration::from_secs(5)).is_empty());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "断开后 poll_wait 必须立即返回"
        );
        let _ = std::fs::remove_file(&path);
    }
}
