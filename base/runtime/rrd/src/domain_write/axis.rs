//! axis 落点：轴档定位、证据门、单键手术与候选档。

use super::*;

/// axis 落点写编排：**必须复用** [`crate::axis_write::rr3_axis_write_run`]（其头注
/// 第 15-16 行："批N4 面板接线时复用同一函数（§7'落盘只许有一份实现'，第二条写
/// 路径即红"）——本函数只做"读旧档 → 单键手术 → 产 tmp 候选档"，十四键全判据、
/// 原子落盘、副本拒全部归它。
/// axis 写口前置：定位轴档、读旧值、过证据门。
#[cfg(unix)]
fn rr3_axis_write_prep(
    paths: &Rr3DomainWritePaths,
    params: &Rr3DomainWriteParams,
) -> Result<(PathBuf, Value, String), (Rr3StatusCode, String)> {
    let axis_path = rr3_axis_path(paths, &params.name)?;
    let old = rr3_axis_old(paths, &axis_path)?;
    // 证据引用门（仅留痕——轴档十四键无 evidence 键，写动作证据住审计留痕）
    let ev = rr3_axis_evidence(paths, params)?;
    Ok((axis_path, old, ev))
}

#[cfg(unix)]
pub(super) fn rr3_write_axis(
    paths: &Rr3DomainWritePaths,
    params: &Rr3DomainWriteParams,
) -> Result<Value, (Rr3StatusCode, String)> {
    let invalid = |msg: String| (Rr3StatusCode::Invalid, msg);
    // key 白名单已在 parse 裁（axis 只认 current_ceil_ma），此处直接编排
    let Rr3WriteValue::Num(n) = params.value else {
        return Err(invalid("current_ceil_ma 要求正整数".to_string()));
    };
    let (axis_path, old, ev) = rr3_axis_write_prep(paths, params)?;
    // 单键手术 → tmp 候选档 → 复用产线写入面（全判据＋原子落盘＋副本拒单点）
    let echo = rr3_axis_swap(&paths.profile, &axis_path, n)?;
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

/// 单键手术到落盘的一段：tmp 候选档 → 产线写入面 → 清候选档。
#[cfg(unix)]
fn rr3_axis_swap(
    profile: &Path,
    axis_path: &Path,
    n: u64,
) -> Result<String, (Rr3StatusCode, String)> {
    let invalid = |msg: String| (Rr3StatusCode::Invalid, msg);
    let tmp = rr3_axis_stage(axis_path, n)?;
    let run = crate::axis_write::rr3_axis_write_run(&crate::axis_write::Rr3AxisWriteArgs {
        profile: profile.to_path_buf(),
        candidate: tmp.clone(),
    });
    let _ = std::fs::remove_file(&tmp);
    run.map_err(invalid)
}

/// 轴档写口证据门：必携引用且过核验（仅留痕，不入档）。
#[cfg(unix)]
fn rr3_axis_evidence(
    paths: &Rr3DomainWritePaths,
    params: &Rr3DomainWriteParams,
) -> Result<String, (Rr3StatusCode, String)> {
    let Some(ev) = params.evidence.clone() else {
        return Err((
            Rr3StatusCode::Permission,
            "current_ceil_ma 安全写必携证据引用 params.evidence（B-15 ③）".to_string(),
        ));
    };
    rr3_write_evidence(&paths.profile, &ev)?;
    Ok(ev)
}

/// 轴实例名形制（纯函数）：单段标识，不含路径分隔符 / `.` / `..` / 控制字符。
/// 名字直接拼进轴档路径，放开就成"写口可改轴目录外的任意在册轴档"——实例名是
/// 标识符不是路径，形制核在拼路径的单点处裁，fail-closed。
pub(super) fn rr3_axis_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
}

