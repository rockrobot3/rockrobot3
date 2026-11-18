//! 装配清单（`atoms/<名>/assemble.toml`，五件套④）的形状契约与注入缺校验
//! （ADR-0019 修订 1 ③：L3 注入面从口号变成被校验的数据，0.0.5 批J G4）。
//!
//! 定形（与方案 §14 G4 行同文）：`[assemble.handles]` 声明件用哪个能力面 trait
//! ← 绑描述符哪个键、`[assemble.ops]` 声明件暴露哪些参数槽 ← 取哪个 config 键。
//! 数据形状（schema_version = 2）：
//!
//! ```toml
//! schema_version = 2
//!
//! [assemble]
//! handles = [
//!   { trait = "Rr3Power", desc_key = "i2c_bus" },
//! ]
//! ops = [
//!   { slot = "low_voltage_mv", src_key = "rr3-power.battery.low_voltage_mv" },
//! ]
//! ```
//!
//! 两级真值、两处消费（`rrd` 与裸机装配段共用本模块，不各说一套）：
//! - **形状级**（[`rr3_assemble_parse`]）：schema 版本、未知字段点名、
//!   trait ∈ 九能力面在册表、desc_key ∈ 九描述符键在册表、参数槽与来源键非空点路径。
//!   空数组**合法**，但**两数组均空时须明写 `no_injection = true`**（0.0.5 批M MC-10 L-5：
//!   把"漏填"与"本件确无注入面"分开——否则空申报与漏填不可区分，门里无内容可校验）。
//! - **注入缺级**（[`rr3_injection_gaps`]）：ops 的 `src_key`
//!   解析域 = 模块 `config.toml`（点路径游走），取不到 = 注入缺。handles 的
//!   desc_key 存在性**不在本模块判**——描述符注入是 spawn 时 `RR3_HAL_BINDINGS`
//!   env 的事，存在性归探测面（实测缺），与 rr-hal 后端选择的 fail-closed 同一路。
//!
//! **电流需求申报面**（`[assemble].demands`，批N2 首件基-36 / 裁定 13"只交一件"，
//! 门 c 补齐五键定形＝ADR-0021 裁定 2）：每条
//! `{ rail, peak_ma, sustained_ma, critical, concurrency_group }`——件在 `rail` 域上的
//! 电流需求（mA）。**每实例语义**：条目是"该件服务的每个角色实例"的需求，装载读者
//! （rrd `profile::rr3_demand_budget`）按角色实例数扩算后再加总比对域 cap
//! （校验⑨，全案 §5），本模块只管形状：rail 非空、peak_ma/sustained_ma 正整数且
//! sustained ≤ peak、critical 显式布尔（不给缺省）、concurrency_group 必须显式写
//! （T-5：显式空数组 = 不分组 = 全体并发最坏口径，合法；缺键即红——同 no_injection
//! 范式，缺省与漏填不可区分）。**缺 `demands` 键 = 空申报合法**——执法面"件未申报
//! 而角色 kind=power → 红"已随门 c（批N2 本体 ⑨-c）落 rrd 装载拒链，不在形状层预支。
//! rail 域在册性也不在此判（rr-registry 无 domains 数据，读者对 profile 比对）。
//! 增键不 bump：demands 缺省语义 = 未申报，已激活 17 份数据语义不变（composite.rs
//! 头注"只增不 bump"同构）。
//!
//! **内存需求申报面**（`[assemble].task_stack_bytes` / `[assemble].heap_peak_bytes`，
//! 校验⑩ 分子面，批N2 n2c-v10 = 全案 §5 判据⑩ / F-13）：件级语义**非每实例**——
//! 件全部实例共享一个任务栈、一条堆账（如双轴共享同一任务栈），装载读者
//! （rrd `rr3_memory_budget`）直加不扩算，与 demands 的每实例扩算语义相反。
//! 缺省 0 = 未申报不进 Σ：申报义务未触发（板侧任务映射未建模，归 0.0.7 批G/批D1），
//! 本批立判据实体与装载拒链，红例用合成申报；给键必须是非负整数（负数 = 物理矛盾）。
//! 两键同样**件级专用**：组合件级 [assemble] 不认，未知键点名即拦（同 demands）。
//! 预算分母不住这里——住 [supplies].budget（rrd 域，ADR-0021 修订 3：三份数据
//! 三个主人），本模块只收分子形状。
//!
//! 装配段对缺项的处置（ADR-0019 修订 1 ③）：**该格 `Degraded`、点名到件，不 fatal**
//! ——件没病，装配声明坏了重启治不了；修数据 + 重启 rrd 是唯一解除路。
//!
//! 在册表与真值源的关系：九 trait 真值源 = `rr-hal-api`（能力面契约），九键真值源 =
//! `rr-hal` `descriptor.rs`（`[hardware]` 键集，C4 裁定**不扩键**）。两表在此按
//! 契约层纪律另立一份并由契约测试钉死——扩 trait/键都走契约级评审，届时两处同批；
//! 本表滞后 = 新 trait/键被装配校验拒绝（fail-closed 方向，不会带病放行）。

