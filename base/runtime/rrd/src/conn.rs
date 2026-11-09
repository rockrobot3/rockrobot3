//! 单连接服务：本线程 reader + 一条 writer 线程。
//!
//! 协议面（spec §5.0 + 偏离 4）：
//! - 第一条帧必须是 `Request` + `dst_task = RR3_TASK_RRD` 的 `rr3.register`，否则按协议错误断开；
//! - `src_task` 由 rrd 分配并在应答 result 里回带；注册后上行帧的 `src_task` 必须等于分配值
//!   （AOS-STD-IPC-024：用户态不得自设）——伪造的那一帧被拒，但连接不断（一次笔误不该让整机掉线）；
//! - 注册完成前不收心跳、不判活（legacy 教训 76c0ad8）；未注册连接由读超时收掉；
//! - `rr3.unregister` 是模块主动退场，**不走** restart_policy（那是崩溃语义）；
//! - 上行 `Event` 是业务发布：过三层归属检查（冒充内建命名空间 → 前缀/在册 owner 越权 →
//!   主题未登记）后原样转发给**除发布者外**的在册连接，头改写成发布者 task + `dst=0`；
//!   任何一种拒法都只回错误帧 + 计数，不断连（spec §6、ADR-0006 决策 2/4）；
//! - 上行 `Response` 是 M3 `rr3.call` 的应答回路：trace_id = 调用方 task（转发时写入），
//!   body 原样、dst 改写为 trace_id 定向回投；trace_id=0 或调用方已不在册都只回错误帧
//!   不断连（M3 定型，见 `rr3_route_call` / `rr3_route_response`）。
//!
//! 写面（本任务的并发形状）：reader 线程写应答、writer 线程写广播，两者持有同一 socket 的
//! `try_clone` 句柄、共用一把 `Mutex<UnixStream>`、且一帧一次 `write_all` → **整帧原子**；
//! 但同一 socket 上"应答 vs 广播"的**相对顺序不保证**，所以测试按类型/字段挑帧，不按序号。

use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rr_bus::RR3_TASK_RRD;
use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read_cap, rr3_frame_write, Rr3Frame,
    Rr3FrameError, Rr3Header, Rr3PayloadType, Rr3StatusCode, RR3_FRAMING_PLEN_CAP_DEFAULT,
};
use rr_registry::{rr3_manifest_chk, Rr3TopicSet};
use serde_json::{json, Value};

use crate::daemon::{rr3_now_ms, rr3_now_ns, rr3_state_frame};
use crate::manifest::{rr3_manifest_abs, rr3_manifest_parse, Rr3ManifestFacts};
use crate::registry::{rr3_phase_for_exit, Rr3Beat, Rr3Death, Rr3Phase, RrdRegistry, RR3_FAN_CAP};
use crate::rpc::{
    rr3_rpc_build_error, rr3_rpc_build_result, rr3_rpc_call_params, rr3_rpc_counters,
    rr3_rpc_deny_status, rr3_rpc_manifest, rr3_rpc_parse, rr3_rpc_reg_status, rr3_rpc_topic_param,
    Rr3RpcMethod, Rr3RpcRequest,
};
use crate::supervisor::Rr3Supervisor;
use crate::{rr3_lock, rr3_log, rr3_log_query};

/// writer 线程单帧写超时（0.0.8b 审计 B-5）：对端不收时 5s 内必须显形——
/// 超时即写失败，writer 退出、通道 drop、fanout 走 Disconnected 回收。
/// 取 5s 与面板/总线读超时同档：正常 Unix socket 写是 µs 级，5s 不可达 = 真卡死。
const RR3_FAN_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// 低电兜底的触发主题（D3 append 的契约主题；owner=rr3-power，rrd 只订阅不发布）
const RR3_BATTERY_LOW_TOPIC: &str = "rr3-power/battery/low";
/// 低电兜底的解除主题（批N5 件二笔3 append 的契约主题；owner=rr3-power，
/// rrd 只订阅不发布）——两个沿一个对，置旗/清旗各听一个主题。
const RR3_BATTERY_OK_TOPIC: &str = "rr3-power/battery/ok";

/// 一条已注册连接的身份证
struct Rr3Peer {
    task: u64,
    name: String,
}

/// daemon 每接受一个连接构造一份
pub struct Rr3ConnCtx {
    pub shared: Arc<Mutex<RrdRegistry>>,
    pub topics: Arc<Rr3TopicSet>,
    pub t0: Instant,
    pub pre_register_timeout_ms: u64,
    /// 重启计划的**唯一去路**（`supervisor.rs`）。conn 只负责"要不要投"，
    /// "起不起得来"由 supervisor 按所有权裁定（E-1）——收尾这一路绝不自己 `Command::spawn`。
    pub sup: Rr3Supervisor,
    /// 运维计数持久化路径（0.0.4 批B，Q2 裁定）：daemon 启动判定一次，`None` =
    /// 不持久化（心跳聚合照常进内存账，只是断电不保）。conn 心跳归并后按
    /// R26 合并写节流落盘（0.0.5 增补轮四 批W3 改判，闸门在 counters.rs）。
    pub counters_path: Option<std::path::PathBuf>,
    /// 运维计数合并写闸门（R26，批W3）：daemon 造一份、全连接共享（`Arc`）——
    /// 每条连接各持一份等于把 M 窗口放大 N 连接倍。
    pub counters_gate: Arc<Mutex<crate::counters::Rr3CountersGate>>,
}

impl Rr3ConnCtx {
    pub fn rr3_ctx_now_ms(&self) -> u64 {
        rr3_now_ms(self.t0)
    }
}

/// 唯一的写出口：锁覆盖整帧 → 与并发 writer 的字节永不交错。
/// 写一帧到 `out`（持锁写，帧级原子）。返回 false = 写失败（含写超时）：
/// writer 线程据此收摊退出（对端不收 = 这条连接写向已死），应答路径忽略返回值
/// ——失败已点名进日志，连接生死由 reader 侧的 EOF/心跳回收裁定。
fn rr3_write(out: &Arc<Mutex<UnixStream>>, frame: &Rr3Frame) -> bool {
    if let Err(e) = rr3_frame_write(&mut *rr3_lock(out), frame) {
        rr3_log(&format!("写帧失败 dst={}: {e:?}", frame.header.dst_task));
        return false;
    }
    true
}

/// 组一帧 rrd → 模块（ty=Response/Notification，dst=对端 task）
fn rr3_out_frame(ty: Rr3PayloadType, dst_task: u64, body: &[u8]) -> Rr3Frame {
    let payload = rr3_body_encode(ty, body);
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: rr3_now_ns(),
            src_task: RR3_TASK_RRD,
            dst_task,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// 成功/失败应答的两个写法（合起来只有一处构造帧）
fn rr3_reply_ok(out: &Arc<Mutex<UnixStream>>, me: &Rr3Peer, id: u64, result: &Value) {
    rr3_write(
        out,
        &rr3_out_frame(
            Rr3PayloadType::Response,
            me.task,
            &rr3_rpc_build_result(id, result),
        ),
    );
}

fn rr3_reply_err(
    out: &Arc<Mutex<UnixStream>>,
    me: &Rr3Peer,
    id: u64,
    code: Rr3StatusCode,
    msg: &str,
) {
    rr3_log(&format!("{} 应答错误 {}: {msg}", me.name, code as i32));
    rr3_write(
        out,
        &rr3_out_frame(
            Rr3PayloadType::Response,
            me.task,
            &rr3_rpc_build_error(id, code, msg),
        ),
    );
}

/// 解析失败时尽力取 JSON-RPC 的 id；取不到用 0（客户端只按 code 判定）
fn rr3_best_effort_id(body: &[u8]) -> u64 {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("id").and_then(Value::as_u64))
        .unwrap_or(0)
}

