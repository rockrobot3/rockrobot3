//! 装配侧策略量写口（批N22 件一，敞口 15 写面半边；判据源三处：conventions §6.1
//! 三写口点名、ADR-0021 裁定 6"拒动类阈值写必须带二次确认与留痕"、B-15 ③"安全写
//! 必须携带证据引用"）。
//!
//! 职责边界（机制与策略分离）：**机制**全在别处单点——文本手术在
//! [`crate::profile::rr3_domain_keys_apply`] / [`crate::profile::rr3_toml_key_line_apply`]，
//! 原子落盘在 `rr_ipc::persist::rr3_atomic_store`（re-export 单点
//! [`crate::counters::rr3_atomic_store`]），装载判据在 [`crate::profile::
//! rr3_profile_boot_inputs`]（与 boot 正门同一份裁决，写面不重复判业务合法性），
//! 证据四道核在 `rr_ipc::evidence`（唯一实现，§6.1 纪律），轴档全判据＋落盘在
//! [`crate::axis_write::rr3_axis_write_run`]（§7"落盘只许有一份实现"，第二条写
//! 路径即红——本模块是它的第二个接线位，不是第二份实现）。**本模块只持写编排
//! 策略**：谁能写（confirm 门＋key 白名单）、凭什么写（证据引用规则）、写到哪
//! （路径自真值集记账推导，零新增全局）、怎么生效（落盘 → 装载链预核 → 原子写回
//! → 契约记账重记；**运行态不动，下次 rrd boot 生效**）。
//!
//! 三写口与证据引用规则（conventions §6.1"哪些写必须带引用"归各写面）：
//! - `armed`：置 true 必携证据引用且**联动入档**（装载期⑥-e 成对纪律——armed=true
//!   必携形制合法引用、armed=false 显式写 `"none"` 不携过期引用，写面直接落出
//!   成对形状）；置 false 不要求引用，档内 evidence 联动写 `"none"`。
//! - `severe_mv` / `current_ceil_ma`：必携证据引用，**仅留痕不入档**（档内
//!   evidence 键语义 = 武装证据，单主键；写动作的证据住 rrd 日志审计留痕）。
//! - 三个键统一 confirm 门与留痕：ADR-0021 裁定 6 点名 severe_mv/armed 必须
//!   二次确认；current_ceil_ma 同为拒动类阈值（写大 = 烧轴风险），同一端点的
//!   三键差别化确认会让端点语义劈叉，取保守侧统一（fail-closed 方向不违判据源）。
//!
//! 生效语义与记账跟随：落盘后运行态（装配快照）维持 boot 时旧值——`effective:
//! "boot"` 应答如实申报；契约记账快照在写口成功后重记（写口的 tmp 产物过装载链
//! 全量预核 = 新验收事实，记账跟住最新验收事实——否则写口自造一笔常驻"sha 漂"
//! 自检红，把真漂移淹没，见 contracts.rs Mutex 化注记）。
//!
//! 快照未在册（`--manifest` 调试路径）如实回不可得：无记账即无路径依据，写口
//! 拒（与 wiring/contracts 读口"如实报不可得"同一条诚实纪律）。

use std::path::PathBuf;

use rr_ipc::Rr3StatusCode;
use serde_json::{json, Value};

use crate::contracts::Rr3TruthSetEntry;

/// 写口目标（params.target 词表，parse 层裁）：domain = 装配单
/// `[[supplies.domain]]` 域块键；axis = 轴档案单键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3WriteTarget {
    Domain,
    Axis,
}

/// 写口值型（parse 层形状核后）：布尔（armed）或正整数（severe_mv /
/// current_ceil_ma）。0 在 parse 层拒——正整数是输入形状核（0 无域界语义），
/// 阈值次序（mild > severe > 0 等）不在写面重复判：判据落装载链（批N2 门 c
/// 消费者侧）则写口经预核自动继承；未落则如实放行（写面不造判据还不在的红）——
/// 写面只核形状与权限，业务合法性永远住装载链。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3WriteValue {
    Bool(bool),
    Num(u64),
}

/// 写口参数（parse 层产出，形状全合法——门与白名单在这里已裁）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3DomainWriteParams {
    pub target: Rr3WriteTarget,
    /// 域名（domain）或轴 instance 名（axis）
    pub name: String,
    /// 键名（白名单：domain → armed/severe_mv；axis → current_ceil_ma——
    /// conventions §6.1 三写口点名集，白名单外即拒）
    pub key: String,
    pub value: Rr3WriteValue,
    /// 证据引用（armed=true / severe_mv / current_ceil_ma 必填，armed=false 不要求）
    pub evidence: Option<String>,
    /// 二次确认位（execute ①的门参数；parse 层不裁门——形状归 parse、门归门序）。
    /// 由 [`Rr3DomainWriteParams::confirm_from`] 从原始 params 提取。
    pub confirm: Option<bool>,
}

