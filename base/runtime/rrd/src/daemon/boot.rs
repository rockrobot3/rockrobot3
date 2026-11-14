//! 装配链与启动摘要：boot 产物构造、后端/降级点名、日志落盘与独占绑定。

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rr_ipc::Rr3UdsListener;
use serde_json::json;

use crate::cli::RrdConfig;
use crate::registry::RrdRegistry;
use crate::{rr3_lock, rr3_log};

/// 装配链产物：--profile 正门与 --manifest 调试后门的统一输入
pub(super) struct Rr3DaemonBoot {
    pub(super) manifests: Vec<PathBuf>,
    /// 缺格模块（`degrade` 中类别为 `missing_slot` 的投影，boot 序跳过这些名字）
    pub(super) degraded: BTreeSet<String>,
    /// 降级原因（模块, 类别, 点名文案）：`missing_slot` / `assemble_broken`，
    /// 单源供启动摘要与 `ctl.health` 读回（远程诊断不必翻启动日志）
    pub(super) degrade: Vec<(String, String, String)>,
    pub(super) hal_bindings: String,
    pub(super) axis_state: String,
    pub(super) predegraded: BTreeSet<String>,
    pub(super) hal_backend: String,
    pub(super) audio_backend: Option<String>,
    pub(super) servo_backend: Option<String>,
    pub(super) display_backend: Option<String>,
    /// 装配单 [platform].tier（`--manifest` 后门为空）
    pub(super) tier: String,
    /// 装配单 [platform].chip（`--manifest` 后门为空）
    pub(super) chip: String,
}

/// 装配链（0.0.2 方案 D2，Q3 裁定）：--profile 是装机正门——enabled_slots 决定起谁，
/// 装载/查册/缺格判定/manifest 解析全部在 bind 之前完成（fatal 即非 0 退出，
/// 套接字不留残迹）；--manifest 调试后门维持原路径。两路径共用 plan_boot 仲裁与
/// supervisor spawn，不双真值。缺格不是致命项（方案 P2：缺格只降点名模块）。
pub(super) fn rr3_daemon_boot(cfg: &RrdConfig) -> Result<Rr3DaemonBoot, String> {
    let Some(p) = &cfg.profile else {
        // None 臂 = 仅 --manifest 调试后门（基-23）：裸起在 cli parse 处已非 0 退出。
        // 后门无装配单：hal_bindings/axis_state/hal_backend 空（spawn 不注入）、
        // 无缺格点名、无装配快照（ctl.wiring 如实回不可得）。
        return Ok(Rr3DaemonBoot {
            manifests: cfg.manifests.clone(),
            degraded: BTreeSet::new(),
            degrade: Vec::new(),
            hal_bindings: String::new(),
            axis_state: String::new(),
            predegraded: BTreeSet::new(),
            hal_backend: String::new(),
            audio_backend: None,
            servo_backend: None,
            display_backend: None,
            tier: String::new(),
            chip: String::new(),
        });
    };
    rr3_boot_profile(cfg, p)
}

