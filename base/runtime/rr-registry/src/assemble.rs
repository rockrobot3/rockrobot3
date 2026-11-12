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

/// 点路径合法性：按 `.` 切分后每段非空（空串/首尾点/连续点都不认）
fn rr3_dotted_key_ok(s: &str) -> bool {
    !s.is_empty() && s.split('.').all(|seg| !seg.is_empty())
}

/// 内联表条目骨架（单点）：非表即拒（点名期望形状）、未知键逐条点名（只认名单内
/// 的键）。两条拒因都进 `errs`（收集式校验：一次报全，不遇错即返）。
fn rr3_item_tbl<'a>(
    item: &'a Value,
    what: &str,
    i: usize,
    shape: &str,
    keys: &[&str],
    errs: &mut Vec<String>,
) -> Option<&'a Value> {
    let Some(t) = item.as_table() else {
        errs.push(format!("{what}[{i}] 应为内联表 {shape}"));
        return None;
    };
    for k in t.keys() {
        if !keys.contains(&k.as_str()) {
            errs.push(format!(
                "{what}[{i}] 未知键 {k}（只认 {}）",
                keys.join(" / ")
            ));
        }
    }
    Some(item)
}

/// 必填字符串键（单点）：缺键/非串与"串但不合判据"两类拒因分述，判据不满足时把
/// 值原样点名（`bad` 承载判据说明）。返 None 表示该键已拒，条目不入列。
fn rr3_item_str<'a>(
    t: &'a Value,
    what: &str,
    i: usize,
    key: &str,
    bad: &str,
    ok: impl Fn(&str) -> bool,
    errs: &mut Vec<String>,
) -> Option<&'a str> {
    match t.get(key).and_then(Value::as_str) {
        Some(v) if ok(v) => Some(v),
        Some(v) => {
            // 值原样点名（空串不回声，避免 `slot "" 不得为空` 这类噪声）
            rr3_item_str_bad(what, i, key, v, bad, errs);
            None
        }
        None => {
            errs.push(format!("{what}[{i}].{key} 应为字符串"));
            None
        }
    }
}

/// 串键值不合判据时的点名（空串不回声值，其余原样点名）。
fn rr3_item_str_bad(what: &str, i: usize, key: &str, v: &str, bad: &str, errs: &mut Vec<String>) {
    if v.is_empty() {
        errs.push(format!("{what}[{i}].{key} {bad}"));
    } else {
        errs.push(format!("{what}[{i}].{key} \"{v}\" {bad}"));
    }
}

/// handles 数组条目级解析与校验（件级与组合件级**同形同规则**，composite.rs 共用；
/// 点名文案两处一致——一份判据一份真值）
pub(crate) fn rr3_handles_parse(items: &[Value]) -> (Vec<Rr3AssembleHandle>, Vec<String>) {
    let mut handles = Vec::new();
    let mut errs: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        rr3_handle_item(item, i, &mut handles, &mut errs);
    }
    (handles, errs)
}

/// 单条 handle：未知键 + trait/desc_key 双域在册（两域都合法才入列）。
fn rr3_handle_item(
    item: &Value,
    i: usize,
    handles: &mut Vec<Rr3AssembleHandle>,
    errs: &mut Vec<String>,
) {
    let Some(h) = rr3_item_tbl(
        item,
        "handles",
        i,
        "{ trait, desc_key }",
        &["trait", "desc_key"],
        errs,
    ) else {
        return;
    };
    let trait_name = rr3_item_str(
        h,
        "handles",
        i,
        "trait",
        "未在册（能力面表见 assemble.rs，扩面走契约级评审）",
        |s| RR3_ASSEMBLE_HANDLE_TRAITS.contains(&s),
        errs,
    );
    let desc_key = rr3_item_str(
        h,
        "handles",
        i,
        "desc_key",
        "未在册（键表见 assemble.rs / descriptor.rs，扩键走契约级评审）",
        |s| RR3_ASSEMBLE_DESC_KEYS.contains(&s),
        errs,
    );
    if let (Some(tr), Some(dk)) = (trait_name, desc_key) {
        handles.push(Rr3AssembleHandle {
            trait_name: tr.into(),
            desc_key: dk.into(),
        });
    }
}