/// params 拆包＋形状核（跨平台纯函数）。confirm 门不在本层（它是权限语义，
/// 归 execute 的门序；parse 只裁"形状合不合法"）。错误码全 Invalid（参数形状病）。
pub fn rr3_domain_write_parse(params: &Value) -> Result<Rr3DomainWriteParams, (Rr3StatusCode, String)> {
    let invalid = |msg: String| (Rr3StatusCode::Invalid, msg);
    let target = match params.get("target").and_then(Value::as_str) {
        Some("domain") => Rr3WriteTarget::Domain,
        Some("axis") => Rr3WriteTarget::Axis,
        _ => {
            return Err(invalid(
                "缺 params.target（要求 \"domain\" 或 \"axis\"）".to_string(),
            ))
        }
    };
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("缺 params.name（要求非空字符串）".to_string()))?
        .to_string();
    let key = params
        .get("key")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("缺 params.key（要求非空字符串）".to_string()))?;
    // key 白名单（conventions §6.1 三写口点名集；词表外 = 申报面错，fail-closed）
    let value = match (target, key) {
        (Rr3WriteTarget::Domain, "armed") => {
            Rr3WriteValue::Bool(params.get("value").and_then(Value::as_bool).ok_or_else(
                || invalid("armed 的 params.value 要求布尔（true/false）".to_string()),
            )?)
        }
        (Rr3WriteTarget::Domain, "severe_mv") | (Rr3WriteTarget::Axis, "current_ceil_ma") => {
            let n = params
                .get("value")
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid(format!("{key} 的 params.value 要求正整数")))?;
            if n == 0 {
                return Err(invalid(format!("{key} 的 params.value 要求正整数（收到 0）")));
            }
            Rr3WriteValue::Num(n)
        }
        _ => {
            return Err(invalid(format!(
                "key {key:?} 不在写口白名单（domain → armed/severe_mv；axis → current_ceil_ma；conventions §6.1 三写口点名集）"
            )))
        }
    };
    let evidence = params
        .get("evidence")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(Rr3DomainWriteParams {
        target,
        name,
        key: key.to_string(),
        value,
        evidence,
        confirm: None,
    })
}

/// 写口路径集（真值集记账推导，零新增全局）：记账成员判据 = 成员相对名（基-40
/// 成员判据 = "装载期读的文件"——写口的读与写打同一批文件，路径与 boot 记账
/// 同源即无第二真值点）。任一必需成员缺 = 记账形状破，拒（fail-closed）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3DomainWritePaths {
    /// 装配单（`profiles/<stem>.toml` 记账成员）
    pub profile: PathBuf,
    pub slots: PathBuf,
    pub roles: PathBuf,
    pub devices: PathBuf,
    /// 机型接线档（`hardware/<stem>/wiring.toml` 记账成员；axis 落点的目录锚）
    pub wiring: PathBuf,
    /// 平台事实表目录（`contracts/platform/<chip>.toml` 记账成员的父目录）
    pub platform_dir: PathBuf,
}

/// 从真值集记账行推导写口路径集（跨平台纯函数）。topics.toml 不取（装载链
/// 内部按 contracts 目录读）。
pub fn rr3_domain_write_paths(
    truth_set: &[Rr3TruthSetEntry],
) -> Result<Rr3DomainWritePaths, String> {
    let mut profile = None;
    let mut slots = None;
    let mut roles = None;
    let mut devices = None;
    let mut wiring = None;
    let mut platform_dir = None;
    for e in truth_set {
        if e.member.starts_with("profiles/") && e.member.ends_with(".toml") {
            profile = Some(e.path.clone());
        }
        match e.member.as_str() {
            m if m == format!("contracts/{}", crate::profile::RR3_CONTRACT_BOOKS[1]) => {
                slots = Some(e.path.clone())
            }
            m if m == format!("contracts/{}", crate::profile::RR3_CONTRACT_BOOKS[2]) => {
                roles = Some(e.path.clone())
            }
            m if m == format!("contracts/{}", crate::profile::RR3_CONTRACT_BOOKS[3]) => {
                devices = Some(e.path.clone())
            }
            _ => {}
        }
        if let Some(rest) = e.member.strip_prefix("contracts/platform/") {
            if rest.ends_with(".toml") {
                platform_dir = e.path.parent().map(PathBuf::from);
            }
        }
        if e.member.starts_with("hardware/") && e.member.ends_with("/wiring.toml") {
            wiring = Some(e.path.clone());
        }
    }
    // 逐条点名缺员（基-40 记账七成员，写口必需六路；缺哪条点哪条，不静默合并）
    let missing = |what: &str, v: &Option<PathBuf>| {
        v.clone()
            .ok_or_else(|| format!("真值集记账缺 {what} 成员（记账形状破），写口拒"))
    };
    Ok(Rr3DomainWritePaths {
        profile: missing("profiles/<stem>.toml", &profile)?,
        slots: missing("contracts/slots.toml", &slots)?,
        roles: missing("contracts/roles.toml", &roles)?,
        devices: missing("contracts/devices.toml", &devices)?,
        wiring: missing("hardware/<stem>/wiring.toml", &wiring)?,
        platform_dir: missing("contracts/platform/<chip>.toml", &platform_dir)?,
    })
}

/// 证据容器根（§6.1：modules-root 同一条 `<root>/profiles/` 布局纪律推导出
/// `<root>/docs/evidence`；与装载期⑥-e 文件半边同一条推导）。
fn rr3_evidence_root(profile_path: &std::path::Path) -> PathBuf {
    profile_path
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("docs/evidence"))
        .unwrap_or_else(|| PathBuf::from("docs/evidence"))
}

/// 证据引用门（B-15 ③）：必填形状 + 四道核（`rr-ipc::evidence` 唯一实现）。
/// 核败 = Permission 拒写（安全写不带合法证据就是无权写）。
fn rr3_write_evidence_gate(
    profile_path: &std::path::Path,
    reference: &str,
) -> Result<(), (Rr3StatusCode, String)> {
    rr_ipc::evidence::rr3_evidence_verify(&rr3_evidence_root(profile_path), reference).map_err(
        |e| {
            (
                Rr3StatusCode::Permission,
                format!("写口证据引用核验不过（B-15 ③——安全写必须携带真板证据）: {e}"),
            )
        },
    )
}