/// 入站帧处理结论
enum Rr3Flow {
    /// 继续读下一帧
    Keep,
    /// 干净收尾（EOF / rr3.unregister）：不做崩溃记账
    QuitClean,
}

/// 注册期拒绝的统一出口：按对端自报的 src_task 回错误帧，再把原因交回上层断开连接
fn rr3_refuse(
    out: &Arc<Mutex<UnixStream>>,
    dst_task: u64,
    id: u64,
    code: Rr3StatusCode,
    msg: &str,
) -> String {
    rr3_log(&format!("注册被拒（dst={dst_task}）: {msg}"));
    rr3_write(
        out,
        &rr3_out_frame(
            Rr3PayloadType::Response,
            dst_task,
            &rr3_rpc_build_error(id, code, msg),
        ),
    );
    msg.to_string()
}

/// 握手：首帧必须是 rr3.register；成功后把出站 Sender 交给在册表并分配 task
///
/// 注册期每一次拒绝都**先回错误帧再断开**（只断连不回码等于让客户端挂在超时上）。
/// 状态码：首帧类型/dst_task/RPC 畸形/manifest 文本非法/`entry` 非绝对 = `Invalid`；
/// 重复名与超额 = `Busy`；保留名 = `Permission`（映射只在 `rr3_rpc_reg_status` 一处）。
fn rr3_handshake(
    reader: &mut UnixStream,
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    fan_tx: SyncSender<Rr3Frame>,
) -> Result<Rr3Peer, String> {
    let (reported_task, body) = rr3_hs_read(reader, out)?;
    let req = match rr3_rpc_parse(&body) {
        Ok(r) => r,
        Err((_, msg)) => {
            return Err(rr3_refuse(
                out,
                reported_task,
                rr3_best_effort_id(&body),
                Rr3StatusCode::Invalid,
                &format!("首帧 RPC 畸形: {msg}"),
            ))
        }
    };
    // ADR-0010 裁定 3：首帧是 rr3.ctl.* → 控制旁支，不注册、一问一答即断
    if req.method.is_ctl() {
        return Err(rr3_serve_ctl(out, ctx, reported_task, req));
    }
    let facts = rr3_hs_facts(&req, out, reported_task, ctx)?;
    rr3_hs_admit(ctx, &facts, fan_tx, out, reported_task, req.id)
}

/// 握手首段：读首帧并过 dst/类型两道门（注册前应答按对端自报 src_task 回）
fn rr3_hs_read(
    reader: &mut UnixStream,
    out: &Arc<Mutex<UnixStream>>,
) -> Result<(u64, Vec<u8>), String> {
    let frame = match rr3_frame_read_cap(&mut *reader, RR3_FRAMING_PLEN_CAP_DEFAULT) {
        Ok(f) => f,
        // 连首帧都没读到：无从应答，只把原因交回上层记一条
        Err(e) => return Err(format!("首帧读取失败: {e:?}")),
    };
    let reported_task = frame.header.src_task;
    let (ty, body) = match rr3_body_split(&frame.payload) {
        Ok(v) => v,
        Err(e) => {
            return Err(rr3_refuse(
                out,
                reported_task,
                0,
                Rr3StatusCode::Invalid,
                &format!("首帧 payload 前缀非法: {e:?}"),
            ))
        }
    };
    if frame.header.dst_task != RR3_TASK_RRD {
        return Err(rr3_refuse(
            out,
            reported_task,
            rr3_best_effort_id(&body),
            Rr3StatusCode::Invalid,
            &format!(
                "首帧 dst_task 必须为 {RR3_TASK_RRD}，收到 {}",
                frame.header.dst_task
            ),
        ));
    }
    if ty != Rr3PayloadType::Request {
        return Err(rr3_refuse(
            out,
            reported_task,
            rr3_best_effort_id(&body),
            Rr3StatusCode::Invalid,
            &format!("首帧 payload 类型必须是 Request，收到 {ty:?}"),
        ));
    }
    Ok((reported_task, body))
}

/// 注册门：方法/参数/形状/entry 绝对路径四道，全 fail-closed 回错误帧
fn rr3_hs_facts(
    req: &Rr3RpcRequest,
    out: &Arc<Mutex<UnixStream>>,
    reported_task: u64,
    ctx: &Rr3ConnCtx,
) -> Result<Rr3ManifestFacts, String> {
    if req.method != Rr3RpcMethod::Register {
        return Err(rr3_refuse(
            out,
            reported_task,
            req.id,
            Rr3StatusCode::Invalid,
            &format!(
                "注册前第一条 RPC 必须是 rr3.register，收到 {}",
                req.method.as_str()
            ),
        ));
    }
    let text = match rr3_rpc_manifest(&req.params) {
        Ok(t) => t,
        Err((_, msg)) => {
            return Err(rr3_refuse(
                out,
                reported_task,
                req.id,
                Rr3StatusCode::Invalid,
                &msg,
            ))
        }
    };
    let facts = match rr3_manifest_parse(&text) {
        Ok(f) => f,
        Err(errs) => {
            return Err(rr3_refuse(
                out,
                reported_task,
                req.id,
                Rr3StatusCode::Invalid,
                &errs.join("; "),
            ))
        }
    };
    let report = rr3_manifest_chk(&text, &ctx.topics);
    if !report.ok() {
        return Err(rr3_refuse(
            out,
            reported_task,
            req.id,
            Rr3StatusCode::Invalid,
            &report.errors.join("; "),
        ));
    }
    // M2 的 manifest 从线上来，没有"所在目录"可依据 → entry 必须是绝对路径。
    // 在仓 modules/*/module.toml 是契约工件而非注册输入（装机时写入绝对 entry）。
    if !rr3_manifest_abs(&facts.entry) {
        return Err(rr3_refuse(
            out,
            reported_task,
            req.id,
            Rr3StatusCode::Invalid,
            &format!("manifest.entry 必须是绝对路径，收到 {}", facts.entry),
        ));
    }
    Ok(facts)
}

