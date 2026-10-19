//! rr3-power 组合件实现（组合件五件套③，0.0.5 批B③ 原子重铺打样）。
//!
//! 组合件 = battery ⊗ charger（composite.toml 的 ⊗ 链；⊗=装配期、执行单元内
//! 函数调用，ADR-0019 裁定 2）。本文件承载装配与运行机制：config 解析（阈值等
//! 策略经 config 数据化注入，机制件零策略载荷）、HAL 电源面开机探活、主循环
//! 四拍（battery 拍 / 低电边沿 / charger 派生拍 / rail 域压拍——批N2 件五）。
//!
//! 启动序列（自原入口迁入，语义零改动）：读 `RR3_MANIFEST`（rrd spawn 注入的
//! manifest 绝对路径，supervisor.rs）→ [`Rr3BusClient::rr3_connect`] → 读 config.toml
//! → 选 HAL 电源面并**开机探活**（Unsupported 即 exit(1)：能力缺失显式回错，不静默
//! 带病常驻）→ 主循环。本模块没有事务面（module.toml 无 [rpc].exposes），主循环只做
//! 四件事（第 4 件 `rail.up = true` 才跑）：
//! 1. 每 `sample_period_ms` 读一次 HAL 电池 → 发 `battery/state`（首拍即采）；
//! 2. 电压越过低电阈值的第一拍发 `battery/low`（Rr3LowEdge：同一次低电段恰一次，
//!    恢复再武装）——rrd 订阅它执行进程内兜底（Q4 裁定），brain 不在场也要能触发；
//! 3. 每 `state_period_ms` 发 `charger/state`（由最近一次电池读数的 charging 位派生）；
//! 4. 每 `rail.sample_period_ms` 读一次域压 → ⑥-c2 按龄期折有效档位 → 判定核 →
//!    发 `rail/state`，越阈首拍出边沿守护行（批N2 件五，P-9 归属搬家）。
//!
//! 失败语义与 sense/interact/act 同一口径：本模块没有调用方，周期采样里**全部**失败
//! 都是传输性/设备级——HAL 读数失败、发布写不出去都 `eprintln` + `exit(1)`，不装
//! "活着"，死交给 supervisor 按 restart_policy=always 重启。订阅面为空是**设计**
//! （module.toml 同条注）：低压关断是本地决策，不依赖总线可达。
//!
//! cutoff 电压阈值**声明未生效**（0.0.5 批M MC-8① 裁定 (c)）：0.0.5 **不关断**
//! （关断动作归 M6 关机链，需 T1/T2 物理断电硬件）。本键保留为**策略数据**、机器
//! 读者 = 注入门（`atoms/battery/assemble.toml` 的 ops 槽，⑧-c 门仓级强制其按点路径
//! 可取），启动日志显式标注"声明未生效"，避免被读成"已实现关断"；本轮唯一低电保护
//! 动作 = MC-5 低电禁动（触发阈 = `low_voltage_mv`，见方案 §20 第 2 条）。

#[path = "../atoms/battery/battery.rs"]
mod battery;
#[path = "../atoms/charger/charger.rs"]
mod charger;

use std::time::{Duration, Instant};

use rr_bus::Rr3BusClient;
use rr_hal::backend::{rr3_hal_power, rr3_hal_rail};
use rr_hal_api::{rr3_rail_tier_effective, Rr3BatteryState, Rr3RailState, Rr3RailTier};
use serde_json::{json, Value};

use battery::{Rr3BatteryCfg, Rr3LowEdge, RR3_BATTERY_LOW_TOPIC, RR3_BATTERY_STATE_TOPIC};
use charger::{Rr3ChargerCfg, RR3_CHARGER_STATE_TOPIC};

/// 电源域电平观测主题（contracts/topics.toml 在册，批N2 件二落的）
const RR3_RAIL_STATE_TOPIC: &str = "rr3-power/rail/state";

/// rrd spawn 时注入的 manifest 绝对路径（supervisor.rs；rr-ipc 尚无常量，模块侧本地定义）
const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml）
const RR3_POWER_CONFIG_ENV: &str = "RR3_POWER_CONFIG";

/// 收帧等待上界兼循环节拍：广播全量扇出会送来与己无关的帧，排空即可，
/// 采样/充电拍点由 Instant 比较驱动，与帧到达无关
const RR3_POLL_WAIT: Duration = Duration::from_millis(100);

