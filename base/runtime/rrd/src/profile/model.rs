//! 装配单数据模型（B9 结构收口，自 `profile.rs` 按"模型/解析/装载"三分）。
//!
//! 本文件只放**形状**：schema 版本、段键词表、三档 tier 词表、八个引脚派生键、
//! [[pin]]/[[supplies.domain]] 字段表，以及 [`Rr3Supplies`]/[`Rr3Budget`]/
//! [`Rr3Domain`]/[`Rr3PinEntry`]/[`Rr3Profile`]/[`Rr3ProfileBoot`] 六型与
//! [`Rr3PinEntry`] 的渲染小件。判据（缺格/档位/预算/引脚闭合）不在这里——
//! 归 `profile_checks.rs`；文本解读归 `parse.rs`；装载链归 `boot.rs`。

use std::collections::BTreeMap;
use std::path::PathBuf;

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

/// 只认 schema_version = 5（4→5 为破坏性变更：`[runtime]` rrd 运行参数段进场——
/// 首键 `log_ring_cap` 日志环容量，批N5 件二 F-30"环容量 ∈ 装配单键集、装载期缺
/// 即红"；新必填段随版本 bump，批N2 [supplies.budget] 3→4 先例；注意与
/// `RR3_ASSEMBLE_SCHEMA_VERSION`（rr-registry，装配清单域）是两个域，不混写）。
/// 不匹配 = 契约事故，拒启动。
pub(super) const RR3_PROFILE_SCHEMA_VERSION: i64 = 5;

/// [runtime] 键（rrd 运行参数段，批N5 件二）——未知键点名与解析白名单共用。
/// - `log_ring_cap`：日志环容量（行数），rrd 自身运维参数。
/// - `hal_backend`：**必填**，装配单声明的全局 HAL 后端（sim/linux/esp32/mac/
///   placeholder）。P0-3 根因门：后端选择是部署事实，必须进装配单显式数据，
///   rrd 装载期校验并注入子进程 `RR3_HAL_BACKEND`——消除"漏 env 静默落 sim"
///   的生产假验证。host 档须 `linux`，组合档全局仍 `linux`（音频面另由
///   `audio_backend` 覆盖）。
/// - `audio_backend`：可选，音频采集面后端覆盖（组合档 Pi×ESP32 用 `esp32`，
///   注入子进程 `RR3_HAL_AUDIO_BACKEND`；host-only 档不设，回落全局 `hal_backend`）。
/// - `servo_backend`：可选，舵机面后端覆盖（组合档舵机在 ESP32 板上用 `esp32`，
///   注入子进程 `RR3_HAL_SERVO_BACKEND`；host-only 档不设回落全局）。
/// - `display_backend`：可选，显示面后端覆盖（组合档 OLED/点阵在 ESP32 板上用
///   `esp32`，注入子进程 `RR3_HAL_DISPLAY_BACKEND`；host-only 档不设回落全局）。
pub(super) const RR3_RUNTIME_KEYS: [&str; 5] = [
    "log_ring_cap",
    "hal_backend",
    "audio_backend",
    "servo_backend",
    "display_backend",
];

/// HAL 后端名白名单（与 rr-hal backend.rs 的 RR3_BACKEND_* 常量同域；rrd 不直接
/// 依赖 rr-hal 符号以保持基座轻量，此处独立钉合法取值集，装载期 fail-closed）。
pub(super) const RR3_HAL_BACKEND_LEGAL: [&str; 5] = ["sim", "linux", "esp32", "mac", "placeholder"];

/// 日志环容量上界（行，本轮静态审计）：环按行常驻内存，容量键此前只有"正整数"
/// 下界——装配单被写坏成一个天文数字时，rrd 会随日志量增长直至吃光板载内存，
/// 且该值经装配侧写口落盘后跨重启常驻。65_536 行（默认 512 的 128 倍）足够
/// 覆盖现场排障，超界即装配单值域错误，装载期点名拒。
pub(super) const RR3_LOG_CAP_MAX: i64 = 65_536;

/// [supplies] 六键（ADR-0018 修订 4 ② 看门狗三键 + 修订 2 ③ 时序预算位三键）——
/// 未知键点名与解析白名单共用（budget 子段单独成块解析，同 domain）
pub(super) const RR3_SUPPLIES_KEYS: [&str; 6] = [
    "watchdog_present",
    "watchdog_timeout_ms",
    "watchdog_feeder",
    "tick_period_ms",
    "tick_jitter_ms",
    "safety_core_interval_ms",
];