use toml::Value;

/// 装配清单 schema 版本（本文件定形即 2：handles/ops 从字符串数组升为结构化条目）
pub const RR3_ASSEMBLE_SCHEMA_VERSION: u32 = 2;

/// 九能力面 trait 在册表（真值源 rr-hal-api；handles[].trait 只认这九个名字。
/// 批N3 件八起 `Rr3Sensor`/`Rr3DigitalIn` 进表——契约面先行无实现件，件一
/// `Rr3Actuator` 先例；roles.toml 感知角色 iface 外键据此查册）
pub const RR3_ASSEMBLE_HANDLE_TRAITS: &[&str] = &[
    "Rr3GpioOut",
    "Rr3PwmServo",
    "Rr3AudioSource",
    "Rr3AudioSink",
    "Rr3Power",
    "Rr3Link",
    "Rr3Display",
    "Rr3Sensor",
    "Rr3DigitalIn",
];

/// 描述符键在册表（真值源 rr-hal descriptor.rs `[hardware]` 键集；C4 批J 裁定
/// 不扩键，0.0.6 链④真件档按裁定流程扩 `spi_device`——两处同批；批N4 件七(b)
/// 同款裁定扩板侧配置三键 `i2c_sda`/`i2c_scl`/`adc_rail_pin`——板侧裸寄存器自愈
/// 路径引脚真值，无 role 对应故不在 [pins] 派生键集，rtos 档 [hardware] 直给；
/// 批N17 件三同款裁定扩 `cred_store`——凭据存储源标记 `nvs:<namespace>` 非 GPIO
/// （N-10 ② 键半边契约级扩批），消费语义＝板侧读数面可见，boot 读链不消费＝
/// 批N14 架构事实，无装配对账语义＝非引脚不进对账锚）
pub const RR3_ASSEMBLE_DESC_KEYS: &[&str] = &[
    "alsa_device",
    "pwm_chip",
    "pwm_channel",
    "gpio_chip",
    "gpio_led_line",
    "gpio_relay_line",
    "i2c_bus",
    "ina219_addr",
    "link_ifname",
    "spi_device",
    // P0-3/P0-4：ESP32 外设桥 UART 设备路径（组合档宿主侧真值，描述符 [hardware].periph_uart）
    "periph_uart",
    "i2c_sda",
    "i2c_scl",
    "adc_rail_pin",
    // 批N9 件五（MC-7 缺③ [hardware] 进位补齐）：板侧主驱动七键（板侧外设引脚
    // 真值，消费语义 = 主驱动装配对账锚，无 role 对应故不在 [pins] 派生键集）
    "uart2_tx",
    "uart2_rx",
    "ledc_pan",
    "ledc_tilt",
    "i2s_bclk",
    "i2s_sd",
    "i2s_ws",
    // 批N17 件三（N-10 ② 键半边）：凭据存储源标记（非 GPIO，板侧读数面可见）
    "cred_store",
    // 批B62（0.0.8 无硬件窗，同款契约级扩批）：INA219 分流校准两键——分流电阻
    // （毫欧，装配事实）与电流 LSB（微安/位，量程策略）。两键齐备才写 0x05 校准；
    // 不在 [pins] 派生键集（无 role 对应），[hardware] 直给。
    "ina219_shunt_mohm",
    "ina219_ilsb_ua",
];

