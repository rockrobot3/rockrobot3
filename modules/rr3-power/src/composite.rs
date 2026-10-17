//! rr3-power 组合件实现（组合件五件套③，0.0.5 批B③ 原子重铺打样）。
//!
//! 组合件 = battery ⊗ charger（composite.toml 的 ⊗ 链；⊗=装配期、执行单元内
//! 函数调用，ADR-0019 裁定 2）。本文件承载装配与运行机制：config 解析（阈值等
//! 策略经 config 数据化注入，机制件零策略载荷）、HAL 电源面开机探活、主循环
//! 三拍（battery 拍 / 低电边沿 / charger 派生拍）。
//!
//! 启动序列（自原入口迁入，语义零改动）：读 `RR3_MANIFEST`（rrd spawn 注入的
//! manifest 绝对路径，supervisor.rs）→ [`Rr3BusClient::rr3_connect`] → 读 config.toml
//! → 选 HAL 电源面并**开机探活**（Unsupported 即 exit(1)：能力缺失显式回错，不静默
//! 带病常驻）→ 主循环。本模块没有事务面（module.toml 无 [rpc].exposes），主循环只做
//! 三件事：
//! 1. 每 `sample_period_ms` 读一次 HAL 电池 → 发 `battery/state`（首拍即采）；
//! 2. 电压越过低电阈值的第一拍发 `battery/low`（Rr3LowEdge：同一次低电段恰一次，
//!    恢复再武装）——rrd 订阅它执行进程内兜底（Q4 裁定），brain 不在场也要能触发；
//! 3. 每 `state_period_ms` 发 `charger/state`（由最近一次电池读数的 charging 位派生）。
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
use rr_hal::backend::rr3_hal_power;
use rr_hal_api::Rr3BatteryState;
use serde_json::{json, Value};

use battery::{Rr3BatteryCfg, Rr3LowEdge, RR3_BATTERY_LOW_TOPIC, RR3_BATTERY_STATE_TOPIC};
use charger::{Rr3ChargerCfg, RR3_CHARGER_STATE_TOPIC};

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
    println!("rr3-power 0.0.1 (0.0.2 T4b 实装)");
    for line in battery::SURFACE
        .iter()
        .copied()
        .chain(charger::SURFACE.iter().copied())
    {
        println!("  {line}");
    }

    // 总线接入：manifest 路径是 rrd spawn 的契约环境变量，直跑（开发调试）须自行设置
    let manifest_path = std::env::var(RR3_MANIFEST_ENV).map_err(|_| {
        format!("缺环境变量 {RR3_MANIFEST_ENV}（rrd spawn 注入；直跑请指向 module.toml）")
    })?;
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读 manifest {manifest_path} 失败: {e}"))?;
    let bus = Rr3BusClient::rr3_connect(&manifest_text)?;
    println!("总线就绪: name={} task={}", bus.rr3_name(), bus.rr3_task());

    let cfg = rr3_config_load(&manifest_path)?;
    // cutoff 显式标注"声明未生效"（MC-8① 裁定 (c)）：回显 ≠ 读者，本行只为运维
    // 自述，不构成"已实现关断"的宣称（关断动作归 M6）。
    println!(
        "config 就绪: battery sample={}ms low={}mV charger state={}ms | cutoff={}mV（声明未生效：本版不关断，归 M6）",
        cfg.battery.sample_period_ms,
        cfg.battery.low_voltage_mv,
        cfg.charger.state_period_ms,
        cfg.battery.cutoff_voltage_mv,
    );

    // HAL 电源面选择 + 开机探活：Unsupported 即致命——显式回错不静默退化
    // （rr-hal 占位件的契约），也不给"没有电池读数还常驻"的假活
    let (dev, backend) = rr3_hal_power();
    let first = dev
        .battery()
        .map_err(|e| format!("电源面探活失败({backend}): {e}"))?;
    println!(
        "电源面就绪: {backend} 电量 {}% {}mV charging={}",
        first.percent, first.voltage_mv, first.charging
    );

    // 主循环状态：两条流的 seq 各自计数、两个拍点、低电边沿机（首拍即武装）
    let mut battery_seq: u64 = 0;
    let mut charger_seq: u64 = 0;
    let mut low_seq: u64 = 0;
    let mut low_edge = Rr3LowEdge::rr3_low_edge_new(cfg.battery.low_voltage_mv);
    let mut next_battery = Instant::now(); // 首拍即采：兜底链路不等第一个周期
    let mut next_charger = Instant::now();
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
                println!(
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
#[derive(Debug, Clone, Copy)]
struct Rr3PowerCfg {
    battery: Rr3BatteryCfg,
    charger: Rr3ChargerCfg,
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
    Ok(Rr3PowerCfg {
        battery: Rr3BatteryCfg {
            sample_period_ms: period_of(&battery_sect, "sample_period_ms", "battery")?,
            low_voltage_mv: u16_of(&battery_sect, "low_voltage_mv", "battery")?,
            cutoff_voltage_mv: u16_of(&battery_sect, "cutoff_voltage_mv", "battery")?,
        },
        charger: Rr3ChargerCfg {
            state_period_ms: period_of(&charger_sect, "state_period_ms", "charger")?,
        },
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
"#;
        let cfg = rr3_power_config_parse(text).unwrap();
        assert_eq!(cfg.battery.sample_period_ms, 5000);
        assert_eq!(cfg.battery.low_voltage_mv, 3300);
        assert_eq!(cfg.battery.cutoff_voltage_mv, 3150);
        assert_eq!(cfg.charger.state_period_ms, 2000);
    }

    #[test]
    fn config_parse_rejects_missing_sections_and_fields() {
        let e = rr3_power_config_parse("[rr3-power]\nlog_level = \"info\"\n").unwrap_err();
        assert!(e.contains("[rr3-power.battery]"), "缺段点名: {e}");
        let e = rr3_power_config_parse(
            "[rr3-power.battery]\nsample_period_ms = 5000\nlow_voltage_mv = 3300\n[rr3-power.charger]\nstate_period_ms = 2000\n",
        )
        .unwrap_err();
        assert!(e.contains("cutoff_voltage_mv"), "缺字段点名: {e}");
        let e = rr3_power_config_parse("garbage =\n").unwrap_err();
        assert!(e.contains("TOML"), "坏文本点名: {e}");
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
"#;
        let cfg = rr3_power_config_parse(text).unwrap();
        assert_eq!(cfg.battery.sample_period_ms, 1, "0 周期钳到 1ms");
        assert_eq!(cfg.charger.state_period_ms, 1, "0 周期钳到 1ms");
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
}