/// [supplies.budget] 四键（校验⑩ 分母，全案 §5 判据⑩ / §4.6 定形裁定③：栈/堆
/// 预算不住 platform 表，住 [supplies].budget——三份数据三个主人，ADR-0021 修订 3）。
/// `not_applicable` 是"合法不适用"的显式声明（host 档校验⑩ 不适用必须显式写 true，
/// 不许留空——F-6）；三数键 host 档不得并存（自相矛盾红）、rtos/bare 档必填。
pub(super) const RR3_BUDGET_KEYS: [&str; 4] = [
    "not_applicable",
    "task_stack_budget_bytes",
    "heap_total_bytes",
    "heap_reserved_bytes",
];

/// [platform] tier 三档（ADR-0018 修订 1：承载二分 → 三档）
pub(super) const RR3_PLATFORM_TIERS: [&str; 3] = ["host", "rtos", "bare"];

/// 描述符物理键集（增补轮六 §4.2：自 [hardware] 下移 [pins] 派生的**八个**引脚侧键——
/// 全案 §4.2/§8 门 n 写"六个"但枚举列了七键，以枚举为准；第八键 `spi_device` 批N3
/// 件六进场（敞口 18 行收口：spidev<N> 词形 + channel 派生 /dev/spidev<N>.<M>））。
/// [hardware] 出现任一即红（门 n 稳态判据）；注入文本键名不变，rr-hal 描述符解析端零改动。
pub(crate) const RR3_PIN_DERIVED_KEYS: [&str; 8] = [
    "pwm_chip",
    "pwm_channel",
    "gpio_chip",
    "gpio_led_line",
    "gpio_relay_line",
    "i2c_bus",
    "ina219_addr",
    "spi_device",
];

/// [[pin]] 条目字段词表（§4.2 [pins] 段；role 的 requires 域即此表子集——
/// 门 a 六表对账的第 5 张表）。批N3 件八起十一键：`device` 器件型号词进场
/// （外键 → contracts/devices.toml，门 o 装载侧执法 + 校验③-c 精化判据字段）。
/// crate 内可见：assembly.rs 渲染完整性测试用门 d 同源词表逐键断言（不抄第二份清单）。
pub(crate) const RR3_PIN_KEYS: [&str; 11] = [
    "role",
    "instance",
    "controller",
    "channel",
    "physical",
    "direction",
    "rail",
    "addr",
    "device",
    "failsafe",
    "mux_group",
];

/// [[supplies.domain]] 条目字段词表（§3.1 家族 A ④ 列 + §5 校验⑥）：域名与阈值三键、
/// 域电流上限、三个绑定引用位（sense/current_sense/switch 一律写 pin_id）与开关/
/// 武装/龄期三键，批N21 件二再加武装证据引用键（⑥-e）。全键必填——"合法不适用"
/// 显式写 `none`/`false`/`0`，留空即红
/// （§5 缺失语义列，同 no_injection 范式）；`max_age_ms = 0` 且 `armed = true`
/// 无运行期读者，由校验⑥-c 拒；`evidence` 与 `armed` 成对：未武装显式写 `"none"`、
/// 武装必须携形制合法的证据引用（文件侧四道核在 [`rr3_evidence_check`]，
/// 由 boot 装载链持仓内容器根调用）。crate 内可见：同 [`RR3_PIN_KEYS`]（assembly.rs
/// 渲染完整性测试同源引用）。
pub(crate) const RR3_DOMAIN_KEYS: [&str; 12] = [
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
    "evidence",
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
    /// [supplies.budget] 内存预算（校验⑩ 分母，schema 4；⑩-a/⑩-b 的判据分母，
    /// 分子 = [assemble] 内存申报，见 [`rr3_memory_budget`]）
    pub budget: Rr3Budget,
}

