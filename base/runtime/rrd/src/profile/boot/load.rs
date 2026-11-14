//! 装配装载：装配单/契约册装载、装载期校验编排与 boot 输入构造。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rr_registry::{
    rr3_dev_parse, rr3_plat_parse, rr3_role_parse, rr3_slot_parse, Rr3DeviceTable,
    Rr3PlatformTable, Rr3RoleTable, Rr3SlotTable,
};

use super::super::model::*;
use super::super::parse::{rr3_profile_abs, rr3_profile_parse};
use super::plan::rr3_platform_file;
use super::reject::rr3_boot_checks;
use crate::profile_checks::{
    rr3_axis_load, rr3_hal_bindings, rr3_manifests, rr3_role_gaps, rr3_slot_gaps,
};

/// 装配链 boot 输入装配（daemon 在 bind 之前调用，fatal 即非 0 退出、套接字不留残迹）。
/// 致命错误（读不到 / 形状不合 / 查册未命中 / 结构不自洽）打包成一条串返回；
/// 缺格**不是**致命项（P2：缺格只降点名模块，缺格≠拒 boot），进 [`Rr3ProfileBoot::missing`]；
/// 缺角色同样不是致命项（校验②后果 = 降），进 [`Rr3ProfileBoot::role_missing`]。
/// `slots_path`/`roles_path`/`devices_path`/`wiring_path` 由调用方给定（`--contracts`
/// 运行期定位或编译期仓内路径；接线档由 [`crate::wiring::rr3_wiring_path`]
/// 按 `hardware/<形态>/wiring.toml` 布局推导）——查册真值仍只有 rr-registry 那几册
/// 与接线档本体，这里只管从哪读。
pub fn rr3_boot_inputs(
    profile_path: &Path,
    slots_path: &Path,
    roles_path: &Path,
    devices_path: &Path,
    wiring_path: &Path,
    platform_dir: &Path,
) -> Result<Rr3ProfileBoot, String> {
    let probe = crate::probe::rr3_probe_default();
    rr3_boot_with_probe(
        profile_path,
        slots_path,
        roles_path,
        devices_path,
        wiring_path,
        platform_dir,
        probe.as_ref(),
    )
}