/// 入册与应答：注册成功 → 回 src_task → 广播 registered → 给后到者补发在册快照
fn rr3_hs_admit(
    ctx: &Rr3ConnCtx,
    facts: &Rr3ManifestFacts,
    fan_tx: SyncSender<Rr3Frame>,
    out: &Arc<Mutex<UnixStream>>,
    reported_task: u64,
    req_id: u64,
) -> Result<Rr3Peer, String> {
    let now = ctx.rr3_ctx_now_ms();
    // 一份 Sender 进在册表（此后它是这条连接唯一的广播入口），一份给补快照用
    let replay_tx = fan_tx.clone();
    // 硬规矩：锁结果先 `let` 绑再 `match`——Guard 临时量不得延寿到 Err 臂的
    // rr3_refuse（那边要取另一把锁），与 rr3_conn_finish 同形。
    let added = rr3_lock(&ctx.shared).rr3_reg_add(facts, fan_tx, now);
    let task = match added {
        Ok(t) => t,
        Err(e) => {
            // 状态码映射只在这一处（rr3_rpc_reg_status），不在 conn 里重写
            let code = rr3_rpc_reg_status(&e);
            return Err(rr3_refuse(out, reported_task, req_id, code, &e.to_string()));
        }
    };
    rr3_log(&format!(
        "注册 {} task={task} hb={}ms wd={}ms policy={}",
        facts.name, facts.heartbeat_ms, facts.watchdog_ms, facts.restart_policy
    ));
    // 应答与广播的先后不确定（两条线程两个写点），测试按类型挑帧，不按序号
    rr3_write(
        out,
        &rr3_out_frame(
            Rr3PayloadType::Response,
            task,
            &rr3_rpc_build_result(req_id, &json!({"src_task": task})),
        ),
    );
    let peer = Rr3Peer {
        task,
        name: facts.name.clone(),
    };
    let n = rr3_lock(&ctx.shared).rr3_reg_fanout(
        &rr3_state_frame(&peer.name, "registered", peer.task),
        RR3_TASK_RRD,
    );
    rr3_log(&format!("广播 registered 到 {n} 个连接"));
    // 后到者补发在册状态：只发转移不发快照的话，后起模块看不到先起模块上线过。
    // 这里只保证入队次序，不构成上线次序——读侧一律按类型/字段挑帧。
    let snapshot = rr3_lock(&ctx.shared).rr3_reg_snapshot();
    let mut replayed = 0usize;
    for (name, t, phase) in snapshot.iter().filter(|(name, _, _)| *name != peer.name) {
        if replay_tx
            .try_send(rr3_state_frame(name, phase.as_str(), *t))
            .is_ok()
        {
            replayed += 1;
        }
    }
    rr3_log(&format!("向 {} 补发在册状态 {replayed} 条", peer.name));
    Ok(peer)
}

/// reboot 执行面的解锁判定（0.0.4 批C）：`Some(解锁口径)` = cfg(linux) 且 env
/// `RR3_REBOOT_OK=1`；`None` = 缺省恒拒。口径文案只进日志——运维在 rrd 日志环
/// 里能看到"是谁解锁的"。为什么是双重门而不是单 cfg：ubuntu CI 的 e2e 也编译
/// linux 面，单 cfg 门会让测试进程拿到真重启能力；解锁旋钮留给装机环境
/// （rrd.service 注入 RR3_REBOOT_OK=1）。
fn rr3_reboot_unlocked() -> Option<&'static str> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    match std::env::var("RR3_REBOOT_OK").as_deref() {
        Ok("1") => Some("cfg(linux) + RR3_REBOOT_OK=1"),
        _ => None,
    }
}

/// ctl.logs 参数（均可选）：`prefix` 子串过滤（空 = 不过滤），`tail` 尾行数
/// （0 = 不限）。坏形 fail-closed 拒——把输错的过滤条件静默折默认，会把
/// "参数写错"伪装成"没有匹配行"，排障时最贵的一类假象。
fn rr3_log_args(params: &Value) -> Result<(String, usize), (Rr3StatusCode, String)> {
    let prefix = match params.get("prefix") {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => {
            return Err((
                Rr3StatusCode::Invalid,
                format!("ctl.logs prefix 应为字符串，收到 {other}"),
            ))
        }
    };
    let tail = match params.get("tail") {
        None => 0,
        Some(v) => match v.as_u64() {
            Some(n) => usize::try_from(n).unwrap_or(usize::MAX),
            None => {
                return Err((
                    Rr3StatusCode::Invalid,
                    format!("ctl.logs tail 应为非负整数，收到 {v}"),
                ))
            }
        },
    };
    Ok((prefix, tail))
}

/// 控制旁支（ADR-0010 裁定 2/4）：一次性查询连接，一问一答即断。
///
/// 不注册、不进在册表、不受 watchdog 管——在册表是"活的业务模块"的账本，运维探针
/// 不记账。返回值作为握手 `Err` 的理由交回 `rr3_conn_serve`：握手失败路径不做死亡记账
/// （`me` 尚不存在），daemon 侧按既有"连接收尾"落一行日志，连接随之关闭。
/// 应答按对端**自报**的 `src_task` 回（与注册期 `rr3_refuse` 同一口径——rrd 尚未分配 task）。
fn rr3_serve_ctl(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    reported_task: u64,
    req: Rr3RpcRequest,
) -> String {
    // 先在锁内取数、锁外应答：与握手主路的锁纪律一致（持 shared 锁时不碰 out 锁）
    let result: Result<Value, (Rr3StatusCode, String)> = match req.method {
        Rr3RpcMethod::CtlStatus => rr3_ctl_status(ctx),
        Rr3RpcMethod::CtlHealth => rr3_ctl_health(ctx),
        Rr3RpcMethod::CtlLogs => rr3_log_args(&req.params)
            .map(|(prefix, tail)| json!({"lines": rr3_log_query(&prefix, tail)})),
        // 运维计数读面（0.0.4 批B，Q1 裁定）：与 CtlHealth 分开——health 是心跳健康
        // 语义（watchdog 判活的输入），counters 是业务运维账（"机器慢先看数字"）。
        // 键 = `模块名/键`（模块自报归并）或 `boots`（rrd 自有）；未持久化时只有内存账。
        Rr3RpcMethod::CtlCounters => {
            let counters = rr3_lock(&ctx.shared).rr3_reg_counters();
            Ok(json!({ "counters": counters }))
        }
        // 恒拒（ADR-0010 裁定 4）：方法名进白名单把协议面定型，语义随 M6；
        // 契约状态码是冻结的 AIRY_E* 子集，用 Permission 而不是自造 NotSupported，
        // 与 rr3-ota-ctl switch 的"恒拒 + 文案点 M6"先例同构。
        Rr3RpcMethod::CtlOta => Err((
            Rr3StatusCode::Permission,
            "OTA（含模块级 staging 更新）随 M6 落地，当前拒绝执行".to_string(),
        )),
        // 系统重启（0.0.4 批C，Q3 裁定）：执行面带**双重门**——cfg(linux) 且
        // env `RR3_REBOOT_OK=1`（装机服务单元/init 脚本注入）才真跑重启命令；
        // 其余一律 Permission 拒 + 日志点名。为什么加 env 门：cfg(linux) 单独
        // 解锁会让 CI 的 ubuntu runner 在 e2e 里真重启——e2e 可测性要求缺省路径
        // 恒拒，解锁面留装机环境（distro 的 rrd.service 与 buildroot 的 S99rrd 同设）。
        Rr3RpcMethod::CtlReboot => rr3_ctl_reboot(),
        // 装配快照读口（批N4 件九笔4）：boot 装载链验收后的四路底账 + [pins]×
        // wiring 全交叉逐条判果，一应答给出（判据真值单点 = assembly.rs）。快照
        // 未在册（--manifest 调试路径）时如实回不可得，不造假数据。
        Rr3RpcMethod::CtlWiring => Ok(crate::assembly::rr3_assembly_json()),
        // 契约完整性审计读口（批N5 件五笔4，Z-4）：boot 装载链验收后的契约四册
        // sha256 记账 + "齐全且 sha 对"自检判果一应答给出（判据真值单点 =
        // contracts.rs）。快照未在册（--manifest 调试路径）如实回不可得。
        Rr3RpcMethod::CtlContracts => Ok(crate::contracts::rr3_contracts_json()),
        // 装配侧写口（批N22 件一，conventions §6.1 三写口 + ADR-0021 裁定 6）：
        // 门序编排全在 domain_write.rs 单点（confirm 门 → 证据四道核 → key 白名单
        // → 文本手术 → 装载链全量预核 → 原子写回 → 契约记账重记），此处只接线。
        // 快照未在册由 run 薄壳如实拒（--manifest 调试路径无装配单可写）。
        Rr3RpcMethod::CtlDomainSet => crate::domain_write::rr3_domain_write_run(&req.params),
        // 到不了：握手只把 is_ctl 的请求分进来（解析器与分流判据同一真值）
        other => Err((
            Rr3StatusCode::Invalid,
            format!("ctl 连接只接受 rr3.ctl.*，收到 {}", other.as_str()),
        )),
    };
    let payload = match &result {
        Ok(v) => rr3_rpc_build_result(req.id, v),
        Err((code, msg)) => rr3_rpc_build_error(req.id, *code, msg),
    };
    rr3_write(
        out,
        &rr3_out_frame(Rr3PayloadType::Response, reported_task, &payload),
    );
    rr3_log(&format!(
        "ctl 查询 {} 已应答（dst={reported_task}），一次性连接收尾",
        req.method.as_str()
    ));
    "ctl 查询完成，一次性控制连接按 ADR-0010 收尾".to_string()
}