/// --profile 正门：契约定位 → 装配链装载 → 日志环/快照/真值集记账定格
fn rr3_boot_profile(cfg: &RrdConfig, profile: &std::path::Path) -> Result<Rr3DaemonBoot, String> {
    // 格位/角色/器件契约与主题契约同目录（--contracts 运行期落点，T5）；接线档按
    // 装机布局由装配单推导（hardware/<形态>/wiring.toml，§4.3）
    let contracts = crate::topics_load::rr3_contracts_dir(cfg.contracts.as_deref());
    let slots = contracts.join("slots.toml");
    let roles = contracts.join("roles.toml");
    let devices = contracts.join("devices.toml");
    let platform = contracts.join("platform");
    let wiring = crate::wiring::rr3_wiring_path(profile)?;
    let boot =
        crate::profile::rr3_boot_inputs(profile, &slots, &roles, &devices, &wiring, &platform)?;
    // 日志环容量一次定格（批N5 件二 F-30）：装配单 [runtime].log_ring_cap；首份胜出
    if crate::rr3_log_ring_cap_set(boot.log_ring_cap) {
        rr3_log(&format!(
            "日志环容量定格 {} 行（装配单 [runtime].log_ring_cap）",
            boot.log_ring_cap
        ));
    }
    rr3_boot_notes(&boot);
    // 降级原因单源（模块, 类别, 点名文案）：缺格与装配缺项在此合流，
    // degraded/predegraded 两集合由它投影，`ctl.health` 读回同一份。
    let degrade: Vec<(String, String, String)> = boot
        .missing
        .iter()
        .map(|(m, slot)| (m.clone(), "missing_slot".to_string(), slot.clone()))
        .chain(
            boot.assemble_broken
                .iter()
                .map(|(m, msg)| (m.clone(), "assemble_broken".to_string(), msg.clone())),
        )
        .collect();
    let degraded: BTreeSet<String> = degrade
        .iter()
        .filter(|(_, kind, _)| kind == "missing_slot")
        .map(|(m, _, _)| m.clone())
        .collect();
    let predegraded: BTreeSet<String> = degrade
        .iter()
        .filter(|(_, kind, _)| kind == "assemble_broken")
        .map(|(m, _, _)| m.clone())
        .collect();
    // 装配快照定格（批N4 件九笔4）：boot 链验收通过的底账一次 set 进 OnceLock，
    // `rr3.ctl.wiring` 的渲染源；首份胜出，重复 boot 不覆盖。
    if let Some(s) = boot.snapshot.clone() {
        crate::assembly::rr3_assembly_set(s);
    }
    // 契约记账快照定格（Z-4/基-40）：契约四册 sha256 + 真值集七成员，`rr3.ctl.contracts`
    // 的渲染源与自检判据锚；记账重读失败 = 文件系统级异常，fail-closed 带因拒 boot。
    let cs =
        crate::contracts::rr3_contracts_scan(profile, &contracts, &wiring, &boot.platform_chip)?;
    if crate::contracts::rr3_contracts_set(cs) {
        rr3_log(
            "真值集记账定格：契约四册 Z-4 账＋真值集七成员基-40 账（rr3.ctl.contracts 自检判据锚）",
        );
    }
    Ok(Rr3DaemonBoot {
        manifests: boot.manifests,
        degraded,
        degrade,
        hal_bindings: boot.hal_bindings,
        axis_state: boot.axis_state,
        predegraded,
        hal_backend: boot.hal_backend,
        audio_backend: boot.audio_backend,
        servo_backend: boot.servo_backend,
        display_backend: boot.display_backend,
        tier: boot.platform_tier,
        chip: boot.platform_chip,
    })
}

/// 启动摘要（一行钉住本次装配的输入面与规模，排障先看这一行）。
/// 空档/芯片 = `--manifest` 后门（无装配单），显示 `-` 不造假值。
pub(super) fn rr3_boot_summary(
    tier: &str,
    chip: &str,
    profile: &str,
    modules: usize,
    predegraded: usize,
    missing: usize,
    backends: &str,
) -> String {
    let dash = |s: &str| {
        if s.is_empty() {
            "-".to_string()
        } else {
            s.to_string()
        }
    };
    format!(
        "启动摘要: tier={} chip={} profile={profile} modules={modules} 预降级={predegraded} 缺格={missing} 后端={backends}",
        dash(tier),
        dash(chip)
    )
}

/// 降级原因 → JSON 数组（`module` / `kind` / `detail`），`ctl.health` 读回。
/// 空表照实回 `[]`：装配链明确无降级，不是"未定格"。
pub(super) fn rr3_degrade_json(notes: &[(String, String, String)]) -> serde_json::Value {
    serde_json::Value::Array(
        notes
            .iter()
            .map(|(module, kind, detail)| json!({"module": module, "kind": kind, "detail": detail}))
            .collect(),
    )
}

/// HAL 后端链摘要：全局 + 三面覆盖（None/空 = 回落全局，显示 `-`）。
pub(super) fn rr3_backend_summary(
    hal: &str,
    audio: Option<&str>,
    servo: Option<&str>,
    display: Option<&str>,
) -> String {
    let hal = if hal.is_empty() { "-" } else { hal };
    format!(
        "hal={hal} audio={} servo={} display={}",
        rr3_dash_opt(audio),
        rr3_dash_opt(servo),
        rr3_dash_opt(display)
    )
}

/// 可选后端名：None/空 → `-`（回落全局，不造假值）。
fn rr3_dash_opt(s: Option<&str>) -> &str {
    s.filter(|v| !v.is_empty()).unwrap_or("-")
}