/// ops 数组条目级解析与校验（同上：件级与组合件级共用）
pub(crate) fn rr3_ops_parse(items: &[Value]) -> (Vec<Rr3AssembleOp>, Vec<String>) {
    let mut ops = Vec::new();
    let mut errs: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        rr3_op_item(item, i, &mut ops, &mut errs);
    }
    (ops, errs)
}

/// 单条 op：未知键 + slot 非空 + src_key 点路径（两域都合法才入列）。
fn rr3_op_item(item: &Value, i: usize, ops: &mut Vec<Rr3AssembleOp>, errs: &mut Vec<String>) {
    let Some(o) = rr3_item_tbl(
        item,
        "ops",
        i,
        "{ slot, src_key }",
        &["slot", "src_key"],
        errs,
    ) else {
        return;
    };
    let slot = rr3_item_str(
        o,
        "ops",
        i,
        "slot",
        "不得为空",
        |s| !s.trim().is_empty(),
        errs,
    );
    let src_key = rr3_item_str(
        o,
        "ops",
        i,
        "src_key",
        "不是合法点路径（按 . 切分后每段须非空）",
        rr3_dotted_key_ok,
        errs,
    );
    if let (Some(s), Some(k)) = (slot, src_key) {
        ops.push(Rr3AssembleOp {
            slot: s.into(),
            src_key: k.into(),
        });
    }
}

/// demands 数组条目级解析与校验（**件级专用**：组合件级 [assemble] 不认 demands——
/// 电流需求是原子件的物理属性，不随组合件申报，组合件清单未知键点名即拦）。
/// 五键全部显式（T-5 / ADR-0021 裁定 2）：`concurrency_group` 缺键即红（同 no_injection
/// 范式——缺省与漏填不可区分），显式空数组 = 不分组 = 全体并发最坏口径，合法。
pub(crate) fn rr3_demands_parse(items: &[Value]) -> (Vec<Rr3AssembleDemand>, Vec<String>) {
    let mut demands = Vec::new();
    let mut errs: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        rr3_demand_item(item, i, &mut demands, &mut errs);
    }
    (demands, errs)
}

/// 单条 demand：骨架 → 五键 → 交叉判据 → 齐备入列（拒项不入列）。
fn rr3_demand_item(
    item: &Value,
    i: usize,
    demands: &mut Vec<Rr3AssembleDemand>,
    errs: &mut Vec<String>,
) {
    let Some(d) = rr3_item_tbl(
        item,
        "demands",
        i,
        "{ rail, peak_ma, sustained_ma, critical, concurrency_group }",
        &[
            "rail",
            "peak_ma",
            "sustained_ma",
            "critical",
            "concurrency_group",
        ],
        errs,
    ) else {
        return;
    };
    let rail = rr3_demand_rail(d, i, errs);
    let peak = rr3_demand_peak(d, i, errs);
    let sustained = rr3_demand_sust(d, i, errs);
    rr3_demand_cmp(peak, sustained, i, errs);
    let critical = rr3_demand_crit(d, i, errs);
    let groups = rr3_demand_grp(d, i, errs);
    if let (Some(rail), Some(peak_ma), Some(sustained_ma), Some(critical)) =
        (rail, peak, sustained, critical)
    {
        rr3_demand_push(demands, rail, peak_ma, sustained_ma, critical, groups);
    }
}

/// 五键齐备的需求入列（判据已在前置各步判完，此处只成型）。
fn rr3_demand_push(
    demands: &mut Vec<Rr3AssembleDemand>,
    rail: String,
    peak_ma: i64,
    sustained_ma: i64,
    critical: bool,
    concurrency_group: Vec<String>,
) {
    demands.push(Rr3AssembleDemand {
        rail,
        peak_ma,
        sustained_ma,
        critical,
        concurrency_group,
    });
}

