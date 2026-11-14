//! 装载期校验族（校验①②⑤⑥⑦⑨⑩ 判据 + 装配扫描 + 缺格探针 + 轴档案装载）。
//!
//! 自 `profile.rs` 拆出（批N45 第三刀）：本模块只持**判定与探针**，输入全部是
//! 已装载的装配单实体（[`Rr3Profile`]）与契约表（格位/角色/平台）——除
//! [`rr3_scan_modules`]（modules-root 扫盘）与 [`rr3_axis_load`]（轴档案读盘）
//! 外不碰文件系统。编排住 [`crate::profile::rr3_boot_with_probe`]（校验次序
//! 与错误聚合在那边，本模块不排总）；引脚能力/占用面住 [`crate::profile_pins`]。
//! 校验项与错误文案逐字保持原样（门禁与文档按文案点名消费，改文案即改契约）。

mod artifacts;
mod budgets;
mod derive;
mod domains;
mod pins;
mod slots;

pub(crate) use artifacts::{rr3_axis_load, rr3_hal_bindings, rr3_manifests};
pub(crate) use budgets::{rr3_check_budgets, rr3_role_gaps, rr3_scan_modules, rr3_slot_gaps};
pub(crate) use domains::{rr3_check_domains, rr3_check_hw_keys, rr3_check_rails};
pub(crate) use pins::rr3_check_pins;
pub(crate) use slots::{rr3_check_claims, rr3_check_slot_refs, rr3_check_tier};
