//! 装配单装载（ADR-0012 裁定 3/5②，0.0.2 方案 D2 / Q3 裁定）。
//!
//! `profiles/<机型>.toml` 是**纯数据**装配单，schema_version=3 起按增补轮六 §4.2
//! 定形为八段：`[os]`（运行期不读）+ `[platform]`（tier 三档 host/rtos/bare）+
//! `[supplies]`（看门狗三键 + 时序预算位三键，修订 4 ② / 修订 2 ③）+ `[slots]`
//! （present = 本板物理在位集）+ `[bindings]` 格位→件、`[enabled_slots]` 模块→本形态
//! 启用的格位集合、`[hardware]` 非引脚键（`alsa_device`/`link_ifname`/`spi_device`
//! 直给）、`[[pin]]` 物理绑定（增补轮六 §4.2：**引脚侧唯一真值点**——七物理键
//! `pwm_chip`/`pwm_channel`/`gpio_chip`/`gpio_led_line`/`gpio_relay_line`/`i2c_bus`/
//! `ina219_addr` 自 [hardware] 下移，role 外键 → `contracts/roles.toml`，host 档由
//! 派生器合成注入文本，rtos/bare 档板侧消费归 0.0.7 批G/批D1）。
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
//!    rr3_wiring_path_for_profile`] 按装配单推导）——校验⑦-a/b/d 的跨档判据在
//!    boot 链合龙（⑦-c 待件 demands 数据面，批N2），接线档形状与闭合面在
//!    [`crate::wiring`] 裁。
//!
//! 基础层格位（`net.online` / `power.battery`）**恒在位**（slots.toml 2026-10-03 注记）：
//! 不进 bindings、不进缺格判定，enabled_slots 里声明它们只是"本形态用到"的记账。
//! `[os].route` 运行期不读（ADR-0012 裁定 3 B3 注记），解析时无视该段。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rr_registry::{
    rr3_registry_device_table_from_text, rr3_registry_platform_table_from_text,
    rr3_registry_role_table_from_text, rr3_registry_slot_table_from_text, Rr3AssembleDemand,
    Rr3DeviceTable, Rr3PlatformTable, Rr3RoleTable, Rr3SlotTable,
};

/// 契约格位表的编译期定位（与 `topics_load::RR3_CONTRACT_TOPICS_PATH` 同深度：
/// base/runtime/<crate> → 仓库根）
pub const RR3_CONTRACT_SLOTS_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../../contracts/slots.toml");

/// 契约角色表定位（与 slots.toml 同目录，增补轮六 §4.1）
pub const RR3_CONTRACT_ROLES_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../../contracts/roles.toml");

/// 契约器件词表定位（与 roles.toml 同目录，增补轮六 §4.1b——校验⑦-d 的 mate 集
/// 判定源；探测期取值域消费归 L-14/出厂自检，装载期不认型号，D10a）
pub const RR3_CONTRACT_DEVICES_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../contracts/devices.toml"
);

/// 只认 schema_version = 3（2→3 为破坏性变更：`[[pin]]` 物理绑定段进场、七物理键
/// 自 [hardware] 下移为 [pins] 派生视图，增补轮六 §4.2 / 门 n P-13；注意与
/// `RR3_ASSEMBLE_SCHEMA_VERSION`（rr-registry，装配清单域）是两个域，不混写）。
/// 不匹配 = 契约事故，拒启动。
const RR3_PROFILE_SCHEMA_VERSION: i64 = 3;

/// [supplies] 六键（ADR-0018 修订 4 ② 看门狗三键 + 修订 2 ③ 时序预算位三键）——
/// 未知键点名与解析白名单共用
const RR3_SUPPLIES_KEYS: [&str; 6] = [
    "watchdog_present",
    "watchdog_timeout_ms",
    "watchdog_feeder",
    "tick_period_ms",
    "tick_jitter_ms",
    "safety_core_interval_ms",
];

/// [platform] tier 三档（ADR-0018 修订 1：承载二分 → 三档）
const RR3_PLATFORM_TIERS: [&str; 3] = ["host", "rtos", "bare"];

/// 描述符物理键集（增补轮六 §4.2：自 [hardware] 下移 [pins] 派生的**七个**引脚侧键——
/// 全案 §4.2/§8 门 n 写"六个"但枚举列了七键，以枚举为准）。[hardware] 出现任一即红
/// （门 n 稳态判据）；注入文本键名不变，rr-hal 描述符解析端零改动。
const RR3_PIN_DERIVED_KEYS: [&str; 7] = [
    "pwm_chip",
    "pwm_channel",
    "gpio_chip",
    "gpio_led_line",
    "gpio_relay_line",
    "i2c_bus",
    "ina219_addr",
];

/// [[pin]] 条目字段词表（§4.2 [pins] 段；role 的 requires 域即此表子集——
/// 门 a 六表对账的第 5 张表）
const RR3_PIN_KEYS: [&str; 10] = [
    "role",
    "instance",
    "controller",
    "channel",
    "physical",
    "direction",
    "rail",
    "addr",
    "failsafe",
    "mux_group",
];

/// [[supplies.domain]] 条目字段词表（§3.1 家族 A ④ 列 + §5 校验⑥）：域名与阈值三键、
/// 域电流上限、三个绑定引用位（sense/current_sense/switch 一律写 pin_id）与开关/
/// 武装/龄期三键。全键必填——"合法不适用"显式写 `none`/`false`/`0`，留空即红
/// （§5 缺失语义列，同 no_injection 范式）；`max_age_ms = 0` 且 `armed = true`
/// 无运行期读者，由校验⑥-c 拒。
const RR3_DOMAIN_KEYS: [&str; 11] = [
    "name",
    "nominal_mv",
    "mild_mv",
    "severe_mv",
    "current_cap_ma",
    "sense",
    "current_sense",
    "switch",
    "switchable",
    "armed",
    "max_age_ms",
];

/// [supplies]：平台供给声明（不可变快照）。看门狗三键 = 有无 / 超时上界 / 由哪一路喂
/// （修订 4 ②）；时序三键 = 机制核节拍周期 / 节拍抖动预算 / 安全核最小复核间隔的
/// **声明预算位**（修订 2 ③）——声明值与生产真值的对账归 0.0.6 点验矩阵，这里只管收。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3Supplies {
    /// 平台看门狗有无（要素一）
    pub watchdog_present: bool,
    /// 看门狗超时上界 ms（要素二；应宽于模块级 health.watchdog_timeout_ms）
    pub watchdog_timeout_ms: i64,
    /// 由哪一路喂（要素三：如 "rrd" = rrd tick 环）
    pub watchdog_feeder: String,
    /// 机制核节拍周期声明 ms
    pub tick_period_ms: i64,
    /// 节拍抖动预算声明 ms
    pub tick_jitter_ms: i64,
    /// 安全核最小复核间隔声明 ms
    pub safety_core_interval_ms: i64,
    /// [[supplies.domain]] 电力网络声明（第七供给位，§3.1 家族 A；域名唯一，
    /// [pins].rail / wire.net 的在册名单源，校验⑥-a/⑦-b）。声明序 = TOML 序，
    /// 不携带顺序语义；查册用名字，无序消费。
    pub domains: Vec<Rr3Domain>,
}

/// 一条电力网络域（§3.1 家族 A 轨与域行 + §5 校验⑥）。阈值三键 = nominal/mild/severe
/// （mild > severe > 0 的次序校验归批N2 门 c——域阈值消费者侧；本批只收形状）。
/// 三个绑定引用位写 `pin_id`（§4.2 定义），`"none"` 是合法显式值——留空即红（⑥-d）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3Domain {
    /// 域名（在册名单源；[pins].rail 外键。全案在册名集 = batt/rail5v/rail3v3，
    /// 新名 = 契约级评审——§18 F-5）
    pub name: String,
    /// 标称电压 mV
    pub nominal_mv: i64,
    /// 轻度欠压阈 mV（tier=Mild 源，批N2 门 c 消费）
    pub mild_mv: i64,
    /// 重度欠压阈 mV（tier=Severe 源，批N2 门 c 消费）
    pub severe_mv: i64,
    /// 域电流上限 mA（校验⑨ 预算分母，批N2 门 c 消费）
    pub current_cap_ma: i64,
    /// 轨压测点（pin_id 外键，⑥-d；无测点显式写 "none"）
    pub sense: String,
    /// 电流采样点（pin_id 外键，⑥-d；无采样显式写 "none"）
    pub current_sense: String,
    /// 断电开关控制位（pin_id 外键，⑥-d；不可切显式写 "none"）
    pub switch: String,
    /// 可切域标记（断电轨 T1/T2 的数据位；与 switch ≠ none 成对）
    pub switchable: bool,
    /// 武装位（§3.1 降级语义：未标定即 false——armed=false 时档位照算越阈照报，
    /// 但不驱动拒动；armed=true 而无测点由校验⑥-c 拒）
    pub armed: bool,
    /// 测点龄期上界 ms（⑥-c：armed=true 必填且 > 0；无龄期快照不得充当拒动依据，§19 Y-4）
    pub max_age_ms: i64,
}

/// 一条 [[pin]] 物理绑定（增补轮六 §4.2）。身份 = `pin_id`：`per_instance=false`
/// ⇒ `pin_id = role`；`per_instance = true` ⇒ `pin_id = role "." instance`——
/// [supplies.domain] 绑定位、wire.signal、轴档案的 pin 键一律写 pin_id，不各造语法。
/// 字段"在场"语义（§5 缺失语义列）：String 键以空串 = 缺、`channel` 以 `None` = 缺、
/// `physical` 以空数组 = 缺——"合法不适用"必须显式写（`rail`/`addr` 写 `"none"`），
/// 留空即红（校验①-b 按 roles.requires 逐项判）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3PinEntry {
    /// 角色外键 → contracts/roles.toml（校验①-a 查册）
    pub role: String,
    /// 多实例名（per_instance=true 必填、false 禁填；如舵机 pan/tilt）
    pub instance: String,
    /// 控制器名（取值 ∈ 平台在册控制器词表，校验⑧-b——词表 = contracts/platform/
    /// <chip>.toml 的 [[controller]]，批N1 基-22 已在册；host 档词形 pwm<N>/gpiochip<N>/
    /// i2c<N> 由派生器解析）
    pub controller: String,
    /// 控制器通道（独立资源维度，校验③-b 的 (controller, channel) 不双占空间）
    pub channel: Option<i64>,
    /// 物理引脚数组（一角色可占多脚；GPIO 类派生取 physical[0]）
    pub physical: Vec<i64>,
    /// 方向（须与 role.direction 相等）
    pub direction: String,
    /// 供电轨名（kind=power 角色必填且须 ∈ [supplies].domains，校验⑥；信号侧控制线
    /// 显式写 "none"——留空即红，与 sense 的显式范式同型）。domains 随批N6 立册后
    /// 校验⑥-a 判在册
    pub rail: String,
    /// I2C 从地址（0x 前缀十六进制；power.battery.meter 类角色用，校验⑨ 互校随批N2）
    pub addr: String,
    /// 复位/欠压时的期望物理态：release | hold | driven_low | driven_high（§6 复位语义③）
    pub failsafe: String,
    /// 互斥组（§4.2 裁定：**不再自愿**——由平台 conflicts 表反推，漏声明即红；
    /// 组名取自平台表 [[conflict]].group，装机档不得自起名，校验④-b 执法）
    pub mux_group: Vec<String>,
}

impl Rr3PinEntry {
    /// pin_id（§4.2 全案外键语法）：per_instance 规则的取值侧。instance 空 = 无实例
    /// 名（调用方须先用 roles 表的 per_instance 判合法性，本函数不判）。
    pub fn rr3_pin_id(&self) -> String {
        if self.instance.is_empty() {
            self.role.clone()
        } else {
            format!("{}.{}", self.role, self.instance)
        }
    }
}

/// 一份装配单（不可变快照）。BTreeMap 让"模块序/格位序"确定化为升序——
/// spawn 序由 `plan_boot` 仲裁裁，这里不携带任何顺序语义。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3Profile {
    /// [platform].tier：本机档位（host / rtos / bare，ADR-0018 修订 1 三档）
    pub platform_tier: String,
    /// [platform].chip：平台事实表外键（§4.6，批N1 基-22）——校验③④⑤⑧按它选
    /// contracts/platform/<chip>.toml（fail-closed：表读不到/形状坏都拒 boot）
    pub platform_chip: String,
    /// [bindings]：格位 → 模块名（只允许 body/cognition 层格位）
    pub bindings: BTreeMap<String, String>,
    /// [enabled_slots]：模块名 → 本形态启用的格位集合
    pub enabled_slots: BTreeMap<String, Vec<String>>,
    /// [slots].present：本板物理在位格位声明（缺格三源合一第一源——声明缺）
    pub slots_present: Vec<String>,
    /// [supplies]：平台供给声明（看门狗三键 + 时序预算位三键）
    pub supplies: Rr3Supplies,
    /// [hardware]：非引脚键直给（`alsa_device`/`link_ifname`/`spi_device`；七物理键
    /// 出现即红——门 n 注入路径唯一，键位语义与取值域由 rr-hal 描述符解析裁，
    /// rrd 只管装配形状——标量字符串化，注入时不解释）
    pub hardware: BTreeMap<String, String>,
    /// [[pin]]：物理绑定（引脚侧唯一真值点，增补轮六 §4.2）。可选段（纯 sim/无硬件
    /// 格形态不给——此时校验②不判，sim 形态无物理面可绑）
    pub pins: Vec<Rr3PinEntry>,
}

/// 装配链交给 daemon 的 boot 输入
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3ProfileBoot {
    /// enabled_slots 模块的 manifest 路径（模块名升序，确定性）
    pub manifests: Vec<PathBuf>,
    /// 缺格 (模块, 格位)：present 声明缺 / enabled_slots 声明但无有效 binding（或
    /// 探测不到）→ 该模块不 spawn
    pub missing: Vec<(String, String)>,
    /// [hardware] 序列化文本（spawn 时注入 `RR3_HAL_BINDINGS`；空 = 未给物理绑定）
    pub hal_bindings: String,
    /// 装配缺项 (模块名, 点名文案)（G4/ADR-0019 修订 1 ③）：atoms/*/assemble.toml
    /// 缺文件/坏形/ops 注入缺 → 模块照常 spawn 但在册即 Degraded（不 fatal，点名到件）
    pub assemble_broken: Vec<(String, String)>,
    /// 缺角色 (格位, 角色)（装载期校验②，后果 = 降不拒）：present ∩ enabled_slots 的
    /// 格位在 roles.toml 有归属角色却无 [[pin]] 绑定——缺角色即缺能力。daemon 点名
    /// 日志；Degraded 可见面归批N4 I-15（今天面板零配置可见）
    pub role_missing: Vec<(String, String)>,
}

/// 装配单路径解析成绝对路径：装载与横幅凭证共用同一个解析结果。
/// 装配单**必须已存在**才可装载，所以用 `fs::canonicalize`（与主题契约/叠加层同工具、
/// 同一条"致命文案不漏 `..`"的理据，见 `topics_load::rr3_contract_resolved`）。
pub fn rr3_profile_absolute_path(path: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path)
        .map_err(|e| format!("路径 {} 无法解析为绝对路径: {e}", path.display()))
}