/// 一条句柄声明：件用 `trait_name` 能力面，绑描述符 `desc_key` 键
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3AssembleHandle {
    pub trait_name: String,
    pub desc_key: String,
}

/// 一条参数槽声明：件暴露 `slot` 槽，取值来自 config 的 `src_key` 点路径
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3AssembleOp {
    pub slot: String,
    pub src_key: String,
}

/// 一条电流需求申报：件在 `rail` 域上的电流需求（批N2 首件基-36 立三键，门 c 补齐
/// 五键定形＝ADR-0021 裁定 2 `[[atom.demand]]` 形状；判据⑨ 的申报分子，加总与域
/// cap 比对在 rrd 读者侧）。**每实例语义**：条目是"该件服务的每个角色实例"的需求，
/// 装载读者按角色实例数扩算（pin 数 × peak_ma），件不预乘。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3AssembleDemand {
    /// 电力网络域名（对 [[supplies.domain]].name 比对，在册性归 rrd 读者）
    pub rail: String,
    /// 峰值电流 mA（正整数；critical=true 时进校验⑨ 预算加总；堵转/满亮等最坏瞬时）
    pub peak_ma: i64,
    /// 连续电流 mA（正整数且 ≤ peak_ma——连续分量超过峰值是物理矛盾，fail-closed）
    pub sustained_ma: i64,
    /// 是否关键需求（显式布尔不给缺省；false 不进 Σ）
    pub critical: bool,
    /// 并发组名集（T-5）：同组条目互斥（至多一个同时取流），收窄校验⑨ 的"全部同时"
    /// 最坏缺省——不重叠必须是数据，不许运行期推断（F-12）。**必须显式写**（同
    /// no_injection 范式）：缺键 = 与"漏填"不可区分，一律红；显式空数组 = 不分组 =
    /// 全体并发（最坏口径）合法。多组归属的条目在读者侧按不收窄计（宁超算不漏算）。
    pub concurrency_group: Vec<String>,
}

/// 一份装配清单的解析产物（空数组合法，但空面须明写 no_injection，见下）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rr3AssembleFacts {
    pub handles: Vec<Rr3AssembleHandle>,
    pub ops: Vec<Rr3AssembleOp>,
    /// 电流需求申报（基-36 首件立三键、门 c 补齐五键；缺 `demands` 键 = 空申报合法
    /// ——"kind=power 未申报 → 红"的执法面在 rrd 装载拒链 ⑨-c）
    pub demands: Vec<Rr3AssembleDemand>,
    /// `no_injection = true`：件明写"本件不需注入面"（0.0.5 批M MC-10 L-5）。
    /// `handles`/`ops` 两数组**均空**时此键必填——否则与"漏填"不可区分，一律判红。
    pub no_injection: bool,
    /// 内存需求申报（校验⑩ 分子面，批N2 n2c-v10）：本件任务栈占用字节数。
    /// **件级语义非每实例**（与 demands 的每实例语义相反）：件全部实例共享一个
    /// 任务栈（如双轴共享同一任务栈），装载读者 [`crate::assemble`] 消费方
    /// （rrd `rr3_memory_budget`）直加不扩算。缺省 0 = 未申报不进 Σ
    /// （申报义务未触发——板侧任务映射未建模，归 0.0.7 批G/批D1；立判据实体
    /// 与执法链在先，数据申报随后）。
    pub task_stack_bytes: i64,
    /// 本件堆峰值占用字节数（同上件级语义；F-13：渲染缓冲/队列等全是堆账，
    /// 只加总栈会击穿半边——⑩-b 的分子）。
    pub heap_peak_bytes: i64,
}

mod asm;
mod empty;
mod item;
#[cfg(test)]
mod tests;

use asm::*;
pub(crate) use empty::rr3_no_injection_err;
pub(crate) use item::{rr3_demands_parse, rr3_handles_parse, rr3_ops_parse};

