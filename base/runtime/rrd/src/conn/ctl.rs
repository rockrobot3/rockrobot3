//! 控制旁支面（自 `conn.rs` 抽出，批B12 件三）：`rr3.ctl.*` 一次性查询连接。
//!
//! ADR-0010 裁定 2/4：ctl 连接不注册、不进在册表、不受 watchdog 管——在册表是
//! "活的业务模块"的账本，运维探针不记账。一问一答即断，应答按对端**自报**的
//! `src_task` 回（rrd 尚未分配 task）。
//!
//! 判据原样搬移：方法派发、状态码映射、reboot 双重门、logs 参数 fail-closed
//! 逐字保留；写出口仍是父模块的单帧原子 `rr3_write`。

use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use super::*;

/// ctl 分派表的臂签名：`(ctx, params)` 入、应答体出。只有 logs/domain.set 用
/// params，其余臂忽略它以统一表型（臂内语义仍单点在各自函数）。
type Rr3CtlArm = fn(&Rr3ConnCtx, &Value) -> Result<Value, (Rr3StatusCode, String)>;

/// 控制旁支（ADR-0010 裁定 2/4）：一次性查询连接，一问一答即断。
///
/// 不注册、不进在册表、不受 watchdog 管——在册表是"活的业务模块"的账本，运维探针
/// 不记账。返回值作为握手 `Err` 的理由交回 `rr3_conn_serve`：握手失败路径不做死亡记账
/// （`me` 尚不存在），daemon 侧按既有"连接收尾"落一行日志，连接随之关闭。
/// 应答按对端**自报**的 `src_task` 回（与注册期 `rr3_refuse` 同一口径——rrd 尚未分配 task）。
pub(super) fn rr3_serve_ctl(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    reported_task: u64,
    req: Rr3RpcRequest,
) -> String {
    // 先在锁内取数、锁外应答：与握手主路的锁纪律一致（持 shared 锁时不碰 out 锁）
    // 分派表键 = 方法变体（成员与 [`Rr3RpcMethod::is_ctl`] 白名单同源）。
    let arms: [(Rr3RpcMethod, Rr3CtlArm); 9] = [
        (Rr3RpcMethod::CtlStatus, |ctx, _| rr3_ctl_status(ctx)),
        (Rr3RpcMethod::CtlHealth, |ctx, _| rr3_ctl_health(ctx)),
        (Rr3RpcMethod::CtlLogs, |_, p| {
            rr3_log_args(p).map(|(prefix, tail)| json!({"lines": rr3_log_query(&prefix, tail)}))
        }),
        // 运维计数读面（0.0.4 批B，Q1 裁定）：与 CtlHealth 分开——health 是心跳健康
        // 语义（watchdog 判活的输入），counters 是业务运维账（"机器慢先看数字"）。
        // 键 = `模块名/键`（模块自报归并）或 `boots`（rrd 自有）；未持久化时只有内存账。
        (Rr3RpcMethod::CtlCounters, |ctx, _| {
            let counters = rr3_lock(&ctx.shared).rr3_reg_counters();
            Ok(json!({ "counters": counters }))
        }),
        // 恒拒（ADR-0010 裁定 4）：方法名进白名单把协议面定型，语义随 M6；
        // 契约状态码是冻结的 AIRY_E* 子集，用 Permission 而不是自造 NotSupported，
        // 与 rr3-ota-ctl switch 的"恒拒 + 文案点 M6"先例同构。
        (Rr3RpcMethod::CtlOta, |_, _| {
            Err((
                Rr3StatusCode::Permission,
                "OTA（含模块级 staging 更新）随 M6 落地，当前拒绝执行".to_string(),
            ))
        }),
        // 系统重启（0.0.4 批C，Q3 裁定）：执行面带**双重门**——cfg(linux) 且
        // env `RR3_REBOOT_OK=1`（装机服务单元/init 脚本注入）才真跑重启命令；
        // 其余一律 Permission 拒 + 日志点名。为什么加 env 门：cfg(linux) 单独
        // 解锁会让 CI 的 ubuntu runner 在 e2e 里真重启——e2e 可测性要求缺省路径
        // 恒拒，解锁面留装机环境（distro 的 rrd.service 与 buildroot 的 S99rrd 同设）。
        (Rr3RpcMethod::CtlReboot, |_, _| rr3_ctl_reboot()),
        // 装配快照读口（批N4 件九笔4）：boot 装载链验收后的四路底账 + [pins]×
        // wiring 全交叉逐条判果，一应答给出（判据真值单点 = assembly.rs）。快照
        // 未在册（--manifest 调试路径）时如实回不可得，不造假数据。
        (Rr3RpcMethod::CtlWiring, |_, _| {
            Ok(crate::assembly::rr3_assembly_json())
        }),
        // 契约完整性审计读口（批N5 件五笔4，Z-4）：boot 装载链验收后的契约四册
        // sha256 记账 + "齐全且 sha 对"自检判果一应答给出（判据真值单点 =
        // contracts.rs）。快照未在册（--manifest 调试路径）如实回不可得。
        (Rr3RpcMethod::CtlContracts, |_, _| {
            Ok(crate::contracts::rr3_contracts_json())
        }),
        // 装配侧写口（批N22 件一，conventions §6.1 三写口 + ADR-0021 裁定 6）：
        // 门序编排全在 domain_write.rs 单点（confirm 门 → 证据四道核 → key 白名单
        // → 文本手术 → 装载链全量预核 → 原子写回 → 契约记账重记），此处只接线。
        // 快照未在册由 run 薄壳如实拒（--manifest 调试路径无装配单可写）。
        (Rr3RpcMethod::CtlDomainSet, |_, p| {
            crate::domain_write::rr3_domain_write_run(p)
        }),
    ];
    let result: Result<Value, (Rr3StatusCode, String)> =
        match arms.iter().find(|(m, _)| *m == req.method) {
            Some((_, arm)) => arm(ctx, &req.params),
            // 到不了：握手只把 is_ctl 的请求分进来（解析器与分流判据同一真值）
            None => Err((
                Rr3StatusCode::Invalid,
                format!("ctl 连接只接受 rr3.ctl.*，收到 {}", req.method.as_str()),
            )),
        };
    let payload = rr3_ctl_payload(req.id, &result);
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

/// 应答体：Ok 出结果帧、Err 出错误帧。
fn rr3_ctl_payload(req_id: u64, result: &Result<Value, (Rr3StatusCode, String)>) -> Vec<u8> {
    match result {
        Ok(v) => rr3_rpc_build_result(req_id, v),
        Err((code, msg)) => rr3_rpc_build_error(req_id, *code, msg),
    }
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
        match rr3_cmd_err(cmd, std::process::Command::new(cmd).args(args).status()) {
            Some(e) => errs.push(e),
            None => return Ok(()),
        }
    }
    Err(errs.join("; "))
}

/// 单条重启命令的结果 → 失败描述（成功返回 None）。
fn rr3_cmd_err(cmd: &str, st: std::io::Result<std::process::ExitStatus>) -> Option<String> {
    match st {
        Ok(st) if st.success() => None,
        Ok(st) => Some(format!("{cmd}: {st}")),
        Err(e) => Some(format!("{cmd}: {e}")),
    }
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