/// ctl.status 读面：在册模块快照 + 越权发布计数（锁内取数、锁外组 JSON）。
fn rr3_ctl_status(ctx: &Rr3ConnCtx) -> Result<Value, (Rr3StatusCode, String)> {
    let (modules, denied) = {
        let g = rr3_lock(&ctx.shared);
        (
            g.rr3_reg_snapshot()
                .into_iter()
                .map(|(name, task, phase)| {
                    json!({"name": name, "task": task, "phase": phase.as_str()})
                })
                .collect::<Vec<Value>>(),
            g.rr3_reg_denied(),
        )
    };
    Ok(json!({
        "modules": modules,
        "denied_publish": denied,
        "boot": crate::rr3_boot_summary_get(),
    }))
}

/// ctl.health 读面：心跳健康快照（watchdog 判活的输入）+ 降级原因
/// （模块 / 类别 / 点名文案；未定格的后门路径为 null，不造假值）。
fn rr3_ctl_health(ctx: &Rr3ConnCtx) -> Result<Value, (Rr3StatusCode, String)> {
    let modules = {
        let g = rr3_lock(&ctx.shared);
        g.rr3_reg_health(ctx.rr3_ctx_now_ms())
            .into_iter()
            .map(|r| {
                json!({
                    "name": r.name, "task": r.src_task, "phase": r.phase.as_str(),
                    "heartbeat_ms": r.heartbeat_ms, "watchdog_ms": r.watchdog_ms,
                    "since_beat_ms": r.since_beat_ms,
                })
            })
            .collect::<Vec<Value>>()
    };
    Ok(json!({
        "modules": modules,
        "degraded": crate::rr3_degrade_notes_get(),
    }))
}

/// ctl.reboot 执行面：双重门（cfg(linux) 且 env `RR3_REBOOT_OK=1`）解锁才真跑
/// 重启命令，其余一律 Permission 拒；e2e 缺省路径恒拒。
fn rr3_ctl_reboot() -> Result<Value, (Rr3StatusCode, String)> {
    match rr3_reboot_unlocked() {
        Some(how) => {
            rr3_log(&format!("ctl.reboot 执行面已解锁（{how}），发起系统重启"));
            rr3_reboot_run()
                .map(|()| json!({"reboot": true}))
                .map_err(|e| (Rr3StatusCode::Permission, format!("重启命令执行失败: {e}")))
        }
        None => Err((
            Rr3StatusCode::Permission,
            "reboot 执行面未解锁（需 cfg(linux) 且 env RR3_REBOOT_OK=1，缺省恒拒）".to_string(),
        )),
    }
}

/// 发起重启：systemd 轨走 `systemctl reboot`，Buildroot/BusyBox 轨回退
/// `/sbin/reboot`、`/bin/reboot`——同一内核动作，两条装机轨都可达。
/// 三条依次尝试，成功即返回；全失败点名每一条（不猜"哪条才该在"）。
fn rr3_reboot_run() -> Result<(), String> {
    const TRIES: [(&str, &[&str]); 3] = [
        ("systemctl", &["reboot"]),
        ("/sbin/reboot", &[]),
        ("/bin/reboot", &[]),
    ];
    let mut errs = Vec::new();
    for (cmd, args) in TRIES {
        match std::process::Command::new(cmd).args(args).status() {
            Ok(st) if st.success() => return Ok(()),
            Ok(st) => errs.push(format!("{cmd}: {st}")),
            Err(e) => errs.push(format!("{cmd}: {e}")),
        }
    }
    Err(errs.join("; "))
}

