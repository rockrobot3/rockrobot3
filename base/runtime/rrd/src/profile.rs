//! 装配单装载（ADR-0012 裁定 3/5②，0.0.2 方案 D2 / Q3 裁定）。
//!
//! `profiles/<机型>.toml` 是**纯数据**装配单，schema_version=3 起按增补轮六 §4.2
//! 定形为八段：`[os]`（运行期不读）+ `[platform]`（tier 三档 host/rtos/bare）+
//! `[supplies]`（看门狗三键 + 时序预算位三键，修订 4 ② / 修订 2 ③）+ `[slots]`
//! （present = 本板物理在位集）+ `[bindings]` 格位→件、`[enabled_slots]` 模块→本形态
//! 启用的格位集合、`[hardware]` 非引脚键（`alsa_device`/`link_ifname` 两键直给；
//! `spi_device` 批N3 件六起同进派生轨——敞口 18 行收口）、`[[pin]]` 物理绑定（增补轮六
//! §4.2：**引脚侧唯一真值点**——八物理键
//! `pwm_chip`/`pwm_channel`/`gpio_chip`/`gpio_led_line`/`gpio_relay_line`/`i2c_bus`/
//! `ina219_addr`/`spi_device` 自 [hardware] 下移，role 外键 → `contracts/roles.toml`，host 档由
//! 派生器合成注入文本，rtos/bare 档板侧消费归 0.0.7 批G/批D1）。
//! schema_version=4 起新增 `[supplies.budget]` 内存预算子段（批N2 n2c-v10，校验⑩
//! 分母——ADR-0021 修订 3：栈/堆预算不住 platform 表，住 [supplies].budget，
//! 三份数据三个主人）。
//! 本模块只做这些事，全部发生在 boot 之前（裁定 4：boot 前消费一次，
//! 运行期不重排，F⊗F 非法）：
//! 1. 解析装配单文本（fail-closed：形状不合逐条点名，一条不静默放行）——
//!    schema_version=2 起 [platform]/[supplies]/[slots] 三段必填；
//! 2. 查册门（裁定 5②）：bindings/enabled_slots/[slots].present 引用的每个格位必须命中
//!    [`contracts/slots.toml`](../../../contracts/slots.toml)（present 另拒基础层格位——
//!    基础层恒在位，进 present 属自相矛盾）——查册真值是
//!    `rr-registry::Rr3SlotTable`，与 CI 全集层共用同一份，不写第二套；
//! 3. 缺格判定（三源合一，方案 §15）：[slots].present 声明缺（第一源）、enabled_slots
//!    声明的格位若非基础层、又没有 binding 指向本模块（注入缺，j5）、或探测不到
//!    （实测缺，见 [`crate::probe`]）→ (模块, 格位) 进缺格清单，由 daemon 点名 `拒绝启动`——
//!    绝不带病起，provides 仍留在可满足性宇宙（下游不连坐，ADR-0009 同款语义）；
//! 4. 档位一致性（ADR-0018 修订 1：无看门狗的板不允许归 bare 档）：tier=bare 而
//!    [supplies].watchdog_present=false 的装配单自相矛盾 → 拒 boot；
//! 5. 装载期校验①②（增补轮六 §5）：`[[pin]]` 的 role 外键查册（roles.toml）、
//!    per_instance/instance 规则、requires 字段留空即红、所需角色齐（②的后果是
//!    "降"——`role_missing` 点名清单随 boot 带出，Degraded 可见面归批N4 I-15）；
//! 6. 门 n 注入路径唯一（§8）：[hardware] 出现物理键即红（稳态判据），[pins] 与
//!    [hardware] 并存时错误信息点名两份取值；
//! 7. 把 enabled_slots 模块名解析成 manifest 路径：`<modules-root>/profiles/<机>.toml` 的上两级即
//!    modules-root，manifest 在 `<modules-root>/modules/<名>/module.toml`——装机布局由
//!    打包步骤保证（modules/README.md 装机约定 + 0.0.2 T5 deb 打包同一布局）；
//! 8. 器件词表与接线档装载（增补轮六 §4.1b/§4.3）：`contracts/devices.toml` 的
//!    mate 集 + `hardware/<形态>/wiring.toml`（路径由 [`crate::wiring::
//!    rr3_wiring_path`] 按装配单推导）——校验⑦-a/b/d 的跨档判据在
//!    boot 链合龙（⑦-c 待件 demands 数据面，批N2），接线档形状与闭合面在
//!    [`crate::wiring`] 裁；
//! 9. 装载期校验⑨⑩（批N2 本体）：装配扫描一次两用——⑨ 电流预算与覆盖判据
//!    （门 c）+ ⑩ 内存预算判据（[`rr3_memory_budget`]，分子 = [assemble]
//!    内存申报件级直加，分母 = [supplies.budget]），超限即拒 boot（§5 后果 = 拒）。
//!
//! 基础层格位（`net.online` / `power.battery`）**恒在位**（slots.toml 2026-10-03 注记）：
//! 不进 bindings、不进缺格判定，enabled_slots 里声明它们只是"本形态用到"的记账。
//! base 层格的**器件缺席**（板上根本没装电池这类）是另一件事：D3 裁定（decisions.md
//! 裁定 5）给 `power.battery` 加"无器件"合法态——`[slots].device_absent` 登记
//! 格名 → 非空理由，校验⑥认它；`present` 仍是本板物理在位格位声明，与
//! `device_absent` 语义正交（base 格不许进 present，缺席表达只有这一条语法）。
//! `[os].route` 运行期不读（ADR-0012 裁定 3 B3 注记），解析时无视该段。

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