/// 写口执行（策略编排，逐门显式；`#[cfg(unix)]` 随落盘序同门——原子写与
/// `rr3_axis_write_run` 都在 unix 门内，非 unix 编译时本函数不存在，conn 臂
/// 如实回"写面仅 unix"）。门序：confirm 门 → 旧值提取（域缺/档缺拒）→
/// 证据引用门 → 文本手术 → tmp 落盘 → 装载链全量预核（Err 即拒写清 tmp，
/// 零字节进正档）→ 原子写回 → 契约记账重记 → 审计留痕。
#[cfg(unix)]
pub fn rr3_domain_write_execute(
    paths: &Rr3DomainWritePaths,
    params: &Rr3DomainWriteParams,
) -> Result<Value, (Rr3StatusCode, String)> {
    let perm = |msg: String| (Rr3StatusCode::Permission, msg);
    // ① confirm 门（ADR-0021 裁定 6：拒动类阈值写必须二次确认——缺/false 一律拒）
    let confirmed = params
        .confirm
        .unwrap_or(false);
    if !confirmed {
        return Err(perm(
            "缺二次确认 params.confirm = true（ADR-0021 裁定 6——拒动类阈值写必须二次确认与留痕）".to_string(),
        ));
    }
    match params.target {
        Rr3WriteTarget::Domain => rr3_domain_write_domain(paths, params),
        Rr3WriteTarget::Axis => rr3_domain_write_axis(paths, params),
    }
}

/// confirm 门参数位（parse 层只看形状不裁门——门归 execute ①）。
impl Rr3DomainWriteParams {
    pub fn confirm_from(params: &Value) -> Option<bool> {
        params.get("confirm").and_then(Value::as_bool)
    }
}

/// domain 落点写编排（`[[supplies.domain]]` 域块键行手术 → 装载预核 → 原子写回
/// → 记账重记）。
#[cfg(unix)]
fn rr3_domain_write_domain(
    paths: &Rr3DomainWritePaths,
    params: &Rr3DomainWriteParams,
) -> Result<Value, (Rr3StatusCode, String)> {
    let invalid = |msg: String| (Rr3StatusCode::Invalid, msg);
    let perm = |msg: String| (Rr3StatusCode::Permission, msg);
    let text = std::fs::read_to_string(&paths.profile).map_err(|e| {
        invalid(format!(
            "读不到装配单 {}: {e}",
            paths.profile.display()
        ))
    })?;
    // 旧值提取（域缺拒——写面不做新域注册）：从装载域拿（与 boot 同一解析器）
    let profile = crate::profile::rr3_profile_from_text(&text).map_err(|errs| {
        invalid(format!(
            "装配单 {} 形状不合: {}",
            paths.profile.display(),
            errs.join("; ")
        ))
    })?;
    let dom = profile
        .supplies
        .domains
        .iter()
        .find(|d| d.name == params.name)
        .ok_or_else(|| {
            invalid(format!(
                "域 {:?} 不在 [[supplies.domain]] 在册（缺域拒——新域是契约事件，不经写口注册）",
                params.name
            ))
        })?;
    let (old_value, updates): (Value, Vec<(&str, String)>) = match (params.key.as_str(), &params.value) {
        // armed 写：evidence 联动（成对纪律——写面直接落出⑥-e 要求的档内形状）
        ("armed", Rr3WriteValue::Bool(true)) => {
            let ev = params.evidence.clone().ok_or_else(|| {
                perm(
                    "armed = true 置位必携证据引用 params.evidence（D11 界面门——必须引用一条真板对拍证据编号）"
                        .to_string(),
                )
            })?;
            rr3_write_evidence_gate(&paths.profile, &ev)?;
            (
                json!(dom.armed),
                vec![("armed", "true".to_string()), ("evidence", format!("\"{ev}\""))],
            )
        }
        ("armed", Rr3WriteValue::Bool(false)) => (
            json!(dom.armed),
            vec![("armed", "false".to_string()), ("evidence", "\"none\"".to_string())],
        ),
        ("severe_mv", Rr3WriteValue::Num(n)) => {
            let ev = params.evidence.clone().ok_or_else(|| {
                perm("severe_mv 安全写必携证据引用 params.evidence（B-15 ③）".to_string())
            })?;
            rr3_write_evidence_gate(&paths.profile, &ev)?;
            (json!(dom.severe_mv), vec![("severe_mv", n.to_string())])
        }
        _ => unreachable!("parse 层白名单已裁，值型与键必配"),
    };
    // 手术 → tmp 落盘 → 装载链全量预核（与 boot 正门同一份裁决；Err 即拒写清 tmp）
    let update_refs: Vec<(&str, &str)> =
        updates.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let new_text = crate::profile::rr3_domain_keys_apply(&text, &params.name, &update_refs)
        .map_err(invalid)?;
    let stem = paths
        .profile
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| invalid("装配单无文件名".to_string()))?;
    let tmp = paths.profile.with_file_name(format!("{stem}.tmp-write.toml"));
    std::fs::write(&tmp, new_text.as_bytes())
        .map_err(|e| invalid(format!("写临时档 {} 失败: {e}", tmp.display())))?;
    let boot = match crate::profile::rr3_profile_boot_inputs(
        &tmp,
        &paths.slots,
        &paths.roles,
        &paths.devices,
        &paths.wiring,
        &paths.platform_dir,
    ) {
        Ok(b) => b,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(invalid(format!(
                "手术后装载链预核不过，拒绝落盘（旧档原封）: {e}"
            )));
        }
    };
    // 原子写回（机制单点 tmp→fsync→rename；失败清 tmp 旧档原封）
    crate::counters::rr3_atomic_store(&paths.profile, &tmp, new_text.as_bytes()).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        invalid(format!("装配单 {} 原子写回失败: {e}", paths.profile.display()))
    })?;
    // 契约记账重记（写口的 tmp 产物已过装载链全量预核 = 新验收事实；重记失败
    // 不回滚落盘——主事实已发生，点名让下次 boot 重定格）
    let contracts_dir = paths.slots.parent().map(PathBuf::from);
    let mut relogged = true;
    if let Some(dir) = contracts_dir {
        match crate::contracts::rr3_contracts_snapshot_capture(
            &paths.profile,
            &dir,
            &paths.wiring,
            &boot.platform_chip,
        )
        .and_then(crate::contracts::rr3_contracts_snapshot_replace)
        {
            Ok(()) => {}
            Err(e) => {
                relogged = false;
                crate::rr3_log(&format!(
                    "domain 写口重记账失败（{e}）——契约自检将报漂直到下次 boot 重定格"
                ));
            }
        }
    }
    let new_value = match &params.value {
        Rr3WriteValue::Bool(b) => json!(b),
        Rr3WriteValue::Num(n) => json!(n),
    };
    crate::rr3_log(&format!(
        "domain 写口: {}.{} {}→{}（evidence {}，effective boot——下次 rrd 装载投影）",
        params.name,
        params.key,
        old_value,
        new_value,
        params.evidence.as_deref().unwrap_or("none"),
    ));
    Ok(json!({
        "ok": true,
        "target": "domain",
        "name": params.name,
        "key": params.key,
        "old": old_value,
        "new": new_value,
        "evidence": params.evidence,
        "effective": "boot",
        "contracts_relogged": relogged,
    }))
}