/// 处理注册后的入站帧。`Err` = 协议违规，该连接按崩溃收尾。
fn rr3_serve_frame(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    frame: &Rr3Frame,
) -> Result<Rr3Flow, String> {
    if frame.header.dst_task != RR3_TASK_RRD {
        return Err(format!(
            "上行 dst_task 必须为 {RR3_TASK_RRD}，收到 {}",
            frame.header.dst_task
        ));
    }
    let (ty, body) =
        rr3_body_split(&frame.payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
    if frame.header.src_task != me.task {
        // AOS-STD-IPC-024：src_task 由 rrd 分配，用户态不得自设。
        // 拒这一帧但不断连——一次笔误不该让整个进程掉线（spec §4"rrd 不崩"的连接版）。
        rr3_reply_err(
            out,
            me,
            rr3_best_effort_id(&body),
            Rr3StatusCode::Invalid,
            &format!(
                "src_task 由 rrd 分配，本连接分配值 {}，收到 {}",
                me.task, frame.header.src_task
            ),
        );
        return Ok(Rr3Flow::Keep);
    }
    match ty {
        Rr3PayloadType::Request => rr3_serve_request(out, ctx, me, &body),
        Rr3PayloadType::Event => rr3_publish(out, ctx, me, &body),
        // M3：上行 Response 是 rr3.call 的应答回路（trace_id 改道回调用方）
        Rr3PayloadType::Response => rr3_route_response(out, ctx, me, frame, &body),
        // 拒是默认值，放行才需要理由：Request/Event/Response 之外的上行（Notification
        // 冒充上行）仍然一律拒帧不断连。
        other => {
            rr3_reply_err(
                out,
                me,
                rr3_best_effort_id(&body),
                Rr3StatusCode::Invalid,
                &format!("上行不支持 payload 类型 {other:?}（业务发布请用 Event）"),
            );
            Ok(Rr3Flow::Keep)
        }
    }
}

/// 业务发布：三层归属检查在 Task 5 的关联函数里，这里只负责"拒的话怎么回、过的话怎么转"。
/// 三种**归属**拒法（冒充 / 越权前缀 / 未在册）都回错误帧并计数，但**不断连**——发布越权是数据面
/// 问题，不该带走控制面（与 `src_task` 伪造、payload 类型不支持同一口径：回帧 + `Rr3Flow::Keep`）。
/// 体的形状错误（非 JSON、缺/坏 `topic`）同理的是**回帧 + 不断连 + 不记崩溃**那一半：那是客户端的
/// 一次笔误，不是协议失步，按崩溃收尾会白记一次 `rr3_reg_death` 并可能触发重启。
/// 它**不计数**——`rr3_reg_note_denied` 只在上面那三种归属拒法上调用，口径与 spec §6 的计数条款
/// （只绑"被拒的越权发布"）一致；把形状错也算进 denied 会让这个数变成"客户端错误总数"。
///
/// `sender_name` 取 `me.name`——它是**这条连接注册时**由 rrd 收下并分配的在册身份
/// （`Rr3Peer` 随握手成功才存在），不是从帧体或注册报文里现读的字。`registry.rs` 那句
/// "唯一防线建在调用侧"的前提到这里是成立的：帧上的 `src_task` 已在上面被强制等于
/// `me.task`，而 `me.name` 与 `me.task` 出自同一次 `rr3_reg_add`。
fn rr3_publish(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    body: &[u8],
) -> Result<Rr3Flow, String> {
    let (parsed, topic) = match rr3_pub_parse(out, me, body) {
        Ok(v) => v,
        Err(flow) => return Ok(flow),
    };
    if let Some(flow) = rr3_pub_gate(out, ctx, me, body, &topic) {
        return Ok(flow);
    }
    rr3_pub_forward(ctx, me, body, &topic);
    rr3_pub_batt(ctx, &topic, &parsed);
    Ok(Rr3Flow::Keep)
}

/// Event 体的解析与 topic 提取。两条形状错（非 JSON / 缺坏 topic）都回 Invalid 帧
/// 并保持连接：`Err` 携带已应答的 `Keep`，调用方不再重复回帧。
fn rr3_pub_parse(
    out: &Arc<Mutex<UnixStream>>,
    me: &Rr3Peer,
    body: &[u8],
) -> Result<(Value, String), Rr3Flow> {
    let parsed = match serde_json::from_slice::<Value>(body) {
        Ok(v) => v,
        Err(e) => {
            rr3_reply_err(
                out,
                me,
                rr3_best_effort_id(body),
                Rr3StatusCode::Invalid,
                &format!("Event 体必须是 JSON 对象: {e}"),
            );
            return Err(Rr3Flow::Keep);
        }
    };
    let topic = match rr3_rpc_topic_param(&parsed) {
        Ok(t) => t,
        Err((_, msg)) => {
            rr3_reply_err(
                out,
                me,
                rr3_best_effort_id(body),
                Rr3StatusCode::Invalid,
                &msg,
            );
            return Err(Rr3Flow::Keep);
        }
    };
    Ok((parsed, topic))
}

/// 归属门（三层归属检查的应答侧）：拒则计数 + 回错误帧 + `Some(Keep)`；
/// 放行返回 `None`。三种归属拒法都不断连（与 src_task 伪造同一口径）。
fn rr3_pub_gate(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    body: &[u8],
    topic: &str,
) -> Option<Rr3Flow> {
    let deny = {
        let check = RrdRegistry::rr3_reg_pub_check(&me.name, topic, &ctx.topics);
        if let Err(e) = check {
            rr3_lock(&ctx.shared).rr3_reg_note_denied();
            Some(e)
        } else {
            None
        }
    };
    let e = deny?;
    rr3_reply_err(
        out,
        me,
        rr3_best_effort_id(body),
        rr3_rpc_deny_status(&e),
        &e.to_string(),
    );
    Some(Rr3Flow::Keep)
}

/// 转发：保留体，头改写成"发布者 task + 广播目的"，类型仍是 Event。
/// 这里第三处自己构造帧（另两处是 `rr3_out_frame` 的应答）：转发帧的 src 是**发布者**
/// 而不是 rrd，套不进那个 src 恒为 RR3_TASK_RRD 的写法。
/// `payload_len` 必须从**编出来的字节串**取，不能从 `body` 取——少了那 2 字节前缀
/// 就是 Task 1 帧层的 `LengthMismatch`，收侧当场把这条链打死。
fn rr3_pub_forward(ctx: &Rr3ConnCtx, me: &Rr3Peer, body: &[u8], topic: &str) {
    let payload = rr3_body_encode(Rr3PayloadType::Event, body);
    let frame = Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: rr3_now_ns(),
            src_task: me.task,
            dst_task: rr_bus::RR3_BROADCAST_DST_TASK,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    };
    // 排除项是发布者自己：模块不该收到自己发出的广播（`rr3_reg_fanout` 只按 task 排除，
    // 不看帧的 dst，见该函数文档）。
    // 留痕只在接收连接数变化（含首次）时落——音频 PCM 每帧一条会把日志环冲垮。
    let (n, changed) = {
        let mut g = rr3_lock(&ctx.shared);
        let n = g.rr3_reg_fanout(&frame, me.task);
        // 影子统计（广播面 ADR 前哨）：只按 manifest subscribes 记账"本应过滤"的
        // 目标数，不改上面的全量投递——计数面 = rr3.ctl.counters 的 fanout_shadow*
        g.rr3_reg_shadow(topic, me.task);
        let changed = g.rr3_reg_fan_changed(topic, n);
        (n, changed)
    };
    if changed {
        rr3_log(&format!("转发 {topic} 到 {n} 个连接"));
    }
}

/// 低电兜底（0.0.2 方案 D6/Q4 裁定，architecture §7.3 横切路径）：rrd 是总线本体，
/// "订阅 battery/low|ok"就落在本转发点——每一帧 Event 都从这里过，谁发、值多少都已知。
/// 四件动作面：置全局低电旗（上升沿恰一次）→ supervisor 的 spawn 漏斗拒新 spawn →
/// 点名广播 rr3-runtime/system/power → 日志环（本函数的 rr3_log 即入口）。
/// 解除路径（批N5 件二笔3，§17 运维留痕）：收到 battery/ok（恢复沿）清旗——
/// 下降沿恰一次点名广播 reason=battery-ok，spawn 漏斗自此放行新 spawn；重复
/// battery/ok（旗本就没置）不重复广播。battery/low|ok 本身照常已转发，兜底是
/// 额外动作，不截断事件流。
fn rr3_pub_batt(ctx: &Rr3ConnCtx, topic: &str, parsed: &Value) {
    if topic == RR3_BATTERY_LOW_TOPIC {
        let rising = rr3_lock(&ctx.shared).rr3_reg_lp_raise();
        if rising {
            let percent = parsed.get("percent").and_then(Value::as_u64).unwrap_or(0);
            let voltage_mv = parsed
                .get("voltage_mv")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let n = rr3_lock(&ctx.shared).rr3_reg_fanout(
                &crate::daemon::rr3_power_frame("battery-low", percent, voltage_mv),
                rr_bus::RR3_TASK_RRD,
            );
            rr3_log(&format!(
                "低电兜底触发：battery/low percent={percent} voltage_mv={voltage_mv} → 置全局低电旗、停止新 spawn，广播 rr3-runtime/system/power 到 {n} 个连接"
            ));
        } else {
            rr3_log("低电兜底已生效：重复 battery/low 不重复广播（同一次低电段只点名一次）");
        }
    } else if topic == RR3_BATTERY_OK_TOPIC {
        let falling = rr3_lock(&ctx.shared).rr3_reg_lp_lower();
        if falling {
            let percent = parsed.get("percent").and_then(Value::as_u64).unwrap_or(0);
            let voltage_mv = parsed
                .get("voltage_mv")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let n = rr3_lock(&ctx.shared).rr3_reg_fanout(
                &crate::daemon::rr3_power_frame("battery-ok", percent, voltage_mv),
                rr_bus::RR3_TASK_RRD,
            );
            rr3_log(&format!(
                "低电兜底解除：battery/ok percent={percent} voltage_mv={voltage_mv} → 清全局低电旗、恢复新 spawn，广播 rr3-runtime/system/power(reason=battery-ok) 到 {n} 个连接"
            ));
        } else {
            rr3_log("低电兜底本未触发：重复 battery/ok 不重复广播（旗未置位无从解除）");
        }
    }
}

