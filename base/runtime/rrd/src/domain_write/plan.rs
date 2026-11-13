//! 证据根、执行编排与 domain 读/计划/提交。

use super::*;

/// 证据容器根（§6.1：modules-root 同一条 `<root>/profiles/` 布局纪律推导出
/// `<root>/docs/evidence`；与装载期⑥-e 文件半边同一条推导）。推不出两级父目录
/// = 装配单路径形状破，**拒**——旧序退回相对路径 `docs/evidence`，root 侧写口
/// 按 CWD 解析等于"证据根由启动目录决定"，这是猜路径不是推导。
pub(super) fn rr3_evidence_root(
    profile_path: &std::path::Path,
) -> Result<PathBuf, (Rr3StatusCode, String)> {
    let Some(root) = profile_path
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("docs/evidence"))
    else {
        return Err((
            Rr3StatusCode::Invalid,
            format!(
                "装配单路径 {} 无两级父目录，证据容器根不可推导——写口拒（不按 CWD 猜证据根）",
                profile_path.display()
            ),
        ));
    };
    Ok(root)
}

/// 证据引用门（B-15 ③）：必填形状 + 四道核（`rr-ipc::evidence` 唯一实现）。
/// 核败 = Permission 拒写（安全写不带合法证据就是无权写）。
pub(super) fn rr3_write_evidence(
    profile_path: &std::path::Path,
    reference: &str,
) -> Result<(), (Rr3StatusCode, String)> {
    let root = rr3_evidence_root(profile_path)?;
    rr_ipc::evidence::rr3_evidence_verify(&root, reference).map_err(|e| {
        (
            Rr3StatusCode::Permission,
            format!("写口证据引用核验不过（B-15 ③——安全写必须携带真板证据）: {e}"),
        )
    })
}

/// 必携证据取得：缺证据即 Permission 拒写，核验通过后回带引用文本。
fn rr3_take_evidence(
    profile_path: &std::path::Path,
    evidence: Option<String>,
    missing: &str,
) -> Result<String, (Rr3StatusCode, String)> {
    let Some(ev) = evidence else {
        return Err((Rr3StatusCode::Permission, missing.to_string()));
    };
    rr3_write_evidence(profile_path, &ev)?;
    Ok(ev)
}

/// 写口执行段进程级串行锁（B38 审计）：ctl 每连接一线程，两个并发写各自
/// "读旧档 → 手术 → 装载预核 → 原子换"互不可见——候选档用固定 tmp 名
/// （`<stem>.tmp-write.toml`），对方 `rr3_stage_excl` 的"清残留"会先删掉己方
/// 候选再建自己的：己方预核读到的是**对方**的文本（自己文本没过预核也可能被
/// 提交 = fail-open），或后写者静默覆盖前写者（丢写；武装写被并发阈值写吞掉
/// 是安全面事故）。锁覆盖执行全段，与 counters 落盘锁同一"原子写单写者"纪律。
/// 跨进程（运维 CLI `rrd axis-write`）不在同一锁面：它是离线立档工具，与 daemon
/// 写口并发须人工避让；两者各产完整合法候选档、落盘都是原子换，不会有半截档。
#[cfg(unix)]
static RR3_WRITE_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 写口执行（策略编排，逐门显式；`#[cfg(unix)]` 随落盘序同门——原子写与
/// `rr3_axis_write_run` 都在 unix 门内，非 unix 编译时本函数不存在，conn 臂
/// 如实回"写面仅 unix"）。门序：confirm 门 → 执行段串行锁 → 旧值提取（域缺/
/// 档缺拒）→ 证据引用门 → 文本手术 → tmp 落盘 → 装载链全量预核（Err 即拒写清
/// tmp，零字节进正档）→ 原子写回 → 契约记账重记 → 审计留痕。
#[cfg(unix)]
pub fn rr3_write_execute(
    paths: &Rr3DomainWritePaths,
    params: &Rr3DomainWriteParams,
) -> Result<Value, (Rr3StatusCode, String)> {
    let perm = |msg: String| (Rr3StatusCode::Permission, msg);
    // ① confirm 门（ADR-0021 裁定 6：拒动类阈值写必须二次确认——缺/false 一律拒）
    let confirmed = params.confirm.unwrap_or(false);
    if !confirmed {
        return Err(perm(
            "缺二次确认 params.confirm = true（ADR-0021 裁定 6——拒动类阈值写必须二次确认与留痕）"
                .to_string(),
        ));
    }
    // ② 执行段串行（见 [`RR3_WRITE_SERIAL`] 注记）：读 → 预核 → 换整段单写者
    let _serial = RR3_WRITE_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    rr3_write_apply(paths, params)
}