/// 未知键点名（五键定形：拼错键不能被静默忽略）。
/// rail 键：非空字符串。
fn rr3_demand_rail(d: &Value, i: usize, errs: &mut Vec<String>) -> Option<String> {
    match d.get("rail").and_then(Value::as_str) {
        Some(r) if !r.trim().is_empty() => Some(r.to_string()),
        Some(_) => {
            errs.push(format!("demands[{i}].rail 不得为空"));
            None
        }
        None => {
            errs.push(format!("demands[{i}].rail 应为字符串"));
            None
        }
    }
}

/// peak_ma：正整数（每实例峰值电流 mA）。
fn rr3_demand_peak(d: &Value, i: usize, errs: &mut Vec<String>) -> Option<i64> {
    match d.get("peak_ma").and_then(Value::as_integer) {
        Some(v) if v > 0 => Some(v),
        Some(v) => {
            errs.push(format!(
                "demands[{i}].peak_ma = {v} 须为正整数（mA，每实例峰值电流需求）"
            ));
            None
        }
        None => {
            errs.push(format!("demands[{i}].peak_ma 应为整数"));
            None
        }
    }
}

/// sustained_ma：正整数，须 ≤ peak_ma（ADR-0021 裁定 2 五键定形）。
fn rr3_demand_sust(d: &Value, i: usize, errs: &mut Vec<String>) -> Option<i64> {
    match d.get("sustained_ma").and_then(Value::as_integer) {
        Some(v) if v > 0 => Some(v),
        Some(v) => {
            errs.push(format!(
                "demands[{i}].sustained_ma = {v} 须为正整数（mA，每实例连续电流；ADR-0021 裁定 2 五键定形）"
            ));
            None
        }
        None => {
            errs.push(format!(
                "demands[{i}].sustained_ma 应为整数（连续分量，须 ≤ peak_ma；缺键即红——不给缺省）"
            ));
            None
        }
    }
}

/// peak/sustained 交叉判据：连续分量超过峰值是物理矛盾（两值都可解析时才判）。
fn rr3_demand_cmp(peak: Option<i64>, sust: Option<i64>, i: usize, errs: &mut Vec<String>) {
    let (Some(p), Some(s)) = (peak, sust) else {
        return;
    };
    if p > 0 && s > p {
        errs.push(format!(
            "demands[{i}].sustained_ma = {s} > peak_ma = {p}（连续分量超过峰值是物理矛盾，fail-closed）"
        ));
    }
}

/// critical：必须显式布尔（true 进校验⑨ 预算加总）。
fn rr3_demand_crit(d: &Value, i: usize, errs: &mut Vec<String>) -> Option<bool> {
    let critical = d.get("critical").and_then(Value::as_bool);
    if critical.is_none() {
        errs.push(format!(
            "demands[{i}].critical 应为布尔（显式写不给缺省：true 进校验⑨ 预算加总）"
        ));
    }
    critical
}

/// concurrency_group（T-5）：必须显式写；空数组 = 不分组 = 全体并发（最坏口径）
/// 合法；非空元素须非空字符串（组名是判据 label 的一部分）。
fn rr3_demand_grp(d: &Value, i: usize, errs: &mut Vec<String>) -> Vec<String> {
    let Some(v) = d.get("concurrency_group") else {
        errs.push(format!(
            "demands[{i}].concurrency_group 缺失（T-5：必须显式写，[] = 不分组 = 全体并发最坏口径；同 no_injection 范式）"
        ));
        return Vec::new();
    };
    let Some(gs) = v.as_array() else {
        errs.push(format!(
            "demands[{i}].concurrency_group 应为字符串数组（[] 显式不分组合法——T-5 必须显式写）"
        ));
        return Vec::new();
    };
    let mut out = Vec::new();
    for (j, g) in gs.iter().enumerate() {
        if let Some(s) = rr3_grp_name(g, i, j, errs) {
            out.push(s);
        }
    }
    out
}