/// 三个上行方法。RPC 解析失败一律回 Invalid——Task 6 已保证解析器不产别的码，
/// 这里不重建 id→code 的映射，避免第二套真值。
fn rr3_serve_request(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    body: &[u8],
) -> Result<Rr3Flow, String> {
    let req = match rr3_rpc_parse(body) {
        Ok(r) => r,
        Err((_, msg)) => {
            rr3_reply_err(
                out,
                me,
                rr3_best_effort_id(body),
                Rr3StatusCode::Invalid,
                &msg,
            );
            return Ok(Rr3Flow::Keep);
        }
    };
    match req.method {
        Rr3RpcMethod::Register => {
            rr3_reply_err(
                out,
                me,
                req.id,
                Rr3StatusCode::Invalid,
                "该连接已注册，拒绝重复 rr3.register",
            );
            Ok(Rr3Flow::Keep)
        }
        // ADR-0010：rr3.ctl.* 只对未注册的控制连接开放（握手分流）；已注册模块连接
        // 上行一律拒——回错误帧不断连，与 src_task 伪造同一口径（一次笔误不掉线）。
        // 显式 OR 臂而非 is_ctl 守卫：带守卫的臂不计入穷尽性，新增方法时这里必须表态。
        Rr3RpcMethod::CtlStatus
        | Rr3RpcMethod::CtlHealth
        | Rr3RpcMethod::CtlLogs
        | Rr3RpcMethod::CtlOta
        | Rr3RpcMethod::CtlCounters
        | Rr3RpcMethod::CtlReboot
        | Rr3RpcMethod::CtlWiring
        | Rr3RpcMethod::CtlContracts
        | Rr3RpcMethod::CtlDomainSet => {
            rr3_reply_err(
                out,
                me,
                req.id,
                Rr3StatusCode::Permission,
                "rr3.ctl.* 只对未注册的控制连接开放，模块请走 rr3.register / rr3.heartbeat",
            );
            Ok(Rr3Flow::Keep)
        }
        Rr3RpcMethod::Heartbeat => rr3_beat_handle(out, ctx, me, req.id, &req.params),
        Rr3RpcMethod::Unregister => {
            // 主动退场 ≠ 崩溃：直接移出在册表，不走 restart_policy。
            // 广播留给收尾处，保证"一次退出只有一条 state"。
            let removed = rr3_lock(&ctx.shared).rr3_reg_remove(&me.name);
            rr3_reply_ok(out, me, req.id, &json!({"removed": removed}));
            Ok(Rr3Flow::QuitClean)
        }
        Rr3RpcMethod::Call => rr3_route_call(out, ctx, me, req.id, &req.params, body),
    }
}

/// 心跳：计数归并先行（对端读 counters 不得读到上一拍）→ D-6 三态回报 →
/// Promoted 时广播 running 并做依赖恢复传播。
fn rr3_beat_handle(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    id: u64,
    params: &Value,
) -> Result<Rr3Flow, String> {
    rr3_beat_counters(ctx, me, params);
    let beat = rr3_lock(&ctx.shared).rr3_reg_beat(me.task, ctx.rr3_ctx_now_ms());
    match beat {
        Rr3Beat::Rejected => {
            rr3_reply_err(
                out,
                me,
                id,
                Rr3StatusCode::TimedOut,
                "该 task 已过期或不在册，需重新注册",
            );
        }
        other => {
            rr3_reply_ok(out, me, id, &json!({"beat": "accepted"}));
            // 只在 Promoted 上广播：Kept 若也发，每次心跳都会重发一条 state
            // （spec §5.0 的 state 是转移流，不是快照流）
            if other == Rr3Beat::Promoted {
                rr3_beat_promote(ctx, me);
            }
        }
    }
    Ok(Rr3Flow::Keep)
}

/// 心跳可选 counters 字段归并 + R26 合并写（变化 → 闸门 → 快照 → 落盘）。
/// 计数是运维账，不跟着判活走（Rejected 也照归并），但必须先于应答落账。
fn rr3_beat_counters(ctx: &Rr3ConnCtx, me: &Rr3Peer, params: &Value) {
    let Some(counters) = rr3_rpc_counters(params) else {
        return;
    };
    let changed = rr3_lock(&ctx.shared).rr3_reg_ctr_merge(&me.name, &counters);
    if !changed || ctx.counters_path.is_none() {
        return;
    }
    // 整段持闸：on_change→快照→store→on_flushed 必须在同一临界区。锁序
    // gate→shared 与全仓既有方向一致（merge 那手的 shared 锁已放）。
    let mut gate = rr3_lock(&ctx.counters_gate);
    if !gate.rr3_on_change(ctx.rr3_ctx_now_ms()) {
        return;
    }
    let snap = rr3_lock(&ctx.shared).rr3_reg_counters();
    let Some(path) = ctx.counters_path.as_deref() else {
        return;
    };
    match crate::counters::rr3_counters_store(path, &snap) {
        Ok(()) => gate.rr3_on_flushed(ctx.rr3_ctx_now_ms()),
        Err(e) => rr3_log(&format!("counters 落盘失败（下一拍窗口到自然重试）: {e}")),
    }
}

/// `registered → running` 的唯一升级时刻：广播 running + 依赖恢复传播。
/// 排除项 `RR3_TASK_RRD` = 不排除——发起者自己也要收到关于自己的升级。
fn rr3_beat_promote(ctx: &Rr3ConnCtx, me: &Rr3Peer) {
    let frame = rr3_state_frame(&me.name, Rr3Phase::Running.as_str(), me.task);
    let n = rr3_lock(&ctx.shared).rr3_reg_fanout(&frame, RR3_TASK_RRD);
    rr3_log(&format!("{} → running（广播到 {n} 个连接）", me.name));
    // ADR-0009 裁定 3 恢复半边：提供者回到 Running → 依赖者逐个重算，全部
    // required 就位才回 Running。谁记的账谁传播。
    let recovered = rr3_lock(&ctx.shared).rr3_reg_undegrade(&me.name);
    for (dep, task) in recovered {
        let n = rr3_lock(&ctx.shared).rr3_reg_fanout(
            &rr3_state_frame(&dep, Rr3Phase::Running.as_str(), task),
            RR3_TASK_RRD,
        );
        rr3_log(&format!(
            "{dep} 依赖恢复 → running（提供者 {} 回到 Running）广播到 {n} 个连接",
            me.name
        ));
    }
}

