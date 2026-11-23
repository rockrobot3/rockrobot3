// SPDX-License-Identifier: Apache-2.0
//! 执行件档案写入面（批N3 件四，全案 §6 路线 A"立即可用，随批N3 落手册与档案
//! 写入"；路线 C 的起步前置同面——§6 前置列"档案写入面即可起步"）。
//!
//! 职责边界（机制与策略分离）：装载、校验、序列化、原子落盘的**机制**全在
//! [`crate::axis`] 与 [`crate::profile`]——本模块只做**写入编排的策略层**：
//! 谁能写（档案外键 pin 必须 ∈ 装配单 [[pins]] 的 pin_id 全集，F-4 写侧半边）、
//! 写到哪（`hardware/<形态>/axis/<instance>.toml`，形态名 = 装配单文件名 stem，
//! 与 [`crate::wiring::rr3_wiring_path`] 同一布局）、什么不许写
//! （坏目标不覆盖——件二装载域"不许覆盖病灶"的写侧镜像；同 pin 异名副本不
//! 制造——副本会让 boot 链"恰一份合法才投影"拒注入，件三已定性，写入时先挡）。
//!
//! 为什么是独立产线小工具而不是 rrd daemon 的子命令：daemon 的 CLI 是守护进程
//! 参数面（socket/tick/manifest），混进一次性写档案会让两种生命周期各说一套；
//! 产线工人（路线 A 治具流程）与批N4 面板动作面要的是同一个写路径——本工具是
//! [`crate::axis::rr3_act_prof_store`] 的第一个接线位，批N4 面板接线时复用
//! 同一函数（§7"落盘只许有一份实现"，第二条写路径即红）。
//!
//! rtos 档不在本面射程内：`Rr3Store` 三项载体性质（工单 S-14）实测前
//! `calibrated` 只许运行期内存态，档案在 rtos 形态今天无处可落（§4.4 载体
//! 语义②）——本工具只服务 host 档，rtos 行如实写"重启即失"。
//!
//! 标定动作纪律（本工具不越权）：N2 收口前只准单步点动＋人工确认（施工计划
//! §3 批N3 行）；本工具只落盘**人工流程的产物**，不驱动任何运动——标定动作
//! 的拒动判据在 [`rr_hal_api::rr3_motion_allowed`]，写入面与运动面零交叠。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::axis::{rr3_act_prof_parse, rr3_act_profile_load, Rr3AxisFile};
// 落盘序随写盘面 unix 门（counters/axis 同径，批N9 件一补钉；run 的接线位同门）
#[cfg(unix)]
use crate::axis::rr3_act_prof_store;

/// 写入参数：`--profile` 装配单（pin_id 全集与形态名的唯一输入）＋
/// `--candidate` 候选档案（十四键平表全文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3AxisWriteArgs {
    /// 装配单路径（`<root>/profiles/<形态>.toml`）
    pub profile: PathBuf,
    /// 候选档案路径（完整十四键，装载判据全绿才落盘）
    pub candidate: PathBuf,
}

const USAGE: &str = "用法: axis_write --profile <装配单.toml> --candidate <候选档案.toml>\n说明: 执行件档案写入面（批N3 件四）——候选档案过装载全判据（十四键全键必填、类型域界、词表在册、行程自洽、pin 外键 ∈ 装配单 [[pins]]）后，原子落盘到 hardware/<形态>/axis/<instance>.toml（tmp→fsync→rename 唯一写路径）。坏目标不覆盖；同 pin 异名副本拒写（boot 链恰一份合法才投影）。产线标定流程见 docs/process/dev/真件装配与标定手册.md 件2b。";

/// 路径型开关取值：缺参数与疑似开关的值都拒（与 rrd cli 的
/// `need_path_value` 同一条口径——fail-open 的形状两处都不许有）。
fn need_path_value(flag: &str, args: &[String], i: usize) -> Result<PathBuf, String> {
    let Some(raw) = args.get(i + 1) else {
        return Err(format!("{flag} 缺参数\n{USAGE}"));
    };
    if raw.starts_with('-') {
        return Err(format!(
            "{flag} 缺参数：收到疑似开关的 {raw}（路径值不得以 - 开头）\n{USAGE}"
        ));
    }
    Ok(PathBuf::from(raw))
}

/// 解析写入参数。未知开关 / 缺参数 / `-` 开头路径一律报错不回落（fail-closed）。
pub fn rr3_axis_write_parse(args: &[String]) -> Result<Rr3AxisWriteArgs, String> {
    let mut profile: Option<PathBuf> = None;
    let mut candidate: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        i = rr3_aw_arg(args, i, &mut profile, &mut candidate)?;
    }
    let Some(profile) = profile else {
        return Err(format!("--profile 缺失\n{USAGE}"));
    };
    let Some(candidate) = candidate else {
        return Err(format!("--candidate 缺失\n{USAGE}"));
    };
    Ok(Rr3AxisWriteArgs { profile, candidate })
}