/// axis 落点写编排：**必须复用** [`crate::axis_write::rr3_axis_write_run`]（其头注
/// 第 15-16 行："批N4 面板接线时复用同一函数（§7'落盘只许有一份实现'，第二条写
/// 路径即红"）——本函数只做"读旧档 → 单键手术 → 产 tmp 候选档"，十四键全判据、
/// 原子落盘、副本拒全部归它。
#[cfg(unix)]
fn rr3_domain_write_axis(
    paths: &Rr3DomainWritePaths,
    params: &Rr3DomainWriteParams,
) -> Result<Value, (Rr3StatusCode, String)> {
    let invalid = |msg: String| (Rr3StatusCode::Invalid, msg);
    // key 白名单已在 parse 裁（axis 只认 current_ceil_ma），此处直接编排
    let Rr3WriteValue::Num(n) = params.value else {
        return Err(invalid("current_ceil_ma 要求正整数".to_string()));
    };
    // 轴档定位：hardware/<形态>/axis/<instance>.toml（接线布局推导的同一目录锚）
    let axis_dir = paths
        .wiring
        .parent()
        .ok_or_else(|| invalid("接线档路径无父目录".to_string()))?
        .join("axis");
    let axis_path = axis_dir.join(format!("{}.toml", params.name));
    if !axis_path.exists() {
        return Err(invalid(format!(
            "轴档 {} 不在场（写口只改在册档单键；立档走产线写入面 axis_write 全判据流程）",
            axis_path.display()
        )));
    }
    // 旧值提取：装载域拿（pin 外键核同一份判据；装载红 = 坏档，拒——
    // axis_write_run 的"坏目标不覆盖"在下游还会挡一道，这里先点名）
    let text = std::fs::read_to_string(&paths.profile).map_err(|e| {
        invalid(format!("读不到装配单 {}: {e}", paths.profile.display()))
    })?;
    let profile = crate::profile::rr3_profile_from_text(&text).map_err(|errs| {
        invalid(format!(
            "装配单 {} 形状不合: {}",
            paths.profile.display(),
            errs.join("; ")
        ))
    })?;
    let pin_ids: std::collections::BTreeSet<String> =
        profile.pins.iter().map(|p| p.rr3_pin_id()).collect();
    let existing = crate::axis::rr3_act_profile_load(&axis_path, &pin_ids)
        .map_err(|errs| invalid(format!("轴档 {} 装载红: {}", axis_path.display(), errs.join("; "))))?;
    let file = existing.ok_or_else(|| {
        invalid(format!("轴档 {} 不在场", axis_path.display()))
    })?;
    let old = json!(file.profile.current_ceil_ma);
    // 证据引用门（仅留痕——轴档十四键无 evidence 键，写动作证据住审计留痕）
    let ev = params.evidence.clone().ok_or_else(|| {
        (
            Rr3StatusCode::Permission,
            "current_ceil_ma 安全写必携证据引用 params.evidence（B-15 ③）".to_string(),
        )
    })?;
    rr3_write_evidence_gate(&paths.profile, &ev)?;
    // 单键手术 → tmp 候选档 → 复用产线写入面（全判据＋原子落盘＋副本拒单点）
    let new_text = crate::profile::rr3_toml_key_line_apply(
        &std::fs::read_to_string(&axis_path)
            .map_err(|e| invalid(format!("读不到轴档 {}: {e}", axis_path.display())))?,
        "current_ceil_ma",
        &n.to_string(),
    )
    .map_err(invalid)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!(
        "rr3-axis-candidate-{}-{nanos}.toml",
        std::process::id()
    ));
    std::fs::write(&tmp, new_text.as_bytes())
        .map_err(|e| invalid(format!("写候选档 {} 失败: {e}", tmp.display())))?;
    let run = crate::axis_write::rr3_axis_write_run(&crate::axis_write::Rr3AxisWriteArgs {
        profile: paths.profile.clone(),
        candidate: tmp.clone(),
    });
    let _ = std::fs::remove_file(&tmp);
    let echo = run.map_err(invalid)?;
    crate::rr3_log(&format!(
        "axis 写口: {} current_ceil_ma {}→{}（evidence {ev}，effective boot——rrd 下次 boot 扫描 axis/ 投影）",
        params.name, old, n,
    ));
    Ok(json!({
        "ok": true,
        "target": "axis",
        "name": params.name,
        "key": "current_ceil_ma",
        "old": old,
        "new": n,
        "evidence": ev,
        "effective": "boot",
        "echo": echo,
    }))
}