/// `rr3.call` 的路由（M3，architecture.md §6"模块间请求"）：按 params.topic 查
/// `[rpc].exposes` 它的在册模块，把**原始 body 原样**转发成 Request 帧——
/// src = 调用方 task、dst = 被调方 task、**trace_id = 调用方 task**（应答回路的
/// 唯一凭据，被调方应答时必须原样带回头上的 trace_id，见 `rr3_route_response`）。
///
/// 这里**没有同步应答**：rrd 是转发器不是代理，调用方在 `rr3_call_topic` 上等的
/// 就是被调方沿 Response 回路送回来的那一帧。拒法（畸形参数 / 无人在册 / 自呼 /
/// 被调方已死）都回错误帧 + 不断连，口径与 publish 的形状错一致；无人在册是
/// `Invalid` 不是 `Busy`——"没人接"是路由事实，不是容量问题。
fn rr3_route_call(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    id: u64,
    params: &Value,
    body: &[u8],
) -> Result<Rr3Flow, String> {
    // 参数先在锁外验（畸形是调用方的笔误，用不着查表）
    let topic = match rr3_rpc_call_params(params) {
        Ok((t, _)) => t,
        Err((_, msg)) => {
            // 解析器只产 Invalid（Task 6 的口径），这里不重建 id→code 映射
            rr3_reply_err(out, me, id, Rr3StatusCode::Invalid, &msg);
            return Ok(Rr3Flow::Keep);
        }
    };
    // 查表 + 构帧 + 投递在同一把锁内完成：避免"查到 callee、锁一放它就死了"的窗口里
    // 构出一帧注定无处投递的转发。锁内 send 与 `rr3_reg_fanout` 同一口径（无界 mpsc 不阻塞）。
    let outcome: Result<bool, String> = {
        let g = rr3_lock(&ctx.shared);
        match g.rr3_reg_callee_for(&topic) {
            None => Err(format!(
                "没有在册模块 exposes 主题 {topic}（被调方须在 manifest 的 [rpc].exposes 里声明）"
            )),
            Some((_, task)) if task == me.task => {
                Err(format!("主题 {topic} 的唯一被调方就是调用方自己，拒绝自呼"))
            }
            Some((name, task)) => {
                let payload = rr3_body_encode(Rr3PayloadType::Request, body);
                let frame = Rr3Frame {
                    header: Rr3Header {
                        opcode: opcode::SEND,
                        flags: 0,
                        trace_id: me.task,
                        timestamp_ns: rr3_now_ns(),
                        src_task: me.task,
                        dst_task: task,
                        payload_len: payload.len() as u32,
                        payload_crc32: rr_ipc::rr3_crc32(&payload),
                    },
                    payload,
                };
                rr3_log(&format!(
                    "rr3.call {topic}: {}(task={}) → {name}(task={task})",
                    me.name, me.task
                ));
                Ok(g.rr3_reg_send_to(task, &frame))
            }
        }
    };
    match outcome {
        Ok(true) => {}
        // 查表时还活着、投递时收件人没了：对调用方而言就是"这一拍没人接"
        Ok(false) => {
            rr3_reply_err(
                out,
                me,
                id,
                Rr3StatusCode::Invalid,
                &format!("主题 {topic} 的被调方在投递前退场"),
            );
        }
        Err(msg) => rr3_reply_err(out, me, id, Rr3StatusCode::Invalid, &msg),
    }
    Ok(Rr3Flow::Keep)
}

/// 上行 Response 的回路（M3）：rr3.call 的应答去路。头上的 `trace_id` 是**调用方
/// task**（`rr3_route_call` 转发时写入，被调方须原样带回）；body 原样转发，只把
/// dst 改写成 trace_id。trace_id = 0 无从投递，回 Invalid；调用方已不在册（已退场 /
/// 已过期）同样无处投递——两种都是"拒帧不断连"，与 src_task 伪造同一口径。
fn rr3_route_response(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    frame: &Rr3Frame,
    body: &[u8],
) -> Result<Rr3Flow, String> {
    let trace = frame.header.trace_id;
    if trace == 0 {
        rr3_reply_err(
            out,
            me,
            rr3_best_effort_id(body),
            Rr3StatusCode::Invalid,
            "上行 Response 缺回路：trace_id 必须为调用方 task（0 = 无从投递）",
        );
        return Ok(Rr3Flow::Keep);
    }
    let payload = rr3_body_encode(Rr3PayloadType::Response, body);
    let fwd = Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: trace,
            timestamp_ns: rr3_now_ns(),
            src_task: me.task,
            dst_task: trace,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    };
    if !rr3_lock(&ctx.shared).rr3_reg_send_to(trace, &fwd) {
        rr3_reply_err(
            out,
            me,
            rr3_best_effort_id(body),
            Rr3StatusCode::Invalid,
            &format!("应答的调用方 task={trace} 已不在册，无处投递"),
        );
    }
    Ok(Rr3Flow::Keep)
}

/// 收尾：死亡记账（决定要不要重启）+ **至多**一条 state 广播 + 把计划交出去。
///
/// 11b 之后"三条公开路"的分工是：本函数（conn 见 EOF/unregister）、watchdog 到期
/// （`daemon.rs`）、子进程退出码（`supervisor.rs` 的 `rr3_reap`）。三条各自只发**自己那一条**
/// state，护栏都在 registry：`rr3_reg_death` 对已 failed 的条目回 `Silent`、对 owned 的回
/// `DeferredToExitStatus`，`rr3_reg_child_exit` 对已 failed 的同样回 `Silent`——
/// 所以"一次死亡被记两次 / 广播两次"在表上不可能（ledger R-A4 结清点，线上由
/// `watchdog_broadcasts_exactly_one_failed_state` 与 `e2e_spawn_restart` 的 S2 各自钉住一半）。
///
/// 另两处 state 广播点（`registered` 与 `running`）不走本函数：它们与死亡无关。
fn rr3_conn_finish(ctx: &Rr3ConnCtx, me: &Rr3Peer, clean_exit: bool, reason: &str) {
    // 死亡记账的结果必须先绑到 `let`：若把 `rr3_lock(..).rr3_reg_death(..)` 直接写进 `match` 的
    // 被匹配表达式，返回的 `MutexGuard` 临时量会活到整个 `match` 语句结束（Rust 的临时量寿命
    // 规则： scrutinee 位置的临时量延寿到所属语句收尾），于是 `Broadcast` 分支里再 `rr3_lock`
    // 同一把在册表锁就成了同线程重入自死锁（std Mutex 不可重入）——收尾永远走不到 fanout 与
    // 日志，e2e 的三条 EOF/退场用例集体 3s 超时。绑到 `let` 后守卫在 `;` 处即释放。
    let death = rr3_lock(&ctx.shared).rr3_reg_death(&me.name, clean_exit, ctx.rr3_ctx_now_ms());
    match death {
        // owned（本守护起的进程）：EOF 不改相位/不广播/不记账，三件事一起让给退出码那一路
        //（`supervisor.rs::rr3_reap`）。这里必须留一行痕迹：否则运维只看日志会以为这条连接
        // 什么都没发生，而"没发生"和"让给了别人"是两件不同的话。
        // 刻意不写"退出"两字：`rr3_assert_no_log("退出")` 那类否定 needle 测的是 EOF 收尾文案，
        // 这一路今天只在传了 `--manifest` 的用例里可达（见 A-5③ 第 3 条）。
        Rr3Death::DeferredToExitStatus => {
            rr3_log(&format!(
                "{} 连接断开（{reason}），终态交给子进程退出码那一路",
                me.name
            ));
        }
        // 已被 watchdog 判死：终态广播与重启预算都归 watchdog 那一拍，收尾不得双计（R-A4）。
        // 同样不打计划、不投 supervisor——那份计划到期时已经投过一次。
        Rr3Death::Silent => {
            rr3_log(&format!(
                "{} 连接断开（{reason}），不重复广播：终态已由到期扫描记过",
                me.name
            ));
        }
        // 刚由 live 转终态，或 unregister 后条目已摘（plan=None）：调用方广播恰好一条 state。
        Rr3Death::Broadcast { plan } => {
            // D-7：死因→phase 的字面量只在 registry 一处，conn 不再各写各的。
            let phase = rr3_phase_for_exit(clean_exit);
            // E-9：这里排 `RR3_TASK_RRD` 等于不排除（发起者自己也收到关于自己的终态）；
            // supervisor 收尸那一路排的是 `crashed.src_task`。两种口径是**已知**的第三/第四种
            // 口径，归 Task 13 的 A-3 文档化，本任务不"统一"——统一成哪一种都没有判据支持。
            let n = rr3_lock(&ctx.shared)
                .rr3_reg_fanout(&rr3_state_frame(&me.name, phase, me.task), RR3_TASK_RRD);
            match plan {
                Some(p) => {
                    rr3_log(&format!(
                        "{} 退出（{reason}）→ 重启计划 attempt={} delay_ms={} entry={}（非本守护起的模块不 spawn）",
                        p.name, p.attempt, p.delay_ms, p.entry
                    ));
                    // 投出去就完事：起不起由 supervisor 按 `jobs`（"我能起谁"的全集）裁定（E-1）。
                    // 顺序要紧：日志在投之前——`rr3_restart` 只做一次 send，但把"计划是什么"
                    // 钉在"谁拿到了它"前面，读日志时才不会先看到结果再找原因。
                    ctx.sup.rr3_restart(p);
                }
                None => rr3_log(&format!(
                    "{} 退出（{reason}），无重启，广播 {phase} 到 {n} 个连接",
                    me.name
                )),
            }
            // ADR-0009 裁定 3：本路刚记完一条死亡/注销（EOF 或 unregister），若死者
            // 是某能力在 boot 全集里的提供者且再无人在位 → 下游逐个置 Degraded 并广播。
            // 只置位与广播，不记账不出计划（裁定 4）。Deferred/Silent 两路不在这里传播：
            // 死亡分别由退出码那一路（supervisor）与到期那一路（watchdog）记，各自的
            // 落点负责各自的传播，一次死亡恰好传播一次。
            let degraded = rr3_lock(&ctx.shared).rr3_reg_degrade(&me.name);
            for (dep, task) in degraded {
                let n = rr3_lock(&ctx.shared).rr3_reg_fanout(
                    &rr3_state_frame(&dep, Rr3Phase::Degraded.as_str(), task),
                    task,
                );
                rr3_log(&format!(
                    "{dep} 依赖断供 → degraded（提供者 {} 离开 Running）广播到 {n} 个连接",
                    me.name
                ));
            }
        }
    }
}