/// 单个组名：非空字符串入列，坏元素逐条点名（不静默丢弃）。
fn rr3_grp_name(g: &Value, i: usize, j: usize, errs: &mut Vec<String>) -> Option<String> {
    match g.as_str() {
        Some(s) if !s.trim().is_empty() => Some(s.to_string()),
        Some(_) => {
            errs.push(format!(
                "demands[{i}].concurrency_group[{j}] 不得为空（组名是互斥判据的 label）"
            ));
            None
        }
        None => {
            errs.push(format!(
                "demands[{i}].concurrency_group[{j}] 应为字符串（组名）"
            ));
            None
        }
    }
}

/// 两数组均在文本中出现且均为空（L-5 空面的前件）。
fn rr3_empty_surface(
    handles_present: bool,
    handles_empty: bool,
    ops_present: bool,
    ops_empty: bool,
) -> bool {
    handles_present && handles_empty && ops_present && ops_empty
}

/// L-5 空面规则（0.0.5 批M MC-10）：两数组均在文本中出现且均为空 ⇒ 必须明写
/// no_injection = true；明写 no_injection 时两数组必须都空。件级与组合件级共用。
pub(crate) fn rr3_no_injection_err(
    handles_present: bool,
    handles_empty: bool,
    ops_present: bool,
    ops_empty: bool,
    no_injection: bool,
) -> Vec<String> {
    let mut errs: Vec<String> = Vec::new();
    let empty = rr3_empty_surface(handles_present, handles_empty, ops_present, ops_empty);
    rr3_noinj_empty(empty, no_injection, &mut errs);
    rr3_noinj_clash(empty, no_injection, &mut errs);
    errs
}

/// 两数组均空却未明写 no_injection = true（L-5）。
fn rr3_noinj_empty(empty: bool, no_injection: bool, errs: &mut Vec<String>) {
    if empty && !no_injection {
        errs.push(
            "注入面两数组均空却未明写 no_injection = true（L-5：或补齐 handles/ops，或明写该件不需注入面）"
                .into(),
        );
    }
}

/// 明写 no_injection 却与注入面并存（两者互斥）。
fn rr3_noinj_clash(empty: bool, no_injection: bool, errs: &mut Vec<String>) {
    if no_injection && !empty {
        errs.push("no_injection = true 只允许与空的 handles/ops 并存（该声明与注入面互斥）".into());
    }
}

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

/// 顶层形状：schema_version（必须 2）与顶层未知键（只认 schema_version / assemble）。
fn rr3_asm_head(t: &toml::Table, errs: &mut Vec<String>) {
    rr3_asm_ver(t, errs);
    rr3_asm_top_keys(t, errs);
}

/// 顶层未知键（只认 schema_version / assemble）。
fn rr3_asm_top_keys(t: &toml::Table, errs: &mut Vec<String>) {
    for k in t.keys() {
        rr3_asm_top_key_err(k, errs);
    }
}

/// 单个顶层键裁决（只认 schema_version / assemble）。
fn rr3_asm_top_key_err(k: &str, errs: &mut Vec<String>) {
    if k != "schema_version" && k != "assemble" {
        errs.push(format!(
            "装配清单顶层未知键 {k}（只认 schema_version / assemble）"
        ));
    }
}

/// schema_version 版本对账（本文件定形即 2）。
fn rr3_asm_ver(t: &toml::Table, errs: &mut Vec<String>) {
    // v1 是"无代码读它"时代的死数据形状，直接点名不迁就
    match t.get("schema_version") {
        Some(Value::Integer(v)) if *v == RR3_ASSEMBLE_SCHEMA_VERSION as i64 => {}
        Some(Value::Integer(v)) => errs.push(format!(
            "装配清单 schema_version 必须为 {RR3_ASSEMBLE_SCHEMA_VERSION}，收到 {v}"
        )),
        Some(other) => errs.push(format!("装配清单 schema_version 应为整数，收到 {other:?}")),
        None => errs.push(format!(
            "装配清单缺 schema_version（必须为 {RR3_ASSEMBLE_SCHEMA_VERSION}）"
        )),
    }
}