/// conn 臂薄壳：params 拆包 → 快照路径推导 → 执行。快照未在册（--manifest
/// 调试路径）如实回不可得——无记账即无路径依据，写口不猜路径。
#[cfg(unix)]
pub fn rr3_domain_write_run(params: &Value) -> Result<Value, (Rr3StatusCode, String)> {
    let parsed = rr3_domain_write_parse(params)?;
    let mut p = parsed.clone();
    p.confirm = Rr3DomainWriteParams::confirm_from(params);
    let Some(snap) = crate::contracts::rr3_contracts_snapshot() else {
        return Err((
            Rr3StatusCode::Permission,
            "契约记账未在册（--manifest 调试路径无装配单），写口不可得——请以 --profile 启动".to_string(),
        ));
    };
    let paths = rr3_domain_write_paths(&snap.truth_set)
        .map_err(|e| (Rr3StatusCode::Invalid, e))?;
    rr3_domain_write_execute(&paths, &p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// parse 正例：三写口各自合法形状全收（armed 布尔、severe_mv/current_ceil_ma 正整数）
    #[test]
    fn parse_accepts_three_named_writes() {
        let p = rr3_domain_write_parse(&json!({
            "target": "domain", "name": "rail5v", "key": "armed",
            "value": true, "confirm": true, "evidence": "实测/0002"
        }))
        .expect("armed 写应过");
        assert_eq!(p.target, Rr3WriteTarget::Domain);
        assert_eq!(p.key, "armed");
        assert_eq!(p.value, Rr3WriteValue::Bool(true));
        assert_eq!(p.evidence.as_deref(), Some("实测/0002"));

        let p = rr3_domain_write_parse(&json!({
            "target": "domain", "name": "rail5v", "key": "severe_mv",
            "value": 4200, "confirm": true, "evidence": "实测/0001"
        }))
        .expect("severe_mv 写应过");
        assert_eq!(p.value, Rr3WriteValue::Num(4200));

        let p = rr3_domain_write_parse(&json!({
            "target": "axis", "name": "pan", "key": "current_ceil_ma",
            "value": 700, "confirm": true, "evidence": "对拍/0001"
        }))
        .expect("current_ceil_ma 写应过");
        assert_eq!(p.target, Rr3WriteTarget::Axis);
        assert_eq!(p.value, Rr3WriteValue::Num(700));
    }

    /// parse 拒面：缺参 / 白名单外键 / 型坏 / 零值——全 Invalid 逐条点名
    #[test]
    fn parse_rejects_bad_shapes_fail_closed() {
        for (v, hint) in [
            (json!({"name":"x","key":"armed","value":true}), "target"),
            (json!({"target":"domain","key":"armed","value":true}), "name"),
            (json!({"target":"domain","name":"x","value":true}), "key"),
            (
                json!({"target":"domain","name":"x","key":"nominal_mv","value":1}),
                "白名单",
            ),
            (
                json!({"target":"axis","name":"x","key":"armed","value":true}),
                "白名单",
            ),
            (
                json!({"target":"domain","name":"x","key":"armed","value":"yes"}),
                "布尔",
            ),
            (
                json!({"target":"domain","name":"x","key":"severe_mv","value":-1}),
                "正整数",
            ),
            (
                json!({"target":"axis","name":"x","key":"current_ceil_ma","value":0}),
                "正整数",
            ),
        ] {
            let (code, msg) = rr3_domain_write_parse(&v).expect_err("坏形状必须拒");
            assert_eq!(code, Rr3StatusCode::Invalid, "{v}");
            assert!(msg.contains(hint), "文案缺 {hint}: {msg}");
        }
    }

    /// 路径推导正例：七成员记账行 → 六路路径（形状 = boot 记账真实成员名）
    #[test]
    fn paths_derive_from_truth_set_entries() {
        let ts = vec![
            ts_entry("profiles/esp32-mini.toml", "/x/profiles/esp32-mini.toml"),
            ts_entry("contracts/topics.toml", "/x/contracts/topics.toml"),
            ts_entry("contracts/slots.toml", "/x/contracts/slots.toml"),
            ts_entry("contracts/roles.toml", "/x/contracts/roles.toml"),
            ts_entry("contracts/devices.toml", "/x/contracts/devices.toml"),
            ts_entry("contracts/platform/esp32.toml", "/x/contracts/platform/esp32.toml"),
            ts_entry("hardware/esp32-mini/wiring.toml", "/x/hardware/esp32-mini/wiring.toml"),
        ];
        let p = rr3_domain_write_paths(&ts).expect("七成员应推全六路");
        assert_eq!(p.profile, PathBuf::from("/x/profiles/esp32-mini.toml"));
        assert_eq!(p.slots, PathBuf::from("/x/contracts/slots.toml"));
        assert_eq!(p.platform_dir, PathBuf::from("/x/contracts/platform"));
        assert_eq!(
            p.wiring,
            PathBuf::from("/x/hardware/esp32-mini/wiring.toml")
        );
    }

    /// 路径推导缺员逐条点名（fail-closed——记账形状破不猜路径）
    #[test]
    fn paths_reject_missing_members_by_name() {
        let e = rr3_domain_write_paths(&[]).expect_err("空记账必须拒");
        assert!(e.contains("profiles/<stem>.toml"), "{e}");
        let ts = vec![ts_entry(
            "profiles/esp32-mini.toml",
            "/x/profiles/esp32-mini.toml",
        )];
        let e = rr3_domain_write_paths(&ts).expect_err("缺契约册必须拒");
        assert!(e.contains("contracts/slots.toml"), "{e}");
    }

    fn ts_entry(member: &str, path: &str) -> Rr3TruthSetEntry {
        Rr3TruthSetEntry {
            member: member.to_string(),
            path: PathBuf::from(path),
            sha256: "0".repeat(64),
            bytes: 1,
        }
    }

    // execute 层集成测试（unix 门）：tmp 目录自建仓形状夹具，不打全局契约快照
    // （全局量进程级单次，同 lib.rs log_ring_cap 单测的污染纪律）。
    #[cfg(unix)]
    mod execute_tests {
        use super::*;
        use std::path::Path;
        use std::sync::atomic::{AtomicU64, Ordering};

        static SEQ: AtomicU64 = AtomicU64::new(0);

        /// 装配单最小夹具（profile.rs GOOD 锚同族缩减：一域 + 一舵机轴 pin +
        /// budget/slots 段全显式）——装载链预核要全链过，段缺即红。
        const PROFILE: &str = r#"
schema_version = 5

[os]
route = "distro"

[runtime]
log_ring_cap = 512

[platform]
tier = "host"
chip = "rk3566"

[supplies]
watchdog_present = false
watchdog_timeout_ms = 15000
watchdog_feeder = "rrd"
tick_period_ms = 200
tick_jitter_ms = 100
safety_core_interval_ms = 1000

[[supplies.domain]]
name = "rail5v"
nominal_mv = 5000
mild_mv = 4600
severe_mv = 4400
current_cap_ma = 2000
sense = "none"
current_sense = "none"
switch = "none"
switchable = false
armed = false
evidence = "none"
max_age_ms = 0

[supplies.budget]
not_applicable = true

[bindings]
"act.servo" = "rr3-act"

[hardware]
alsa_device = "default"
link_ifname = "wlan0"

[[pin]]
role = "act.axis.pwm"
instance = "pan"
controller = "pwm0"
channel = 0
physical = [12]
direction = "out"
rail = "rail5v"
failsafe = "release"
mux_group = []

[[pin]]
role = "sensor.env.meter"
controller = "i2c1"
addr = "0x70"
device = "SHTC3"
direction = "in"
mux_group = []

[enabled_slots]
rr3-act = ["act.servo", "sensor.env"]

[slots]
present = ["act.servo", "sensor.env"]
"#;

    /// 接线档夹具（§4.3 九字段，profile.rs 测试区 GOOD_WIRING 同形状）：单线组只挂
    /// vcc/gnd（对 [[pin]] 增删免疫——⑦-a 不点名 pin_id），net=rail5v 在夹具域在册
    /// （⑦-b）。不复制真仓 default/wiring.toml——它引 batt/rail3v3 域与真机 pin 集，
    /// 与最小装配单必然失配，写口预核红例必须由真实判据产生、不由夹具失配产生。
    const WIRING: &str = r#"
[[wire]]
connector = "J-T"
hdr_pin = [1, 2]
signal = ["vcc", "gnd"]
net = "rail5v"
feeds = []
mate = "none"
keyed = false
rated_a = 1.0
gnd_common = "star-1"
"#;

    /// rr3-act 的 servo 件装配清单（profile.rs 测试区 stage_servo_module 同形状）：
    /// 装载链门 c 对 kind=power 挂轨角色执法⑨ 覆盖判据——无 stage 件则一切
    /// "合法过写"用例被 ⑨ 误红（写口预核要全链过，夹具必须供件申报面）。
    const ASSEMBLE: &str = concat!(
        "schema_version = 2\n\n[assemble]\n",
        "handles = [\n",
        "  { trait = \"Rr3PwmServo\", desc_key = \"pwm_chip\" },\n",
        "  { trait = \"Rr3PwmServo\", desc_key = \"pwm_channel\" },\n",
        "]\n",
        "ops = []\n",
        "demands = [\n",
        "  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = [] },\n",
        "]\n",
    );

        struct Fixture {
            dir: PathBuf,
            paths: Rr3DomainWritePaths,
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.dir);
            }
        }

        /// tmp 仓形状夹具：profiles/ + contracts/ 四册 + platform/ + hardware/axis/
        /// + docs/evidence/。契约册用最小合法形状（装载链要读）；平台事实表取
        /// contracts/platform/rk3566.toml 的仓内真册（⑤-a/⑧-b 词表判据源）。
        fn fixture() -> Fixture {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("rr3-dw-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("profiles")).unwrap();
            std::fs::create_dir_all(dir.join("contracts/platform")).unwrap();
            std::fs::create_dir_all(dir.join("hardware/test/axis")).unwrap();
            std::fs::create_dir_all(dir.join("docs/evidence")).unwrap();
            let profile = dir.join("profiles/test.toml");
            std::fs::write(&profile, PROFILE).unwrap();
            std::fs::write(
                dir.join("contracts/slots.toml"),
                include_str!("../../../../contracts/slots.toml"),
            )
            .unwrap();
            std::fs::write(
                dir.join("contracts/roles.toml"),
                include_str!("../../../../contracts/roles.toml"),
            )
            .unwrap();
            std::fs::write(
                dir.join("contracts/devices.toml"),
                include_str!("../../../../contracts/devices.toml"),
            )
            .unwrap();
            // 平台事实表：从仓根复制真册（test 面相对 CARGO_MANIFEST_DIR）
            let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
            let plat = std::fs::read(repo.join("contracts/platform/rk3566.toml"))
                .expect("仓内 rk3566 平台册必须在");
            std::fs::write(dir.join("contracts/platform/rk3566.toml"), plat).unwrap();
            std::fs::write(dir.join("hardware/test/wiring.toml"), WIRING).unwrap();
            // 件申报面：门 c（⑨ 覆盖判据）要求 kind=power 挂轨角色的绑定模块有
            // demands 申报——stage 一份 servo 件（与 profile.rs 测试区同形状）
            let asm = dir.join("modules/rr3-act/atoms/servo/assemble.toml");
            std::fs::create_dir_all(asm.parent().expect("必有父目录")).unwrap();
            std::fs::write(asm, ASSEMBLE).unwrap();
            let paths = Rr3DomainWritePaths {
                profile: profile.clone(),
                slots: dir.join("contracts/slots.toml"),
                roles: dir.join("contracts/roles.toml"),
                devices: dir.join("contracts/devices.toml"),
                wiring: dir.join("hardware/test/wiring.toml"),
                platform_dir: dir.join("contracts/platform"),
            };
            Fixture { dir, paths }
        }

        /// 证据文件夹具（frontmatter 六键全，form 与目录一致——四道核全过的形状）
        fn write_evidence(dir: &Path, form: &str, num: u32) {
            let fdir = dir.join("docs/evidence").join(form);
            std::fs::create_dir_all(&fdir).unwrap();
            std::fs::write(
                fdir.join(format!("{num:04}.md")),
                format!(
                    "+++\nform = \"{form}\"\nslot_or_role = \"rail5v\"\nmethod = \"板上实测\"\nconclusion = \"阈值实测\"\ndate = \"2026-10-23\"\nrecorder = \"批N22\"\n+++\n\n正文。\n"
                ),
            )
            .unwrap();
        }

        fn params_armed_true() -> Value {
            json!({
                "target": "domain", "name": "rail5v", "key": "armed",
                "value": true, "evidence": "实测/0001"
            })
        }

        /// 红一：无确认拒——confirm 缺/false 一律 Permission，零字节落盘
        #[cfg(unix)]
        #[test]
        fn rejects_without_confirm_and_writes_nothing() {
            let fx = fixture();
            let before = std::fs::read_to_string(&fx.paths.profile).unwrap();
            let mut p = rr3_domain_write_parse(&params_armed_true()).unwrap();
            p.confirm = None;
            let (code, msg) = rr3_domain_write_execute(&fx.paths, &p).expect_err("无确认必须拒");
            assert_eq!(code, Rr3StatusCode::Permission);
            assert!(msg.contains("二次确认"), "{msg}");
            let after = std::fs::read_to_string(&fx.paths.profile).unwrap();
            assert_eq!(before, after, "拒写零字节落盘");
        }

        /// 红二：证据引用非法拒写——形制坏/文件缺都拒，零字节落盘
        #[cfg(unix)]
        #[test]
        fn rejects_bad_evidence_and_writes_nothing() {
            let fx = fixture();
            let before = std::fs::read_to_string(&fx.paths.profile).unwrap();
            // 文件缺（形制合法但四道核第二道红）
            let mut p = rr3_domain_write_parse(&params_armed_true()).unwrap();
            p.confirm = Some(true);
            let (code, msg) = rr3_domain_write_execute(&fx.paths, &p).expect_err("证据缺必须拒");
            assert_eq!(code, Rr3StatusCode::Permission);
            assert!(msg.contains("证据引用核验不过"), "{msg}");
            // 形制坏
            let mut p = rr3_domain_write_parse(&json!({
                "target": "domain", "name": "rail5v", "key": "armed",
                "value": true, "evidence": "not-a-ref"
            }))
            .unwrap();
            p.confirm = Some(true);
            let (code, _) = rr3_domain_write_execute(&fx.paths, &p).expect_err("形制坏必须拒");
            assert_eq!(code, Rr3StatusCode::Permission);
            assert_eq!(
                before,
                std::fs::read_to_string(&fx.paths.profile).unwrap(),
                "两次拒写零字节落盘"
            );
        }

        /// 绿：合法过写——armed=true + 证据四道核全过，档内 armed/evidence 联动
        /// 成对落盘，装载预核过，应答带 effective:boot。武装前提（⑥-c：有测点 +
        /// 龄期上界 > 0）由夹具覆写供齐——profile.rs 测试区
        /// armed_domain_evidence_file_check_gate 同一构造（sense 指在册 i2c 测点）。
        #[cfg(unix)]
        #[test]
        fn armed_write_persists_paired_shape_and_answers_boot() {
            let fx = fixture();
            write_evidence(&fx.dir, "实测", 1);
            // 武装前提：域 sense 绑在册测点 pin + max_age_ms > 0（⑥-c 两前件）
            let base = std::fs::read_to_string(&fx.paths.profile).unwrap();
            let text = base
                .replace(
                    "sense = \"none\"",
                    "sense = \"sensor.env.meter\"",
                )
                .replace("max_age_ms = 0", "max_age_ms = 30000");
            std::fs::write(&fx.paths.profile, text).unwrap();
            let mut p = rr3_domain_write_parse(&params_armed_true()).unwrap();
            p.confirm = Some(true);
            let v = rr3_domain_write_execute(&fx.paths, &p).expect("合法过写应成功");
            assert_eq!(v["ok"], true);
            assert_eq!(v["old"], false);
            assert_eq!(v["new"], true);
            assert_eq!(v["effective"], "boot");
            let after = std::fs::read_to_string(&fx.paths.profile).unwrap();
            assert!(after.contains("armed = true"), "{after}");
            assert!(after.contains("evidence = \"实测/0001\""), "{after}");
        }

        /// 绿：severe_mv 过写——只动目标键行，联动 evidence 不入档（仅留痕）
        #[cfg(unix)]
        #[test]
        fn severe_mv_write_touches_target_line_only() {
            let fx = fixture();
            write_evidence(&fx.dir, "实测", 1);
            let mut p = rr3_domain_write_parse(&json!({
                "target": "domain", "name": "rail5v", "key": "severe_mv",
                "value": 4300, "evidence": "实测/0001"
            }))
            .unwrap();
            p.confirm = Some(true);
            let v = rr3_domain_write_execute(&fx.paths, &p).expect("合法过写应成功");
            assert_eq!(v["old"], 4400);
            assert_eq!(v["new"], 4300);
            let after = std::fs::read_to_string(&fx.paths.profile).unwrap();
            assert!(after.contains("severe_mv = 4300"), "{after}");
            assert!(after.contains("evidence = \"none\""), "evidence 不入档: {after}");
        }

        /// 红三：手术后装载预核不过拒落盘——未标定域（sense=none、max_age_ms=0）
        /// 武装即撞装载链 ⑥-c：写口证据门过 ≠ 预核免（双门串联），tmp 产物零字节
        /// 进正档、旧档原封。mild>severe 次序判据归批N2 门 c（消费者侧，未落装载
        /// 链）——预核红例必须用装载链真判，不许造"判据还不在"的假红例。
        #[cfg(unix)]
        #[test]
        fn rejects_when_preboot_check_fails() {
            let fx = fixture();
            write_evidence(&fx.dir, "实测", 1);
            let before = std::fs::read_to_string(&fx.paths.profile).unwrap();
            let mut p = rr3_domain_write_parse(&params_armed_true()).unwrap();
            p.confirm = Some(true);
            let (code, msg) = rr3_domain_write_execute(&fx.paths, &p).expect_err("预核红必须拒");
            assert_eq!(code, Rr3StatusCode::Invalid);
            assert!(msg.contains("预核不过"), "{msg}");
            assert!(msg.contains("⑥-c"), "预核红须点名装载链判据: {msg}");
            assert_eq!(
                before,
                std::fs::read_to_string(&fx.paths.profile).unwrap(),
                "预核拒零字节落盘"
            );
            // tmp 不残留
            let stem = fx.paths.profile.file_stem().unwrap().to_str().unwrap();
            assert!(!fx
                .paths
                .profile
                .with_file_name(format!("{stem}.tmp-write.toml"))
                .exists(), "拒写须清 tmp");
        }

        /// axis 落点：合法过写复用产线写入面——轴档 current_ceil_ma 变更落盘
        #[cfg(unix)]
        #[test]
        fn axis_write_reuses_production_writer() {
            let fx = fixture();
            write_evidence(&fx.dir, "对拍", 1);
            // 轴档夹具（十四键，pin 对账装配单 [[pin]]）
            std::fs::write(
                fx.dir.join("hardware/test/axis/pan.toml"),
                "schema = 1\npin = \"act.axis.pwm.pan\"\ncenter_deg = 90\nlo_deg = 60\nhi_deg = 120\ndir_sign = 1\nslew_deg_s = 100\nsettle_ms = 200\nhold = \"release\"\ncurrent_ceil_ma = 750\nwarmup_ms = 0\nhard_stop = \"yes\"\ncalibrated = true\nsource = \"manual\"\n",
            )
            .unwrap();
            let mut p = rr3_domain_write_parse(&json!({
                "target": "axis", "name": "pan", "key": "current_ceil_ma",
                "value": 700, "evidence": "对拍/0001"
            }))
            .unwrap();
            p.confirm = Some(true);
            let v = rr3_domain_write_execute(&fx.paths, &p).expect("axis 过写应成功");
            assert_eq!(v["old"], 750);
            assert_eq!(v["new"], 700);
            assert_eq!(v["effective"], "boot");
            let after = std::fs::read_to_string(fx.dir.join("hardware/test/axis/pan.toml"))
                .unwrap();
            assert!(after.contains("current_ceil_ma = 700"), "{after}");
        }

        /// axis 落点红面：轴档不在场拒（立档走产线写入面，写口不立新档）
        #[cfg(unix)]
        #[test]
        fn axis_write_rejects_missing_axis_file() {
            let fx = fixture();
            write_evidence(&fx.dir, "对拍", 1);
            let mut p = rr3_domain_write_parse(&json!({
                "target": "axis", "name": "pan", "key": "current_ceil_ma",
                "value": 700, "evidence": "对拍/0001"
            }))
            .unwrap();
            p.confirm = Some(true);
            let (code, msg) = rr3_domain_write_execute(&fx.paths, &p).expect_err("档缺必须拒");
            assert_eq!(code, Rr3StatusCode::Invalid);
            assert!(msg.contains("不在场"), "{msg}");
        }
    }
}