/// 写目标分派（`#[cfg(unix)]` 与两臂的落盘实现同门：本函数体只转发，
/// 非 unix 目标下没有可转发对象——门少写一处即 Windows 编面红，⑨-y 已抓过）。
#[cfg(unix)]
fn rr3_write_apply(
    paths: &Rr3DomainWritePaths,
    params: &Rr3DomainWriteParams,
) -> Result<Value, (Rr3StatusCode, String)> {
    match params.target {
        Rr3WriteTarget::Domain => rr3_write_domain(paths, params),
        Rr3WriteTarget::Axis => rr3_write_axis(paths, params),
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
fn rr3_write_domain(
    paths: &Rr3DomainWritePaths,
    params: &Rr3DomainWriteParams,
) -> Result<Value, (Rr3StatusCode, String)> {
    let (text, dom) = rr3_domain_read(paths, params)?;
    let (old_value, updates) = rr3_domain_plan(paths, params, &dom)?;
    let boot = rr3_domain_commit(paths, &text, params, &updates)?;
    let relogged = rr3_domain_relog(paths, &boot);
    let new_value = rr3_json_value(&params.value);
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

/// 写口值 → JSON 应答值。
#[cfg(unix)]
fn rr3_json_value(v: &Rr3WriteValue) -> Value {
    match v {
        Rr3WriteValue::Bool(b) => json!(b),
        Rr3WriteValue::Num(n) => json!(n),
    }
}

/// 读装配单并定位目标域（域缺拒——写面不做新域注册；与 boot 同一解析器）
#[cfg(unix)]
fn rr3_domain_read(
    paths: &Rr3DomainWritePaths,
    params: &Rr3DomainWriteParams,
) -> Result<(String, crate::profile::Rr3Domain), (Rr3StatusCode, String)> {
    let invalid = |msg: String| (Rr3StatusCode::Invalid, msg);
    let (text, profile) = rr3_profile_load(paths)?;
    let Some(dom) = profile
        .supplies
        .domains
        .iter()
        .find(|d| d.name == params.name)
        .cloned()
    else {
        return Err(invalid(format!(
            "域 {:?} 不在 [[supplies.domain]] 在册（缺域拒——新域是契约事件，不经写口注册）",
            params.name
        )));
    };
    Ok((text, dom))
}

/// 读装配单文本并解析（读不到 / 形状不合各自带因点名）。
#[cfg(unix)]
fn rr3_profile_load(
    paths: &Rr3DomainWritePaths,
) -> Result<(String, crate::profile::Rr3Profile), (Rr3StatusCode, String)> {
    let invalid = |msg: String| (Rr3StatusCode::Invalid, msg);
    let text = std::fs::read_to_string(&paths.profile)
        .map_err(|e| invalid(format!("读不到装配单 {}: {e}", paths.profile.display())))?;
    let profile = crate::profile::rr3_profile_parse(&text).map_err(|errs| {
        invalid(format!(
            "装配单 {} 形状不合: {}",
            paths.profile.display(),
            errs.join("; ")
        ))
    })?;
    Ok((text, profile))
}

/// 键行更新集：键名 → 新值文本（手术输入）
#[cfg(unix)]
type Rr3DomainUpdates = Vec<(&'static str, String)>;

/// 写入计划：旧值提取 + 键行更新集（armed 与 evidence 成对，severe_mv 必携证据）
#[cfg(unix)]
pub(super) fn rr3_domain_plan(
    paths: &Rr3DomainWritePaths,
    params: &Rr3DomainWriteParams,
    dom: &crate::profile::Rr3Domain,
) -> Result<(Value, Rr3DomainUpdates), (Rr3StatusCode, String)> {
    let invalid = |msg: String| (Rr3StatusCode::Invalid, msg);
    let plan = match (params.key.as_str(), &params.value) {
        // armed 写：evidence 联动（成对纪律——写面直接落出⑥-e 要求的档内形状）
        ("armed", Rr3WriteValue::Bool(armed)) => rr3_plan_armed(paths, params, dom, *armed)?,
        ("severe_mv", Rr3WriteValue::Num(n)) => {
            rr3_take_evidence(
                &paths.profile,
                params.evidence.clone(),
                "severe_mv 安全写必携证据引用 params.evidence（B-15 ③）",
            )?;
            (json!(dom.severe_mv), vec![("severe_mv", n.to_string())])
        }
        // 键/值型不配：parse 层白名单已裁，正常到不了；但参数结构对库内可见可构造，
        // 不变式破损必须落成"拒"而不是 panic——写面在守护线程里，panic 断连接丢应答。
        (key, _) => {
            return Err(invalid(format!(
                "写口键 {key:?} 与值型不配（domain → armed 布尔 / severe_mv 正整数），拒"
            )))
        }
    };
    Ok(plan)
}

/// armed 写计划：置 true 携证据并在档内成对写 evidence，置 false 显式写 none。
#[cfg(unix)]
fn rr3_plan_armed(
    paths: &Rr3DomainWritePaths,
    params: &Rr3DomainWriteParams,
    dom: &crate::profile::Rr3Domain,
    armed: bool,
) -> Result<(Value, Rr3DomainUpdates), (Rr3StatusCode, String)> {
    if armed {
        let ev = rr3_take_evidence(
            &paths.profile,
            params.evidence.clone(),
            "armed = true 置位必携证据引用 params.evidence（D11 界面门——必须引用一条真板对拍证据编号）",
        )?;
        Ok((
            json!(dom.armed),
            vec![
                ("armed", "true".to_string()),
                ("evidence", format!("\"{ev}\"")),
            ],
        ))
    } else {
        Ok((
            json!(dom.armed),
            vec![
                ("armed", "false".to_string()),
                ("evidence", "\"none\"".to_string()),
            ],
        ))
    }
}

/// 手术 → tmp 落盘 → 装载链全量预核 → 原子写回（Err 即清 tmp，旧档原封）
#[cfg(unix)]
fn rr3_domain_commit(
    paths: &Rr3DomainWritePaths,
    text: &str,
    params: &Rr3DomainWriteParams,
    updates: &[(&str, String)],
) -> Result<crate::profile::Rr3ProfileBoot, (Rr3StatusCode, String)> {
    let invalid = |msg: String| (Rr3StatusCode::Invalid, msg);
    let update_refs: Vec<(&str, &str)> = updates.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let new_text =
        crate::profile::rr3_domain_apply(text, &params.name, &update_refs).map_err(invalid)?;
    let Some(stem) = paths.profile.file_stem().and_then(std::ffi::OsStr::to_str) else {
        return Err(invalid("装配单无文件名".to_string()));
    };
    let tmp = paths
        .profile
        .with_file_name(format!("{stem}.tmp-write.toml"));
    // 候选档独占建（机制单点 rr-ipc）：tmp 位同机可写，跟随符号链接写会变成
    // 任意文件截断器；独占建 + 定权把这条路径关死（残留由机制先清）。
    rr_ipc::rr3_stage_excl(&tmp, new_text.as_bytes(), 0o644)
        .map_err(|e| invalid(format!("写临时档 {} 失败: {e}", tmp.display())))?;
    let boot = rr3_commit_preboot(paths, &tmp)?;
    // 原子写回（机制单点 tmp→fsync→rename；失败清 tmp 旧档原封）
    crate::counters::rr3_atomic_store(&paths.profile, &tmp, new_text.as_bytes()).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        invalid(format!(
            "装配单 {} 原子写回失败: {e}",
            paths.profile.display()
        ))
    })?;
    Ok(boot)
}

/// 候选档装载链全量预核：红即清 tmp 并带因拒（旧档原封）。
#[cfg(unix)]
fn rr3_commit_preboot(
    paths: &Rr3DomainWritePaths,
    tmp: &Path,
) -> Result<crate::profile::Rr3ProfileBoot, (Rr3StatusCode, String)> {
    match crate::profile::rr3_boot_inputs(
        tmp,
        &paths.slots,
        &paths.roles,
        &paths.devices,
        &paths.wiring,
        &paths.platform_dir,
    ) {
        Ok(b) => Ok(b),
        Err(e) => {
            let _ = std::fs::remove_file(tmp);
            Err((
                Rr3StatusCode::Invalid,
                format!("手术后装载链预核不过，拒绝落盘（旧档原封）: {e}"),
            ))
        }
    }
}

/// 契约记账重记：写口的 tmp 产物已过装载链预核 = 新验收事实；重记失败不回滚
/// 落盘（主事实已发生），点名让下次 boot 重定格。
#[cfg(unix)]
fn rr3_domain_relog(paths: &Rr3DomainWritePaths, boot: &crate::profile::Rr3ProfileBoot) -> bool {
    let Some(dir) = paths.slots.parent().map(PathBuf::from) else {
        return true;
    };
    match crate::contracts::rr3_contracts_scan(
        &paths.profile,
        &dir,
        &paths.wiring,
        &boot.platform_chip,
    )
    .and_then(crate::contracts::rr3_contracts_rebook)
    {
        Ok(()) => true,
        Err(e) => {
            crate::rr3_log(&format!(
                "domain 写口重记账失败（{e}）——契约自检将报漂直到下次 boot 重定格"
            ));
            false
        }
    }
}