/// 组合件入口：装配原子件并跑主循环（原 main.rs `rr3_run` 逐字迁入，打印序列不变）。
pub fn rr3_power_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    eprintln!("rr3-power 0.0.1 (0.0.2 T4b 实装)");
    for line in battery::SURFACE
        .iter()
        .copied()
        .chain(charger::SURFACE.iter().copied())
    {
        eprintln!("  {line}");
    }

    // 总线接入：manifest 路径是 rrd spawn 的契约环境变量，直跑（开发调试）须自行设置
    let manifest_path = std::env::var(RR3_MANIFEST_ENV).map_err(|_| {
        format!("缺环境变量 {RR3_MANIFEST_ENV}（rrd spawn 注入；直跑请指向 module.toml）")
    })?;
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读 manifest {manifest_path} 失败: {e}"))?;
    let bus = Rr3BusClient::rr3_connect(&manifest_text)?;
    eprintln!("总线就绪: name={} task={}", bus.rr3_name(), bus.rr3_task());

    let cfg = rr3_config_load(&manifest_path)?;
    // cutoff 显式标注"声明未生效"（MC-8① 裁定 (c)）：回显 ≠ 读者，本行只为运维
    // 自述，不构成"已实现关断"的宣称（关断动作归 M6）。
    eprintln!(
        "config 就绪: battery sample={}ms low={}mV charger state={}ms | cutoff={}mV（声明未生效：本版不关断，归 M6）",
        cfg.battery.sample_period_ms,
        cfg.battery.low_voltage_mv,
        cfg.charger.state_period_ms,
        cfg.battery.cutoff_voltage_mv,
    );
    // rail 段回显（批N2 件五）：独立一行，不动上面的 config 就绪行（逐字红线）
    if cfg.rail.up {
        eprintln!(
            "rail 监测就绪: domain={} mild={}mV severe={}mV max_age={}ms period={}ms",
            cfg.rail.domain,
            cfg.rail.mild_mv,
            cfg.rail.severe_mv,
            cfg.rail.max_age_ms,
            cfg.rail.sample_period_ms
        );
    } else {
        eprintln!("rail 未启用（up=false，合法不适用显式写）");
    }

    // HAL 电源面选择 + 开机探活：Unsupported 即致命——显式回错不静默退化
    // （rr-hal 占位件的契约），也不给"没有电池读数还常驻"的假活
    let (dev, backend) = rr3_hal_power();
    let first = dev
        .battery()
        .map_err(|e| format!("电源面探活失败({backend}): {e}"))?;
    eprintln!(
        "电源面就绪: {backend} 电量 {}% {}mV charging={}",
        first.percent, first.voltage_mv, first.charging
    );

    // 轨压面选择 + 开机探活（批N2 件五）：`rail.up = true` 时能力缺失显式回错，
    // 不静默带病常驻（与电源面同一 fail-closed 口径；今日全后端占位，宿主真件归
    // 0.0.7 敞口 12 行——up=false 是 host 档合法不适用，无测点不探活）
    let (rail_dev, rail_backend) = rr3_hal_rail();
    if cfg.rail.up {
        rail_dev
            .rr3_read(&cfg.rail.domain)
            .map_err(|e| format!("轨压面探活失败({rail_backend}): {e}"))?;
        eprintln!("轨压面就绪: {rail_backend} domain={}", cfg.rail.domain);
    }

    // 主循环状态：各流 seq 各自计数、各拍点、低电边沿机（首拍即武装）、轨压判定核
    let mut battery_seq: u64 = 0;
    let mut charger_seq: u64 = 0;
    let mut low_seq: u64 = 0;
    let mut rail_seq: u64 = 0;
    let mut low_edge = Rr3LowEdge::rr3_low_edge_new(cfg.battery.low_voltage_mv);
    let mut rail_judge = Rr3RailJudge::rr3_rail_judge_new(cfg.rail.mild_mv, cfg.rail.severe_mv);
    let mut next_battery = Instant::now(); // 首拍即采：兜底链路不等第一个周期
    let mut next_charger = Instant::now();
    let mut next_rail = Instant::now();
    let mut last = Some(first);

    loop {
        // 总线死亡探针：断开后 poll_wait 立即返回空 Vec，与"暂时没帧"不可区分——
        // 不探测，总线一死本循环就满转成僵尸。假了就走"不装活着"的退出语义。
        if !bus.rr3_connected() {
            return Err("总线已断开（reader EOF）：交 supervisor 按 restart_policy 裁定".into());
        }
        // 订阅面为空：广播全量扇出送到的帧（module/state 等）只排空不处理
        for frame in bus.rr3_poll_wait(RR3_POLL_WAIT) {
            let _ = frame;
        }

        // battery 拍：读数 → 周期流 → 低电边沿。HAL 读数失败 = 设备/传输级，exit(1)；
        // 失败先计入 counters（0.0.5 批I P-2：数据新鲜度可观测），再走退出语义。
        let now = Instant::now();
        if now >= next_battery {
            let st = match dev.battery() {
                Ok(st) => st,
                Err(e) => {
                    bus.rr3_counter_bump("battery_read_fail", 1);
                    return Err(format!("电池读数失败（{backend}）: {e}"));
                }
            };
            battery_seq += 1;
            bus.rr3_publish(
                RR3_BATTERY_STATE_TOPIC,
                rr3_battery_state_body(battery_seq, st),
            )?;
            if low_edge.rr3_low_edge_cross(st.voltage_mv) {
                low_seq += 1;
                bus.rr3_publish(RR3_BATTERY_LOW_TOPIC, rr3_battery_low_body(low_seq, st))?;
                eprintln!(
                    "battery/low: {}% {}mV（越阈首拍，段内不重发）",
                    st.percent, st.voltage_mv
                );
            }
            last = Some(st);
            next_battery = now + Duration::from_millis(cfg.battery.sample_period_ms);
        }

        // charger 拍：由最近一次电池读数派生；首拍 battery 之前不发（读数还没着落）
        if now >= next_charger {
            if let Some(st) = last {
                charger_seq += 1;
                bus.rr3_publish(
                    RR3_CHARGER_STATE_TOPIC,
                    rr3_charger_state_body(charger_seq, st.charging),
                )?;
            }
            next_charger = now + Duration::from_millis(cfg.charger.state_period_ms);
        }

        // rail 拍（批N2 件五）：读域压 → ⑥-c2 按龄期折有效档位 → 判定核 → 周期流
        // ＋边沿守护行。HAL 读数失败同 battery_read_fail 纪律：counter 先行再退——
        // 失败是设备/传输级，exit(1) 不装活着。
        if cfg.rail.up && now >= next_rail {
            let st = match rail_dev.rr3_read(&cfg.rail.domain) {
                Ok(st) => st,
                Err(e) => {
                    bus.rr3_counter_bump("rail_read_fail", 1);
                    return Err(format!("轨压读数失败（{rail_backend}）: {e}"));
                }
            };
            rail_seq += 1;
            let (tier, event) = rr3_rail_beat(&mut rail_judge, st, cfg.rail.max_age_ms);
            bus.rr3_publish(
                RR3_RAIL_STATE_TOPIC,
                rr3_rail_state_body(rail_seq, st.mv, st.age_ms, tier.rr3_rail_tier_name()),
            )?;
            if event == Some(true) {
                bus.rr3_counter_bump("rail_edge", 1);
                eprintln!(
                    "rail/edge: {}mV tier={}（越阈首拍，段内不重发）",
                    st.mv,
                    tier.rr3_rail_tier_name()
                );
            }
            next_rail = now + Duration::from_millis(cfg.rail.sample_period_ms);
        }
    }
}

