// SPDX-License-Identifier: Apache-2.0
//! 装配单装载（ADR-0012 裁定 3/5②）：boot 之前消费一次，运行期不重排。
//!
//! `profiles/<机型>.toml` 是纯数据，八段：`[os]`（运行期不读）+ `[platform]`
//! （tier 三档 host/rtos/bare）+ `[supplies]`（看门狗三键 + `[supplies.budget]`
//! 内存预算）+ `[slots]`（present / device_absent）+ `[bindings]`（格位→件）+
//! `[enabled_slots]`（模块→启用的格位）+ `[hardware]`（非引脚键）+
//! `[[pin]]`（**引脚侧唯一真值点**，role 外键指向 `contracts/roles.toml`）。
//!
//! 本模块只做装载期的事，fail-closed：形状不合逐条点名，一条不静默放行。
//! 1. 解析文本；`[platform]`/`[supplies]`/`[slots]` 三段必填。
//! 2. 查册门：bindings / enabled_slots / `[slots].present` 的每个格位必须命中
//!    [`contracts/slots.toml`](../../../contracts/slots.toml)；present 另拒基础层
//!    （基础层恒在位，进 present 自相矛盾）。查册真值是 `rr-registry::Rr3SlotTable`，
//!    与 CI 全集层共用，不写第二套。
//! 3. 缺格三源合一：present 声明缺、enabled_slots 无 binding 指向（注入缺）、
//!    探测不到（实测缺）→ 由 daemon 点名 `拒绝启动`；provides 仍计可满足性宇宙
//!    （下游不连坐，ADR-0009 同款）。
//! 4. 档位一致：tier=bare 而 `watchdog_present=false` 自相矛盾 → 拒 boot。
//! 5. 装载期校验①②：`[[pin]]` 的 role 查册、per_instance/instance 规则、
//!    requires 留空即红；缺角色后果是"降"（`role_missing` 随 boot 带出）。
//! 6. 门 n：`[hardware]` 出现物理键即红；与 `[pins]` 并存时点名两份取值。
//! 7. 模块名解析成 manifest 路径（`<modules-root>/modules/<名>/module.toml`），
//!    装机布局由打包步骤保证。
//! 8. 器件词表与接线档装载（`devices.toml` + `hardware/<形态>/wiring.toml`），
//!    路径按装配单推导；形状与闭合面在 [`crate::wiring`] 裁。
//! 9. 校验⑨⑩：电流预算与覆盖（门 c）+ 内存预算（[`rr3_memory_budget`]，
//!    分子件级直加、分母 `[supplies.budget]`），超限即拒 boot。
//!
//! 基础层格位（`net.online` / `power.battery`）恒在位：不进 bindings、不参与缺格
//! 判定，enabled_slots 声明它们只是记账。器件缺席（如没装电池）走
//! `[slots].device_absent` 登记格名 → 非空理由，与 present 语义正交。
//! `[os].route` 解析时无视。

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