/// 单个参数位分派，返回下一个待解析下标：未知开关 / 缺参数 / `-` 开头值即拒。
fn rr3_aw_arg(
    args: &[String],
    i: usize,
    profile: &mut Option<PathBuf>,
    candidate: &mut Option<PathBuf>,
) -> Result<usize, String> {
    let flag = args[i].as_str();
    if flag == "--profile" {
        *profile = Some(need_path_value("--profile", args, i)?);
        return Ok(i + 2);
    }
    if flag == "--candidate" {
        *candidate = Some(need_path_value("--candidate", args, i)?);
        return Ok(i + 2);
    }
    if ["-h", "--help"].contains(&flag) {
        return Err(USAGE.to_string());
    }
    Err(format!("未知选项 {flag}\n{USAGE}"))
}

/// 写入编排（策略全在本函数，逐条显式）：装配单装载 → 候选全判据 → 拒绝面
/// （坏目标不覆盖 / 同 pin 异名副本不制造）→ 原子落盘。成功回显写入路径与
/// 档案要点；失败逐条点名，**不落任何字节**。随落盘序 unix 门（批N9 件一
/// 补钉）——解析面 [`rr3_axis_write_parse`] 跨平台照常在册。
#[cfg(unix)]
pub fn rr3_axis_write_run(cfg: &Rr3AxisWriteArgs) -> Result<String, String> {
    let (pin_ids, axis_dir) = rr3_aw_layout(cfg)?;
    let (file, target) = rr3_aw_candidate(cfg, &pin_ids, &axis_dir)?;
    rr3_aw_existing(&target, &file, &pin_ids)?;
    rr3_aw_duplicates(&axis_dir, &target, &file, &pin_ids)?;
    rr3_aw_store(&axis_dir, &target, &file)
}

/// 装配单装载与落位：pin_id 全集来自 [[pins]]，axis/ 由接线布局推导。
/// 写入面只吃装载域，不做 boot 的十条校验（整机合法性归 boot 正门）。
#[cfg(unix)]
fn rr3_aw_layout(cfg: &Rr3AxisWriteArgs) -> Result<(BTreeSet<String>, PathBuf), String> {
    let profile = rr3_aw_profile(cfg)?;
    let pin_ids: BTreeSet<String> = profile.pins.iter().map(|p| p.rr3_pin_id()).collect();
    let wiring_path = crate::wiring::rr3_wiring_path(&cfg.profile)?;
    let axis_dir = wiring_path
        .parent()
        .expect("wiring 路径必有父目录")
        .join("axis");
    Ok((pin_ids, axis_dir))
}

/// 装配单读入并解析（路径绝对化后读文本）。
#[cfg(unix)]
fn rr3_aw_profile(cfg: &Rr3AxisWriteArgs) -> Result<crate::profile::Rr3Profile, String> {
    let profile_abs = crate::profile::rr3_profile_abs(&cfg.profile)?;
    let profile_text = std::fs::read_to_string(&profile_abs)
        .map_err(|e| format!("读不到装配单 {}: {e}", profile_abs.display()))?;
    crate::profile::rr3_profile_parse(&profile_text).map_err(|errs| {
        format!(
            "装配单 {} 形状不合: {}",
            profile_abs.display(),
            errs.join("; ")
        )
    })
}

/// 候选档案全判据 + 落位文件名（pin 末段 instance；非两段式拒猜）
#[cfg(unix)]
fn rr3_aw_candidate(
    cfg: &Rr3AxisWriteArgs,
    pin_ids: &BTreeSet<String>,
    axis_dir: &Path,
) -> Result<(Rr3AxisFile, PathBuf), String> {
    let candidate_path = crate::profile::rr3_profile_abs(&cfg.candidate)?;
    let candidate_text = std::fs::read_to_string(&candidate_path)
        .map_err(|e| format!("读不到候选档案 {}: {e}", candidate_path.display()))?;
    let file = rr3_act_prof_parse(
        &candidate_text,
        &candidate_path.display().to_string(),
        pin_ids,
    )
    .map_err(|errs| errs.join("; "))?;
    let Some(instance) = file
        .pin
        .rsplit('.')
        .next()
        .filter(|seg| !seg.is_empty() && *seg != file.pin)
    else {
        return Err(format!(
            "候选 pin = {:?} 不是 role.instance 两段式（act.axis.pwm.pan 形），拒绝猜测落位",
            file.pin
        ));
    };
    let target = axis_dir.join(format!("{instance}.toml"));
    Ok((file, target))
}