/// `[assemble]` 未知键点名（内存申报键 task_stack_bytes/heap_peak_bytes 是件级
/// 专用——组合件级 [assemble] 不认，同 demands：内存账是原子件的物理属性）。
fn rr3_asm_keys(asm: &toml::Table, errs: &mut Vec<String>) {
    for k in asm.keys() {
        rr3_asm_key_err(k, errs);
    }
}

/// `[assemble]` 单个键裁决（六键外点名）。
fn rr3_asm_key_err(k: &str, errs: &mut Vec<String>) {
    if ![
        "handles",
        "ops",
        "no_injection",
        "demands",
        "task_stack_bytes",
        "heap_peak_bytes",
    ]
    .contains(&k)
    {
        errs.push(format!(
            "[assemble] 未知键 {k}（只认 handles / ops / no_injection / demands / task_stack_bytes / heap_peak_bytes）"
        ));
    }
}

/// no_injection = true 是"本件不需注入面"的显式声明（L-5）：只认布尔 true。
fn rr3_asm_noinj(asm: &toml::Table, facts: &mut Rr3AssembleFacts, errs: &mut Vec<String>) {
    match asm.get("no_injection") {
        Some(Value::Boolean(true)) => facts.no_injection = true,
        Some(Value::Boolean(false)) => {
            errs.push("no_injection 只认 true（本件不需注入面时明写；非空面不得写此键）".into())
        }
        Some(other) => errs.push(format!("no_injection 应为布尔 true，收到 {other:?}")),
        None => {}
    }
}

/// handles：数组，每条 = { trait, desc_key }，两键都在册表内（解析与组合件级共用
/// 一份）。返回（键在文本中出现, 数组为空）供 L-5 空面规则使用。
fn rr3_asm_handles(
    asm: &toml::Table,
    facts: &mut Rr3AssembleFacts,
    errs: &mut Vec<String>,
) -> (bool, bool) {
    let Some(v) = asm.get("handles") else {
        errs.push("[assemble] 缺 handles 键（空数组合法，键本身必填）".into());
        return (false, false);
    };
    let Some(items) = v.as_array() else {
        errs.push(format!("[assemble].handles 应为数组，收到 {v:?}"));
        return (false, false);
    };
    let (parsed, mut item_errs) = rr3_handles_parse(items);
    facts.handles = parsed;
    errs.append(&mut item_errs);
    (true, items.is_empty())
}

/// ops：数组，每条 = { slot, src_key }，slot 非空、src_key 为合法点路径（共用同上）。
/// 返回（键在文本中出现, 数组为空）供 L-5 空面规则使用。
fn rr3_asm_ops(
    asm: &toml::Table,
    facts: &mut Rr3AssembleFacts,
    errs: &mut Vec<String>,
) -> (bool, bool) {
    let Some(v) = asm.get("ops") else {
        errs.push("[assemble] 缺 ops 键（空数组合法，键本身必填）".into());
        return (false, false);
    };
    let Some(items) = v.as_array() else {
        errs.push(format!("[assemble].ops 应为数组，收到 {v:?}"));
        return (false, false);
    };
    let (parsed, mut item_errs) = rr3_ops_parse(items);
    facts.ops = parsed;
    errs.append(&mut item_errs);
    (true, items.is_empty())
}

/// demands：电流需求申报（基-36 首件）。缺键 = 空申报合法（执法面归门 c，
/// 不在形状层强制——L-5 空面规则只管注入面 handles/ops，不涉 demands）。
fn rr3_asm_demands(asm: &toml::Table, facts: &mut Rr3AssembleFacts, errs: &mut Vec<String>) {
    let Some(v) = asm.get("demands") else {
        return;
    };
    let Some(items) = v.as_array() else {
        errs.push(format!("[assemble].demands 应为数组，收到 {v:?}"));
        return;
    };
    let (parsed, mut item_errs) = rr3_demands_parse(items);
    facts.demands = parsed;
    errs.append(&mut item_errs);
}