/// 轴档定位：hardware/<形态>/axis/<instance>.toml（接线布局推导的同一目录锚），
/// 不在场即拒（写口只改在册档单键；立档走产线写入面全判据流程）。
fn rr3_axis_path(
    paths: &Rr3DomainWritePaths,
    name: &str,
) -> Result<PathBuf, (Rr3StatusCode, String)> {
    rr3_axis_name_gate(name)?;
    let axis_dir = rr3_axis_dir(paths)?;
    let axis_path = axis_dir.join(format!("{name}.toml"));
    rr3_axis_must_exist(axis_path)
}

/// 轴实例名形制门：不合即拒并点名缺的是哪一层（形制判据在 [`rr3_axis_name_ok`]）。
fn rr3_axis_name_gate(name: &str) -> Result<(), (Rr3StatusCode, String)> {
    if rr3_axis_name_ok(name) {
        return Ok(());
    }
    Err((
        Rr3StatusCode::Invalid,
        format!(
            "轴实例名 {name:?} 须为单段标识（不含路径分隔符 / `..` / 控制字符）——写口只改本形态 axis/ 目录内的在册档"
        ),
    ))
}

/// 轴档目录（hardware/<形态>/axis）：由接线档的父目录推导，无父目录即拒。
fn rr3_axis_dir(paths: &Rr3DomainWritePaths) -> Result<PathBuf, (Rr3StatusCode, String)> {
    let Some(parent) = paths.wiring.parent() else {
        return Err((Rr3StatusCode::Invalid, "接线档路径无父目录".to_string()));
    };
    Ok(parent.join("axis"))
}

fn rr3_axis_must_exist(axis_path: PathBuf) -> Result<PathBuf, (Rr3StatusCode, String)> {
    if !axis_path.exists() {
        return Err((
            Rr3StatusCode::Invalid,
            format!(
                "轴档 {} 不在场（写口只改在册档单键；立档走产线写入面 axis_write 全判据流程）",
                axis_path.display()
            ),
        ));
    }
    Ok(axis_path)
}

/// 旧值提取：装载域拿（pin 外键核同一份判据；装载红 = 坏档，拒——
/// axis_write_run 的"坏目标不覆盖"在下游还会挡一道，这里先点名）。
fn rr3_axis_old(
    paths: &Rr3DomainWritePaths,
    axis_path: &Path,
) -> Result<Value, (Rr3StatusCode, String)> {
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
    let pin_ids: std::collections::BTreeSet<String> =
        profile.pins.iter().map(|p| p.rr3_pin_id()).collect();
    let existing = crate::axis::rr3_act_profile_load(axis_path, &pin_ids).map_err(|errs| {
        invalid(format!(
            "轴档 {} 装载红: {}",
            axis_path.display(),
            errs.join("; ")
        ))
    })?;
    let Some(file) = existing else {
        return Err(invalid(format!("轴档 {} 不在场", axis_path.display())));
    };
    Ok(json!(file.profile.current_ceil_ma))
}

/// 单键手术产出 tmp 候选档：读旧档 → 改 current_ceil_ma → 独占写到临时路径
/// （机制单点 rr-ipc 的 0600 候选落盘——文件名只作去重、不承担安全，落在世界
/// 可写的 tmp 目录里时安全属性由独占建给足）。随落盘序 unix 门：唯一调用方
/// [`rr3_write_axis`] 同门，非 unix 编译面不得引用该门内的机制件。
#[cfg(unix)]
fn rr3_axis_stage(axis_path: &Path, n: u64) -> Result<PathBuf, (Rr3StatusCode, String)> {
    let invalid = |msg: String| (Rr3StatusCode::Invalid, msg);
    let new_text = crate::profile::rr3_key_line_apply(
        &std::fs::read_to_string(axis_path)
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
    rr_ipc::rr3_stage_excl(&tmp, new_text.as_bytes(), 0o600)
        .map_err(|e| invalid(format!("写候选档 {} 失败: {e}", tmp.display())))?;
    Ok(tmp)
}