/// 拒绝面 a：不在场 = 无旧档可保护；坏档/读不出不覆盖；同档异 pin 拒覆盖（pin
/// 才是档案身份键）。**不以 `exists()` 先判**：判与读之间档会被移除（运维并发、
/// 外部清理），那样"装载回 None"只能落进 panic——写面拒面不许用 panic 表达
/// 文件系统事实，daemon 写口线程 panic 会断连接、丢应答。装载三态即三分支本身。
#[cfg(unix)]
fn rr3_aw_existing(
    target: &Path,
    file: &Rr3AxisFile,
    pin_ids: &BTreeSet<String>,
) -> Result<(), String> {
    match rr3_act_profile_load(target, pin_ids) {
        Ok(Some(existing)) if existing.pin != file.pin => Err(format!(
            "目标 {} 已存在且 pin = {:?}（候选 {:?}）——instance 落位与档案身份冲突，拒绝覆盖",
            target.display(),
            existing.pin,
            file.pin
        )),
        Ok(_) => Ok(()),
        Err(errs) => Err(format!(
            "目标 {} 已存在且装载红（{}）——坏档案不许覆盖，先修或手工移除后再写",
            target.display(),
            errs.join("; ")
        )),
    }
}

/// 目录项与目标同 pin：跳过目标本身与非 toml，命中同 pin 返回点名原因。
fn rr3_aw_same_pin(
    p: &Path,
    target: &Path,
    file: &Rr3AxisFile,
    pin_ids: &BTreeSet<String>,
) -> Option<String> {
    if rr3_aw_peer_skip(p, target) {
        return None;
    }
    rr3_aw_pin_hit(p, target, file, pin_ids)
}

/// 装载一份目录项：与候选同 pin 即交出拒写原因（坏档/异 pin 不越权判）。
fn rr3_aw_pin_hit(
    p: &Path,
    target: &Path,
    file: &Rr3AxisFile,
    pin_ids: &BTreeSet<String>,
) -> Option<String> {
    match rr3_act_profile_load(p, pin_ids) {
        Ok(Some(other)) if other.pin == file.pin => Some(rr3_aw_dup_msg(p, target, file)),
        _ => None,
    }
}

/// 目录项过滤：目标本身与非 `.toml` 都不是副本候选。
fn rr3_aw_peer_skip(p: &Path, target: &Path) -> bool {
    p == target || p.extension().and_then(|x| x.to_str()) != Some("toml")
}

/// 同 pin 异名副本的拒写文案（点名副本、pin 与候选目标）。
fn rr3_aw_dup_msg(p: &Path, target: &Path, file: &Rr3AxisFile) -> String {
    format!(
        "axis/ 下已有 {} 装同一 pin {:?}（候选目标 {}）——同 pin 两份合法档会让 boot 链拒注入（恰一份才投影），先清理副本",
        p.display(),
        file.pin,
        target.display()
    )
}

/// 拒绝面 b：同 pin 异名副本不制造（boot 链恰一份合法才投影；装载红散件不越权判）
#[cfg(unix)]
fn rr3_aw_duplicates(
    axis_dir: &Path,
    target: &Path,
    file: &Rr3AxisFile,
    pin_ids: &BTreeSet<String>,
) -> Result<(), String> {
    if let Some(msg) = rr3_aw_scan(axis_dir, target, file, pin_ids) {
        return Err(msg);
    }
    Ok(())
}

/// 扫 axis/ 目录：首个同 pin 副本即交出拒写原因（目录读不到 = 无副本可判）。
fn rr3_aw_scan(
    axis_dir: &Path,
    target: &Path,
    file: &Rr3AxisFile,
    pin_ids: &BTreeSet<String>,
) -> Option<String> {
    let entries = std::fs::read_dir(axis_dir).ok()?;
    entries
        .flatten()
        .find_map(|e| rr3_aw_same_pin(&e.path(), target, file, pin_ids))
}

/// 原子落盘（tmp→fsync→rename 唯一写路径）并回显写入路径与档案要点
#[cfg(unix)]
fn rr3_aw_store(axis_dir: &Path, target: &Path, file: &Rr3AxisFile) -> Result<String, String> {
    std::fs::create_dir_all(axis_dir)
        .map_err(|e| format!("建档案目录 {} 失败: {e}", axis_dir.display()))?;
    rr3_act_prof_store(target, file).map_err(|e| format!("写入 {} 失败: {e}", target.display()))?;
    let p = &file.profile;
    Ok(format!(
        "已写入 {}\n  pin = {}  calibrated = {}  hard_stop = {}  source = {}\n  生效时点：rrd 下次 --profile boot 扫描 axis/ 时投影执行轴状态（恰一份合法才投影）",
        target.display(),
        file.pin,
        p.calibrated,
        p.hard_stop.rr3_hard_stop_name(),
        p.source.rr3_act_source_name(),
    ))
}

#[cfg(all(test, unix))]
mod tests;