/// task_stack_bytes / heap_peak_bytes：内存需求申报（校验⑩ 分子面，批N2 n2c-v10）。
/// 缺键 = 未申报 = 0 不进 Σ（申报义务未触发——板侧任务映射未建模，0.0.7 批G/批D1；
/// 形状层不强制，同 demands 的"缺键 = 空申报合法"）；给键必须是非负整数
/// （负数 = 物理矛盾，fail-closed）。件级语义非每实例——判据消费在
/// rrd `rr3_memory_budget`，直加不扩算。
fn rr3_asm_mem(asm: &toml::Table, facts: &mut Rr3AssembleFacts, errs: &mut Vec<String>) {
    for (key, slot) in [
        ("task_stack_bytes", &mut facts.task_stack_bytes),
        ("heap_peak_bytes", &mut facts.heap_peak_bytes),
    ] {
        rr3_asm_mem_one(asm, key, slot, errs);
    }
}

/// 单个内存申报键：非负整数入档，负数/非整数各点名；缺键 = 未申报。
fn rr3_asm_mem_one(asm: &toml::Table, key: &str, slot: &mut i64, errs: &mut Vec<String>) {
    match asm.get(key) {
        Some(Value::Integer(v)) if *v >= 0 => *slot = *v,
        Some(Value::Integer(v)) => errs.push(format!(
            "[assemble].{key} = {v} 须为非负整数（本件内存占用字节数，负数是物理矛盾）"
        )),
        Some(other) => errs.push(format!("[assemble].{key} 应为整数，收到 {other:?}")),
        None => {}
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    const OK_SAMPLE: &str = r#"
schema_version = 2

[assemble]
handles = [
  { trait = "Rr3Power", desc_key = "i2c_bus" },
  { trait = "Rr3Power", desc_key = "ina219_addr" },
]
ops = [
  { slot = "low_voltage_mv", src_key = "rr3-power.battery.low_voltage_mv" },
]
"#;

    #[test]
    fn sample_and_empty_arrays_parse() {
        let f = rr3_assemble_parse(OK_SAMPLE).expect("正例必须过");
        assert_eq!(f.handles.len(), 2);
        assert_eq!(f.handles[0].trait_name, "Rr3Power");
        assert_eq!(f.handles[0].desc_key, "i2c_bus");
        assert_eq!(f.ops.len(), 1);
        assert_eq!(f.ops[0].slot, "low_voltage_mv");

        // 空数组合法，但须明写 no_injection（L-5）
        let f = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = true\n",
        )
        .expect("明写不需注入面的空数组必须合法");
        assert!(f.handles.is_empty() && f.ops.is_empty() && f.no_injection);
    }

    /// L-5 空面规则：两数组均空而漏写 no_injection ⇒ 红；no_injection 与非空面并存 ⇒ 红。
    #[test]
    fn empty_surface_requires_no_injection_marker() {
        // 漏写 → 点名"未明写 no_injection"
        let errs = rr3_assemble_parse("schema_version = 2\n\n[assemble]\nhandles = []\nops = []\n")
            .expect_err("空面漏写声明必须红");
        assert!(
            errs.iter().any(|e| e.contains("未明写 no_injection")),
            "{errs:?}"
        );

        // 写了 no_injection 却又给了 handles → 互斥，红
        let errs = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nno_injection = true\nhandles = [ { trait = \"Rr3Power\", desc_key = \"i2c_bus\" } ]\nops = []\n",
        )
        .expect_err("声明与注入面并存必须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("只允许与空的 handles/ops 并存")),
            "{errs:?}"
        );

        // no_injection = false 不认（只认 true）
        let errs = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = false\n",
        )
        .expect_err("no_injection=false 必须红");
        assert!(errs.iter().any(|e| e.contains("只认 true")), "{errs:?}");
    }

    #[test]
    fn bad_shapes_are_all_named() {
        let text = concat!(
            "schema_version = 1\n", // 版本不认
            "mystery = 1\n",        // 顶层未知键
            "\n[assemble]\n",
            "extra = 1\n",   // [assemble] 未知键
            "handles = [\n", // 条目两处不在册
            "  { trait = \"Rr3Nope\", desc_key = \"nope_key\" },\n",
            "  \"bare-string\",\n", // 非表条目
            "]\n",
            "ops = [\n",
            "  { slot = \"\", src_key = \"a..b\" },\n", // 空槽 + 坏点路径
            "  { slot = \"s\", src_key = 3 },\n",       // 非字符串
            "]\n",
        );
        let errs = rr3_assemble_parse(text).expect_err("坏形必须报错");
        assert!(
            errs.iter().any(|e| e.contains("schema_version 必须为 2")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("顶层未知键 mystery")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("[assemble] 未知键 extra")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("trait \"Rr3Nope\" 未在册")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("desc_key \"nope_key\" 未在册")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("handles[1] 应为内联表")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("ops[0].slot 不得为空")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("src_key \"a..b\" 不是合法点路径")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("ops[1].src_key 应为字符串")),
            "{errs:?}"
        );
    }

    #[test]
    fn missing_required_keys_are_named() {
        // 缺 schema_version / 缺 [assemble] / 缺两键
        let errs =
            rr3_assemble_parse("[assemble]\nhandles = []\nops = []\n").expect_err("缺版本必须点名");
        assert!(errs[0].contains("缺 schema_version"), "{errs:?}");

        let errs = rr3_assemble_parse("schema_version = 2\n").expect_err("缺 [assemble] 必须点名");
        assert!(
            errs.iter().any(|e| e.contains("缺 [assemble] 表")),
            "{errs:?}"
        );

        let errs = rr3_assemble_parse("schema_version = 2\n[assemble]\n")
            .expect_err("缺 handles/ops 必须点名");
        assert!(errs.iter().any(|e| e.contains("缺 handles 键")), "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("缺 ops 键")), "{errs:?}");
    }

    #[test]
    fn injection_gaps_walk_config_paths() {
        let facts = rr3_assemble_parse(OK_SAMPLE).expect("正例必须过");
        let cfg = "[rr3-power.battery]\nlow_voltage_mv = 3300\n";
        // i2c 句柄不参与注入缺；ops 的一条在、一条缺 → 恰一条缺口
        let gaps = rr3_injection_gaps(&facts, cfg);
        assert_eq!(gaps.len(), 0, "{gaps:?}");

        let facts2 = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = [\n  { slot = \"a\", src_key = \"rr3-power.battery.low_voltage_mv\" },\n  { slot = \"b\", src_key = \"rr3-power.battery.missing_key\" },\n  { slot = \"c\", src_key = \"no.such.path\" },\n]\n",
        )
        .expect("必须过形状级");
        let gaps = rr3_injection_gaps(&facts2, cfg);
        assert_eq!(gaps.len(), 2, "{gaps:?}");
        assert!(
            gaps[0].contains("missing_key") && gaps[0].contains("注入缺"),
            "{gaps:?}"
        );

        // config 解析失败 = 全部来源键不可证
        let gaps = rr3_injection_gaps(&facts, "garbage =\n");
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        assert!(gaps[0].contains("解析失败"), "{gaps:?}");

        // ops 为空 → 永无缺口（config 坏也不点名）；空面须明写 no_injection（L-5）
        let empty = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = true\n",
        )
        .unwrap();
        assert!(rr3_injection_gaps(&empty, "garbage =\n").is_empty());
    }

    /// 电流需求申报面（基-36 首件）：正例解析 + 缺键 = 空申报合法 + 坏形逐条点名。
    /// 判据本体（Σ ≤ cap）不在本模块——读者在 rrd rr3_demand_budget。
    #[test]
    fn demands_parse_and_shape_errors_are_named() {
        // 正例：servo 真实申报形状（五键定形 = ADR-0021 裁定 2；handles 同 servo 真实
        // 句柄，demands 不豁免 L-5 空面规则——它不是注入面）
        let f = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = [\n  { trait = \"Rr3PwmServo\", desc_key = \"pwm_chip\" },\n]\nops = []\ndemands = [\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = [] },\n]\n",
        )
        .expect("正例必须过");
        assert_eq!(f.demands.len(), 1);
        assert_eq!(f.demands[0].rail, "rail5v");
        assert_eq!(f.demands[0].peak_ma, 700);
        assert_eq!(f.demands[0].sustained_ma, 120);
        assert!(f.demands[0].critical);
        assert!(f.demands[0].concurrency_group.is_empty());

        // 缺 demands 键 = 空申报合法（执法面归门 c）
        let f = rr3_assemble_parse(OK_SAMPLE).expect("无 demands 必须过");
        assert!(f.demands.is_empty());

        // 坏形逐条点名（五键全显式，T-5）：空 rail / 峰值非正 / 缺 sustained_ma /
        // sustained>peak 物理矛盾 / critical 缺失 / concurrency_group 缺键 / 非数组 /
        // 空组名与非串元素 / 未知键 / 非表条目
        let errs = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = [\n  { trait = \"Rr3PwmServo\", desc_key = \"pwm_chip\" },\n]\nops = []\ndemands = [\n  { rail = \"\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = [] },\n  { rail = \"rail5v\", peak_ma = 0, sustained_ma = 120, critical = true, concurrency_group = [] },\n  { rail = \"rail5v\", peak_ma = 700, critical = true, concurrency_group = [] },\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 800, critical = true, concurrency_group = [] },\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, concurrency_group = [] },\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true },\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = \"axis\" },\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = [\"\", 1] },\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = [], wat = 1 },\n  \"bare\",\n]\n",
        )
        .expect_err("坏形必须报错");
        assert!(
            errs.iter().any(|e| e.contains("demands[0].rail 不得为空")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[1].peak_ma = 0 须为正整数")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[2].sustained_ma 应为整数")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains(
                "demands[3].sustained_ma = 800 > peak_ma = 700（连续分量超过峰值是物理矛盾"
            )),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[4].critical 应为布尔")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[5].concurrency_group 缺失")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[6].concurrency_group 应为字符串数组")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[7].concurrency_group[0] 不得为空")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[7].concurrency_group[1] 应为字符串")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("demands[8] 未知键 wat")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("demands[9] 应为内联表")),
            "{errs:?}"
        );
    }

    /// 内存需求申报面（校验⑩ 分子，批N2 n2c-v10）：正例解析 + 缺键 = 未申报 0
    /// 合法 + 负数/非整数红。判据本体（Σ ≤ 预算）不在本模块——读者在
    /// rrd `rr3_memory_budget`（分母住 [supplies].budget）。
    #[test]
    fn memory_bytes_parse_and_shape_errors_are_named() {
        // 正例：两键显式给出（件级语义非每实例，直加不扩算——头注同源）
        let f = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = [\n  { trait = \"Rr3Display\", desc_key = \"spi_device\" },\n]\nops = []\ntask_stack_bytes = 8192\nheap_peak_bytes = 24576\n",
        )
        .expect("正例必须过");
        assert_eq!(f.task_stack_bytes, 8192);
        assert_eq!(f.heap_peak_bytes, 24576);

        // 缺两键 = 未申报 = 0 不进 Σ（申报义务未触发，同 demands 缺键合法）
        let f = rr3_assemble_parse(OK_SAMPLE).expect("无内存键必须过");
        assert_eq!(f.task_stack_bytes, 0);
        assert_eq!(f.heap_peak_bytes, 0);

        // 坏形逐条点名：负数（物理矛盾）/ 非整数 / 未知键拼错（词表封闭）
        let errs = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = true\ntask_stack_bytes = -1\nheap_peak_bytes = \"16k\"\nstack_bytes = 4096\n",
        )
        .expect_err("坏形必须报错");
        assert!(
            errs.iter()
                .any(|e| e.contains("task_stack_bytes = -1 须为非负整数")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("heap_peak_bytes 应为整数")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("[assemble] 未知键 stack_bytes")),
            "{errs:?}"
        );
    }
}