/// [supplies.budget]：平台内存预算（校验⑩ 分母，不可变快照；ADR-0021 修订 3——
/// 板侧常量数据化的住处，与 board.rs 真值对账归点验矩阵/真板批）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rr3Budget {
    /// host 档"合法不适用"显式态（校验⑩ 对 host 不适用必须显式写 true，不许
    /// 留空——F-6；与三数键互斥：并存 = 自相矛盾红）
    pub not_applicable: bool,
    /// 任务栈总预算字节（⑩-a 分母：Σ task_stack_bytes ≤ 本值）
    pub task_stack_budget_bytes: i64,
    /// 平台可用堆字节（⑩-b 分母基数）
    pub heap_total_bytes: i64,
    /// 平台常驻占用字节（⑩-b 分母减项：可用堆 = total − reserved——板侧常量
    /// 如 UART2_TX_RING=16384 的数据化住处，F-13）
    pub heap_reserved_bytes: i64,
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
    /// 武装证据引用（⑥-e，批N21 件二：D11 界面门的装载期半边）——`<形态>/<四位编号>`
    /// 形制（规则住 conventions §6.1，机制真值 rr-ipc::evidence）。与 `armed` 成对：
    /// 未武装显式写 `"none"`；武装必须携引用——形制核在解析期（本模块 ⑥-e），文件侧
    /// 四道核（存在/frontmatter/form 一致）在 [`rr3_evidence_check`]，
    /// boot 装载链持仓内容器根（仓内 `docs/evidence`）调用。
    pub evidence: String,
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
    /// <chip>.toml 的 [[controller]]，批N1 基-22 已在册；host 档词形派生的前缀
    /// 同源读该表 [controller_wordform] 段，实-6 (a)）
    pub controller: String,
    /// 控制器通道（独立资源维度，校验③-b 的 (controller, channel) 不双占空间）
    pub channel: Option<i64>,
    /// 物理引脚数组（一角色可占多脚；GPIO 类派生取 physical[0]）
    pub physical: Vec<i64>,
    /// 方向（须与 role.direction 相等）
    pub direction: String,
    /// 供电轨名（kind=power 角色必填且须 ∈ [supplies].domains，校验⑥；信号侧控制线
    /// 显式写 "none"——留空即红，与 sense 的显式范式同型）。domains 已随批N6 立册，
    /// 校验⑥-a 判在册
    pub rail: String,
    /// I2C 从地址（0x 前缀十六进制；power.battery.meter 类角色用，校验⑨ 互校随批N2）
    pub addr: String,
    /// 器件型号词（批N3 件八，外键 → contracts/devices.toml——门 o 装载侧执法 +
    /// 校验③-c 精化判据字段：同 (bus, addr) 的多条 pin 须同 device 且型号在册，
    /// P-5 共读源形态由此可判。空串 = 未给（requires 不含 device 的角色不填）
    pub device: String,
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
    /// [slots].device_absent：base 层格的器件缺席登记（格名 → 非空理由，D3 裁定 5）。
    /// "恒在位"是**通道**承诺不是**器件**承诺——板上没装电池时 power.battery 的通道
    /// 仍在（基座内建），但器件缺席须显式登记，校验⑥认它为合法态；retired 会让
    /// power.budget 的判据前件二次落空（L-28 型），故改层不 retired。
    pub slots_device_absent: BTreeMap<String, String>,
    /// [supplies]：平台供给声明（看门狗三键 + 时序预算位三键）
    pub supplies: Rr3Supplies,
    /// [hardware]：非引脚键直给（`alsa_device`/`link_ifname`；八物理键
    /// 出现即红——门 n 注入路径唯一，`spi_device` 批N3 件六起同进物理键集（敞口 18 行
    /// 收口），键位语义与取值域由 rr-hal 描述符解析裁，
    /// rrd 只管装配形状——标量字符串化，注入时不解释）
    pub hardware: BTreeMap<String, String>,
    /// [runtime].log_ring_cap：日志环容量（行数，批N5 件二 F-30）。rrd 自身的
    /// 运维参数——daemon 启动一次定格进日志环（lib.rs `rr3_log_ring_cap_set`），
    /// 不进 [hardware] 注入面（不上桥）。正整数必填，缺即红。
    pub log_ring_cap: usize,
    /// [runtime].hal_backend：**必填**，装配单声明的全局 HAL 后端（P0-3 根因门）。
    /// daemon 注入子进程 `RR3_HAL_BACKEND`——后端选择是部署事实，必须进装配单
    /// 显式数据，不靠隐式缺省（漏 env 静默落 sim = 生产假验证）。host 档须
    /// `linux`，组合档全局仍 `linux`（音频面另由 audio_backend 覆盖）。
    pub hal_backend: String,
    /// [runtime].audio_backend：可选，音频采集面后端覆盖。组合档 Pi×ESP32 设
    /// `esp32`（注入子进程 `RR3_HAL_AUDIO_BACKEND`，音频面独占 UART）；host-only
    /// 档不设，音频面回落全局 hal_backend。
    pub audio_backend: Option<String>,
    /// [runtime].servo_backend：可选，舵机面后端覆盖（P0-4 组合档归属）。组合档
    /// 舵机在 ESP32 板上时设 `esp32`（注入子进程 `RR3_HAL_SERVO_BACKEND`）；
    /// host-only 档不设，舵机面回落全局 hal_backend。
    pub servo_backend: Option<String>,
    /// [runtime].display_backend：可选，显示面后端覆盖（P0-4 组合档归属）。组合档
    /// OLED/点阵在 ESP32 板上时设 `esp32`（注入子进程 `RR3_HAL_DISPLAY_BACKEND`）；
    /// host-only 档不设，显示面回落全局 hal_backend。
    pub display_backend: Option<String>,
    /// [[pin]]：物理绑定（引脚侧唯一真值点，增补轮六 §4.2）。可选段（纯 sim/无硬件
    /// 格形态不给——此时校验②不判，sim 形态无物理面可绑）
    pub pins: Vec<Rr3PinEntry>,
}