// ---------- 载荷组装面（0.0.5 批I P-7：上移承载层，ADR-0019 修订 4） ----------
// 件的对外契约面 = 结构体 + 判定函数；`*_body` 的 json! 组装归本文件，
// 线上字节逐字不变（schema/golden 用例随迁到本文件，继续守字节面）。

/// battery/state 流体：seq 是本流自己的计数（envelope 惯例，从 1 起）
fn rr3_battery_state_body(seq: u64, st: Rr3BatteryState) -> Value {
    json!({
        "seq": seq,
        "voltage_mv": st.voltage_mv,
        "percent": st.percent,
        "charging": st.charging,
    })
}

/// battery/low 事件体：形状钉在 contracts/events/power-battery-low.schema.json
/// （seq/percent/voltage_mv 三必填，additionalProperties=false——不加多余字段）。
fn rr3_battery_low_body(seq: u64, st: Rr3BatteryState) -> Value {
    json!({
        "seq": seq,
        "percent": st.percent,
        "voltage_mv": st.voltage_mv,
    })
}

/// charger/state 流体：charging 是最近一次 battery 读数的派生值
fn rr3_charger_state_body(seq: u64, charging: bool) -> Value {
    json!({ "seq": seq, "charging": charging })
}

/// rail/state 流体（批N2 件五）：四键 `{seq, mv, age_ms, tier}`——tier 串走
/// [`Rr3RailTier::rr3_rail_tier_name`] 同源词表（fresh 拍 = 判定核分类值，Unknown
/// 拍 = "unknown" 原样，展示面不折）。无独立 schema json，四键不加戏。
fn rr3_rail_state_body(seq: u64, mv: u16, age_ms: u32, tier_name: &str) -> Value {
    json!({
        "seq": seq,
        "mv": mv,
        "age_ms": age_ms,
        "tier": tier_name,
    })
}

// ---------- 轨压档位机制切片（批N2 件五，P-9 归属搬家——机制不动） ----------
// 板侧真值 = tools/esp32-periph-base/src/power.rs 的 `tier_of`/`RailJudge`，逐字
// 对照迁入组合件层；板侧文件不改（rtos 档自持）。搬家后两条线同源一个判定核：
// 电平管档位（每拍分类），边沿管事件（每低电段首拍报一次），共用同一对阈值，
// "向下档变"与"边沿事件"逐拍等价。阈值策略量由 config 注入，机制零策略载荷。

/// 电平分类（纯 mV 比较，板侧 `tier_of` 对照）：`mv <= severe ⇒ Severe`、
/// `mv <= mild ⇒ Mild`、否则 Ok。不含迟滞/事件语义（事件走 [`Rr3RailJudge`]）。
fn rr3_rail_tier_of(mv: u16, mild_mv: u16, severe_mv: u16) -> Rr3RailTier {
    if mv <= severe_mv {
        Rr3RailTier::Severe
    } else if mv <= mild_mv {
        Rr3RailTier::Mild
    } else {
        Rr3RailTier::Ok
    }
}

/// 欠压判定核（板侧 `RailJudge` 对照，**一核两条线**）：mild/severe 两条迟滞边沿
/// 各由 [`Rr3LowEdge`] 一实例承载，不另写第二个边沿点（裁定 5：调用方只能喂电压
/// 不能改语义）。event = 两线任一低电段首拍，逐拍等价于向下档变（Ok→Mild 轻度线
/// 报；Ok→Severe / Mild→Severe 重度线报；回阈不报，只是再武装）。
struct Rr3RailJudge {
    mild: Rr3LowEdge,
    severe: Rr3LowEdge,
    mild_mv: u16,
    severe_mv: u16,
}

impl Rr3RailJudge {
    /// 装配：两线阈值取 config rail 段（重度 < 轻度，解析面已执法倒挂）
    fn rr3_rail_judge_new(mild_mv: u16, severe_mv: u16) -> Self {
        Self {
            mild: Rr3LowEdge::rr3_low_edge_new(mild_mv),
            severe: Rr3LowEdge::rr3_low_edge_new(severe_mv),
            mild_mv,
            severe_mv,
        }
    }

    /// 喂一拍轨压 mV：返回 `(本拍电平档位, 是否越阈首发事件)`
    fn rr3_rail_feed(&mut self, mv: u16) -> (Rr3RailTier, bool) {
        let mild_hit = self.mild.rr3_low_edge_cross(mv);
        let severe_hit = self.severe.rr3_low_edge_cross(mv);
        (
            rr3_rail_tier_of(mv, self.mild_mv, self.severe_mv),
            mild_hit || severe_hit,
        )
    }
}