/// 同 [`rr3_boot_inputs`]，但探测件由调用方注入（MC-4 接缝）：生产走
/// [`crate::probe::rr3_probe_default`]（非 test 面按目标分流），门禁用例注入
/// 恒缺/假根探针钉"缺格 → Degraded 点名件与键"。探测是纯读面、无状态，注入
/// 不改变 boot 前的单次消费语义（ADR-0012 裁定 4）。
pub fn rr3_boot_with_probe(
    profile_path: &Path,
    slots_path: &Path,
    roles_path: &Path,
    devices_path: &Path,
    wiring_path: &Path,
    platform_dir: &Path,
    probe: &dyn crate::probe::Rr3Probe,
) -> Result<Rr3ProfileBoot, String> {
    let (abs, profile) = rr3_load_profile(profile_path)?;

    // 查册门（裁定 5②）：格位真值只有 rr-registry 那一册，CI 全集层与本入口共用。
    let table = rr3_load_slots(slots_path)?;

    // 角色契约装载（增补轮六 §4.1，校验①②判据源）：与格位契约同一 fail-closed
    // 口径——读不到 / 解析空 / 形状越界都是契约事故，拒启动（空角色表不是放行许可证）。
    let roles = rr3_load_roles(roles_path)?;

    // 器件词表装载（增补轮六 §4.1b，校验⑦-d 的 mate 集判定源）：与格位/角色契约
    // 同一 fail-closed 口径——空器件表是契约事故，不是放行许可证。装载期只查
    // mate 外键闭合，不认型号能力（D10a：addrs/baud_ok 是探测期/出厂自检判据）。
    let devices = rr3_load_devices(devices_path)?;

    // 接线档装载（增补轮六 §4.3，校验⑦ 判据面）：装机档必在——读不到/空表/
    // 形状不合/闭合面破都是装配事故，拒启动（接线闭合是校验⑦的前提，没有
    // "无线的机器"）。
    let (wiring_abs, wiring) = rr3_load_wiring(wiring_path)?;

    // 平台事实表装载（增补轮六 §4.6，批N1 基-22；校验③④⑤⑧的判据前件）：按
    // [platform].chip 在 platform_dir 下选册（daemon 传 contracts/platform/，与
    // roles/devices 同源）。fail-closed 与 roles 同口径但更严——platform.rs 解析本就
    // 回 Err 版（roles/devices 宽容回空表走"表为空"点名，那是集合词表；平台表宽容 =
    // ④⑤⑧ 静默不判，§18 F-6 同病），读不到/形状坏都逐条点名拒 boot，不降级。
    let platform = rr3_load_platform(platform_dir, &profile.platform_chip)?;

    let pin_ids: BTreeSet<String> = profile.pins.iter().map(|p| p.rr3_pin_id()).collect();
    let root = abs.parent().and_then(Path::parent).ok_or_else(|| {
        format!(
            "装配单 {} 无法定位 modules-root（须在 <root>/profiles/ 下）",
            abs.display()
        )
    })?;
    // 装载期校验族整段交 [`rr3_boot_checks`]（追加序即判据序，与逐条写在本处等价）。
    let (errs, derived, assemble_broken) = rr3_boot_checks(
        &profile, &table, &roles, &devices, &platform, &wiring, root, &pin_ids,
    );
    if !errs.is_empty() {
        return Err(errs.join("; "));
    }
    // 装载期校验②（后果 = 降不拒）：present ∩ enabled_slots 的缺角色清单
    let role_missing = rr3_role_gaps(&profile, &roles);

    // 缺格判定：enabled_slots 声明的非基础层格位没有指向本模块的有效 binding（或探测不到）。
    // 基础层恒在位，跳过（slots.toml 注记：net.online / power.battery boot 即在位、恒不降级）。
    let missing = rr3_slot_gaps(&profile, &table, &roles, probe);

    let manifests = rr3_manifests(&profile, root);
    let hal_bindings = rr3_hal_bindings(derived, &profile.hardware);
    let (axis_files, axis_state, axis_notes) = rr3_axis_load(&wiring_abs, &pin_ids);

    // G4 装配段校验（ADR-0019 修订 1 ③）已在门 c 段随装配扫描一次完成：缺文件/
    // 坏形/ops 注入缺逐条点名进 assemble_broken，该模块 boot 入 Degraded（不 fatal；
    // 修数据 + 重启是唯一解除路）。此处只消费上文的 assemble_broken。

    // 装配快照（批N4 件九笔4，`rr3.ctl.wiring` 判据源）：至此装配单/接线档/轴
    // 档案/角色表四路全部过完装载期校验（fatal 面早已逐条拒净），这是"验收通过的
    // 世界"唯一收账点——clone 进快照交给 daemon 定格 OnceLock；boot 函数自身不碰
    // 全局态（纯构造，住处由调用方拍）。快照与 boot 返回值同生命周期，装载面之后
    // 对文件的改动进不了本进程的 ctl 应答（快照语义，assembly.rs 模块头注）。
    let snapshot = crate::assembly::Rr3AssemblySnapshot {
        profile_name: abs
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string(),
        profile: profile.clone(),
        wiring: wiring.clone(),
        peak_by_mate: devices.rr3_dev_peak_by_mate(),
        axis: axis_files,
        roles: roles.clone(),
    };

    Ok(Rr3ProfileBoot {
        manifests,
        missing,
        hal_bindings,
        assemble_broken,
        role_missing,
        axis_state,
        axis_notes,
        log_ring_cap: profile.log_ring_cap,
        hal_backend: profile.hal_backend.clone(),
        audio_backend: profile.audio_backend.clone(),
        servo_backend: profile.servo_backend.clone(),
        display_backend: profile.display_backend.clone(),
        platform_chip: profile.platform_chip.clone(),
        platform_tier: profile.platform_tier.clone(),
        snapshot: Some(snapshot),
    })
}