/// 装配链交给 daemon 的 boot 输入。`Eq` 已随批N4 件九笔4 摘除：`snapshot` 成员
/// 载 `Rr3Wiring`（`rated_a: f64` 只有 `PartialEq`），结构体的判等面不受 daemon
/// 任何消费点依赖（boot 输入只做 destructure 与 clone），摘 `Eq` 零行为漂移。
#[derive(Debug, Clone, PartialEq)]
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
    /// 执行轴状态注入文本（批N3 件三，`RR3_ACT_AXIS_STATE`）：`calibrated=<bool>,hard_stop=<词>`
    /// ——轴档案（`hardware/<形态>/axis/*.toml`，件二装载器真值）**恰一份**合法装载时
    /// 的投影，是 rr3-act 拒动判据 [`rr_hal_api::rr3_motion_allowed`] 的输入面。
    /// 空串 = 不注入（无档案/坏件/多轴未消歧），模块侧折保守缺省——§6 复位语义④：
    /// 缺真相就按最窄面起跑（未标定不得自动），拒绝不是安全向、保守才是。
    pub axis_state: String,
    /// 轴档案装载点名（坏件逐条 / 多份合法）：daemon 逐条日志，不拒 boot——档案装载
    /// 是降级面，与缺格/缺角色同档；拒绝启动面留给契约事故
    pub axis_notes: Vec<String>,
    /// [runtime].log_ring_cap（批N5 件二 F-30）：日志环容量（行数）。daemon 收货后
    /// 一次定格进 lib.rs 的 OnceLock（`rr3_log_ring_cap_set`，首份胜出）——
    /// `--manifest` 调试路径不读装配单，环按 512 兜底。
    pub log_ring_cap: usize,
    /// [runtime].hal_backend（P0-3 根因门）：装配单声明的全局 HAL 后端，daemon
    /// 注入子进程 `RR3_HAL_BACKEND`。空串 = --manifest 调试路径不注入（模块侧
    /// 走平台缺省，CI/本地开发可显式 env 覆盖）。
    pub hal_backend: String,
    /// [runtime].audio_backend：可选，音频采集面后端覆盖。Some 时 daemon 注入
    /// 子进程 `RR3_HAL_AUDIO_BACKEND`；None 不注入（音频面回落全局 hal_backend）。
    pub audio_backend: Option<String>,
    /// [runtime].servo_backend：可选，舵机面后端覆盖（P0-4 组合档归属）。Some 时
    /// daemon 注入子进程 `RR3_HAL_SERVO_BACKEND`；None 不注入（舵机面回落全局）。
    pub servo_backend: Option<String>,
    /// [runtime].display_backend：可选，显示面后端覆盖（P0-4 组合档归属）。Some 时
    /// daemon 注入子进程 `RR3_HAL_DISPLAY_BACKEND`；None 不注入（显示面回落全局）。
    pub display_backend: Option<String>,
    /// [platform].chip（基-40 boot 底账）：boot 链验收通过的选册 chip，真值集
    /// 记账（[`rr3_truth_locations`] 消费）按它定位平台事实表——
    /// daemon 臂不再二次解析装配单拿 chip（双真值即漂移温床）。
    pub platform_chip: String,
    /// [platform].tier（host/rtos/bare）：启动摘要与排障面回显用；同源于
    /// [`Rr3Profile::platform_tier`]，不二次解析装配单。
    pub platform_tier: String,
    /// 装配快照（批N4 件九笔4，`rr3.ctl.wiring` 判据源）：boot 装载链验收通过后
    /// 的四元不可变底账（装配单本体 / 接线档 / 轴档案 / 角色表）。None = 本 boot
    /// 未走装配单路径（`--manifest` 调试路径）或接线档尚未在册（快照无接线档即
    /// 不成形，如实缺位不造假）。daemon 收货后一次定格进 OnceLock（assembly.rs
    /// 单点），boot 函数自身不碰全局态——纯构造，快照住处由调用方拍。
    pub snapshot: Option<crate::assembly::Rr3AssemblySnapshot>,
}