/// ⑥-c2 消费执法可测点（纯函数，批N2 件五）：先按采样龄期折有效档位，再过判定核。
///
/// - 有效档 = Unknown ⇒ `(Unknown, None)`：**不喂判定核**（执行面保持置位），
///   产出侧已折的 Unknown 原样透传——mv 再好也不许从 mv 重分类出 Ok；
/// - 否则 `(tier, Some(event)) = judge.rr3_rail_feed(st.mv)`：判定核 tier 与后端
///   `st.tier` 在同域阈值下一致（机制产出）；0.0.7 真件对账归敞口 12 行。
fn rr3_rail_beat(
    judge: &mut Rr3RailJudge,
    st: Rr3RailState,
    max_age_ms: u32,
) -> (Rr3RailTier, Option<bool>) {
    let effective = rr3_rail_tier_effective(st.tier, Some(st.age_ms), max_age_ms);
    if effective == Rr3RailTier::Unknown {
        return (Rr3RailTier::Unknown, None);
    }
    let (tier, event) = judge.rr3_rail_feed(st.mv);
    (tier, Some(event))
}

/// config.toml 解析：env 覆盖优先，缺省 manifest 同目录。
fn rr3_config_load(manifest_path: &str) -> Result<Rr3PowerCfg, String> {
    let path = std::env::var(RR3_POWER_CONFIG_ENV).unwrap_or_else(|_| {
        std::path::Path::new(manifest_path)
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("config.toml")
            .to_string_lossy()
            .into_owned()
    });
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读 config {path} 失败: {e}"))?;
    rr3_power_config_parse(&text)
}

/// 本模块 config 的汇总形状
#[derive(Debug, Clone)]
struct Rr3PowerCfg {
    battery: Rr3BatteryCfg,
    charger: Rr3ChargerCfg,
    rail: Rr3RailCfg,
}

/// rail 段形状（批N2 件五）：段必填＋全部键必填——`up = false` 时其余键为惰性
/// 但显式写（F-6 精神：合法不适用也要写出来，统一解析路径无分支）；`max_age_ms
/// = 0` 合法（0 = 龄期恒超界 ⇒ 恒 Unknown，诚实语义）；`severe_mv < mild_mv`
/// 严格校验（相等也红——Mild 档不可达）。
#[derive(Debug, Clone)]
struct Rr3RailCfg {
    up: bool,
    domain: String,
    sample_period_ms: u64,
    mild_mv: u16,
    severe_mv: u16,
    max_age_ms: u32,
}