/// 解析 + 形状级校验一份装配清单文本。fail-closed：一条不静默放行，全部错误
/// 逐条点名（与 manifest/描述符校验同一口径）。空 `[assemble]` 数组合法。
pub fn rr3_assemble_parse(text: &str) -> Result<Rr3AssembleFacts, Vec<String>> {
    let root: Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("装配清单 TOML 解析失败: {e}")]),
    };
    let Some(t) = root.as_table() else {
        return Err(vec![format!("装配清单顶层必须是 TOML 表，收到 {root:?}")]);
    };

    let mut errs: Vec<String> = Vec::new();
    rr3_asm_head(t, &mut errs);

    let Some(asm) = t.get("assemble").and_then(Value::as_table) else {
        return rr3_asm_no_table(errs);
    };
    rr3_asm_keys(asm, &mut errs);

    let mut facts = Rr3AssembleFacts::default();
    rr3_asm_noinj(asm, &mut facts, &mut errs);
    let (handles_present, handles_empty) = rr3_asm_handles(asm, &mut facts, &mut errs);
    let (ops_present, ops_empty) = rr3_asm_ops(asm, &mut facts, &mut errs);
    rr3_asm_demands(asm, &mut facts, &mut errs);
    rr3_asm_mem(asm, &mut facts, &mut errs);

    // 空面规则（L-5）：两数组均在文本中出现且均为空 ⇒ 必须明写 no_injection = true；
    // 反之，明写 no_injection 时两数组必须都空。这条把"漏填"与"本件确无注入面"分开——
    // 光看空数组分不清，于是此前 14 份空申报在门里无内容可校验（方案 §12.4 C3）。
    errs.extend(rr3_no_injection_err(
        handles_present,
        handles_empty,
        ops_present,
        ops_empty,
        facts.no_injection,
    ));

    rr3_asm_finish(facts, errs)
}

/// 终局：错误集空交表，否则整册拒（与旧内联分支逐字同形）。
fn rr3_asm_finish(
    facts: Rr3AssembleFacts,
    errs: Vec<String>,
) -> Result<Rr3AssembleFacts, Vec<String>> {
    if errs.is_empty() {
        Ok(facts)
    } else {
        Err(errs)
    }
}

/// 缺 `[assemble]` 段的终局：点名后按错误集判（与旧内联分支逐字同形）。
fn rr3_asm_no_table(mut errs: Vec<String>) -> Result<Rr3AssembleFacts, Vec<String>> {
    errs.push("装配清单缺 [assemble] 表（handles / ops 两键都必填，空数组合法）".into());
    rr3_asm_finish(Rr3AssembleFacts::default(), errs)
}

/// 注入缺校验：ops 的每条 `src_key` 必须能在 `config_text`（模块 config.toml）上
/// 按点路径取到值；取不到 = 注入缺，逐条点名（参数槽 + 来源键）。config 文本
/// 解析失败 = 全部来源键不可证，逐条点同名缺口。handles 不在此判（见模块头注）。
/// 返回空 = 无缺口。
pub fn rr3_injection_gaps(facts: &Rr3AssembleFacts, config_text: &str) -> Vec<String> {
    let cfg: Value = match toml::from_str(config_text) {
        Ok(v) => v,
        Err(e) => return rr3_gaps_unparsable(facts, &e),
    };
    facts
        .ops
        .iter()
        .filter(|o| rr3_path_get(&cfg, &o.src_key).is_none())
        .map(|o| {
            format!(
                "参数槽 {} 的来源键 {} 在 config 中取不到（注入缺）",
                o.slot, o.src_key
            )
        })
        .collect()
}

/// 点路径游走：任一段取不到即 `None`（注入缺级的判定本体）。
fn rr3_path_get<'a>(cfg: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.').try_fold(cfg, |cur, seg| cur.get(seg))
}

/// config 文本解析失败 = 全部来源键不可证，逐条点同名缺口（文案同旧内联分支）。
fn rr3_gaps_unparsable(facts: &Rr3AssembleFacts, e: &toml::de::Error) -> Vec<String> {
    facts
        .ops
        .iter()
        .map(|o| {
            format!(
                "参数槽 {} 的来源键 {} 不可证：config 文本解析失败 ({e})",
                o.slot, o.src_key
            )
        })
        .collect()
}
