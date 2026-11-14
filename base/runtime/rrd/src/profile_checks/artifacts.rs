//! 装载产物：manifest 路径、hal_bindings 注入文本与执行轴档案装载投影。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::profile::{rr3_axis_files_load, rr3_axis_state_text, rr3_bindings_text, Rr3Profile};

/// manifest 路径（模块名升序：BTreeMap 键序，确定性）
pub(crate) fn rr3_manifests(profile: &Rr3Profile, root: &Path) -> Vec<PathBuf> {
    profile
        .enabled_slots
        .keys()
        .map(|m| root.join("modules").join(m).join("module.toml"))
        .collect()
}

/// hal_bindings 注入文本：host 档派生视图 ∪ [hardware] 非引脚键（门 n 已保证
/// 物理键不双占，合并无覆盖歧义）；rtos/bare 档 derived 为空，[hardware] 原样。
pub(crate) fn rr3_hal_bindings(
    derived: BTreeMap<String, String>,
    hardware: &BTreeMap<String, String>,
) -> String {
    let mut merged = derived;
    for (k, v) in hardware {
        merged.insert(k.clone(), v.clone());
    }
    rr3_bindings_text(&merged)
}

/// 执行轴装载与投影：目录 = 接线档同级 axis/，pin_ids = [[pin]] 全集；
/// 装载一次，清单同时进装配快照（面板出图要看见全部轴），投影只吃这份清单。
pub(crate) fn rr3_axis_load(
    wiring_abs: &Path,
    pin_ids: &BTreeSet<String>,
) -> (Vec<crate::axis::Rr3AxisFile>, String, Vec<String>) {
    let axis_dir = wiring_abs.parent().expect("接线档必有父目录").join("axis");
    let axis_wanted = std::env::var(rr_hal_api::RR3_PERIPH_AXIS_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| rr_hal_api::RR3_PERIPH_AXIS_DEFAULT.to_string());
    let (axis_files, axis_notes) = rr3_axis_files_load(&axis_dir, pin_ids);
    let (axis_state, axis_notes_project) = rr3_axis_state_text(&axis_files, &axis_wanted);
    let mut axis_notes = axis_notes;
    axis_notes.extend(axis_notes_project);
    (axis_files, axis_state, axis_notes)
}