/// 缺角色点名（校验②：该格缺能力，修装配单 [pins] 后重启解除）。
fn rr3_note_roles(boot: &crate::profile::Rr3ProfileBoot) {
    for (slot, role) in &boot.role_missing {
        rr3_log(&format!(
            "缺角色 {slot}：角色 {role} 无 [pins] 绑定（校验②——该格缺能力，修装配单 [pins] 后重启解除）"
        ));
    }
}

/// 装配链点名：缺格 / 缺角色 / 装配缺项 / 轴档案注记——四类分开，不混写
fn rr3_boot_notes(boot: &crate::profile::Rr3ProfileBoot) {
    rr3_note_missing(boot);
    rr3_note_roles(boot);
    rr3_note_broken(boot);
    for note in &boot.axis_notes {
        rr3_log(note);
    }
}

fn rr3_note_missing(boot: &crate::profile::Rr3ProfileBoot) {
    for (module, slot) in &boot.missing {
        rr3_log(&format!(
            "拒绝启动 {module}：缺格 {slot}（装配单 bindings 无此格或探测不到，fail-closed 绝不带病起）"
        ));
    }
}

fn rr3_note_broken(boot: &crate::profile::Rr3ProfileBoot) {
    for (module, msg) in &boot.assemble_broken {
        rr3_log(&format!(
            "装配缺项 {module}：{msg}（该模块在册即 Degraded，不致命——修数据后重启 rrd 解除）"
        ));
    }
}

/// rrd 自身日志落盘（§17 运维留痕）：打不开 = 点名退纯内存，不放大成可用性故障
pub(super) fn rr3_daemon_logs(cfg: &RrdConfig) {
    if let Err(e) = crate::rr3_log_sink_set(&cfg.socket) {
        rr3_log(&format!(
            "rrd 日志落盘不可用: {e} —— 运维留痕退纯内存（rr3.ctl.logs 仅本次启动的环）"
        ));
    }
    crate::rr3_log_backfill();
}

/// 独占绑定 + 访问控制放行（chmod 0o660/chgrp 属主组，尽力而为不拒启）
pub(super) fn rr3_daemon_bind(cfg: &RrdConfig) -> Result<Rr3UdsListener, String> {
    let listener = Rr3UdsListener::rr3_bind_exclusive(&cfg.socket)
        .map_err(|e| format!("绑定 {} 失败: {e}", cfg.socket.display()))?;
    super::ctl::rr3_socket_acl_grant(&cfg.socket);
    rr3_log(&format!("监听 {}", cfg.socket.display()));
    Ok(listener)
}

/// 运维计数持久化：启动加载全表 → boots 自增 → 立即回写；无数据分区 = 内存账
pub(super) fn rr3_daemon_counters(shared: &Arc<Mutex<RrdRegistry>>) -> Option<PathBuf> {
    let counters_path = crate::counters::rr3_counters_path();
    match &counters_path {
        Some(path) => {
            let (mut table, note) = crate::counters::rr3_counters_load(path);
            if let Some(n) = note {
                rr3_log(&format!("counters 启动加载按空表起账: {n}"));
            }
            *table
                .entry(crate::counters::RR3_COUNTERS_BOOTS_KEY.to_string())
                .or_insert(0) += 1;
            rr3_lock(shared).rr3_reg_ctr_replace(table.clone());
            // 键数封面（可定位）：正常六模块远低于上界；逼近/超界说明历史异常键
            // 或故障模块换键，启动一行即见，不必等首拍心跳的拒键日志。
            let cap = crate::registry::RR3_CTR_KEY_CAP;
            if table.len() > cap {
                rr3_log(&format!(
                    "counters 启动加载 {} 键，已超上界 {cap}：新键将被拒（查历史异常键）",
                    table.len()
                ));
            } else {
                rr3_log(&format!(
                    "counters 启动加载 {} 键（上界 {cap}）",
                    table.len()
                ));
            }
            match crate::counters::rr3_counters_store(path, &table) {
                Ok(()) => rr3_log(&format!(
                    "counters 持久化已启用: {}（boots 自增已回写）",
                    path.display()
                )),
                Err(e) => rr3_log(&format!("counters 回写失败（心跳拍会自然重试）: {e}")),
            }
        }
        None => rr3_log(
            "counters 不持久化（无 RR3_DATA_DIR 且非 linux 缺省面）: 计数仅内存账，断电不保",
        ),
    }
    counters_path
}