/// 服务一条连接。返回 `Err` 只描述**这一条**连接的断开原因；守护进程与别的连接不受影响。
pub fn rr3_conn_serve(stream: UnixStream, ctx: Rr3ConnCtx) -> Result<(), String> {
    let out: Arc<Mutex<UnixStream>> = Arc::new(Mutex::new(
        stream
            .try_clone()
            .map_err(|e| format!("try_clone 失败: {e}"))?,
    ));
    // 写超时（0.0.8b 审计 B-5）：对端不收时 write 不得无限期占着 writer 线程与
    // out 锁——5s 写不进 = 对端判死，writer 退出、通道 drop、fanout 走 Disconnected
    // 回收。缺了它，一个卡死的订阅者能永久占一条 writer 线程。
    rr3_lock(&out)
        .set_write_timeout(Some(RR3_FAN_WRITE_TIMEOUT))
        .map_err(|e| format!("set_write_timeout 失败: {e}"))?;
    // 有界收件队列（RR3_FAN_CAP）：满侧策略见 registry.rs fanout doc——僵尸订阅者
    // 的积压被封顶在"上界 × 帧大小"，不再无界吃到 rrd 被 OOM 杀。
    let (fan_tx, fan_rx) = mpsc::sync_channel::<Rr3Frame>(RR3_FAN_CAP);
    let writer: JoinHandle<()> = {
        let out = out.clone();
        thread::spawn(move || {
            for frame in fan_rx.iter() {
                if !rr3_write(&out, &frame) {
                    break;
                }
            }
        })
    };

    // 注册前设读超时：连上却不注册的客户端不能永久占着线程（spec §5.0）
    stream
        .set_read_timeout(Some(Duration::from_millis(
            ctx.pre_register_timeout_ms.max(1),
        )))
        .map_err(|e| format!("set_read_timeout 失败: {e}"))?;
    let mut reader = stream;
    let me = rr3_handshake(&mut reader, &out, &ctx, fan_tx)?;
    reader
        .set_read_timeout(None)
        .map_err(|e| format!("清读超时失败: {e}"))?;

    let outcome: Result<Rr3Flow, String> = loop {
        match rr3_frame_read_cap(&mut reader, RR3_FRAMING_PLEN_CAP_DEFAULT) {
            Ok(frame) => match rr3_serve_frame(&out, &ctx, &me, &frame) {
                Ok(Rr3Flow::Keep) => {}
                Ok(Rr3Flow::QuitClean) => break Ok(Rr3Flow::QuitClean),
                Err(reason) => break Err(reason),
            },
            Err(Rr3FrameError::Closed) => break Ok(Rr3Flow::QuitClean),
            Err(e) => break Err(format!("帧读取失败: {e:?}")),
        }
    };

    let clean_exit = outcome.is_ok();
    let reason = match &outcome {
        Ok(_) => "干净收尾".to_string(),
        Err(e) => e.clone(),
    };
    rr3_conn_finish(&ctx, &me, clean_exit, &reason);
    // 通道收口（0.0.8b 审计 B-5 落地，原 Task 10 carry）：条目随注册表释放 →
    // 最后一个 Sender drop → writer 的 iter() 自然结束。僵尸在册条目（注册后不再
    // 读 socket 的模块）由两路兜住：① 有界队列——writer 阻塞在写时积压封顶
    // RR3_FAN_CAP 帧，fanout 满侧**丢帧但保留连接**（本轮审计修正：置 tx=None 会
    // 让心跳仍通的模块永久失聪）；② 写超时——writer 5s 写不进即退出、通道 drop，
    // fanout 走 Disconnected 才置 tx=None。两条路都不再无界吃内存。
    let _ = writer.join();
    let _ = reader.shutdown(std::net::Shutdown::Both);
    outcome.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ctl.logs 参数：缺省合法；坏形 fail-closed 拒（不静默折默认）。
    #[test]
    fn log_args_default_and_bad_shapes() {
        let (p, t) = rr3_log_args(&json!({})).expect("全缺省合法");
        assert!(p.is_empty() && t == 0);
        let (p, t) = rr3_log_args(&json!({"prefix": "启动摘要", "tail": 3})).expect("正例");
        assert_eq!((p.as_str(), t), ("启动摘要", 3));
        assert!(
            rr3_log_args(&json!({"prefix": 1})).is_err(),
            "prefix 非串须拒"
        );
        assert!(rr3_log_args(&json!({"tail": -1})).is_err(), "tail 负数须拒");
        assert!(
            rr3_log_args(&json!({"tail": "x"})).is_err(),
            "tail 非数须拒"
        );
    }
}