/// 装配单装载：绝对化 → 读文本 → 解析（形状不合逐条点名，一条不静默放行）
fn rr3_load_profile(path: &Path) -> Result<(PathBuf, Rr3Profile), String> {
    let abs = rr3_profile_abs(path)?;
    let text = std::fs::read_to_string(&abs)
        .map_err(|e| format!("读不到装配单 {}: {e}", abs.display()))?;
    let profile = rr3_profile_parse(&text).map_err(|errs| {
        errs.iter()
            .map(|e| format!("装配单 {} 形状不合: {e}", abs.display()))
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    Ok((abs, profile))
}

/// 格位契约装载 + 空表/形状门（查册真值 = rr-registry 那一册，与 CI 全集层同源）
fn rr3_load_slots(path: &Path) -> Result<Rr3SlotTable, String> {
    let (abs, text) = rr3_boot_text(path, "格位契约")?;
    let table: Rr3SlotTable = rr3_slot_parse(&text);
    rr3_slots_gate(&abs, &table)?;
    Ok(table)
}

/// 格位契约的空表门与形状门（与 topics 装载同构：CI 全集层与运行期门两轴缺一即带病）。
fn rr3_slots_gate(abs: &Path, table: &Rr3SlotTable) -> Result<(), String> {
    if table.rr3_slot_is_empty() {
        return Err(format!(
            "格位契约 {} 解析后为空，拒绝启动（空格位表是契约事故）",
            abs.display()
        ));
    }
    rr3_slots_shape(abs, table)
}

/// 格位契约的形状门（空表门已在调用方）。
fn rr3_slots_shape(abs: &Path, table: &Rr3SlotTable) -> Result<(), String> {
    let errs = table.rr3_slot_check();
    if errs.is_empty() {
        return Ok(());
    }
    Err(format!(
        "格位契约 {} 形状不合，拒绝启动: {}",
        abs.display(),
        errs.join("; ")
    ))
}

/// 角色契约装载 + 空表/形状门（与格位契约同一 fail-closed 口径）
fn rr3_load_roles(path: &Path) -> Result<Rr3RoleTable, String> {
    let (abs, text) = rr3_boot_text(path, "角色契约")?;
    let roles: Rr3RoleTable = rr3_role_parse(&text);
    rr3_roles_gate(&abs, &roles)?;
    Ok(roles)
}

/// 角色契约的空表门与形状门（与格位契约同一 fail-closed 口径）。
fn rr3_roles_gate(abs: &Path, roles: &Rr3RoleTable) -> Result<(), String> {
    if roles.rr3_role_is_empty() {
        return Err(format!(
            "角色契约 {} 解析后为空，拒绝启动（空角色表是契约事故）",
            abs.display()
        ));
    }
    rr3_roles_shape(abs, roles)
}

/// 角色契约的形状门（空表门已在调用方）。
fn rr3_roles_shape(abs: &Path, roles: &Rr3RoleTable) -> Result<(), String> {
    let errs = roles.rr3_role_check();
    if errs.is_empty() {
        return Ok(());
    }
    Err(format!(
        "角色契约 {} 形状不合，拒绝启动: {}",
        abs.display(),
        errs.join("; ")
    ))
}

/// 器件词表装载 + 空表/形状门（装载期只查 mate 外键闭合，不认型号能力，D10a）
fn rr3_load_devices(path: &Path) -> Result<Rr3DeviceTable, String> {
    let (abs, text) = rr3_boot_text(path, "器件词表")?;
    let devices: Rr3DeviceTable = rr3_dev_parse(&text);
    rr3_devs_gate(&abs, &devices)?;
    Ok(devices)
}

/// 契约册装载前两步：绝对化 + 读文本（两处失败同一句"读不到{what}"）。
fn rr3_boot_text(path: &Path, what: &str) -> Result<(PathBuf, String), String> {
    let abs =
        std::fs::canonicalize(path).map_err(|e| format!("读不到{what} {}: {e}", path.display()))?;
    let text = std::fs::read_to_string(&abs)
        .map_err(|e| format!("读不到{what} {}: {e}", abs.display()))?;
    Ok((abs, text))
}

/// 器件词表的空表门与形状门（与格位/角色同一 fail-closed 口径）。
fn rr3_devs_gate(abs: &Path, devices: &Rr3DeviceTable) -> Result<(), String> {
    if devices.rr3_dev_is_empty() {
        return Err(format!(
            "器件词表 {} 解析后为空，拒绝启动（空器件表是契约事故）",
            abs.display()
        ));
    }
    rr3_devs_shape(abs, devices)
}

/// 器件词表的形状门（空表门已在调用方）。
fn rr3_devs_shape(abs: &Path, devices: &Rr3DeviceTable) -> Result<(), String> {
    let errs = devices.rr3_dev_check();
    if errs.is_empty() {
        return Ok(());
    }
    Err(format!(
        "器件词表 {} 形状不合，拒绝启动: {}",
        abs.display(),
        errs.join("; ")
    ))
}

/// 接线档装载（装机档必在：读不到/形状不合都拒启动；返回绝对路径供轴目录推导）
fn rr3_load_wiring(path: &Path) -> Result<(PathBuf, crate::wiring::Rr3Wiring), String> {
    let (abs, text) = rr3_boot_text(path, "接线档")?;
    let wiring = crate::wiring::rr3_wiring_from_text(&text).map_err(|errs| {
        format!(
            "接线档 {} 形状不合，拒绝启动: {}",
            abs.display(),
            errs.join("; ")
        )
    })?;
    Ok((abs, wiring))
}

/// 平台事实表装载：按 [platform].chip 选册，读不到/形状坏逐条点名拒 boot（不降级）
fn rr3_load_platform(dir: &Path, chip: &str) -> Result<Rr3PlatformTable, String> {
    let path = dir.join(rr3_platform_file(chip));
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "读不到平台事实表 {}（[platform].chip = {chip} 的选册）: {e}",
            path.display()
        )
    })?;
    rr3_plat_parse(&text).map_err(|errs| {
        format!(
            "平台事实表 {} 形状不合，拒绝启动: {}",
            path.display(),
            errs.join("; ")
        )
    })
}