/// 解析装配单文本。形状错误全部收集完才返回（运维一次改对），逐条点名、不静默放行。
pub fn rr3_profile_from_text(text: &str) -> Result<Rr3Profile, Vec<String>> {
    let root: toml::Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("装配单 TOML 解析失败: {e}")]),
    };
    let mut errs = Vec::new();
    match root.get("schema_version").and_then(toml::Value::as_integer) {
        Some(v) if v == RR3_PROFILE_SCHEMA_VERSION => {}
        other => errs.push(format!(
            "schema_version 必须为 {RR3_PROFILE_SCHEMA_VERSION}，收到 {other:?}"
        )),
    }
    // [bindings]：非空表、每格非空字符串值。[os] 段运行期不读，解析时无视（见文件头）。
    let mut bindings = BTreeMap::new();
    match root.get("bindings").and_then(toml::Value::as_table) {
        None => errs
            .push("缺少 [bindings]（body/cognition 格位 → 模块；可为空表但段必须在）".to_string()),
        Some(t) => {
            for (slot, v) in t {
                match v.as_str() {
                    Some(m) if !m.is_empty() => {
                        bindings.insert(slot.clone(), m.to_string());
                    }
                    _ => errs.push(format!("bindings.{slot} 应为非空字符串（模块名）")),
                }
            }
        }
    }
    // [enabled_slots]：非空表、每值非空字符串数组。enabled_slots 决定起谁（Q3 裁定），空表 = 无可起模块。
    let mut enabled_slots = BTreeMap::new();
    match root.get("enabled_slots").and_then(toml::Value::as_table) {
        None => errs.push("缺少 [enabled_slots]（模块 → 本形态启用的格位集合）".to_string()),
        Some(t) => {
            if t.is_empty() {
                errs.push(
                    "[enabled_slots] 为空：enabled_slots 决定起谁，空装配单没有可起模块"
                        .to_string(),
                );
            }
            for (module, v) in t {
                match v.as_array() {
                    Some(items) => {
                        let mut slots = Vec::new();
                        for item in items {
                            match item.as_str() {
                                Some(s) if !s.is_empty() => slots.push(s.to_string()),
                                _ => errs.push(format!(
                                    "enabled_slots.{module} 的每一项都必须是非空字符串（格位名）"
                                )),
                            }
                        }
                        enabled_slots.insert(module.clone(), slots);
                    }
                    None => errs.push(format!("enabled_slots.{module} 应为字符串数组")),
                }
            }
        }
    }
    // [hardware]：可选段（纯 sim/无硬件格形态可以不给）。每键非空、值必须是
    // 字符串或整数标量（整数按十进制字符串化——rr-hal 描述符解析端全按字符串
    // 收）；布尔/浮点/数组/表逐条点名，一条不静默放行。
    let mut hardware = BTreeMap::new();
    match root.get("hardware").and_then(toml::Value::as_table) {
        None => {}
        Some(t) => {
            for (k, v) in t {
                if k.is_empty() {
                    errs.push("[hardware] 键不得为空串".to_string());
                    continue;
                }
                match v {
                    toml::Value::String(s) => {
                        hardware.insert(k.clone(), s.clone());
                    }
                    toml::Value::Integer(i) => {
                        hardware.insert(k.clone(), i.to_string());
                    }
                    other => errs.push(format!(
                        "hardware.{k} 应为字符串或整数标量（物理绑定值），收到 {other:?}"
                    )),
                }
            }
        }
    }
    // [[pin]]：可选段（schema 3，增补轮六 §4.2——纯 sim/无硬件格形态不给，此时校验②
    // 不判：无物理面可绑不等于角色缺失）。字段白名单 RR3_PIN_KEYS 十键、类型逐键点名；
    // 身份键 role/direction 解析期即须在场（①缺失语义：role/direction 缺 → 红，判定
    // 不依赖角色表）；instance/controller 及各 requires 键的"在场与留空"在 boot 链按
    // 角色表逐条裁——本函数不带契约真值，不越权判角色语义。
    let mut pins: Vec<Rr3PinEntry> = Vec::new();
    match root.get("pin").and_then(toml::Value::as_array) {
        None => {}
        Some(items) => {
            for (i, item) in items.iter().enumerate() {
                let t = match item.as_table() {
                    Some(t) => t,
                    None => {
                        errs.push(format!("pin #{i} 须为 [[pin]] 表（数组内出现非表值）"));
                        continue;
                    }
                };
                let mut pin = Rr3PinEntry::default();
                for (k, v) in t {
                    match k.as_str() {
                        "role" => match v.as_str() {
                            Some(s) if !s.is_empty() => pin.role = s.to_string(),
                            _ => errs.push(format!("pin #{i}.role 缺失或为空（角色外键）")),
                        },
                        "instance" => match v.as_str() {
                            Some(s) => pin.instance = s.to_string(),
                            None => errs.push(format!("pin #{i}.instance 应为字符串")),
                        },
                        "controller" => match v.as_str() {
                            Some(s) => pin.controller = s.to_string(),
                            None => errs.push(format!("pin #{i}.controller 应为字符串")),
                        },
                        "channel" => match v.as_integer() {
                            Some(n) => pin.channel = Some(n),
                            None => errs.push(format!("pin #{i}.channel 应为整数")),
                        },
                        "physical" => match v.as_array() {
                            Some(a) => {
                                for x in a {
                                    match x.as_integer() {
                                        Some(n) => pin.physical.push(n),
                                        None => errs.push(format!(
                                            "pin #{i}.physical 的每一项都必须是整数（物理引脚号）"
                                        )),
                                    }
                                }
                            }
                            None => errs.push(format!("pin #{i}.physical 应为整数数组")),
                        },
                        "direction" => match v.as_str() {
                            Some(s) if !s.is_empty() => pin.direction = s.to_string(),
                            _ => errs.push(format!(
                                "pin #{i}.direction 缺失或为空（校验①缺失语义：direction 缺 → 红）"
                            )),
                        },
                        "rail" => match v.as_str() {
                            Some(s) => pin.rail = s.to_string(),
                            None => errs.push(format!(
                                "pin #{i}.rail 应为字符串（供电轨名；信号侧控制线显式写 \"none\"）"
                            )),
                        },
                        "addr" => match v.as_str() {
                            Some(s) => pin.addr = s.to_string(),
                            None => errs.push(format!(
                                "pin #{i}.addr 应为字符串（0x 前缀十六进制；\"合法不适用\"显式写 \"none\"）"
                            )),
                        },
                        "failsafe" => match v.as_str() {
                            Some(s) => pin.failsafe = s.to_string(),
                            None => errs.push(format!(
                                "pin #{i}.failsafe 应为字符串（release/hold/driven_low/driven_high）"
                            )),
                        },
                        "mux_group" => match v.as_array() {
                            Some(a) => {
                                for x in a {
                                    match x.as_str() {
                                        Some(s) if !s.is_empty() => {
                                            pin.mux_group.push(s.to_string())
                                        }
                                        _ => errs.push(format!(
                                            "pin #{i}.mux_group 的每一项都必须是非空字符串"
                                        )),
                                    }
                                }
                            }
                            None => errs.push(format!(
                                "pin #{i}.mux_group 应为字符串数组（显式 [] 合法，互斥组归平台 conflicts 表批N7）"
                            )),
                        },
                        other => errs.push(format!(
                            "pin #{i} 未知键 {other}（只认 {}）",
                            RR3_PIN_KEYS.join("/")
                        )),
                    }
                }
                if !t.contains_key("role") {
                    errs.push(format!(
                        "pin #{i} 缺 role（角色外键——引脚侧唯一真值点必须挂角色）"
                    ));
                }
                if !t.contains_key("direction") {
                    errs.push(format!("pin #{i} 缺 direction（校验①缺失语义）"));
                }
                pins.push(pin);
            }
        }
    }
    // [platform]：必填段、单键 tier（ADR-0018 修订 1 ④ 七段之一）。tier 取值域
    // host/rtos/bare 三档；chip = 平台事实表外键（§4.6，校验③④⑤⑧的判据前件选册
    // ——contracts/platform/<chip>.toml，批N1 基-22），非空必填：芯片是装机事实，
    // 留空 = ⑤⑧ 判不了 = fail-closed（同"留空即红"纪律），未知键逐条点名。
    let mut platform_tier = String::new();
    let mut platform_chip = String::new();
    match root.get("platform").and_then(toml::Value::as_table) {
        None => errs
            .push("缺少 [platform]（tier = host/rtos/bare；schema_version=2 起必填）".to_string()),
        Some(t) => {
            match t.get("tier") {
                None => errs.push("[platform] 缺 tier（host/rtos/bare）".to_string()),
                Some(v) => match v.as_str() {
                    Some(s) if RR3_PLATFORM_TIERS.contains(&s) => platform_tier = s.to_string(),
                    other => errs.push(format!(
                        "platform.tier 必须是 host/rtos/bare 之一，收到 {other:?}"
                    )),
                },
            }
            match t.get("chip") {
                Some(toml::Value::String(s)) if !s.trim().is_empty() => platform_chip = s.clone(),
                Some(toml::Value::String(_)) => {
                    errs.push("[platform].chip 不得为空串（平台事实表外键，contracts/platform/<chip>.toml 选册）".to_string())
                }
                Some(other) => errs.push(format!("[platform].chip 应为字符串，收到 {other:?}")),
                None => errs.push(
                    "[platform] 缺 chip（平台事实表外键——校验③④⑤⑧按它选 contracts/platform/<chip>.toml，留空即红）"
                        .to_string(),
                ),
            }
        }
    }
    if let Some(t) = root.get("platform").and_then(toml::Value::as_table) {
        for k in t.keys() {
            if k != "tier" && k != "chip" {
                errs.push(format!("[platform] 未知键 {k}（只认 tier / chip）"));
            }
        }
    }
    // [supplies]：必填段（看门狗三键 + 时序预算位三键，六键全必填、类型逐键检查、
    // 未知键点名）——这是 G6 档位一致性校验与 0.0.6 点验矩阵的输入事实。
    let mut supplies = Rr3Supplies::default();
    match root.get("supplies").and_then(toml::Value::as_table) {
        None => errs.push(
            "缺少 [supplies]（平台看门狗与时序预算声明；schema_version=2 起必填）".to_string(),
        ),
        Some(t) => {
            match t.get("watchdog_present").and_then(toml::Value::as_bool) {
                Some(b) => supplies.watchdog_present = b,
                None => errs.push("supplies.watchdog_present 缺失或不是布尔".to_string()),
            }
            for key in [
                "watchdog_timeout_ms",
                "tick_period_ms",
                "tick_jitter_ms",
                "safety_core_interval_ms",
            ] {
                match supplies_int(t.get(key), key) {
                    Ok(v) => match key {
                        "watchdog_timeout_ms" => supplies.watchdog_timeout_ms = v,
                        "tick_period_ms" => supplies.tick_period_ms = v,
                        "tick_jitter_ms" => supplies.tick_jitter_ms = v,
                        _ => supplies.safety_core_interval_ms = v,
                    },
                    Err(e) => errs.push(e),
                }
            }
            match t.get("watchdog_feeder").and_then(toml::Value::as_str) {
                Some(s) if !s.is_empty() => supplies.watchdog_feeder = s.to_string(),
                _ => errs.push("supplies.watchdog_feeder 缺失或不是非空字符串".to_string()),
            }
            for k in t.keys() {
                // domain 是 [[supplies.domain]] 数组键，单独成块解析（第七供给位）
                if k != "domain" && !RR3_SUPPLIES_KEYS.contains(&k.as_str()) {
                    errs.push(format!(
                        "[supplies] 未知键 {k}（只认 {}）",
                        RR3_SUPPLIES_KEYS.join("/")
                    ));
                }
            }
            // [[supplies.domain]]：电力网络声明（第七供给位，§3.1 家族 A；校验⑥ 判据源）。
            // 缺数组 = 红吗？——不：域集是形态事实（无电力网络声明的形态不存在，
            // 但 schema 3 装载期校验⑥-a 需要名单源，pins.rail 非空即须命中本表；
            // 空数组 = 全 pin 无轨 = ⑥-b 若有 power 角色即红），解析层只收形状。
            match t.get("domain").and_then(toml::Value::as_array) {
                Some(items) => {
                    for (i, item) in items.iter().enumerate() {
                        let label = format!("supplies.domains[{i}]");
                        let Some(dt) = item.as_table() else {
                            errs.push(format!("{label} 必须是 [[supplies.domain]] 表"));
                            continue;
                        };
                        let mut d = Rr3Domain::default();
                        for key in RR3_DOMAIN_KEYS {
                            let path = format!("{label}.{key}");
                            match key {
                                "name" | "sense" | "current_sense" | "switch" => {
                                    match dt.get(key).and_then(toml::Value::as_str) {
                                        Some(s) if !s.is_empty() => {
                                            let v = s.to_string();
                                            match key {
                                                "name" => d.name = v,
                                                "sense" => d.sense = v,
                                                "current_sense" => d.current_sense = v,
                                                _ => d.switch = v,
                                            }
                                        }
                                        _ => errs.push(format!("{path} 缺失或不是非空字符串")),
                                    }
                                }
                                "switchable" | "armed" => {
                                    match (key, dt.get(key).and_then(toml::Value::as_bool)) {
                                        ("switchable", Some(b)) => d.switchable = b,
                                        ("armed", Some(b)) => d.armed = b,
                                        _ => errs.push(format!("{path} 缺失或不是布尔")),
                                    }
                                }
                                _ => match supplies_int(dt.get(key), &path) {
                                    Ok(v) => match key {
                                        "nominal_mv" => d.nominal_mv = v,
                                        "mild_mv" => d.mild_mv = v,
                                        "severe_mv" => d.severe_mv = v,
                                        "current_cap_ma" => d.current_cap_ma = v,
                                        _ => d.max_age_ms = v,
                                    },
                                    Err(e) => errs.push(e),
                                },
                            }
                        }
                        for k in dt.keys() {
                            if !RR3_DOMAIN_KEYS.contains(&k.as_str()) {
                                errs.push(format!(
                                    "{label} 未知键 {k}（只认 {}）",
                                    RR3_DOMAIN_KEYS.join("/")
                                ));
                            }
                        }
                        supplies.domains.push(d);
                    }
                }
                None => errs.push(
                    "缺少 [[supplies.domain]]（电力网络声明，第七供给位；schema_version=3 起必填，至少声明一域）"
                        .to_string(),
                ),
            }
        }
    }
    // [slots]：必填段、present = 本板物理在位格位声明（缺格三源合一第一源）。
    // 每项非空字符串；未知键点名。空数组合法——板可以什么都不在位（全 Degraded）。
    let mut slots_present: Vec<String> = Vec::new();
    match root.get("slots").and_then(toml::Value::as_table) {
        None => errs.push(
            "缺少 [slots]（present = 本板物理在位格位声明；schema_version=2 起必填）".to_string(),
        ),
        Some(t) => {
            match t.get("present").and_then(toml::Value::as_array) {
                Some(items) => {
                    for item in items {
                        match item.as_str() {
                            Some(s) if !s.is_empty() => slots_present.push(s.to_string()),
                            _ => errs.push(
                                "[slots].present 的每一项都必须是非空字符串（格位名）".to_string(),
                            ),
                        }
                    }
                }
                None => errs.push("[slots] 缺 present（本板物理在位格位数组）".to_string()),
            }
            for k in t.keys() {
                if k != "present" {
                    errs.push(format!("[slots] 未知键 {k}（只认 present）"));
                }
            }
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(Rr3Profile {
        platform_tier,
        platform_chip,
        bindings,
        enabled_slots,
        slots_present,
        supplies,
        hardware,
        pins,
    })
}

/// [supplies] 的整数键读取：缺失或类型不合同一条错误点名（运维一次改对）
fn supplies_int(v: Option<&toml::Value>, key: &str) -> Result<i64, String> {
    v.and_then(toml::Value::as_integer)
        .ok_or_else(|| format!("supplies.{key} 缺失或不是整数"))
}

/// 控制器词形解析（host 档派生规则的机制侧）：`<前缀><十进制编号>` 才算命中，返回
/// 编号串——roles.toml 注记的取值语义（pwm<N> 派生 pwm_chip=N），与旧 [hardware]
/// 的整数字符串化逐字节对齐，注入文本形状不变。
fn rr3_controller_wordform(controller: &str, prefix: &str) -> Option<String> {
    let rest = controller.strip_prefix(prefix)?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(rest.to_string())
}

/// 单条 pin 的派生取值（机制侧，七物理键封闭）：role.derives（策略，contracts/
/// roles.toml）只声明"该角色供给哪些键"，取值法一律查本函数——键不在册/来源字段
/// 留空/控制器词形不合都红。gpio_led_line 与 gpio_relay_line 同取 physical[0]，由
/// 角色声明区分（两角色不共用同一派生键名，不歧义）。
fn rr3_pin_derive_value(pin: &Rr3PinEntry, key: &str) -> Result<String, String> {
    match key {
        "pwm_chip" => rr3_controller_wordform(&pin.controller, "pwm")
            .ok_or_else(|| format!("controller 词形 {} 不合 pwm<N>", pin.controller)),
        "pwm_channel" => pin
            .channel
            .map(|c| c.to_string())
            .ok_or_else(|| "channel 留空".to_string()),
        "gpio_chip" => rr3_controller_wordform(&pin.controller, "gpiochip")
            .ok_or_else(|| format!("controller 词形 {} 不合 gpiochip<N>", pin.controller)),
        "gpio_led_line" | "gpio_relay_line" => pin
            .physical
            .first()
            .map(|n| n.to_string())
            .ok_or_else(|| "physical 留空".to_string()),
        "i2c_bus" => rr3_controller_wordform(&pin.controller, "i2c")
            .ok_or_else(|| format!("controller 词形 {} 不合 i2c<N>", pin.controller)),
        "ina219_addr" => {
            if pin.addr.is_empty() {
                Err("addr 留空".to_string())
            } else {
                Ok(pin.addr.clone())
            }
        }
        other => Err(format!("派生键 {other} 不在派生规则表（七物理键封闭）")),
    }
}

/// 校验①-b 的"字段在场"判定（§5 缺失语义列）：String 空串 = 缺、channel 以 None =
/// 缺、physical 以空数组 = 缺。mux_group 不进 requires（互斥组由平台 conflicts 表
/// 反推，批N7）；词表外字段名由调用方点名（门 a 六表对账第 5 张的漂移）。
fn rr3_pin_field_present(pin: &Rr3PinEntry, field: &str) -> bool {
    match field {
        "role" => !pin.role.is_empty(),
        "instance" => !pin.instance.is_empty(),
        "controller" => !pin.controller.is_empty(),
        "channel" => pin.channel.is_some(),
        "physical" => !pin.physical.is_empty(),
        "direction" => !pin.direction.is_empty(),
        "rail" => !pin.rail.is_empty(),
        "addr" => !pin.addr.is_empty(),
        "failsafe" => !pin.failsafe.is_empty(),
        _ => false,
    }
}

/// [hardware] 段序列化成注入文本：每行 `键 = "值"`（值已字符串化，TOML 基本
/// 字符串转义只涉及反斜杠与双引号）。BTreeMap 升序 = 确定性；空表 → 空串
/// （调用方见空不注入，模块侧维持"无描述符"状态）。
fn rr3_hardware_bindings_text(hardware: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    for (k, v) in hardware {
        out.push_str(&format!(
            "{k} = \"{}\"\n",
            v.replace('\\', "\\\\").replace('"', "\\\"")
        ));
    }
    out
}

/// 装配链 boot 输入装配（daemon 在 bind 之前调用，fatal 即非 0 退出、套接字不留残迹）。
/// 致命错误（读不到 / 形状不合 / 查册未命中 / 结构不自洽）打包成一条串返回；
/// 缺格**不是**致命项（P2：缺格只降点名模块，缺格≠拒 boot），进 [`Rr3ProfileBoot::missing`]；
/// 缺角色同样不是致命项（校验②后果 = 降），进 [`Rr3ProfileBoot::role_missing`]。
/// `slots_path`/`roles_path`/`devices_path`/`wiring_path` 由调用方给定（`--contracts`
/// 运行期定位或编译期仓内路径；接线档由 [`crate::wiring::rr3_wiring_path_for_profile`]
/// 按 `hardware/<形态>/wiring.toml` 布局推导）——查册真值仍只有 rr-registry 那几册
/// 与接线档本体，这里只管从哪读。
pub fn rr3_profile_boot_inputs(
    profile_path: &Path,
    slots_path: &Path,
    roles_path: &Path,
    devices_path: &Path,
    wiring_path: &Path,
    platform_dir: &Path,
) -> Result<Rr3ProfileBoot, String> {
    let probe = crate::probe::rr3_probe_default();
    rr3_profile_boot_inputs_with_probe(
        profile_path,
        slots_path,
        roles_path,
        devices_path,
        wiring_path,
        platform_dir,
        probe.as_ref(),
    )
}

/// 同 [`rr3_profile_boot_inputs`]，但探测件由调用方注入（MC-4 接缝）：生产走
/// [`crate::probe::rr3_probe_default`]（非 test 面按目标分流），门禁用例注入
/// 恒缺/假根探针钉"缺格 → Degraded 点名件与键"。探测是纯读面、无状态，注入
/// 不改变 boot 前的单次消费语义（ADR-0012 裁定 4）。
pub fn rr3_profile_boot_inputs_with_probe(
    profile_path: &Path,
    slots_path: &Path,
    roles_path: &Path,
    devices_path: &Path,
    wiring_path: &Path,
    platform_dir: &Path,
    probe: &dyn crate::probe::Rr3Probe,
) -> Result<Rr3ProfileBoot, String> {
    let abs = rr3_profile_absolute_path(profile_path)?;
    let text = std::fs::read_to_string(&abs)
        .map_err(|e| format!("读不到装配单 {}: {e}", abs.display()))?;
    let profile = rr3_profile_from_text(&text).map_err(|errs| {
        errs.iter()
            .map(|e| format!("装配单 {} 形状不合: {e}", abs.display()))
            .collect::<Vec<_>>()
            .join("; ")
    })?;

    // 查册门（裁定 5②）：格位真值只有 rr-registry 那一册，CI 全集层与本入口共用。
    let slots_abs = std::fs::canonicalize(slots_path)
        .map_err(|e| format!("读不到格位契约 {}: {e}", slots_path.display()))?;
    let slots_text = std::fs::read_to_string(&slots_abs)
        .map_err(|e| format!("读不到格位契约 {}: {e}", slots_abs.display()))?;
    let table: Rr3SlotTable = rr3_registry_slot_table_from_text(&slots_text);
    if table.rr3_slot_table_is_empty() {
        return Err(format!(
            "格位契约 {} 解析后为空，拒绝启动（空格位表是契约事故）",
            slots_abs.display()
        ));
    }
    // 形状门（0.0.5 批J 深挖收口；与 topics 装载的 rr3_topic_set_check 同构——
    // CI 契约测试钉 21 格是全集层门，这里是运行期门，两轴缺一即带病）：
    // 格名规则、层名、status/tier_scope 白名单逐条点名。
    let slot_errs = table.rr3_slot_table_check();
    if !slot_errs.is_empty() {
        return Err(format!(
            "格位契约 {} 形状不合，拒绝启动: {}",
            slots_abs.display(),
            slot_errs.join("; ")
        ));
    }

    // 角色契约装载（增补轮六 §4.1，校验①②判据源）：与格位契约同一 fail-closed
    // 口径——读不到 / 解析空 / 形状越界都是契约事故，拒启动（空角色表不是放行许可证）。
    let roles_abs = std::fs::canonicalize(roles_path)
        .map_err(|e| format!("读不到角色契约 {}: {e}", roles_path.display()))?;
    let roles_text = std::fs::read_to_string(&roles_abs)
        .map_err(|e| format!("读不到角色契约 {}: {e}", roles_abs.display()))?;
    let roles: Rr3RoleTable = rr3_registry_role_table_from_text(&roles_text);
    if roles.rr3_role_table_is_empty() {
        return Err(format!(
            "角色契约 {} 解析后为空，拒绝启动（空角色表是契约事故）",
            roles_abs.display()
        ));
    }
    let role_errs = roles.rr3_role_table_check();
    if !role_errs.is_empty() {
        return Err(format!(
            "角色契约 {} 形状不合，拒绝启动: {}",
            roles_abs.display(),
            role_errs.join("; ")
        ));
    }

    // 器件词表装载（增补轮六 §4.1b，校验⑦-d 的 mate 集判定源）：与格位/角色契约
    // 同一 fail-closed 口径——空器件表是契约事故，不是放行许可证。装载期只查
    // mate 外键闭合，不认型号能力（D10a：addrs/baud_ok 是探测期/出厂自检判据）。
    let devices_abs = std::fs::canonicalize(devices_path)
        .map_err(|e| format!("读不到器件词表 {}: {e}", devices_path.display()))?;
    let devices_text = std::fs::read_to_string(&devices_abs)
        .map_err(|e| format!("读不到器件词表 {}: {e}", devices_abs.display()))?;
    let devices: Rr3DeviceTable = rr3_registry_device_table_from_text(&devices_text);
    if devices.rr3_device_table_is_empty() {
        return Err(format!(
            "器件词表 {} 解析后为空，拒绝启动（空器件表是契约事故）",
            devices_abs.display()
        ));
    }
    let device_errs = devices.rr3_device_table_check();
    if !device_errs.is_empty() {
        return Err(format!(
            "器件词表 {} 形状不合，拒绝启动: {}",
            devices_abs.display(),
            device_errs.join("; ")
        ));
    }

    // 接线档装载（增补轮六 §4.3，校验⑦ 判据面）：装机档必在——读不到/空表/
    // 形状不合/闭合面破都是装配事故，拒启动（接线闭合是校验⑦的前提，没有
    // "无线的机器"）。
    let wiring_abs = std::fs::canonicalize(wiring_path)
        .map_err(|e| format!("读不到接线档 {}: {e}", wiring_path.display()))?;
    let wiring_text = std::fs::read_to_string(&wiring_abs)
        .map_err(|e| format!("读不到接线档 {}: {e}", wiring_abs.display()))?;
    let wiring = crate::wiring::rr3_wiring_from_text(&wiring_text).map_err(|errs| {
        format!(
            "接线档 {} 形状不合，拒绝启动: {}",
            wiring_abs.display(),
            errs.join("; ")
        )
    })?;

    // 平台事实表装载（增补轮六 §4.6，批N1 基-22；校验③④⑤⑧的判据前件）：按
    // [platform].chip 在 platform_dir 下选册（daemon 传 contracts/platform/，与
    // roles/devices 同源）。fail-closed 与 roles 同口径但更严——platform.rs 解析本就
    // 回 Err 版（roles/devices 宽容回空表走"表为空"点名，那是集合词表；平台表宽容 =
    // ④⑤⑧ 静默不判，§18 F-6 同病），读不到/形状坏都逐条点名拒 boot，不降级。
    let platform_path = platform_dir.join(format!("{}.toml", profile.platform_chip));
    let platform_text = std::fs::read_to_string(&platform_path).map_err(|e| {
        format!(
            "读不到平台事实表 {}（[platform].chip = {} 的选册）: {e}",
            platform_path.display(),
            profile.platform_chip
        )
    })?;
    let platform: Rr3PlatformTable =
        rr3_registry_platform_table_from_text(&platform_text).map_err(|errs| {
            format!(
                "平台事实表 {} 形状不合，拒绝启动: {}",
                platform_path.display(),
                errs.join("; ")
            )
        })?;

    let mut errs: Vec<String> = Vec::new();
    // bindings 的格位必须查册命中，且不得是基础层（基础层由基座内建承载，slots.toml 注记）
    for slot in profile.bindings.keys() {
        if !table.rr3_slot_table_contains(slot) {
            errs.push(format!(
                "bindings 格位 {slot} 未在册（contracts/slots.toml 无此格，装配单不得自造格位）"
            ));
        } else if table.rr3_slot_table_layer(slot).as_deref() == Some("base") {
            errs.push(format!(
                "bindings 不得绑定基础层格位 {slot}（基础层由基座内建承载，恒在位）"
            ));
        }
    }
    // enabled_slots 的格位必须查册命中（基础层允许——恒在位，只记账不绑定）
    for (module, slots) in &profile.enabled_slots {
        for slot in slots {
            if !table.rr3_slot_table_contains(slot) {
                errs.push(format!(
                    "enabled_slots 模块 {module} 的格位 {slot} 未在册（contracts/slots.toml 无此格）"
                ));
            }
        }
    }
    // [slots].present 查册门：声明集同样只认 rr-registry 那一册；基础层格位恒在位
    // （slots.toml 注记），进 present 属自相矛盾 → 点名拒。
    for slot in &profile.slots_present {
        if !table.rr3_slot_table_contains(slot) {
            errs.push(format!(
                "[slots].present 格位 {slot} 未在册（contracts/slots.toml 无此格）"
            ));
        } else if table.rr3_slot_table_layer(slot).as_deref() == Some("base") {
            errs.push(format!(
                "[slots].present 不得声明基础层格位 {slot}（基础层由基座内建承载，恒在位）"
            ));
        }
    }
    // G6 档位一致性（ADR-0018 修订 1：无看门狗的板不允许归 bare 档）：
    // tier=bare 而 watchdog_present=false 的装配单自相矛盾 → 拒 boot。
    if profile.platform_tier == "bare" && !profile.supplies.watchdog_present {
        errs.push(
            "档位一致性校验失败：tier = bare 但 supplies.watchdog_present = false——无看门狗的板不允许归 bare 档（ADR-0018 修订 1），拒绝启动"
                .to_string(),
        );
    }
    // L-10 tier↔tier_scope 交叉校验（ADR-0012 增补注记 base-supply/base-bus 分族的
    // 装配侧执法，0.0.5 批K）：tier_scope 说"这档要不要这张格"——tier 档引用
    // scope 外的格 = 自相矛盾（如 bare 自足档无总线，引用 bus.* 即假账）→ 拒 boot
    // 点名。三处引用（bindings / enabled_slots / [slots].present）合并去重逐格查；
    // 未在册格由上面三处查册门点名，此处只对在册格判 scope，不重复报。
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    referenced.extend(profile.bindings.keys().cloned());
    for slots in profile.enabled_slots.values() {
        referenced.extend(slots.iter().cloned());
    }
    referenced.extend(profile.slots_present.iter().cloned());
    for slot in &referenced {
        if !table.rr3_slot_table_contains(slot) {
            continue;
        }
        let scope = table.rr3_slot_table_tier_scope(slot).unwrap_or_default();
        if !scope.iter().any(|t| t == &profile.platform_tier) {
            errs.push(format!(
                "tier↔tier_scope 交叉校验失败：装配档 {tier} 引用格 {slot}，其 tier_scope = {scope:?} 不含 {tier}（该档不适用此格，ADR-0012 增补注记分族），拒绝启动",
                tier = profile.platform_tier
            ));
        }
    }
    // 结构自洽：每条 binding 都必须被 enabled_slots 认领（enabled_slots[M] 含该格位）——
    // 否则它是死数据：绑了没人用，装配单与 enabled_slots 各说一套。
    for (slot, module) in &profile.bindings {
        let claimed = profile
            .enabled_slots
            .get(module)
            .is_some_and(|slots| slots.contains(slot));
        if !claimed {
            errs.push(format!(
                "bindings 条目 {slot}={module} 未被 enabled_slots 认领（模块 {module} 不在 enabled_slots，或其 enabled_slots 不含 {slot}）"
            ));
        }
    }
    // 装载期校验①（增补轮六 §5，后果 = 拒）：[[pin]] 逐条——role 查册、per_instance
    // 规则、requires 逐项在场（留空即红）、direction 与角色一致、role.slot ∈ slots。
    // 同环做 host 档派生（机制常量规则 × 角色 derives 声明）：rtos/bare 档不派生，
    // 板侧描述符消费归 0.0.7 批G/批D1——pins 的校验照跑（物理真值不分档）。
    let mut derived: BTreeMap<String, String> = BTreeMap::new();
    for pin in &profile.pins {
        let label = format!("pins[{}]", pin.rr3_pin_id());
        // ①-a 角色在册（未在册的角色后续判据无从谈起，点名后跳过该条，不级联刷屏）
        let Some(derives) = roles.rr3_role_table_derives(&pin.role) else {
            errs.push(format!(
                "{label} 角色 {} 未在册（contracts/roles.toml 无此角色，装配单不得自造角色）",
                pin.role
            ));
            continue;
        };
        // ①-b per_instance 规则：true ⇒ instance 必填（pin_id = role.instance），
        // false ⇒ instance 禁填（pin_id = role）
        match roles.rr3_role_table_per_instance(&pin.role) {
            Some(true) if pin.instance.is_empty() => errs.push(format!(
                "{label} 角色 {} per_instance = true，instance 必填（多实例角色的身份一半在 instance）",
                pin.role
            )),
            Some(false) if !pin.instance.is_empty() => errs.push(format!(
                "{label} 角色 {} per_instance = false，instance 禁填（填了 pin_id 就有两个真值点）",
                pin.role
            )),
            _ => {}
        }
        // ①-b requires 逐项在场（§5 缺失语义：String 空串 = 缺、channel None = 缺、
        // physical 空数组 = 缺；rail/addr 的合法显式值 "none" 照常算在场——
        // "合法不适用"必须显式写，留空即红）
        for f in roles.rr3_role_table_requires(&pin.role).unwrap_or_default() {
            if !RR3_PIN_KEYS.contains(&f.as_str()) {
                errs.push(format!(
                    "角色 {} 的 requires 引用未知字段 {f}（[pins] 字段词表外——门 a 六表对账第 5 张的漂移）",
                    pin.role
                ));
            } else if !rr3_pin_field_present(pin, &f) {
                errs.push(format!(
                    "{label} 缺 {f}（角色 {} requires 必填，留空即红——§5 缺失语义；\"合法不适用\"须显式写 none）",
                    pin.role
                ));
            }
        }
        // ① direction 相等（pin 的物理方向与角色声明的方向各说一套 = 数据矛盾）
        if let Some(d) = roles.rr3_role_table_direction(&pin.role) {
            if pin.direction != d {
                errs.push(format!(
                    "{label} direction = {} 与角色 {} direction = {d} 不一致（校验①）",
                    pin.direction, pin.role
                ));
            }
        }
        // ① role.slot ∈ slots（roles/slots 两表对账的装载侧执法——真值各在对方表）
        if let Some(slot) = roles.rr3_role_table_slot(&pin.role) {
            if !table.rr3_slot_table_contains(&slot) {
                errs.push(format!(
                    "角色 {} 的 slot {slot} 未在格位契约在册（roles/slots 两表对账）",
                    pin.role
                ));
            }
        }
        if profile.platform_tier != "host" {
            continue;
        }
        // host 档派生：derives 声明逐键取值；多实例共键须同值（双轴同 chip 合法，
        // 两 chip 抢一个键名 = 资源矛盾）
        for d in &derives {
            match rr3_pin_derive_value(pin, d) {
                Ok(v) => match derived.get(d) {
                    None => {
                        derived.insert(d.clone(), v);
                    }
                    Some(prev) if prev != &v => errs.push(format!(
                        "{label} 派生键 {d} 与他条取值不同（{prev} vs {v}）——多实例共键须同值（同一资源维）"
                    )),
                    _ => {}
                },
                Err(e) => errs.push(format!("{label} 派生 {d} 失败: {e}")),
            }
        }
        // 词形漂移：控制器词形已指明总线族，该族派生键必须被角色声明（否则派生视图
        // 会漏键——roles.derives 与 pin 实况各说一套）
        for (prefix, key) in [
            ("pwm", "pwm_chip"),
            ("gpiochip", "gpio_chip"),
            ("i2c", "i2c_bus"),
        ] {
            if rr3_controller_wordform(&pin.controller, prefix).is_some()
                && !derives.iter().any(|d| d == key)
            {
                errs.push(format!(
                    "{label} controller 词形 {} 指明 {key}，但角色 {} derives 未声明（派生漂移）",
                    pin.controller, pin.role
                ));
            }
        }
    }
    // 门 n（§8 注入路径唯一，power P-13）：同一 handle(trait, key) 的取值只能有一条
    // 来源路径。[hardware] 出现物理键一律红——pin 实际派生该键时点名两份取值（两处
    // 并存判据），否则按稳态判据点名；check.sh 拟 ⑨-n 的静态扫面与本条互为冗余。
    for k in RR3_PIN_DERIVED_KEYS {
        if let Some(hv) = profile.hardware.get(k) {
            match derived.get(k) {
                Some(dv) => errs.push(format!(
                    "hardware.{k} = \"{hv}\" 与 [pins] 派生值 \"{dv}\" 两处并存（门 n：注入路径唯一，[pins] 派生或 [hardware] 直给二者取一）"
                )),
                None => errs.push(format!(
                    "hardware.{k} 是引脚侧派生键（schema 3 起唯一来源 = [pins] 派生，[hardware] 不得直给——门 n 稳态判据）"
                )),
            }
        }
    }
    // 装载期校验⑤-a/⑤-b/⑧-b（增补轮六 §5，后果 = 拒；Task 4）：方向/能力/词表
    // 三面接平台表——判据真值只有 contracts/platform/<chip>.toml 一册，不推断、
    // 不运行期回退；表在上方已 fail-closed 装载，此处只判。
    errs.extend(rr3_check_pin_capabilities(&profile, &roles, &platform));
    // 装载期校验③-a/③-b/③-c + ④-b（增补轮六 §5，后果 = 拒；Task 5）：三个资源
    // 空间各自不双占 + 平台 conflicts 反推互斥（组名取自平台表，漏声明即红）。
    errs.extend(rr3_check_pin_occupancy(&profile, &platform));
    // 装载期校验⑥（增补轮六 §5，后果 = 拒）：有轨、轨在册、绑定归一。
    // ⑥-a pins.rail ∈ supplies.domain.names；⑥-b kind=power 角色必有在册轨
    // （"none" 只对 signal 侧控制线合法）；⑥-c armed=true 必有测点且龄期上界 > 0
    // （无龄期快照不得充当拒动依据，§19 Y-4）；⑥-d 域的三个绑定引用位写 pin_id
    // 且能在 [pins] 取到（F-2：第二套语法一条校验都打不到）。
    let domain_names: BTreeSet<String> = profile
        .supplies
        .domains
        .iter()
        .map(|d| d.name.clone())
        .collect();
    for pin in &profile.pins {
        // rail 缺省的合法性与角色 kind 挂钩：power 面（⑥-b）必须有在册轨；
        // signal 面（meter 类，requires 无 rail）不写 rail 合法——写了非 none 的
        // 名字则一律查在册（⑥-a）。"none" 是 signal 侧控制线的显式不适值。
        let kind = roles.rr3_role_table_kind(&pin.role);
        let rail_unlisted = pin.rail.is_empty() || pin.rail == "none";
        if rail_unlisted {
            if kind.as_deref() == Some("power") {
                errs.push(format!(
                    "pins[{}] rail {} 但角色 {} kind = power（校验⑥-b：功率面必须挂真实供电轨，缺 rail 或 none 即红）",
                    pin.rr3_pin_id(),
                    if pin.rail.is_empty() { "缺失" } else { "= none" },
                    pin.role
                ));
            }
        } else if !domain_names.contains(pin.rail.as_str()) {
            errs.push(format!(
                "pins[{}] rail = {} 未在 [[supplies.domain]] 在册（校验⑥-a；域名单源在该形态的域声明，§18 F-5）",
                pin.rr3_pin_id(),
                pin.rail
            ));
        }
    }
    for d in &profile.supplies.domains {
        let label = format!("supplies.domains[{}]", d.name);
        if d.armed && (d.sense == "none" || d.max_age_ms <= 0) {
            errs.push(format!(
                "{label} armed = true 但 sense = none 或 max_age_ms ≤ 0（校验⑥-c：无测点/无龄期上界的域不得武装拒动——未标定即 armed=false，§3.1 降级语义）"
            ));
        }
        for (key, val) in [
            ("sense", &d.sense),
            ("current_sense", &d.current_sense),
            ("switch", &d.switch),
        ] {
            if val == "none" {
                continue;
            }
            if !profile.pins.iter().any(|p| p.rr3_pin_id() == *val) {
                errs.push(format!(
                    "{label}.{key} = {val} 不在 [pins] 的 pin_id 集（校验⑥-d：绑定位一律写 pin_id，\"none\" 是合法显式值——第二套语法如 adc:gp34 一律红，F-2）"
                ));
            }
        }
        if d.switchable && d.switch == "none" {
            errs.push(format!(
                "{label} switchable = true 但 switch = none（可切域必有开关控制位，两键成对）"
            ));
        }
    }
    // 装载期校验⑦（增补轮六 §5，后果 = 拒）：接线闭合与额定加总。⑦-a wire.signal
    // ⊆ pin_id ∪ {vcc,gnd}；⑦-b wire.net ⊆ domains；⑦-d wire.mate ∈ devices.toml
    // 的 mate 集；闭合面（feeds 位号集）在接线档装载时已判。⑦-c（rated_a × 1000 ≥
    // Σ(feeds 闭包内 peak_ma) × 裕量）的判据对象是件 demands 申报（n1-7 才立数据
    // 面），批N2 同链补——本批不预支。
    let pin_ids: BTreeSet<String> = profile.pins.iter().map(|p| p.rr3_pin_id()).collect();
    let mate_set = devices.rr3_device_table_mates();
    errs.extend(wiring.rr3_wiring_check(&pin_ids, &domain_names, &mate_set));
    if !errs.is_empty() {
        return Err(errs.join("; "));
    }
    // 装载期校验②（增补轮六 §5，后果 = 降不拒）：present ∩ enabled_slots 的每个格位，
    // roles 表归入它的角色必须都有 [[pin]] 绑定——缺角色即缺能力。(格位, 角色) 进
    // role_missing 清单随 boot 带出，由 daemon 点名（Degraded 可见面归批N4 I-15）。
    let mut role_missing: Vec<(String, String)> = Vec::new();
    let enabled_all: BTreeSet<String> = profile.enabled_slots.values().flatten().cloned().collect();
    for slot in &enabled_all {
        if !profile.slots_present.contains(&slot) {
            continue;
        }
        for role in roles.rr3_role_table_names_for_slot(&slot) {
            if !profile.pins.iter().any(|p| p.role == role) {
                role_missing.push((slot.clone(), role));
            }
        }
    }
    role_missing.sort();

    // 缺格判定：enabled_slots 声明的非基础层格位没有指向本模块的有效 binding（或探测不到）。
    // 基础层恒在位，跳过（slots.toml 注记：net.online / power.battery boot 即在位、恒不降级）。
    let mut missing: Vec<(String, String)> = Vec::new();
    // L-14 器件级探测的 (bus, addr) 组装：格位 → 该格角色在 [pins] 的 I2C 绑定
    // （controller 词形 i2c<N> → N，addr 非 none 非空才有效）。装配单未声明 [pins]
    // / 该格无 I2C 绑定 → 传 None → 探针回落总线级五面判据（无 [pins] 形态兼容面）。
    let mut i2c_bind: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for slot in &enabled_all {
        for role in roles.rr3_role_table_names_for_slot(&slot) {
            for pin in profile.pins.iter().filter(|p| p.role == role) {
                let Some(n) = pin
                    .controller
                    .strip_prefix("i2c")
                    .and_then(|rest| rest.parse::<u32>().ok())
                else {
                    continue;
                };
                if !pin.addr.is_empty() && pin.addr != "none" {
                    i2c_bind
                        .entry(slot.clone())
                        .or_insert_with(|| (n, pin.addr.clone()));
                }
            }
        }
    }
    // 探测先行：bindings 声明的格位逐格探测（MC-4：linux 走五面真值探测，非 linux
    // 生产面恒缺，test 面/显式注入走注入件）。探测自身故障按"探测不到"处理
    // （fail-closed，方案 §4）；缺格三源合一的"实测缺"这一源即出自本环。
    for (slot, module) in &profile.bindings {
        let (bus, addr) = match i2c_bind.get(slot) {
            Some((n, a)) => (Some(*n), Some(a.as_str())),
            None => (None, None),
        };
        if !probe.rr3_probe_slot_bound(slot, bus, addr) {
            missing.push((module.clone(), slot.clone()));
        }
    }
    for (module, slots) in &profile.enabled_slots {
        for slot in slots {
            let base = table.rr3_slot_table_layer(slot).as_deref() == Some("base");
            if base {
                continue;
            }
            // 缺格三源合一第一源（方案 §15）：本板声明缺（[slots].present 不含）→ 缺格，
            // 不再看 binding 与探测——板说没有，装配单绑了也白绑。
            if !profile.slots_present.contains(slot) {
                if !missing.contains(&(module.clone(), slot.clone())) {
                    missing.push((module.clone(), slot.clone()));
                }
                continue;
            }
            if profile.bindings.get(slot) != Some(module)
                && !missing.contains(&(module.clone(), slot.clone()))
            {
                missing.push((module.clone(), slot.clone()));
            }
        }
    }
    missing.sort();

    // manifest 解析：modules-root = 装配单路径的上两级（<root>/profiles/<机>.toml）。
    let root = abs.parent().and_then(Path::parent).ok_or_else(|| {
        format!(
            "装配单 {} 无法定位 modules-root（须在 <root>/profiles/ 下）",
            abs.display()
        )
    })?;
    let manifests: Vec<PathBuf> = profile
        .enabled_slots
        .keys()
        .map(|m| root.join("modules").join(m).join("module.toml"))
        .collect();
    // hal_bindings（spawn 时注入 RR3_HAL_BINDINGS）：host 档 = [pins] 派生视图 ∪
    // [hardware] 三非引脚键（键名不变，rr-hal descriptor.rs 解析端零改动）；rtos/bare
    // 档不派生，[hardware] 原样直给。门 n 已保证物理键不双占，合并无覆盖歧义。
    let mut merged = derived;
    for (k, v) in &profile.hardware {
        merged.insert(k.clone(), v.clone());
    }
    let hal_bindings = rr3_hardware_bindings_text(&merged);

    // G4 装配段校验（ADR-0019 修订 1 ③）：enabled_slots 的每个模块扫
    // atoms/*/assemble.toml——缺文件/坏形/ops 注入缺逐条点名，该模块 boot 入
    // Degraded（不 fatal；修数据 + 重启是唯一解除路）。
    let mut assemble_broken: Vec<(String, String)> = Vec::new();
    for m in profile.enabled_slots.keys() {
        for msg in rr3_assemble_scan(&root.join("modules").join(m)) {
            assemble_broken.push((m.clone(), msg));
        }
    }

    Ok(Rr3ProfileBoot {
        manifests,
        missing,
        hal_bindings,
        assemble_broken,
        role_missing,
    })
}

/// 装载期校验⑤-a/⑤-b/⑧-b（增补轮六 §5，后果 = 拒；施工图 Task 4）：方向与能力
/// 合法、控制器在册，判据真值 = 平台事实表（`contracts/platform/<chip>.toml`，
/// 基-22）。三条判定逐条可判：
/// - ⑧-b `pin.controller ∈ 平台在册控制器词表`（词表不是器件型号，D10a 同族；
///   controller 留空的缺失语义归校验①-b requires 判，此处不重复点名）；
/// - ⑤-a `direction = out` 且角色 `input_only_ok = false` → `physical` 不得落平台
///   input-only 脚集（input-only 脚无输出驱动能力，写 out 即假账）；
/// - ⑤-b 模拟测点必须落平台 ADC 能力集——测点 = 域 `sense`/`current_sense` 按
///   pin_id 引用的条目（⑥-d 的同一外键语法），其 `physical` 元素逐个对 ADC 集；
///   I2C 测点无直接脚（INA219 在片上量），`physical` 空即无判据对象，不算红。
fn rr3_check_pin_capabilities(
    profile: &Rr3Profile,
    roles: &Rr3RoleTable,
    platform: &Rr3PlatformTable,
) -> Vec<String> {
    let mut errs = Vec::new();
    let plat = platform.rr3_platform_name();
    for pin in &profile.pins {
        let label = format!("pins[{}]", pin.rr3_pin_id());
        // ⑧-b 控制器在册（词表随平台表，装配单不得自造控制器）
        if !pin.controller.is_empty()
            && !platform
                .rr3_platform_controller_names()
                .iter()
                .any(|c| c == &pin.controller)
        {
            errs.push(format!(
                "{label} controller \"{}\" 不在平台 {plat} 的在册控制器词表（校验⑧-b——词表源 contracts/platform/{plat}.toml 的 [[controller]]，装配单不得自造控制器）",
                pin.controller
            ));
        }
        // ⑤-a output 落 input-only 集（角色 input_only_ok 的另一侧输入取 roles 表）
        if pin.direction == "out" && roles.rr3_role_table_input_only_ok(&pin.role) == Some(false) {
            for p in &pin.physical {
                if platform.rr3_platform_input_only_pins().contains(p) {
                    errs.push(format!(
                        "{label} physical 元素 {p} 落在平台 {plat} 的 input-only 脚集（校验⑤-a：direction = out 且角色 {} input_only_ok = false，input-only 脚无输出驱动能力）",
                        pin.role
                    ));
                }
            }
        }
    }
    // ⑤-b 模拟测点落 ADC 能力集（测点引用面 = 域绑定位，与 ⑥-d 同一 pin_id 语法）
    for d in &profile.supplies.domains {
        for (key, val) in [("sense", &d.sense), ("current_sense", &d.current_sense)] {
            if val == "none" {
                continue;
            }
            let Some(pin) = profile.pins.iter().find(|p| &p.rr3_pin_id() == val) else {
                continue; // 悬空 pin_id 由 ⑥-d 点名，此处不重复报
            };
            for p in &pin.physical {
                if !platform.rr3_platform_adc_pins().contains(p) {
                    errs.push(format!(
                        "pins[{}] 是域 {} 的模拟测点（{key} 绑定），physical 元素 {p} 不在平台 {plat} 的 ADC 能力集（校验⑤-b：测点落非 ADC 脚读回恒值，档位恒真或恒假——F-9）",
                        pin.rr3_pin_id(),
                        d.name
                    ));
                }
            }
        }
    }
    errs
}

/// 装载期校验③-a/③-b/③-c + ④-b（增补轮六 §5，后果 = 拒；施工图 Task 5）：不双占
/// 与互斥。三个资源空间各自 ≤ 1 次（启用集 = [pins] 全集——[[pin]] 无使能位，
/// 在册即启用）：
/// - ③-a 同一 `physical` 元素 ≤ 1 次；
/// - ③-b 同一 `(controller, channel)` ≤ 1 次（channel 缺席不占通道维，F-7：脚 +
///   通道两维，`board.rs:146` 双轴两通道即证）；
/// - ③-c 同一 `(bus, addr)` ≤ 1 次（addr = "none" 是显式不适值，不占地址维）；
/// - ④-b 平台 `conflicts` 命中同一子集而未申报同组 ⇒ 漏声明即红——组名取自平台表
///   `[[conflict]].group`，装机档不得自起名（否则 `mux_group` 退回自愿填写）；命中
///   是单向含入：构造/冲突表按芯片事实收全，profile 现役绑定只是子集，平台表超出
///   现役的部分不算红（芯片事实 ≠ 现役占用）。
fn rr3_check_pin_occupancy(profile: &Rr3Profile, platform: &Rr3PlatformTable) -> Vec<String> {
    let mut errs = Vec::new();
    let plat = platform.rr3_platform_name();
    // ③-a physical 元素唯一（BTreeMap 定化点名序：首占者记录）。owner 值用
    // pin 序号判"同一条"，不用 pin_id 字符串——per_instance=false 角色的多条
    // pin 无 instance 可分，pin_id 同名会把双占误判成"自己撞自己"而吞红。
    let mut phys_owner: BTreeMap<i64, usize> = BTreeMap::new();
    // ③-b (controller, channel) 唯一
    let mut chan_owner: BTreeMap<(String, i64), usize> = BTreeMap::new();
    // ③-c (bus, addr) 唯一
    let mut addr_owner: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (idx, pin) in profile.pins.iter().enumerate() {
        let label = format!("pins[{}]", pin.rr3_pin_id());
        for p in &pin.physical {
            match phys_owner.get(p) {
                Some(first) if *first != idx => errs.push(format!(
                    "{label} physical 元素 {p} 与 pins[{}] 双占（校验③-a：同一 physical 元素在启用集里 ≤ 1 次）",
                    profile.pins[*first].rr3_pin_id()
                )),
                None => {
                    phys_owner.insert(*p, idx);
                }
                _ => {}
            }
        }
        if let (true, Some(ch)) = (!pin.controller.is_empty(), pin.channel) {
            let key = (pin.controller.clone(), ch);
            match chan_owner.get(&key) {
                Some(first) if *first != idx => errs.push(format!(
                    "{label} (controller {}, channel {ch}) 与 pins[{}] 双占（校验③-b：同一 (controller, channel) ≤ 1 次——F-7 脚+通道两维）",
                    pin.controller,
                    profile.pins[*first].rr3_pin_id()
                )),
                None => {
                    chan_owner.insert(key, idx);
                }
                _ => {}
            }
        }
        if !pin.controller.is_empty() && !pin.addr.is_empty() && pin.addr != "none" {
            let key = (pin.controller.clone(), pin.addr.clone());
            match addr_owner.get(&key) {
                Some(first) if *first != idx => errs.push(format!(
                    "{label} (bus {}, addr {}) 与 pins[{}] 双占（校验③-c：同一 (bus, addr) ≤ 1 次）",
                    pin.controller,
                    pin.addr,
                    profile.pins[*first].rr3_pin_id()
                )),
                None => {
                    addr_owner.insert(key, idx);
                }
                _ => {}
            }
        }
    }
    // ④-b conflicts 反推互斥：同一条冲突登记面的命中者 ≥ 2 且任一未申报该组 → 红
    for entry in platform.rr3_platform_conflicts() {
        let group = entry.rr3_conflict_group();
        let hits: Vec<&Rr3PinEntry> = profile
            .pins
            .iter()
            .filter(|p| {
                p.physical.iter().any(|x| entry.rr3_conflict_pins().contains(x))
                    || (!p.controller.is_empty()
                        && entry.rr3_conflict_controllers().contains(&p.controller))
            })
            .collect();
        if hits.len() < 2 {
            continue; // 冲突未成形：另一成员不在现役绑定集（单向含入语义）
        }
        for p in hits {
            if !p.mux_group.iter().any(|g| g == group) {
                errs.push(format!(
                    "pins[{}] 命中平台 {plat} conflicts 组 {group}（{}）但 mux_group 未申报该组（校验④-b：漏声明即红——组名取自平台表，装机档不得自起名）",
                    p.rr3_pin_id(),
                    entry.rr3_conflict_why()
                ));
            }
        }
    }
    errs
}

/// 扫描单个模块目录的装配清单（G4/ADR-0019 修订 1 ③）：`atoms/<件>/` 必有
/// assemble.toml（五件套④），形状校验走 rr-registry 共用真值；ops 非空时对模块
/// config.toml 做注入缺点路径游走。无 atoms/ 目录的模块（夹具树/两件式形态）返回空。
fn rr3_assemble_scan(module_dir: &Path) -> Vec<String> {
    let atoms_dir = module_dir.join("atoms");
    let Ok(entries) = std::fs::read_dir(&atoms_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    names.sort();
    let mut out = Vec::new();
    for atom in names {
        let path = atoms_dir.join(&atom).join("assemble.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => {
                out.push(format!("件 {atom} 缺 assemble.toml（五件套④，装配段点名）"));
                continue;
            }
        };
        let facts = match rr_registry::rr3_registry_assemble_from_text(&text) {
            Ok(f) => f,
            Err(errs) => {
                for e in errs {
                    out.push(format!("件 {atom} 装配清单坏形: {e}"));
                }
                continue;
            }
        };
        if facts.ops.is_empty() {
            continue;
        }
        // 注入缺：ops 声明的参数槽必须能从模块 config.toml 按点路径取到
        match std::fs::read_to_string(module_dir.join("config.toml")) {
            Ok(cfg) => {
                for gap in rr_registry::rr3_registry_assemble_injection_gaps(&facts, &cfg) {
                    out.push(format!("件 {atom} {gap}"));
                }
            }
            Err(e) => out.push(format!(
                "件 {atom} 参数槽来源不可证：config.toml 读不到 ({e})"
            )),
        }
    }
    out
}

/// 校验⑨ 最小读者（批N2 首件基-36，全案 §5 判据⑨）：`Σ demands.peak_ma
/// (critical=true) ≤ domains[rail].current_cap_ma`。fail-closed 逐条点名：
/// - demand.rail 不在 [[supplies.domain]] ⇒ 红（域缺 = `current_cap_ma` 缺，预算无分母）；
/// - per rail Σ(critical=true) 超域 cap ⇒ 红点名（域 + 加总值 + cap + 申报者清单）。
/// critical=false 不进 Σ（判据原文只算 critical=true）；并发缺省 = 全部同时（最坏），
/// `concurrency_group` 收窄随门 c（批N2 本体）同批，本读者不预支。
/// **基-36 只落判据实体（本函数）＋反例用例（tests ①b）**：装载拒链（专属执法位
/// ⑨-c / 门 c）仍待建，待建门计数与 check.sh step 行数不变——接链随批N2 本体同批。
fn rr3_demand_budget_check(
    declared: &[(String, Vec<Rr3AssembleDemand>)],
    domains: &[Rr3Domain],
) -> Vec<String> {
    let mut errs = Vec::new();
    // 域在册性先行：rail 不在 domains ⇒ 该条需求无分母可比，逐条点名
    for (owner, demands) in declared {
        for d in demands {
            if !domains.iter().any(|dm| dm.name == d.rail) {
                errs.push(format!(
                    "件 {owner} 申报 rail = {} 不在 [[supplies.domain]] 在册域集（校验⑨：域缺即 current_cap_ma 缺，预算无分母）",
                    d.rail
                ));
            }
        }
    }
    // per rail 预算加总（并发缺省 = 全部同时，最坏口径）
    for dm in domains {
        let sum: i64 = declared
            .iter()
            .flat_map(|(_, demands)| demands.iter())
            .filter(|d| d.critical && d.rail == dm.name)
            .map(|d| d.peak_ma)
            .sum();
        if sum > dm.current_cap_ma {
            let owners: Vec<&str> = declared
                .iter()
                .filter(|(_, demands)| {
                    demands
                        .iter()
                        .any(|d| d.critical && d.rail == dm.name)
                })
                .map(|(o, _)| o.as_str())
                .collect();
            errs.push(format!(
                "域 {} 电流预算超限（校验⑨）：Σ demands.peak_ma (critical=true) = {sum} mA > current_cap_ma = {} mA（申报者：{}；并发缺省 = 全部同时）",
                dm.name,
                dm.current_cap_ma,
                owners.join("、")
            ));
        }
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 校验⑨ 最小读者反例组（批N2 首件基-36，判据实体进 ①b）：正例（servo 首条
    /// 真实申报形状 Σ700 ≤ cap 2000 ⇒ 绿）＋三条反例——同域加总超 cap ⇒ 红、
    /// rail 不在 domains ⇒ 红、critical=false 不进 Σ。装载拒链（⑨-c / 门 c）仍
    /// 待建：本测试只钉判据本体，step 行数与待建门计数不变。
    #[test]
    fn demand_budget_check_nine() {
        let rail5v = || Rr3Domain {
            name: "rail5v".into(),
            current_cap_ma: 2000,
            ..Default::default()
        };
        let d = |rail: &str, peak: i64, critical: bool| Rr3AssembleDemand {
            rail: rail.into(),
            peak_ma: peak,
            critical,
        };

        // 正例：servo 首条申报形状（rail5v peak 700 critical）⇒ 无红
        let ok = vec![("rr3-act.servo".to_string(), vec![d("rail5v", 700, true)])];
        assert!(
            rr3_demand_budget_check(&ok, &[rail5v()]).is_empty(),
            "Σ700 ≤ cap 2000 必须绿"
        );

        // 反例①：两件同域加总超 cap（Σ2400 > 2000）⇒ 红点名域 + 加总 + cap + 申报者
        let over = vec![
            ("rr3-act.servo".to_string(), vec![d("rail5v", 700, true)]),
            ("rr3-x.dummy".to_string(), vec![d("rail5v", 1700, true)]),
        ];
        let errs = rr3_demand_budget_check(&over, &[rail5v()]);
        assert!(
            errs.iter().any(|e| e.contains("电流预算超限")
                && e.contains("rail5v")
                && e.contains("2400")
                && e.contains("2000")
                && e.contains("rr3-act.servo")
                && e.contains("rr3-x.dummy")),
            "{errs:?}"
        );

        // 反例②：rail 不在 domains ⇒ 红（域缺即 cap 缺，预算无分母）
        let ghost = vec![("rr3-x.dummy".to_string(), vec![d("rail9v", 1, true)])];
        let errs = rr3_demand_budget_check(&ghost, &[rail5v()]);
        assert!(
            errs.iter()
                .any(|e| e.contains("rail9v") && e.contains("不在 [[supplies.domain]]")),
            "{errs:?}"
        );

        // 反例③：critical=false 不进 Σ（判据只算 critical=true）——5000 mA 也绿
        let noncrit = vec![(
            "rr3-x.dummy".to_string(),
            vec![d("rail5v", 5000, false)],
        )];
        assert!(
            rr3_demand_budget_check(&noncrit, &[rail5v()]).is_empty(),
            "critical=false 不得进加总"
        );
    }

    /// 全部规则走真的 contracts/slots.toml 与 contracts/roles.toml（查册真值同源），
    /// 夹具落 pid 唯一临时目录
    struct Root {
        dir: PathBuf,
    }

    const GOOD: &str = r#"
schema_version = 3

[os]
route = "distro"

[platform]
tier = "host"
chip = "rk3566"

[supplies]
watchdog_present = true
watchdog_timeout_ms = 15000
watchdog_feeder = "rrd"
tick_period_ms = 200
tick_jitter_ms = 100
safety_core_interval_ms = 1000

[[supplies.domain]]
name = "batt"
nominal_mv = 3700
mild_mv = 3500
severe_mv = 3300
current_cap_ma = 2000
sense = "none"
current_sense = "none"
switch = "none"
switchable = false
armed = false
max_age_ms = 0

[[supplies.domain]]
name = "rail5v"
nominal_mv = 5000
mild_mv = 4600
severe_mv = 4400
current_cap_ma = 2000
sense = "none"
current_sense = "none"
switch = "none"
switchable = false
armed = false
max_age_ms = 0

[bindings]
"act.servo" = "rr3-pa"
"sensor.env" = "rr3-pb"
"cognition.llm" = "rr3-pc"

[enabled_slots]
rr3-pa = ["act.servo"]
rr3-pb = ["sensor.env"]
rr3-pc = ["cognition.llm"]

[slots]
present = ["act.servo", "cognition.llm", "sensor.env"]

[[pin]]
role = "act.axis.pwm"
instance = "pan"
controller = "pwm0"
channel = 0
physical = [12]
direction = "out"
rail = "rail5v"
failsafe = "release"
mux_group = []
"#;

    /// 接线档底稿（§4.3 九字段，与 wiring.rs 测试区 GOOD 同形状）：单线组只挂
    /// vcc/gnd 与电力导体（net=rail5v 在 GOOD 两域在册）——对 [[pin]] 增删免疫，
    /// 删 pin 的用例不因 ⑦-a 误红。校验⑦ 反例用 `write_wiring` 覆写。
    const GOOD_WIRING: &str = r#"
[[wire]]
connector = "J-T"
hdr_pin = [1, 2]
signal = ["vcc", "gnd"]
net = "rail5v"
feeds = []
mate = "none"
keyed = false
rated_a = 1.0
gnd_common = "star-1"
"#;

    impl Root {
        fn new(tag: &str) -> Root {
            let dir =
                std::env::temp_dir().join(format!("rr3-profile-{}-{tag}", std::process::id()));
            std::fs::create_dir_all(dir.join("profiles")).expect("建 profiles 目录");
            Root { dir }
        }

        /// 落装配单并同批 stage 接线档（`hardware/<形态>/wiring.toml`，与
        /// [`crate::wiring::rr3_wiring_path_for_profile`] 推导同一布局）——装载期
        /// 校验⑦要求装机档必在，缺省给 GOOD_WIRING，反例随后覆写。
        fn write_profile(&self, name: &str, text: &str) -> PathBuf {
            let p = self.dir.join("profiles").join(name);
            std::fs::write(&p, text).expect("写装配单");
            self.write_wiring(&p, GOOD_WIRING);
            p
        }

        /// 落一份接线档到 `<root>/hardware/<装配单 stem>/wiring.toml`。
        fn write_wiring(&self, profile: &Path, text: &str) {
            let stem = profile
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .expect("夹具装配单须有文件名");
            let w = self.dir.join("hardware").join(stem).join("wiring.toml");
            std::fs::create_dir_all(w.parent().expect("接线档必有父目录"))
                .expect("建 hardware 目录");
            std::fs::write(&w, text).expect("写接线档");
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// 测试本地帮手：真契约三册（slots/roles/devices）+ 由装配单路径推导的接线档
    /// （`hardware/<形态>/wiring.toml`）——与 daemon `--profile` 正门同一条推导路，
    /// 顺带把路径推导钉进每条用例。平台表目录同样走编译期真值（rr-registry 常量，
    /// 与真仓 contracts/platform 同一册——夹具不复制契约，真值同源纪律）。
    fn boot_inputs(p: &Path) -> Result<Rr3ProfileBoot, String> {
        let wiring =
            crate::wiring::rr3_wiring_path_for_profile(p).expect("夹具装配单必须能推导接线档");
        rr3_profile_boot_inputs(
            p,
            Path::new(RR3_CONTRACT_SLOTS_PATH),
            Path::new(RR3_CONTRACT_ROLES_PATH),
            Path::new(RR3_CONTRACT_DEVICES_PATH),
            &wiring,
            Path::new(rr_registry::RR3_CONTRACT_PLATFORM_PATH),
        )
    }

    /// 同 [`boot_inputs`]，探测件注入版（MC-4 门禁用例）。
    fn boot_inputs_with_probe(
        p: &Path,
        probe: &dyn crate::probe::Rr3Probe,
    ) -> Result<Rr3ProfileBoot, String> {
        let wiring =
            crate::wiring::rr3_wiring_path_for_profile(p).expect("夹具装配单必须能推导接线档");
        rr3_profile_boot_inputs_with_probe(
            p,
            Path::new(RR3_CONTRACT_SLOTS_PATH),
            Path::new(RR3_CONTRACT_ROLES_PATH),
            Path::new(RR3_CONTRACT_DEVICES_PATH),
            &wiring,
            Path::new(rr_registry::RR3_CONTRACT_PLATFORM_PATH),
            probe,
        )
    }

    /// 平台事实表真值点（校验③④⑤⑧的判据前件，esp32 真形态实例——与
    /// rr-registry 平台单测同一 include 路径，真值同源不复制）。
    fn platform_esp32_text() -> &'static str {
        include_str!("../../../../contracts/platform/esp32.toml")
    }

    /// 平台文本注入版帮手（施工图 Task 4 Interfaces）：platform 文本落 Root 临时
    /// 目录的 `platform/<chip>.toml`——与 daemon 的 contracts/platform/ 选册同布局，
    /// chip 固定 esp32（本组用例的夹具装配单都声明 chip = "esp32"）。
    fn rr3_profile_from_text_with_platform(
        tag: &str,
        profile_text: &str,
        platform_text: &str,
    ) -> Result<Rr3ProfileBoot, String> {
        let root = Root::new(tag);
        let p = root.write_profile("plat.toml", profile_text);
        let dir = root.dir.join("platform");
        std::fs::create_dir_all(&dir).expect("建 platform 目录");
        std::fs::write(dir.join("esp32.toml"), platform_text).expect("写平台表");
        boot_inputs_in(&root, &p, &dir)
    }

    /// [`boot_inputs`] 的平台目录显式版（上面两个帮手的共同实现）。
    fn boot_inputs_in(root: &Root, p: &Path, platform_dir: &Path) -> Result<Rr3ProfileBoot, String> {
        let _ = root; // Root 只为临时目录的生命周期在场（Drop 清树）
        let wiring =
            crate::wiring::rr3_wiring_path_for_profile(p).expect("夹具装配单必须能推导接线档");
        rr3_profile_boot_inputs(
            p,
            Path::new(RR3_CONTRACT_SLOTS_PATH),
            Path::new(RR3_CONTRACT_ROLES_PATH),
            Path::new(RR3_CONTRACT_DEVICES_PATH),
            &wiring,
            platform_dir,
        )
    }

    /// 校验⑤⑧判据的 esp32 夹具底稿（rtos 档真形态）：LEDC 轴 + ADC 直读电表——
    /// 电表 pin（controller = adc1、physical = [34]）是 batt 域的模拟测点
    /// （sense 按 pin_id 引用，⑥-d 同一语法），34 ∈ esp32 adc_pins ⇒ 全绿基线；
    /// 反例经定点替换构造。rtos 档不派生（host 档派生器不识别板侧词形）。
    const ESP32_GOOD: &str = r#"
schema_version = 3

[platform]
tier = "rtos"
chip = "esp32"

[supplies]
watchdog_present = true
watchdog_timeout_ms = 15000
watchdog_feeder = "rr-core"
tick_period_ms = 200
tick_jitter_ms = 100
safety_core_interval_ms = 1000

[[supplies.domain]]
name = "batt"
nominal_mv = 3700
mild_mv = 3500
severe_mv = 3300
current_cap_ma = 2000
sense = "power.battery.meter"
current_sense = "none"
switch = "none"
switchable = false
armed = false
max_age_ms = 0

[[supplies.domain]]
name = "rail5v"
nominal_mv = 5000
mild_mv = 4600
severe_mv = 4400
current_cap_ma = 2000
sense = "none"
current_sense = "none"
switch = "none"
switchable = false
armed = false
max_age_ms = 0

[bindings]
"act.servo" = "rr3-pa"
"sensor.env" = "rr3-pb"
"cognition.llm" = "rr3-pc"

[enabled_slots]
rr3-pa = ["act.servo"]
rr3-pb = ["sensor.env"]
rr3-pc = ["cognition.llm"]

[slots]
present = ["act.servo", "cognition.llm", "sensor.env"]

[[pin]]
role = "act.axis.pwm"
instance = "pan"
controller = "ledc0"
channel = 0
physical = [32]
direction = "out"
rail = "rail5v"
failsafe = "release"
mux_group = []

[[pin]]
role = "power.battery.meter"
controller = "adc1"
addr = "none"
physical = [34]
direction = "in"
mux_group = []
"#;

    /// G4 装配段扫描直测（ADR-0019 修订 1 ③）：缺文件/坏形/注入缺逐条点名，
    /// 无 atoms 目录与合法空数组不产生缺口
    #[test]
    fn assemble_scan_names_missing_broken_and_injection_gaps() {
        let root = Root::new("assemble-scan");
        // 无 atoms/ 目录 → 空（夹具树/两件式形态）
        assert!(rr3_assemble_scan(&root.dir.join("no-atoms")).is_empty());

        // 合法空声明（v2，明写不需注入面）→ 空
        let ok = root.dir.join("ok");
        std::fs::create_dir_all(ok.join("atoms/led")).unwrap();
        std::fs::write(
            ok.join("atoms/led/assemble.toml"),
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = true\n",
        )
        .unwrap();
        assert!(rr3_assemble_scan(&ok).is_empty(), "合法空声明不得报缺");

        // 缺文件 / 坏形 / 注入缺 三路点名，互不遮蔽
        let bad = root.dir.join("bad");
        std::fs::create_dir_all(bad.join("atoms/nofile")).unwrap();
        std::fs::create_dir_all(bad.join("atoms/broken")).unwrap();
        // 只缺 ops 一键 → 恰一条点名（schema=1 那种坏法会连出两条，不便数总数）
        std::fs::write(
            bad.join("atoms/broken/assemble.toml"),
            "schema_version = 2\n\n[assemble]\nhandles = []\n",
        )
        .unwrap();
        std::fs::create_dir_all(bad.join("atoms/gap")).unwrap();
        std::fs::write(
            bad.join("atoms/gap/assemble.toml"),
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = [ { slot = \"s\", src_key = \"x.y.z\" } ]\n",
        )
        .unwrap();
        std::fs::write(bad.join("config.toml"), "[x.y]\nw = 1\n").unwrap();
        let out = rr3_assemble_scan(&bad);
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(
            out.iter()
                .any(|m| m.contains("nofile") && m.contains("缺 assemble.toml")),
            "{out:?}"
        );
        assert!(
            out.iter()
                .any(|m| m.contains("broken") && m.contains("坏形")),
            "{out:?}"
        );
        assert!(
            out.iter()
                .any(|m| m.contains("gap") && m.contains("注入缺") && m.contains("x.y.z")),
            "{out:?}"
        );
    }

    #[test]
    fn good_profile_parses() {
        let p = rr3_profile_from_text(GOOD).expect("应能解析");
        assert_eq!(p.bindings.len(), 3);
        assert_eq!(p.enabled_slots.len(), 3);
        assert_eq!(p.enabled_slots["rr3-pa"], vec!["act.servo".to_string()]);
        // 描述符八段（schema_version=3）：[platform]/[supplies]/[slots]/[[pin]] 逐字段收齐
        assert_eq!(p.platform_tier, "host");
        // 平台事实表外键（§4.6，校验③④⑤⑧判据前件选册）
        assert_eq!(p.platform_chip, "rk3566");
        assert!(p.supplies.watchdog_present);
        assert_eq!(p.supplies.watchdog_timeout_ms, 15000);
        assert_eq!(p.supplies.watchdog_feeder, "rrd");
        assert_eq!(p.supplies.tick_period_ms, 200);
        assert_eq!(p.supplies.tick_jitter_ms, 100);
        assert_eq!(p.supplies.safety_core_interval_ms, 1000);
        assert_eq!(
            p.slots_present,
            ["act.servo", "cognition.llm", "sensor.env"]
        );
        // [[pin]]：十键逐字段收齐，pin_id 按 per_instance 语法拼装
        assert_eq!(p.pins.len(), 1, "{:?}", p.pins);
        let pin = &p.pins[0];
        assert_eq!(pin.rr3_pin_id(), "act.axis.pwm.pan");
        assert_eq!(pin.controller, "pwm0");
        assert_eq!(pin.channel, Some(0));
        assert_eq!(pin.physical, vec![12]);
        assert_eq!(pin.direction, "out");
        assert_eq!(pin.rail, "rail5v");
        assert_eq!(pin.failsafe, "release");
        assert!(pin.mux_group.is_empty());
        // [os].route 被无视（运行期不读），但它的存在不构成错误
    }

    #[test]
    fn parse_errors_are_all_collected_and_named() {
        let text = GOOD
            .replace("schema_version = 3", "schema_version = 9")
            .replace("\"act.servo\" = \"rr3-pa\"", "\"act.servo\" = \"\"")
            .replace("rr3-pb = [\"sensor.env\"]", "rr3-pb = [\"\"]")
            .replace("rr3-pc = [\"cognition.llm\"]", "rr3-pc = 3");
        let errs = rr3_profile_from_text(&text).expect_err("形状不合必须报错");
        assert_eq!(errs.len(), 4, "{errs:?}");
        assert!(
            errs.iter().any(|e| e.contains("schema_version")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("bindings.act.servo")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("enabled_slots.rr3-pb 的每一项")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("enabled_slots.rr3-pc 应为字符串数组")),
            "{errs:?}"
        );
    }

    #[test]
    fn missing_sections_and_empty_enabled_slots_are_errors() {
        let errs = rr3_profile_from_text("schema_version = 3\n").expect_err("缺段必须报错");
        assert!(
            errs.iter().any(|e| e.contains("缺少 [bindings]")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("缺少 [enabled_slots]")),
            "{errs:?}"
        );

        let errs = rr3_profile_from_text("schema_version = 3\n[bindings]\n\n[enabled_slots]\n")
            .expect_err("空 enabled_slots 必须报错");
        assert!(
            errs.iter().any(|e| e.contains("[enabled_slots] 为空")),
            "{errs:?}"
        );
    }

    #[test]
    fn unparsable_toml_is_single_error() {
        let errs = rr3_profile_from_text("[bindings\n").expect_err("必须报错");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].starts_with("装配单 TOML 解析失败"), "{errs:?}");
    }

    /// 满配：manifest 路径按 <root>/modules/<名>/module.toml 解析，缺格为空
    #[test]
    fn full_profile_resolves_manifests_and_finds_no_missing() {
        let root = Root::new("full");
        let p = root.write_profile("full.toml", GOOD);
        let boot = boot_inputs(&p).expect("满配装配单应通过");
        assert!(boot.missing.is_empty(), "{:?}", boot.missing);
        assert_eq!(boot.manifests.len(), 3);
        // 比对基准必须是**规范化后**的根目录：boot_inputs 拿到的 manifest 路径派生自
        // canonicalize 过的装配单路径（macOS 的 /tmp 是 /private/tmp 的符号链接），
        // 拿夹具的原始路径比对就会红在系统布局上，不是红在被测行为。
        let modules_root = std::fs::canonicalize(&root.dir)
            .expect("夹具根目录必须可规范化")
            .display()
            .to_string();
        for (i, name) in ["rr3-pa", "rr3-pb", "rr3-pc"].iter().enumerate() {
            assert_eq!(
                boot.manifests[i],
                Path::new(&modules_root)
                    .join("modules")
                    .join(name)
                    .join("module.toml"),
                "manifest 路径必须按 modules-root 解析且升序"
            );
        }
    }

    /// MC-4 门禁用例①：注入非 linux 生产探针（有硬件面的格位恒缺）→ 这些格位进缺格
    /// 清单，且**点名到件与键**（(模块, 格位) 成对，模块不错配到别的格）——这条钉的
    /// 是"缺格 → Degraded 点名件与键"的检出真值（探测拿不到答案不当作在位的下游）。
    /// 无硬件面可反证的格位（cognition.llm）不在缺格清单：那是"没有反证"不是"拿不到答案"。
    #[test]
    fn injected_absent_probe_names_missing_module_and_slot() {
        let root = Root::new("probe-absent");
        let p = root.write_profile("absent.toml", GOOD);
        let boot = boot_inputs_with_probe(&p, &crate::probe::Rr3AbsentProbe)
            .expect("缺格不是致命项，boot 输入必须成功返回");
        assert_eq!(
            boot.missing,
            vec![
                ("rr3-pa".to_string(), "act.servo".to_string()),
                ("rr3-pb".to_string(), "sensor.env".to_string()),
            ],
            "有硬件面的格位必须点名件与键、升序去重（软件格 cognition.llm 不在内）: {:?}",
            boot.missing
        );
    }

    /// MC-4 门禁用例②：探测件只对一格报缺 → 只有那一格的点名模块进缺格，
    /// 其余同在位——证明缺格判定确实**逐格读探测结果**，不是整单连坐。
    #[test]
    fn injected_selective_probe_marks_only_its_slot() {
        struct OnlyServoAbsent;
        impl crate::probe::Rr3Probe for OnlyServoAbsent {
            fn rr3_probe_slot(&self, slot: &str) -> bool {
                slot != "act.servo"
            }
        }
        let root = Root::new("probe-selective");
        let p = root.write_profile("selective.toml", GOOD);
        let boot = boot_inputs_with_probe(&p, &OnlyServoAbsent).expect("部分缺格不致命");
        assert_eq!(
            boot.missing,
            vec![("rr3-pa".to_string(), "act.servo".to_string())],
            "只有探到缺的那一格点名件与键: {:?}",
            boot.missing
        );
    }

    /// 缺格：bindings 移除任一格 → (模块, 格位) 进缺格清单，其余不受牵连
    #[test]
    fn removed_binding_is_missing_for_its_module_only() {
        let root = Root::new("missing");
        let p = root.write_profile(
            "missing.toml",
            &GOOD.replace("\"sensor.env\" = \"rr3-pb\"\n", ""),
        );
        let boot = boot_inputs(&p).expect("缺格不是致命项");
        assert_eq!(
            boot.missing,
            vec![("rr3-pb".to_string(), "sensor.env".to_string())],
            "只有点名模块缺格"
        );
        assert_eq!(
            boot.manifests.len(),
            3,
            "缺格模块的 manifest 仍在解析结果里（供日志点名）"
        );
    }

    /// enabled_slots 声明了格位但 binding 指向别的模块：对点名模块算缺格，同时构成"binding 未被认领"
    /// 的结构不自洽 → 致命（fail-closed，装配单各说一套必须当场听见）
    #[test]
    fn binding_claimed_by_another_module_is_fatal() {
        let root = Root::new("cross");
        let text = GOOD.replace("\"sensor.env\" = \"rr3-pb\"", "\"sensor.env\" = \"rr3-pc\"");
        let p = root.write_profile("cross.toml", &text);
        let e = boot_inputs(&p).expect_err("binding 被他人认领必须致命");
        assert!(e.contains("未被 enabled_slots 认领"), "{e}");
    }

    /// 基础层格位进 bindings = 违反"基础层由基座内建承载"（slots.toml 注记）→ 致命
    #[test]
    fn base_layer_binding_is_fatal() {
        let root = Root::new("base");
        let text = GOOD.replace("[bindings]\n", "[bindings]\n\"bus.publish\" = \"rr3-pa\"\n");
        let p = root.write_profile("base.toml", &text);
        let e = boot_inputs(&p).expect_err("基础层 binding 必须致命");
        assert!(e.contains("bindings 不得绑定基础层格位 bus.publish"), "{e}");
    }

    /// 查册门：enabled_slots/bindings 引用未在册格位 → 致命并点名（真契约上跑）
    #[test]
    fn unknown_slot_is_fatal_and_named() {
        let root = Root::new("unknown");
        let text = GOOD.replace("rr3-pc = [\"cognition.llm\"]", "rr3-pc = [\"sensor.wifi\"]");
        let p = root.write_profile("unknown.toml", &text);
        let e = boot_inputs(&p).expect_err("未在册格位必须致命");
        assert!(
            e.contains("enabled_slots 模块 rr3-pc 的格位 sensor.wifi 未在册"),
            "{e}"
        );
    }

    /// 基础层格位进 enabled_slots 只记账、不算缺格（power.battery 恒在位，default 机同款形状）
    #[test]
    fn base_layer_enabled_slot_is_never_missing() {
        let root = Root::new("baseslot");
        let text = GOOD.replace(
            "rr3-pc = [\"cognition.llm\"]",
            "rr3-pc = [\"cognition.llm\", \"power.battery\"]",
        );
        let p = root.write_profile("baseslot.toml", &text);
        let boot = boot_inputs(&p).expect("基础层 enabled_slots 格位不缺格");
        assert!(boot.missing.is_empty(), "{:?}", boot.missing);
    }

    /// [hardware] 段（schema 3 只余非引脚键）：字符串/整数标量收、整数字符串化、
    /// 缺段可选；坏类型逐条点名。物理键的"出现即红"在 boot 链裁（门 n），解析层
    /// 不带契约真值不越权。
    #[test]
    fn hardware_section_is_optional_scalars_stringified() {
        // 缺段：合法（纯 sim/无硬件格形态），hal_bindings 为空文本
        let p = rr3_profile_from_text(GOOD).expect("缺 [hardware] 段必须合法");
        assert!(p.hardware.is_empty(), "{:?}", p.hardware);

        // 字符串 + 整数混合：整数按十进制字符串化（[hardware] 不设键白名单，
        // 键位语义归 rr-hal 描述符解析，rrd 只管装配形状）
        let text =
            format!("{GOOD}\n[hardware]\nspi_device = \"/dev/spidev0.0\"\nlink_metric = 100\n");
        let p = rr3_profile_from_text(&text).expect("标量 [hardware] 必须合法");
        assert_eq!(
            p.hardware.get("spi_device").map(String::as_str),
            Some("/dev/spidev0.0")
        );
        assert_eq!(
            p.hardware.get("link_metric").map(String::as_str),
            Some("100")
        );

        // 布尔/浮点/数组/表逐条点名，一次全收集
        let text = format!(
            "{GOOD}\n[hardware]\nflag = true\nratio = 0.5\nlines = [4]\nsub = {{ k = 1 }}\n"
        );
        let errs = rr3_profile_from_text(&text).expect_err("坏类型必须报错");
        assert_eq!(errs.len(), 4, "{errs:?}");
        for key in ["flag", "ratio", "lines", "sub"] {
            assert!(
                errs.iter().any(|e| e.contains(&format!("hardware.{key} "))),
                "{errs:?}"
            );
        }

        // 空键点名
        let text = format!("{GOOD}\n[hardware]\n\"\" = 1\n");
        let errs = rr3_profile_from_text(&text).expect_err("空键必须报错");
        assert!(errs.iter().any(|e| e.contains("键不得为空串")), "{errs:?}");
    }

    /// [hardware] → 注入文本：BTreeMap 升序确定性、整数已是字符串形状、空表空串。
    #[test]
    fn hardware_bindings_text_is_sorted_and_escaped() {
        assert_eq!(
            rr3_hardware_bindings_text(&BTreeMap::new()),
            "",
            "空表 → 空串"
        );
        let mut hw = BTreeMap::new();
        hw.insert("spi_device".to_string(), "0".to_string());
        hw.insert("link_ifname".to_string(), "wl\"x\\".to_string());
        let text = rr3_hardware_bindings_text(&hw);
        // BTreeMap 升序：link_ifname < spi_device
        assert_eq!(
            text, "link_ifname = \"wl\\\"x\\\\\"\nspi_device = \"0\"\n",
            "升序 + 转义（双引号/反斜杠）"
        );
    }

    /// 派生器正例（host 档）：[pins] 派生视图 ∪ [hardware] 非引脚键合成注入文本——
    /// 取值与旧 [hardware] 逐字节对齐（pwm<N> 派生 pwm_chip=N、行号取 physical[0]、
    /// addr 原样），键名不变，rr-hal descriptor.rs 解析端零改动；四角色全派生 +
    /// 多条 pin 共键同值（led/relay 同 gpiochip0）不误报。
    #[test]
    fn boot_inputs_derives_and_merges_hal_bindings() {
        let root = Root::new("derive");
        let text = format!(
            "{GOOD}\n[hardware]\nalsa_device = \"default\"\nlink_ifname = \"wlan0\"\n\
             [[pin]]\nrole = \"act.led.gpio\"\ncontroller = \"gpiochip0\"\nphysical = [4]\n\
             direction = \"out\"\nrail = \"none\"\nfailsafe = \"hold\"\nmux_group = []\n\
             [[pin]]\nrole = \"act.relay.gpio\"\ncontroller = \"gpiochip0\"\nphysical = [17]\n\
             direction = \"out\"\nrail = \"none\"\nfailsafe = \"driven_low\"\nmux_group = []\n\
             [[pin]]\nrole = \"power.battery.meter\"\ncontroller = \"i2c1\"\naddr = \"0x40\"\n\
             direction = \"in\"\nmux_group = []\n"
        );
        let p = root.write_profile("derive.toml", &text);
        let boot = boot_inputs(&p).expect("满配装配单应通过");
        assert_eq!(
            boot.hal_bindings,
            "alsa_device = \"default\"\n\
             gpio_chip = \"0\"\n\
             gpio_led_line = \"4\"\n\
             gpio_relay_line = \"17\"\n\
             i2c_bus = \"1\"\n\
             ina219_addr = \"0x40\"\n\
             link_ifname = \"wlan0\"\n\
             pwm_channel = \"0\"\n\
             pwm_chip = \"0\"\n",
            "派生视图与非引脚键按 BTreeMap 升序合并，取值与旧 [hardware] 一致"
        );
        assert!(boot.role_missing.is_empty(), "{:?}", boot.role_missing);
    }

    /// 校验①-a：role 未在 roles.toml 在册 → 致命点名（真契约上跑）
    #[test]
    fn pin_unknown_role_is_fatal() {
        let root = Root::new("pin-role");
        let text = GOOD.replace("role = \"act.axis.pwm\"", "role = \"act.axis.magical\"");
        let p = root.write_profile("pin-role.toml", &text);
        let e = boot_inputs(&p).expect_err("自造角色必须致命");
        assert!(e.contains("角色 act.axis.magical 未在册"), "{e}");
    }

    /// 校验①-b per_instance 两侧：true 角色缺 instance / false 角色填 instance 都红
    #[test]
    fn pin_per_instance_rules_are_fatal_both_sides() {
        let root = Root::new("pin-perinst");
        // act.axis.pwm per_instance=true，删 instance → 必填缺
        let text = GOOD.replace("instance = \"pan\"\n", "");
        let p = root.write_profile("perinst-missing.toml", &text);
        let e = boot_inputs(&p).expect_err("per_instance=true 缺 instance 必须致命");
        assert!(e.contains("instance 必填"), "{e}");

        // act.led.gpio per_instance=false，填 instance → 禁填
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"act.led.gpio\"\ninstance = \"x\"\n\
             controller = \"gpiochip0\"\nphysical = [4]\ndirection = \"out\"\nfailsafe = \"hold\"\n"
        );
        let p = root.write_profile("perinst-extra.toml", &text);
        let e = boot_inputs(&p).expect_err("per_instance=false 填 instance 必须致命");
        assert!(e.contains("instance 禁填"), "{e}");
    }

    /// 校验①-b 留空反例（§5 缺失语义）：requires 必填字段删键即红——channel 以
    /// None = 缺、failsafe 以空串 = 缺，一次收集、逐条点名，一条不静默放行
    #[test]
    fn pin_requires_left_blank_is_fatal() {
        let root = Root::new("pin-blank");
        let text = GOOD
            .replace("channel = 0\n", "")
            .replace("failsafe = \"release\"\n", "");
        let p = root.write_profile("pin-blank.toml", &text);
        let e = boot_inputs(&p).expect_err("requires 字段留空必须致命");
        assert!(e.contains("缺 channel"), "{e}");
        assert!(e.contains("缺 failsafe"), "{e}");
    }

    /// 校验① direction 相等：pin.direction 与角色 direction 各说一套 → 致命点名
    #[test]
    fn pin_direction_mismatch_is_fatal() {
        let root = Root::new("pin-direction");
        let text = GOOD.replace("direction = \"out\"", "direction = \"in\"");
        let p = root.write_profile("pin-direction.toml", &text);
        let e = boot_inputs(&p).expect_err("direction 与角色不一致必须致命");
        assert!(
            e.contains("direction = in 与角色 act.axis.pwm direction = out 不一致"),
            "{e}"
        );
    }

    /// [[pin]] 形状：未知键/坏类型/缺身份键逐条点名，一次收集完
    #[test]
    fn pin_section_shape_errors_are_named() {
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"\"\ndirection = \"out\"\nwat = 1\nphysical = [\"x\"]\n"
        );
        let errs = rr3_profile_from_text(&text).expect_err("形状不合必须报错");
        assert!(
            errs.iter().any(|e| e.contains("pin #1.role 缺失或为空")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("pin #1 未知键 wat")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("pin #1.physical 的每一项都必须是整数")),
            "{errs:?}"
        );
        // 缺 role / 缺 direction（身份键缺席，解析期即红）
        let errs =
            rr3_profile_from_text(format!("{GOOD}\n[[pin]]\ncontroller = \"pwm1\"\n").as_str())
                .expect_err("缺 role/direction 必须报错");
        assert!(
            errs.iter().any(|e| e.contains("pin #1 缺 role")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("pin #1 缺 direction")),
            "{errs:?}"
        );
    }

    /// 校验⑥-a/⑥-b 反例（§18 F-5 的自造域名 + 功率面挂 none）：未在册 rail 与
    /// kind=power 而 rail=none 都必须拒 boot 并点名
    #[test]
    fn pin_rail_domain_gate_is_fatal() {
        let root = Root::new("rail-gate");
        // ⑥-a：自造域名（F-5：rail24 不在任何一处在册集——文档示例值会被装机档抄）
        let text = GOOD.replace("rail = \"rail5v\"", "rail = \"rail24\"");
        let p = root.write_profile("rail-unknown.toml", &text);
        let e = boot_inputs(&p).expect_err("自造域名必须致命");
        assert!(
            e.contains("rail = rail24 未在 [[supplies.domain]] 在册"),
            "{e}"
        );

        // ⑥-b：kind=power 角色挂 none（act.axis.pwm 是 power，none 是 signal 侧显式值）
        let text = GOOD.replace("rail = \"rail5v\"", "rail = \"none\"");
        let p = root.write_profile("rail-none-power.toml", &text);
        let e = boot_inputs(&p).expect_err("功率面挂 none 必须致命");
        assert!(e.contains("kind = power"), "{e}");
    }

    /// 校验⑥-c 反例：armed=true 而 sense=none / max_age_ms=0——未标定即不武装，
    /// 无测点/无龄期上界的域不得武装拒动（§3.1 降级语义 + §19 Y-4）。
    /// GOOD 的 batt 域恰是 sense=none + max_age_ms=0 的最小反例，armed 一开即双红。
    #[test]
    fn domain_armed_without_sense_is_fatal() {
        let root = Root::new("armed-gate");
        let text = GOOD.replace(
            "switchable = false\narmed = false\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
            "switchable = false\narmed = true\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        );
        let p = root.write_profile("armed.toml", &text);
        let e = boot_inputs(&p).expect_err("武装而无测点/龄期必须致命");
        assert!(e.contains("校验⑥-c"), "{e}");
    }

    /// 校验⑥-d 反例：域绑定位写第二套语法（F-2 的 adc:gp34）——不在 pin_id 集
    /// 即红，"一律写 pin_id，不再各造语法"
    #[test]
    fn domain_binding_uses_pin_id_not_adhoc_syntax() {
        let root = Root::new("pid-gate");
        let text = GOOD.replace(
            "sense = \"none\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = false\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
            "sense = \"adc:gp34\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = false\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        );
        let p = root.write_profile("pid.toml", &text);
        let e = boot_inputs(&p).expect_err("第二套语法必须致命");
        assert!(e.contains("不在 [pins] 的 pin_id 集"), "{e}");
        assert!(e.contains("sense = adc:gp34"), "{e}");
    }

    /// [[supplies.domain]] 形状门（解析层）：缺键逐键点名、未知键点名、
    /// 缺整个域数组点名——一次收集完，运维一次改对
    #[test]
    fn domain_section_shape_errors_are_named() {
        // 缺整个域数组（schema 3 起必填）：取 GOOD 的 [supplies] 段头做最小文本
        let minimal_head = GOOD.split("[[supplies.domain]]").next().unwrap_or(GOOD);
        let errs = rr3_profile_from_text(minimal_head).expect_err("缺域数组必须报错");
        assert!(
            errs.iter().any(|e| e.contains("缺少 [[supplies.domain]]")),
            "{errs:?}"
        );

        // 域条目缺键 + 未知键逐条点名
        let errs = rr3_profile_from_text(&format!(
            "{minimal_head}[[supplies.domain]]\nname = \"batt\"\nnominal_mv = 3700\nghost = 1\n"
        ))
        .expect_err("缺键/未知键必须报错");
        for want in [
            "supplies.domains[0].mild_mv",
            "supplies.domains[0].severe_mv",
            "supplies.domains[0].current_cap_ma",
            "supplies.domains[0].sense",
            "supplies.domains[0].current_sense",
            "supplies.domains[0].switch",
            "supplies.domains[0].switchable",
            "supplies.domains[0].armed",
            "supplies.domains[0].max_age_ms",
            "supplies.domains[0] 未知键 ghost",
        ] {
            assert!(errs.iter().any(|e| e.contains(want)), "缺 {want}: {errs:?}");
        }
    }

    /// 门 n 稳态判据（§8，power P-13）：[hardware] 出现物理键即红——GOOD 无电表
    /// pin，i2c_bus 无派生者，按稳态文案点名
    #[test]
    fn gate_n_hardware_physical_key_is_fatal() {
        let root = Root::new("gate-n-static");
        let text = format!("{GOOD}\n[hardware]\ni2c_bus = 1\n");
        let p = root.write_profile("gate-n-static.toml", &text);
        let e = boot_inputs(&p).expect_err("[hardware] 物理键必须致命");
        assert!(
            e.contains("hardware.i2c_bus 是引脚侧派生键") && e.contains("门 n 稳态判据"),
            "{e}"
        );
    }

    /// 门 n 两处并存判据：pin 实际派生该键时点名两份取值（[hardware] 值 vs 派生值）
    #[test]
    fn gate_n_coexistence_names_both_values() {
        let root = Root::new("gate-n-both");
        let text = format!("{GOOD}\n[hardware]\npwm_chip = 5\n");
        let p = root.write_profile("gate-n-both.toml", &text);
        let e = boot_inputs(&p).expect_err("两处并存必须致命");
        assert!(
            e.contains("hardware.pwm_chip = \"5\" 与 [pins] 派生值 \"0\" 两处并存"),
            "{e}"
        );
    }

    /// 校验②（后果 = 降不拒）：格位在位且被启用但角色无 [[pin]] 绑定 → (格位, 角色)
    /// 进 role_missing 清单随 boot 带出，模块照常起——缺角色是缺能力，不是拒 boot
    #[test]
    fn role_missing_is_listed_not_fatal() {
        let root = Root::new("role-missing");
        let text = GOOD.replace(
            "\n[[pin]]\nrole = \"act.axis.pwm\"\ninstance = \"pan\"\ncontroller = \"pwm0\"\nchannel = 0\nphysical = [12]\ndirection = \"out\"\nrail = \"rail5v\"\nfailsafe = \"release\"\nmux_group = []\n",
            "",
        );
        let p = root.write_profile("role-missing.toml", &text);
        let boot = boot_inputs(&p).expect("缺角色不是致命项");
        assert_eq!(
            boot.role_missing,
            vec![("act.servo".to_string(), "act.axis.pwm".to_string()),],
            "present ∩ enabled_slots 的格位缺角色逐条点名（sensor.env/cognition.llm 无角色不误报）"
        );
        assert!(boot.missing.is_empty(), "{:?}", boot.missing);
    }

    /// 格位契约形状门进运行期装载（0.0.5 批J 深挖收口）：status/tier_scope 越界的
    /// slots.toml 在 boot 即拒并点名——不能只靠 CI 契约测试兜底（装机现场改坏的
    /// 契约同样不得带病起）。
    #[test]
    fn boot_inputs_rejects_out_of_whitelist_slot_contract() {
        let root = Root::new("slot-shape");
        let bad_slots = root.dir.join("slots-bad.toml");
        std::fs::write(
            &bad_slots,
            r#"
[[slot]]
name = "act.servo"
layer = "body"
status = "deprecated"
tier_scope = ["quantum"]
"#,
        )
        .unwrap();
        let p = root.write_profile("ok.toml", GOOD);
        // slots 册由本用例自备（被测对象就是它），其余各册照常走真值——接线档由
        // 推导函数定位（write_profile 已同批 stage）
        let wiring =
            crate::wiring::rr3_wiring_path_for_profile(&p).expect("夹具装配单必须能推导接线档");
        let errs = rr3_profile_boot_inputs(
            &p,
            &bad_slots,
            Path::new(RR3_CONTRACT_ROLES_PATH),
            Path::new(RR3_CONTRACT_DEVICES_PATH),
            &wiring,
            Path::new(rr_registry::RR3_CONTRACT_PLATFORM_PATH),
        )
        .expect_err("越界契约必须拒");
        assert!(errs.contains("status 非法"), "{errs}");
        assert!(errs.contains("tier_scope 含非法档 quantum"), "{errs}");
    }

    /// schema_version=2 三新段必填：缺段逐条点名（fail-closed，一次收集完）
    #[test]
    fn new_sections_are_required_and_named() {
        let errs = rr3_profile_from_text("schema_version = 2\n").expect_err("缺新段必须报错");
        for want in [
            "缺少 [platform]",
            "缺少 [supplies]",
            "缺少 [slots]",
            "缺少 [bindings]",
            "缺少 [enabled_slots]",
        ] {
            assert!(errs.iter().any(|e| e.contains(want)), "缺 {want}: {errs:?}");
        }
    }

    /// [platform]/[supplies] 形状：tier 值域、类型逐键检查，坏值一次全收集
    #[test]
    fn platform_and_supplies_shape_errors_are_named() {
        let text = GOOD
            .replace("tier = \"host\"", "tier = \"nano\"")
            .replace("watchdog_present = true", "watchdog_present = 1")
            .replace(
                "watchdog_timeout_ms = 15000",
                "watchdog_timeout_ms = \"15s\"",
            )
            .replace("watchdog_feeder = \"rrd\"", "watchdog_feeder = \"\"");
        let errs = rr3_profile_from_text(&text).expect_err("形状不合必须报错");
        assert_eq!(errs.len(), 4, "{errs:?}");
        for want in [
            "platform.tier",
            "supplies.watchdog_present",
            "supplies.watchdog_timeout_ms",
            "supplies.watchdog_feeder",
        ] {
            assert!(errs.iter().any(|e| e.contains(want)), "缺 {want}: {errs:?}");
        }
    }

    /// 三新段未知键逐条点名（一条不静默放行）
    #[test]
    fn new_sections_unknown_keys_are_named() {
        let text = GOOD
            .replace("tier = \"host\"", "tier = \"host\"\nfoo = 1")
            .replace(
                "watchdog_present = true",
                "watchdog_present = true\nbar = 1",
            )
            .replace("[slots]\n", "[slots]\nbaz = []\n");
        let errs = rr3_profile_from_text(&text).expect_err("未知键必须报错");
        assert_eq!(errs.len(), 3, "{errs:?}");
        for want in [
            "[platform] 未知键 foo",
            "[supplies] 未知键 bar",
            "[slots] 未知键 baz",
        ] {
            assert!(errs.iter().any(|e| e.contains(want)), "缺 {want}: {errs:?}");
        }
    }

    /// G6 档位一致性（ADR-0018 修订 1）：tier=bare 且无看门狗 → 致命点名；
    /// bare + 看门狗在场 → 一致，放行。
    #[test]
    fn bare_tier_without_watchdog_is_fatal() {
        let root = Root::new("g6");
        let bad = GOOD
            .replace("tier = \"host\"", "tier = \"bare\"")
            .replace("watchdog_present = true", "watchdog_present = false");
        let p = root.write_profile("bare-nowd.toml", &bad);
        let e = boot_inputs(&p).expect_err("无看门狗的 bare 档必须被拒");
        assert!(e.contains("无看门狗的板不允许归 bare 档"), "{e}");

        let ok = GOOD.replace("tier = \"host\"", "tier = \"bare\"");
        let p = root.write_profile("bare-wd.toml", &ok);
        boot_inputs(&p).expect("bare 档 + 看门狗在场应放行");
    }

    /// L-10 tier↔tier_scope 交叉校验（0.0.5 批K，ADR-0012 增补注记分族的装配侧
    /// 执法）：bare 档引用 bus.publish（tier_scope = host/rtos）→ 拒 boot 点名；
    /// 引用 scope 全档的 power.battery → 放行。enabled_slots 是唯一允许基础层
    /// 格位的引用面，反例经它构造。
    #[test]
    fn tier_outside_slot_tier_scope_is_fatal() {
        let root = Root::new("l10");
        let bad = GOOD.replace("tier = \"host\"", "tier = \"bare\"").replace(
            "rr3-pa = [\"act.servo\"]",
            "rr3-pa = [\"act.servo\"]\nrr3-pd = [\"bus.publish\"]",
        );
        let p = root.write_profile("bare-bus.toml", &bad);
        let e = boot_inputs(&p).expect_err("bare 档引用 bus 格必须被拒");
        assert!(
            e.contains("tier↔tier_scope 交叉校验失败") && e.contains("bus.publish"),
            "{e}"
        );

        let ok = GOOD.replace("tier = \"host\"", "tier = \"bare\"").replace(
            "rr3-pa = [\"act.servo\"]",
            "rr3-pa = [\"act.servo\"]\nrr3-pd = [\"power.battery\"]",
        );
        let p = root.write_profile("bare-battery.toml", &ok);
        boot_inputs(&p).expect("bare 档引用 scope 全档的 power.battery 应放行");
    }

    /// 校验⑦ 跨档反例（后果 = 拒；boot 链把 [pins]/domains/devices 三份真值接起来
    /// 判，wiring.rs 单测已钉判定逻辑，这里钉的是跨档接线）：signal 越出
    /// pin_id ∪ {vcc,gnd}（F-3 自造语法）、net 越出 domains（F-5）、mate 越出
    /// devices mate 集（§4.1b 门 o）逐条点名；命中集全绿由末段正例钉住。
    #[test]
    fn wiring_cross_file_gates_are_fatal() {
        // ⑦-a：自造语法 pwm:12 既非 pin_id 也非 vcc/gnd（F-2 同款病，接线侧再现）
        let root = Root::new("wiring-7a");
        let p = root.write_profile("wiring-7a.toml", GOOD);
        root.write_wiring(
            &p,
            &GOOD_WIRING.replace(
                "signal = [\"vcc\", \"gnd\"]",
                "signal = [\"vcc\", \"pwm:12\"]",
            ),
        );
        let e = boot_inputs(&p).expect_err("signal 自造语法必须致命");
        assert!(e.contains("校验⑦-a") && e.contains("pwm:12"), "{e}");

        // ⑦-b：自造域名（F-5：文档示例值会被装机档抄）
        let root = Root::new("wiring-7b");
        let p = root.write_profile("wiring-7b.toml", GOOD);
        root.write_wiring(
            &p,
            &GOOD_WIRING.replace("net = \"rail5v\"", "net = \"rail24\""),
        );
        let e = boot_inputs(&p).expect_err("自造域名必须致命");
        assert!(e.contains("校验⑦-b") && e.contains("rail24"), "{e}");

        // ⑦-d：对端型号不在 devices.toml mate 集（词形错误——在册的是 SG90-3P）
        let root = Root::new("wiring-7d");
        let p = root.write_profile("wiring-7d.toml", GOOD);
        root.write_wiring(
            &p,
            &GOOD_WIRING.replace("mate = \"none\"", "mate = \"SG90-9G\""),
        );
        let e = boot_inputs(&p).expect_err("mate 不在册必须致命");
        assert!(e.contains("校验⑦-d") && e.contains("SG90-9G"), "{e}");

        // 命中集全绿：signal 写 [pins] 真值 pin_id、net/mate 写在册值——⑦ 三判据同过
        let root = Root::new("wiring-7-ok");
        let p = root.write_profile("wiring-7-ok.toml", GOOD);
        root.write_wiring(
            &p,
            r#"
[[wire]]
connector = "J-S"
hdr_pin = [1, 2, 3]
signal = ["vcc", "act.axis.pwm.pan", "gnd"]
net = "rail5v"
feeds = []
mate = "SG90-3P"
keyed = true
rated_a = 1.0
gnd_common = "star-1"
"#,
        );
        boot_inputs(&p).expect("signal/net/mate 全命中在册集必须放行");
    }

    /// 接线档装载门（fail-closed，后果 = 拒）：留空反例（§5 缺失语义列——rated_a
    /// 无不适值可言，缺即红）与闭合面破（feeds 悬空引用）都在 boot 即拒并点名
    #[test]
    fn wiring_shape_and_closure_gates_are_fatal_at_boot() {
        let root = Root::new("wiring-blank");
        let p = root.write_profile("wiring-blank.toml", GOOD);
        root.write_wiring(&p, &GOOD_WIRING.replace("rated_a = 1.0\n", ""));
        let e = boot_inputs(&p).expect_err("rated_a 留空必须致命");
        assert!(e.contains("形状不合，拒绝启动"), "{e}");
        assert!(e.contains("rated_a 缺失或不是正数"), "{e}");

        let root = Root::new("wiring-dangle");
        let p = root.write_profile("wiring-dangle.toml", GOOD);
        root.write_wiring(
            &p,
            &GOOD_WIRING.replace("feeds = []", "feeds = [\"J-NOPE\"]"),
        );
        let e = boot_inputs(&p).expect_err("悬空 feeds 必须致命");
        assert!(
            e.contains("wiring[J-T].feeds = J-NOPE") && e.contains("位号集"),
            "{e}"
        );
    }

    /// 真仓 default 机 drill：profiles/default.toml + hardware/default/wiring.toml
    /// 过完整装载门——⑦-a 的四个 pin_id（act.axis.pwm.pan/act.led.gpio/
    /// act.relay.gpio/power.battery.meter）、⑦-b 的三域名（batt/rail5v/rail3v3）、
    /// ⑦-d 的 SG90-3P 全部命中仓内真值（跨档对账的三份真值两两闭合）。
    #[test]
    fn default_profile_wiring_drill() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let p = repo.join("profiles/default.toml");
        let wiring = crate::wiring::rr3_wiring_path_for_profile(&p).expect("真仓装配单可推导");
        assert!(
            wiring.exists(),
            "真仓接线档必须随批N1 落位: {}",
            wiring.display()
        );
        let boot = boot_inputs(&p).expect("default 机装配单+接线档必须过完整装载门（host 档）");
        assert_eq!(boot.manifests.len(), 6, "enabled_slots 六模块（升序）");
    }

    /// 真仓 distro 机 drill：profiles/distro.toml + hardware/distro/wiring.toml
    /// （与 default 同板同接线的镜像）过完整装载门——镜像双轨各自对各自的装配单
    /// 闭合（distro.toml 的 pin/域集真值 + 同一 devices.toml mate 集），镜像漂移
    /// 即此测试红；装机 deb 面由此件随 rr3-pack 入包（Z-4）。
    #[test]
    fn distro_profile_wiring_drill() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let p = repo.join("profiles/distro.toml");
        let boot = boot_inputs(&p).expect("distro 机装配单+接线档必须过完整装载门（host 档）");
        assert_eq!(boot.manifests.len(), 6, "enabled_slots 六模块（升序）");
    }

    /// F-B 桥接微型形态装配演练（0.0.5 批K H4/R3-D6）：对仓内真件
    /// `profiles/esp32-mini.toml` + 真 contracts/slots.toml 走完整 boot 输入装配——
    /// 查册/形状门、G6 档位一致性、L-10 tier↔tier_scope 全过；三格自洽
    /// （missing 空 = present/bindings/enabled_slots 三源一致）、装配段扫描零缺口、
    /// boot 队列恰为两模块、[hardware] 缺省不注入。真板侧运行（限位/时限/饱和
    /// 通告、桥断仍守护）触发条件 = 0.0.7 轻承载档真板，本测试只证装配链层
    /// "裁剪 = 改数据"（CI 全集层形状 pin 在 rr-registry contract.rs）。
    #[test]
    fn esp32_mini_profile_boot_inputs_drill() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let p = repo.join("profiles/esp32-mini.toml");
        let boot = boot_inputs(&p).expect("esp32-mini 装配单必须过完整装载门（rtos 档）");
        let mods: Vec<String> = boot
            .manifests
            .iter()
            .map(|m| {
                m.parent()
                    .and_then(Path::file_name)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(
            mods,
            vec!["rr3-act", "rr3-sense"],
            "boot 队列恰为 F-B 两模块（enabled_slots 升序，BTreeMap 确定性）"
        );
        assert!(
            boot.missing.is_empty(),
            "三格自洽（present/bindings/enabled_slots 三源一致）: {:?}",
            boot.missing
        );
        assert!(
            boot.assemble_broken.is_empty(),
            "真件装配段扫描零缺口: {:?}",
            boot.assemble_broken
        );
        assert!(
            boot.role_missing.is_empty(),
            "三格全有角色绑定（[[pin]] 真值点在位）: {:?}",
            boot.role_missing
        );
        assert!(
            boot.hal_bindings.is_empty(),
            "[hardware] 缺省 + rtos 档不派生 → 不注入物理绑定（板侧描述符归 0.0.7）"
        );
    }

    /// [slots].present 声明缺是缺格三源合一第一源：present 移除某格 →
    /// (模块, 格位) 进缺格清单（缺格不是致命项，其余模块不牵连）
    #[test]
    fn absent_slot_declaration_is_missing_source() {
        let root = Root::new("present");
        let text = GOOD.replace(", \"sensor.env\"", "");
        let p = root.write_profile("absent.toml", &text);
        let boot = boot_inputs(&p).expect("声明缺不是致命项");
        assert_eq!(
            boot.missing,
            vec![("rr3-pb".to_string(), "sensor.env".to_string())],
            "present 声明缺的格位对点名模块记缺格"
        );
    }

    /// [slots].present 查册门：未在册格位、基础层格位 → 致命点名
    #[test]
    fn present_registry_gate_rejects_unknown_and_base_slots() {
        let root = Root::new("presentgate");
        let unknown = GOOD.replace(
            "present = [\"act.servo\"",
            "present = [\"sensor.wifi\", \"act.servo\"",
        );
        let p = root.write_profile("present-unknown.toml", &unknown);
        let e = boot_inputs(&p).expect_err("present 未在册格位必须致命");
        assert!(e.contains("格位 sensor.wifi 未在册"), "{e}");

        let base = GOOD.replace(
            "present = [\"act.servo\"",
            "present = [\"bus.publish\", \"act.servo\"",
        );
        let p = root.write_profile("present-base.toml", &base);
        let e = boot_inputs(&p).expect_err("present 声明基础层格位必须致命");
        assert!(e.contains("不得声明基础层格位 bus.publish"), "{e}");
    }

    /// 装配单文件不存在 = 致命，文案是绝对化口径（canonicalize 的 ENOENT）。
    /// 接线路径随意给：装配单 canonicalize 先败，轮不到它（有 Root 帮手的用例都
    /// 走真推导，本条只钉读不到装配单的文案口径）。
    #[test]
    fn missing_profile_file_is_fatal() {
        let e = rr3_profile_boot_inputs(
            Path::new("/nonexistent/rr3-nope.toml"),
            Path::new(RR3_CONTRACT_SLOTS_PATH),
            Path::new(RR3_CONTRACT_ROLES_PATH),
            Path::new(RR3_CONTRACT_DEVICES_PATH),
            Path::new("/nonexistent/rr3-nope-wiring.toml"),
            Path::new("/nonexistent/rr3-nope-platform"),
        )
        .expect_err("读不到装配单必须致命");
        assert!(e.contains("无法解析为绝对路径"), "{e}");
    }

    /// 校验③④⑤⑧判据前件的 fail-closed（§4.6 裁定①）：[platform].chip 选不到表 =
    /// 拒 boot，不许降级成"无表 = 无冲突/无词表"（宽容 = ④⑤⑧ 静默不判，F-6 同病）
    #[test]
    fn platform_table_missing_for_chip_is_fatal() {
        let root = Root::new("plat-missing");
        let p = root.write_profile("plat-missing.toml", GOOD);
        let wiring =
            crate::wiring::rr3_wiring_path_for_profile(&p).expect("夹具装配单必须能推导接线档");
        let e = rr3_profile_boot_inputs(
            &p,
            Path::new(RR3_CONTRACT_SLOTS_PATH),
            Path::new(RR3_CONTRACT_ROLES_PATH),
            Path::new(RR3_CONTRACT_DEVICES_PATH),
            &wiring,
            &root.dir.join("platform-empty"),
        )
        .expect_err("chip 选不到平台表必须拒 boot");
        assert!(e.contains("读不到平台事实表") && e.contains("rk3566"), "{e}");
    }

    /// [platform].chip 留空反例（F-6 缺字段可被留空绕过的封堵面）：chip 是平台
    /// 事实外键，"合法不适用"不存在——留空即红
    #[test]
    fn platform_chip_left_blank_is_fatal() {
        let text = GOOD.replace("chip = \"rk3566\"", "");
        let errs = rr3_profile_from_text(&text).expect_err("chip 留空必须报错");
        assert!(errs.iter().any(|e| e.contains("缺 chip")), "{errs:?}");

        let text = GOOD.replace("chip = \"rk3566\"", "chip = \"\"");
        let errs = rr3_profile_from_text(&text).expect_err("chip 空串必须报错");
        assert!(
            errs.iter().any(|e| e.contains("chip 不得为空串")),
            "{errs:?}"
        );
    }

    /// 校验⑤-b 反例（F-9）：模拟测点落非 ADC 脚——GPIO23 读回 0 mV ⇒ 档位恒真或
    /// 恒假。ESP32_GOOD 的电表 pin physical = [34] ∈ adc_pins 是绿基线，替换成
    /// 23 即红（测点引用面 = 域 sense 的 pin_id 外键）。
    #[test]
    fn analog_sense_pin_off_adc_set_is_rejected() {
        let bad = ESP32_GOOD.replace("physical = [34]", "physical = [23]");
        let e = rr3_profile_from_text_with_platform("adc-off", &bad, platform_esp32_text())
            .expect_err("测点落非 ADC 脚必须拒 boot");
        assert!(e.contains("ADC 能力集") && e.contains("校验⑤-b"), "{e}");
    }

    /// 校验⑤-a 反例：direction = out（角色 input_only_ok = false）落 input-only
    /// 脚集——GPIO35 无输出驱动能力，写 out 即假账。换到 35 避开电表 34 的 ③-a
    /// 双占级联，红因单一。
    #[test]
    fn output_on_input_only_pin_is_rejected() {
        let bad = ESP32_GOOD.replace("physical = [32]", "physical = [35]");
        let e = rr3_profile_from_text_with_platform("input-only", &bad, platform_esp32_text())
            .expect_err("output 落 input-only 脚必须拒 boot");
        assert!(e.contains("input-only") && e.contains("校验⑤-a"), "{e}");
    }

    /// 校验⑧-b 反例：controller 不在平台在册控制器词表——装配单不得自造控制器
    /// （ledc9 词形合法但词表无此名，词表不是词形）。
    #[test]
    fn controller_off_platform_vocabulary_is_rejected() {
        let bad = ESP32_GOOD.replace("controller = \"ledc0\"", "controller = \"ledc9\"");
        let e = rr3_profile_from_text_with_platform("ctl-word", &bad, platform_esp32_text())
            .expect_err("自造控制器必须拒 boot");
        assert!(e.contains("在册控制器词表") && e.contains("校验⑧-b"), "{e}");
    }

    /// 校验③-a 反例：同一 physical 元素两条 pin 双占（启用集 = [pins] 全集）
    #[test]
    fn two_pins_same_physical_is_rejected() {
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"act.led.gpio\"\ncontroller = \"gpiochip0\"\n\
             physical = [12]\ndirection = \"out\"\nrail = \"none\"\nfailsafe = \"hold\"\n\
             mux_group = []\n"
        );
        let root = Root::new("dup-phys");
        let p = root.write_profile("dup-phys.toml", &text);
        let e = boot_inputs(&p).expect_err("physical 双占必须拒 boot");
        assert!(
            e.contains("校验③-a") && e.contains("physical 元素 12"),
            "{e}"
        );
    }

    /// 校验③-b 反例：同一 (controller, channel) 双占——F-7 的"脚 + 通道"两维，
    /// 双轴两通道即证；占同通道不同脚照红。
    #[test]
    fn two_pins_same_controller_channel_is_rejected() {
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"act.axis.pwm\"\ninstance = \"tilt\"\n\
             controller = \"pwm0\"\nchannel = 0\nphysical = [13]\ndirection = \"out\"\n\
             rail = \"rail5v\"\nfailsafe = \"release\"\nmux_group = []\n"
        );
        let root = Root::new("dup-chan");
        let p = root.write_profile("dup-chan.toml", &text);
        let e = boot_inputs(&p).expect_err("(controller, channel) 双占必须拒 boot");
        assert!(
            e.contains("校验③-b") && e.contains("(controller pwm0, channel 0)"),
            "{e}"
        );
    }

    /// 校验③-c 反例：同一 (bus, addr) 双占——I2C 地址是独立资源维，同总线上两条
    /// 绑定抢一个从地址即假账（"none" 是显式不适值，不占地址维不误报）。GOOD 只有
    /// servo 一条 pin，这里成对追加两条同 (i2c1, 0x40) 的 meter 才构成双占。
    #[test]
    fn two_pins_same_bus_addr_is_rejected() {
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"power.battery.meter\"\ncontroller = \"i2c1\"\n\
             addr = \"0x40\"\ndirection = \"in\"\nmux_group = []\n\
             [[pin]]\nrole = \"power.battery.meter\"\ncontroller = \"i2c1\"\n\
             addr = \"0x40\"\ndirection = \"in\"\nmux_group = []\n"
        );
        let root = Root::new("dup-addr");
        let p = root.write_profile("dup-addr.toml", &text);
        let e = boot_inputs(&p).expect_err("(bus, addr) 双占必须拒 boot");
        assert!(e.contains("校验③-c") && e.contains("(bus i2c1, addr 0x40)"), "{e}");
    }

    /// 校验④-b 反例：两条 pin 的 physical 命中平台 conflicts 同一子集（esp32 组
    /// i2c0 = {21, 22}）而 mux_group 都未申报该组 ⇒ 漏声明即红；组名取自平台表
    /// （装机档不得自起名，否则 mux_group 退回自愿填写）。
    #[test]
    fn conflict_subset_without_mux_group_is_rejected() {
        let text = format!(
            "{ESP32_GOOD}\n[[pin]]\nrole = \"act.led.gpio\"\ncontroller = \"gpio0\"\n\
             physical = [22]\ndirection = \"out\"\nrail = \"none\"\nfailsafe = \"hold\"\n\
             mux_group = []\n"
        )
        .replace("physical = [32]", "physical = [21]");
        let e = rr3_profile_from_text_with_platform("conflict-miss", &text, platform_esp32_text())
            .expect_err("命中 conflicts 子集而未申报同组必须拒 boot");
        assert!(
            e.contains("mux_group") && e.contains("conflicts") && e.contains("校验④-b"),
            "{e}"
        );
        // 漏声明的两条各点名一条（成员成对才成形，点名到每一条漏报者）
        assert_eq!(e.matches("校验④-b").count(), 2, "{e}");
    }

    /// 校验④-b 正例：同一冲突子集的两条 pin 都申报平台组名（mux_group = ["i2c0"]）
    /// → 放行。平台表超出现役绑定的成员（18/19 组）不算红——芯片事实 ≠ 现役占用，
    /// 单向含入语义。
    #[test]
    fn conflict_subset_with_mux_group_passes() {
        let text = format!(
            "{ESP32_GOOD}\n[[pin]]\nrole = \"act.led.gpio\"\ncontroller = \"gpio0\"\n\
             physical = [22]\ndirection = \"out\"\nrail = \"none\"\nfailsafe = \"hold\"\n\
             mux_group = [\"i2c0\"]\n"
        )
        .replace("physical = [32]", "physical = [21]")
        .replace("mux_group = []\n\n[[pin]]\nrole = \"power.battery.meter\"", "mux_group = [\"i2c0\"]\n\n[[pin]]\nrole = \"power.battery.meter\"");
        rr3_profile_from_text_with_platform("conflict-ok", &text, platform_esp32_text())
            .expect("同组申报的两条命中者必须放行");
    }
}