/// TOML 文本 → Rr3PowerCfg。手工下钻不引 serde derive：字段个位数，
/// 点名缺字段的文案比派生宏的默认报错可读（rr3-act 同款）。
fn rr3_power_config_parse(text: &str) -> Result<Rr3PowerCfg, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("config TOML 解析失败: {e}"))?;
    let sect = |name: &str| -> Result<&toml::Value, String> {
        v.get("rr3-power")
            .and_then(|s| s.get(name))
            .ok_or_else(|| format!("config 缺 [rr3-power.{name}] 段"))
    };
    let u16_of = |sect: &toml::Value, key: &str, sect_name: &str| -> Result<u16, String> {
        sect.get(key)
            .and_then(toml::Value::as_integer)
            .filter(|n| *n >= 0 && *n <= u16::MAX as i64)
            .map(|n| n as u16)
            .ok_or_else(|| format!("config [rr3-power.{sect_name}] 缺 {key}（0–65535 整数）"))
    };
    let u32_of = |sect: &toml::Value, key: &str, sect_name: &str| -> Result<u32, String> {
        sect.get(key)
            .and_then(toml::Value::as_integer)
            .filter(|n| *n >= 0 && *n <= u32::MAX as i64)
            .map(|n| n as u32)
            .ok_or_else(|| format!("config [rr3-power.{sect_name}] 缺 {key}（0–4294967295 整数）"))
    };
    let u64_of = |sect: &toml::Value, key: &str, sect_name: &str| -> Result<u64, String> {
        sect.get(key)
            .and_then(toml::Value::as_integer)
            .filter(|n| *n >= 0)
            .map(|n| n as u64)
            .ok_or_else(|| format!("config [rr3-power.{sect_name}] 缺 {key}（非负整数）"))
    };
    let battery_sect = sect("battery")?.clone();
    let charger_sect = sect("charger")?.clone();
    // 周期键钳制留痕（0.0.5 批I P-2）：0 无节拍意义，收敛 1ms 并打一行
    // （哪个键、原值、钳后值）——钳位不再静默
    let period_of = |sect: &toml::Value, key: &str, sect_name: &str| -> Result<u64, String> {
        let raw = u64_of(sect, key, sect_name)?;
        if raw < 1 {
            eprintln!("config [rr3-power.{sect_name}] {key}={raw} 无节拍意义，钳到 1ms");
            Ok(1)
        } else {
            Ok(raw)
        }
    };
    // battery/charger 先解析（既有红例的次序契约：缺字段点名先于 rail 段点名），
    // rail 段（批N2 件五）殿后——段必填＋全键必填，up=false 惰性但显式写
    let battery = Rr3BatteryCfg {
        sample_period_ms: period_of(&battery_sect, "sample_period_ms", "battery")?,
        low_voltage_mv: u16_of(&battery_sect, "low_voltage_mv", "battery")?,
        cutoff_voltage_mv: u16_of(&battery_sect, "cutoff_voltage_mv", "battery")?,
    };
    let charger = Rr3ChargerCfg {
        state_period_ms: period_of(&charger_sect, "state_period_ms", "charger")?,
    };
    let rail_sect = sect("rail")?.clone();
    let rail = Rr3RailCfg {
        up: rail_sect
            .get("up")
            .and_then(toml::Value::as_bool)
            .ok_or_else(|| "config [rr3-power.rail] 缺 up（布尔）".to_string())?,
        domain: rail_sect
            .get("domain")
            .and_then(toml::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .ok_or_else(|| "config [rr3-power.rail] 缺 domain（非空字符串）".to_string())?,
        sample_period_ms: period_of(&rail_sect, "sample_period_ms", "rail")?,
        mild_mv: u16_of(&rail_sect, "mild_mv", "rail")?,
        severe_mv: u16_of(&rail_sect, "severe_mv", "rail")?,
        max_age_ms: u32_of(&rail_sect, "max_age_ms", "rail")?,
    };
    if rail.severe_mv >= rail.mild_mv {
        return Err(format!(
            "config [rr3-power.rail] 阈值倒挂: severe={}mV 必须 < mild={}mV（相等则 Mild 档不可达）",
            rail.severe_mv, rail.mild_mv
        ));
    }
    Ok(Rr3PowerCfg {
        battery,
        charger,
        rail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(mv: u16, percent: u8) -> Rr3BatteryState {
        Rr3BatteryState {
            voltage_mv: mv,
            percent,
            charging: false,
        }
    }

    #[test]
    fn state_body_carries_full_summary_with_own_seq() {
        let v = rr3_battery_state_body(7, st(3700, 50));
        assert_eq!(v["seq"], 7);
        assert_eq!(v["voltage_mv"], 3700);
        assert_eq!(v["percent"], 50);
        assert_eq!(v["charging"], false);
    }

    /// battery/low 体与 schema 逐字段对齐：三必填，且**没有**多余字段
    /// （additionalProperties=false，多带 charging 会直接违反契约）。
    #[test]
    fn low_body_matches_schema_exactly() {
        let v = rr3_battery_low_body(1, st(3250, 5));
        assert_eq!(v["seq"], 1);
        assert_eq!(v["percent"], 5);
        assert_eq!(v["voltage_mv"], 3250);
        assert_eq!(
            v.as_object().unwrap().len(),
            3,
            "schema additionalProperties=false，字段超集即违约: {v}"
        );
    }

    #[test]
    fn charger_body_carries_own_seq_and_charging() {
        let v = rr3_charger_state_body(3, true);
        assert_eq!(v["seq"], 3);
        assert_eq!(v["charging"], true);
        assert_eq!(v.as_object().unwrap().len(), 2, "两字段，不加戏: {v}");
    }

    /// 与仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红
    #[test]
    fn config_parse_covers_repo_shape() {
        let text = r#"
[rr3-power]
log_level = "info"

[rr3-power.battery]
sample_period_ms = 5000
low_voltage_mv = 3300
cutoff_voltage_mv = 3150

[rr3-power.charger]
state_period_ms = 2000

[rr3-power.rail]
up = false
domain = "rail5v"
sample_period_ms = 1000
mild_mv = 4600
severe_mv = 4400
max_age_ms = 0
"#;
        let cfg = rr3_power_config_parse(text).unwrap();
        assert_eq!(cfg.battery.sample_period_ms, 5000);
        assert_eq!(cfg.battery.low_voltage_mv, 3300);
        assert_eq!(cfg.battery.cutoff_voltage_mv, 3150);
        assert_eq!(cfg.charger.state_period_ms, 2000);
        // rail 段逐键（批N2 件五）：up=false 全键惰性但显式写，max_age_ms=0 合法
        assert!(!cfg.rail.up);
        assert_eq!(cfg.rail.domain, "rail5v");
        assert_eq!(cfg.rail.sample_period_ms, 1000);
        assert_eq!(cfg.rail.mild_mv, 4600);
        assert_eq!(cfg.rail.severe_mv, 4400);
        assert_eq!(cfg.rail.max_age_ms, 0);
    }

    #[test]
    fn config_parse_rejects_missing_sections_and_fields() {
        let e = rr3_power_config_parse("[rr3-power]\nlog_level = \"info\"\n").unwrap_err();
        assert!(e.contains("[rr3-power.battery]"), "缺段点名（battery 先解析）: {e}");
        let e = rr3_power_config_parse(
            "[rr3-power.battery]\nsample_period_ms = 5000\nlow_voltage_mv = 3300\n[rr3-power.charger]\nstate_period_ms = 2000\n",
        )
        .unwrap_err();
        assert!(e.contains("cutoff_voltage_mv"), "缺字段点名: {e}");
        // rail 段缺段点名（批N2 件五：段必填）＋缺 up 键点名（全键必填）
        let e = rr3_power_config_parse(
            "[rr3-power.battery]\nsample_period_ms = 5000\nlow_voltage_mv = 3300\ncutoff_voltage_mv = 3150\n[rr3-power.charger]\nstate_period_ms = 2000\n",
        )
        .unwrap_err();
        assert!(e.contains("[rr3-power.rail]"), "rail 缺段点名: {e}");
        let e = rr3_power_config_parse(
            "[rr3-power.battery]\nsample_period_ms = 5000\nlow_voltage_mv = 3300\ncutoff_voltage_mv = 3150\n[rr3-power.charger]\nstate_period_ms = 2000\n[rr3-power.rail]\ndomain = \"rail5v\"\nsample_period_ms = 1000\nmild_mv = 4600\nsevere_mv = 4400\nmax_age_ms = 0\n",
        )
        .unwrap_err();
        assert!(e.contains("缺 up"), "rail 缺 up 键点名: {e}");
        let e = rr3_power_config_parse("garbage =\n").unwrap_err();
        assert!(e.contains("TOML"), "坏文本点名: {e}");
    }

    /// 阈值倒挂执法（批N2 件五）：severe >= mild 即红——相等也红（Mild 档不可达）。
    /// up=false 不豁免：解析路径统一无分支，策略量合法性不看开关。
    #[test]
    fn config_parse_rejects_inverted_rail_thresholds() {
        let base = "\ncutoff_voltage_mv = 3150\n[rr3-power.charger]\nstate_period_ms = 2000\n[rr3-power.rail]\nup = false\ndomain = \"rail5v\"\nsample_period_ms = 1000\n";
        let e = rr3_power_config_parse(
            &format!("[rr3-power.battery]\nsample_period_ms = 5000\nlow_voltage_mv = 3300\n{base}mild_mv = 4400\nsevere_mv = 4400\nmax_age_ms = 0\n"),
        )
        .unwrap_err();
        assert!(e.contains("阈值倒挂"), "相等也红: {e}");
        let e = rr3_power_config_parse(
            &format!("[rr3-power.battery]\nsample_period_ms = 5000\nlow_voltage_mv = 3300\n{base}mild_mv = 4400\nsevere_mv = 4500\nmax_age_ms = 0\n"),
        )
        .unwrap_err();
        assert!(e.contains("阈值倒挂"), "severe > mild 红: {e}");
    }

    /// 钳制留痕（0.0.5 批I P-2）：0 周期收敛 1 不拒 boot——采样拍不能自毁；
    /// eprintln 留痕在解析期打（键名/原值/钳后值），此处钉收敛结果
    #[test]
    fn config_parse_clamps_zero_period_to_one() {
        let text = r#"
[rr3-power.battery]
sample_period_ms = 0
low_voltage_mv = 3300
cutoff_voltage_mv = 3150

[rr3-power.charger]
state_period_ms = 0

[rr3-power.rail]
up = false
domain = "rail5v"
sample_period_ms = 0
mild_mv = 4600
severe_mv = 4400
max_age_ms = 0
"#;
        let cfg = rr3_power_config_parse(text).unwrap();
        assert_eq!(cfg.battery.sample_period_ms, 1, "0 周期钳到 1ms");
        assert_eq!(cfg.charger.state_period_ms, 1, "0 周期钳到 1ms");
        assert_eq!(cfg.rail.sample_period_ms, 1, "rail 0 周期同钳 1ms");
    }

    // ---------- 两腿对拍用例（0.0.5 批K H1，architecture §4.0.3 判据②） ----------
    // battery/charger 的载荷 body fn 是 composite 私有（批I P-7 上移承载层），
    // 对拍用例随载荷落本文件（对拍计数门扫 atom mech.rs + 模块 composite.rs 两处）。

    #[test]
    fn parity_leg1_battery_output_train_is_deterministic() {
        // 腿一·确定性：同一电压序列喂两跑，(state 体, low 触发) 输出轨迹逐拍相等
        fn run() -> Vec<(Value, bool)> {
            let mut edge = Rr3LowEdge::rr3_low_edge_new(3300);
            [3700u16, 3310, 3300, 3290, 3295, 3400, 3299]
                .iter()
                .map(|&mv| {
                    let st = Rr3BatteryState {
                        voltage_mv: mv,
                        percent: 50,
                        charging: false,
                    };
                    let low = edge.rr3_low_edge_cross(mv);
                    (rr3_battery_state_body(1, st), low)
                })
                .collect()
        }
        assert_eq!(run(), run(), "同输入序列两跑逐拍相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_battery_matches_ruling_semantics() {
        // 腿二·正确性：低电边沿裁定语义独立重写（真值 = rr-core Rr3LowEdge）
        let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
        assert!(!e.rr3_low_edge_cross(3301), "严格大于阈值：不触发");
        assert!(e.rr3_low_edge_cross(3300), "等号算低：v == threshold 触发");
        assert!(!e.rr3_low_edge_cross(3290), "段内不重发（武装已解除）");
        assert!(!e.rr3_low_edge_cross(3300), "段内等号也不重发");
        assert!(!e.rr3_low_edge_cross(3310), "回充不返回 true（只是再武装）");
        assert!(e.rr3_low_edge_cross(3299), "再武装后再次越阈才再发");
        // state 体四字段、low 体三必填无加戏（schema additionalProperties=false）
        let v = rr3_battery_state_body(9, st(3700, 50));
        assert_eq!(v["seq"], 9);
        assert_eq!(v["voltage_mv"], 3700);
        assert_eq!(v["percent"], 50);
        assert_eq!(v["charging"], false);
        assert_eq!(v.as_object().unwrap().len(), 4, "state 四字段: {v}");
        let low = rr3_battery_low_body(1, st(3300, 10));
        assert_eq!(low["percent"], 10);
        assert_eq!(low["voltage_mv"], 3300);
        assert_eq!(low.as_object().unwrap().len(), 3, "low 三必填无加戏: {low}");
    }

    #[test]
    fn parity_leg1_charger_output_train_is_deterministic() {
        // 腿一·确定性：同一 charging 派生序列两跑，(seq, charging) 流逐拍相等
        fn run() -> Vec<(u64, bool)> {
            let mut seq = 0;
            [true, true, false, false, true]
                .iter()
                .map(|&charging| {
                    seq += 1;
                    (
                        seq,
                        rr3_charger_state_body(seq, charging)["charging"]
                            .as_bool()
                            .unwrap(),
                    )
                })
                .collect()
        }
        assert_eq!(run(), run(), "同输入序列两跑逐拍相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_charger_matches_ruling_semantics() {
        // 腿二·正确性：seq 本流自计数（与 battery/state 的 seq 独立）+ 两字段体
        // + 派生语义（charging 位来自最近一次 battery 读数，body 原样透传）
        let v = rr3_charger_state_body(3, true);
        assert_eq!(v["seq"], 3);
        assert_eq!(v["charging"], true, "charging 位是 battery 读数的派生透传");
        assert_eq!(v.as_object().unwrap().len(), 2, "两字段不加戏: {v}");
        assert_ne!(
            rr3_charger_state_body(1, false)["charging"],
            rr3_charger_state_body(2, true)["charging"],
            "派生值随读数变，不受 seq 影响"
        );
    }

    // ---------- 轨压档位对拍（批N2 件四，⑥-c2 真值面） ----------
    // rr_hal_api 的 rr3_rail_tier_effective 是契约层纯函数（无原子件 mech.rs，⑥-c2
    // 判据单真值点），对拍随其运行期唯一消费模块 rr3-power 落本文件。power 不是
    // atom 名（本模块原子 = battery/charger），⑧-a 计数门不数这两条（不占原子门
    // 分母，命名前缀照模块名走）。

    #[test]
    fn parity_leg1_power_rail_tier_effective_train_is_deterministic() {
        // 腿一·确定性：同一 (档位, 龄期, 上界) 合成序列喂两跑，输出档位序列逐拍相等
        use rr_hal_api::{rr3_rail_tier_effective, Rr3RailTier};
        fn run() -> Vec<Rr3RailTier> {
            let max = 250u32;
            [
                (Rr3RailTier::Ok, Some(0u32)),
                (Rr3RailTier::Ok, Some(max)),     // 边界：不折
                (Rr3RailTier::Ok, Some(max + 1)), // 越上界：折
                (Rr3RailTier::Mild, Some(max)),
                (Rr3RailTier::Mild, Some(max + 1)),
                (Rr3RailTier::Severe, Some(9)),
                (Rr3RailTier::Severe, None),     // 取不到基准：折
                (Rr3RailTier::Unknown, Some(0)), // 已是 Unknown：原样
                (Rr3RailTier::Unknown, None),
            ]
            .iter()
            .map(|&(tier, age)| rr3_rail_tier_effective(tier, age, max))
            .collect()
        }
        assert_eq!(run(), run(), "同输入序列两跑逐拍相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_power_rail_tier_effective_matches_ruling_semantics() {
        // 腿二·正确性：裁定语义逐条点名（真值 = rr_hal_api rr3_rail_tier_effective，
        // 全案 §6 T-6 运行期读数可信度 + Y-4 如实申报）
        use rr_hal_api::{rr3_rail_tier_effective, Rr3RailTier};
        let max = 250u32;
        // 取不到基准（None）⇒ Unknown，与档位无关——新鲜 Ok 也折不出"可信"
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Ok, None, max),
            Rr3RailTier::Unknown,
            "时基取不到基准 ⇒ Unknown（Y-4：None 是如实申报，不许编龄期冒充新鲜）"
        );
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Severe, None, max),
            Rr3RailTier::Unknown,
            "None 折档与输入档位无关"
        );
        // 越上界 ⇒ Unknown：陈旧的 Severe 与新鲜的 Severe 必须可区分（T-6）
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Severe, Some(max + 1), max),
            Rr3RailTier::Unknown,
            "age > max ⇒ Unknown（陈旧 Severe ≠ 新鲜 Severe，T-6）"
        );
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Ok, Some(max + 1), max),
            Rr3RailTier::Unknown
        );
        // 边界 age == max 不折（新鲜度上界是闭区间内）；龄期内原样透传
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Severe, Some(max), max),
            Rr3RailTier::Severe,
            "age == max 边界不折（闭区间内）"
        );
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Ok, Some(0), max),
            Rr3RailTier::Ok,
            "新鲜读数原样透传"
        );
        // 档名串同源：第四态 "unknown" 不冒充 "severe"（守护输出行 INFO rail_tier=
        // 与板侧 RailTier::name 同一词表——展示面原样的数据基础）
        assert_eq!(Rr3RailTier::Unknown.rr3_rail_tier_name(), "unknown");
        assert_ne!(Rr3RailTier::Unknown.rr3_rail_tier_name(), "severe");
        // 消费纪律的类型基础（本函数只折档不判处置，执法归⑥-c2 消费侧）：Unknown
        // 是独立第四态（执行面 match 得到独立臂按 Severe 那一侧处置），且折档结果
        // 不许再折成 Ok（展示面原样）
        assert_ne!(
            rr3_rail_tier_effective(Rr3RailTier::Severe, None, max),
            Rr3RailTier::Severe,
            "折出的 Unknown 与 Severe 可区分（执行面独立处置的判据基础）"
        );
        assert_ne!(
            rr3_rail_tier_effective(Rr3RailTier::Severe, None, max),
            Rr3RailTier::Ok,
            "Unknown 不许折算成 Ok（展示面原样不折）"
        );
    }

    // ---------- 轨压监测归属搬家用例（批N2 件五，P-9：机制不动、归属搬家） ----------
    // 判定核切片（rr3_rail_tier_of / Rr3RailJudge）与 ⑥-c2 消费执法点（rr3_rail_beat）
    // 的用例落本文件；板侧 tools/esp32-periph-base 的 tier_of/RailJudge 不改，语义逐字对照。

    #[test]
    fn rail_state_body_carries_four_keys() {
        let v = rr3_rail_state_body(5, 4500, 120, "mild");
        assert_eq!(v["seq"], 5);
        assert_eq!(v["mv"], 4500);
        assert_eq!(v["age_ms"], 120);
        assert_eq!(v["tier"], "mild");
        assert_eq!(v.as_object().unwrap().len(), 4, "四键不加戏: {v}");
    }

    #[test]
    fn rail_judge_feed_matches_board_semantics() {
        // 板侧 RailJudge::feed 逐字对照（mild=4600/severe=4400）：档位 = tier_of 纯
        // mV 分类，event = 两线任一低电段首拍。4500 对 severe 线是"严格回阈 ⇒ 再
        // 武装"（Rr3LowEdge 语义），故 4300 是新段首发再报——与"向下档变 ⇔ 边沿
        // 事件"一致，不是同段重发。
        let mut j = Rr3RailJudge::rr3_rail_judge_new(4600, 4400);
        let seq = [
            (4700u16, Rr3RailTier::Ok, false),
            (4600, Rr3RailTier::Mild, true),   // 轻度线首发（Ok→Mild）
            (4500, Rr3RailTier::Mild, false),  // 段内不重发
            (4400, Rr3RailTier::Severe, true), // 重度线首发（Mild→Severe）
            (4500, Rr3RailTier::Mild, false),  // 回阈不报（severe 线再武装）
            (4300, Rr3RailTier::Severe, true), // severe 新段首发（4500 已严格回阈）
            (4700, Rr3RailTier::Ok, false),    // 回 Ok（mild 线再武装）
            (4600, Rr3RailTier::Mild, true),   // 再武装后越阈再发
        ];
        for (mv, want_tier, want_event) in seq {
            let (tier, event) = j.rr3_rail_feed(mv);
            assert_eq!(tier, want_tier, "mv={mv} 档位");
            assert_eq!(event, want_event, "mv={mv} 事件");
        }
    }

    #[test]
    fn parity_leg1_power_rail_judge_train_is_deterministic() {
        // 腿一·确定性：同一 mV 序列喂两跑，(档位, 事件) 轨迹逐拍相等
        fn run() -> Vec<(Rr3RailTier, bool)> {
            let mut j = Rr3RailJudge::rr3_rail_judge_new(4600, 4400);
            [4700u16, 4600, 4500, 4400, 4500, 4300, 4700, 4600]
                .iter()
                .map(|&mv| j.rr3_rail_feed(mv))
                .collect()
        }
        assert_eq!(run(), run(), "同输入序列两跑逐拍相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_power_rail_judge_edge_equivalent_to_downward_transition() {
        // 腿二·正确性：边沿事件 ⇔ 向下档变（板侧 RailJudge 类型注的逐拍等价性）。
        // 档序 Ok=0/Mild=1/Severe=2，prev 初值 Ok：event ⇔ rank 增；回阈（rank 降）
        // 不报。与 rail_judge_feed_matches_board_semantics 同一序列、判据独立重写。
        fn rank(t: Rr3RailTier) -> u8 {
            match t {
                Rr3RailTier::Ok => 0,
                Rr3RailTier::Mild => 1,
                Rr3RailTier::Severe => 2,
                Rr3RailTier::Unknown => 3,
            }
        }
        let mut j = Rr3RailJudge::rr3_rail_judge_new(4600, 4400);
        let mut prev = rank(Rr3RailTier::Ok);
        for mv in [4700u16, 4600, 4500, 4400, 4500, 4300, 4700, 4600] {
            let (tier, event) = j.rr3_rail_feed(mv);
            assert_eq!(event, rank(tier) > prev, "mv={mv}: 事件 ⇔ 向下档变");
            prev = rank(tier);
        }
    }

    #[test]
    fn rail_beat_unknown_not_refed_and_passthrough() {
        // ⑥-c2 消费执法两条（P-15 末列"执行面按保守侧处置、展示面原样显示"的落点）：
        // ① 产出侧已折的 Unknown 原样透传——mv 再好也不许从 mv 重分类出 Ok（不喂核，
        //    执行面保持置位）；
        let mut j = Rr3RailJudge::rr3_rail_judge_new(4600, 4400);
        let st = Rr3RailState {
            mv: 4700,
            tier: Rr3RailTier::Unknown,
            age_ms: 0,
        };
        let (tier, event) = rr3_rail_beat(&mut j, st, 250);
        assert_eq!(tier, Rr3RailTier::Unknown, "Unknown 原样透传，不折 Ok");
        assert_eq!(event, None, "Unknown 不喂判定核");
        // ② 陈旧读数（age > max）折 Unknown——同样不喂核（T-6：陈旧 Ok ≠ 新鲜 Ok）
        let st = Rr3RailState {
            mv: 4700,
            tier: Rr3RailTier::Ok,
            age_ms: 251,
        };
        let (tier, event) = rr3_rail_beat(&mut j, st, 250);
        assert_eq!(tier, Rr3RailTier::Unknown, "陈旧 Ok 折 Unknown（T-6）");
        assert_eq!(event, None);
        // 新鲜读数（age == max 边界不折）正常喂核：档位 = 判定核分类值（机制产出）
        let st = Rr3RailState {
            mv: 4300,
            tier: Rr3RailTier::Severe,
            age_ms: 250,
        };
        let (tier, event) = rr3_rail_beat(&mut j, st, 250);
        assert_eq!(tier, Rr3RailTier::Severe);
        assert_eq!(event, Some(true), "fresh 拍喂核，severe 线首发");
    }
}
