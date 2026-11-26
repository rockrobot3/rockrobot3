// SPDX-License-Identifier: Apache-2.0
//! 装配单装载（ADR-0012）：boot 前消费一次，运行期不重排，fail-closed 逐条点名。
//!
//! `profiles/<机型>.toml` 八段：`[os]`（运行期不读，route 无视）/ `[platform]`
//! （host·rtos·bare）/ `[supplies]`（看门狗三键 + 内存预算）/ `[slots]`
//! （present·device_absent）/ `[bindings]` / `[enabled_slots]` / `[hardware]` /
//! `[[pin]]`（引脚唯一真值点，role 指向 `contracts/roles.toml`）。
//!
//! 装载期九查：三段必填；格位查册（真值 `rr-registry::Rr3SlotTable`，present 拒基础层）；
//! 缺格三源合一（声明缺/注入缺/实测缺，只点名不连坐下游）；tier=bare 必须
//! `watchdog_present`；`[[pin]]` role/实例/requires；`[hardware]` 禁物理键；
//! 模块名→manifest 路径；器件与接线档装载；电流（门 c）与内存预算，超限拒 boot。
//!
//! 基础层格位（`net.online` / `power.battery`）恒在位，不进 bindings、不参与缺格；
//! `device_absent` 只登记缺席格名与理由，与 present 正交。

mod boot;
mod model;
mod parse;

pub(crate) use boot::{
    rr3_assemble_scan, rr3_axis_files_load, rr3_axis_state_text, rr3_bindings_text,
    rr3_demand_budget, rr3_demand_expand, rr3_memory_budget, rr3_power_gaps,
};
pub use boot::{
    rr3_boot_inputs, rr3_boot_with_probe, rr3_domain_apply, rr3_evidence_check, rr3_key_line_apply,
    rr3_truth_locations, rr3_truth_set, Rr3RuntimeTruthSet, Rr3TruthSetLocation,
    RR3_CONTRACT_BOOKS,
};
#[cfg(test)]
pub(crate) use model::RR3_DOMAIN_KEYS;
pub use model::{
    Rr3Budget, Rr3Domain, Rr3PinEntry, Rr3Profile, Rr3ProfileBoot, Rr3Supplies,
    RR3_CONTRACT_DEVICES_PATH, RR3_CONTRACT_ROLES_PATH, RR3_CONTRACT_SLOTS_PATH,
};
pub(crate) use model::{RR3_PIN_DERIVED_KEYS, RR3_PIN_KEYS};
pub use parse::{rr3_profile_abs, rr3_profile_parse};

#[cfg(test)]
mod tests;
