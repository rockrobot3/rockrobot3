//! rr3-act 接入层（0.0.8 W-A 批5 拆分，ADR-0017 五层表接入层）。
//!
//! config schema 解析（自原 composite.rs 迁入：TOML 下钻、缺字段点名——文案是
//! 装配观测面，逐字不变）＋ 执行轴状态装载（批N3 件三建、件五扩形：类型＋五键
//! 严格解析随迁，env 读＋坏形/缺席折保守缺省并点名，安全向不退）＋ HAL 执行面
//! 三面选择与开机探活编排（Unsupported / 探活读失败即 Err 上抛——显式回错不
//! 静默带病常驻，也不给"半个执行器"的假活）＋ 反射环宿主线程 spawn（0.0.8 方案
//! §4.6：该 BusHandle clone 只归反射环，反射环与 Dispatcher 零共享状态）。
//! config 路径解析（RR3_ACT_CONFIG_ENV 覆盖 + manifest 同目录缺省）已随批0 并入
//! rr3mod `load_config_text` 单点，本层不再持有路径段（RR3_MANIFEST_ENV 本地常量
//! 随批退役）。本层零业务 match——策略量经 config 数据化注入，机制件零策略载荷。

use rr3mod::BusHandle;
use rr_hal::backend::{rr3_hal_gpio_out, rr3_hal_servo, RR3_BACKEND_ESP32};
use rr_hal_api::{Rr3GpioOut, Rr3HalError, Rr3PwmServo};

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "remote.rs"]
mod remote;

use crate::led::{Rr3LedEffect, RR3_LED_EFFECT_VALUES};
use crate::reflex::{rr3_reflex_loop, Rr3ReflexCfg};
use crate::relay::RR3_RELAY_SET_TOPIC;
use crate::servo::{Rr3ServoCfg, RR3_SERVO_COMMAND_TOPIC};

/// rrd spawn 注入的执行轴状态（批N3 件三，supervisor 与 RR3_HAL_BINDINGS 同位注入）：
/// 装配链装载 `hardware/<形态>/axis/*.toml`、按 `RR3_HAL_PERIPH_AXIS`（缺省契约
/// 常量）消歧后，对 instance 相等的恰一份合法档案投影
/// `calibrated=<bool>,hard_stop=<档案词>,lo=<n>,hi=<n>,center=<n>`。未注入（无档案/
/// 坏件/副本/轴错配）与坏形文本一律折保守缺省——§6"未标定不得自动"是代码拒动，
/// 缺真相就按最窄面起跑（行程窄域除外：保守行程 = 器件全域，点动面必须活着）。
const RR3_ACT_AXIS_STATE_ENV: &str = "RR3_ACT_AXIS_STATE";

/// 执行轴状态（批N3 件三建，件五扩形）：拒动判据 [`rr_hal_api::rr3_motion_allowed`]
/// 的输入面＋**行程真值**（门 g 收敛：模块行程改读档案——config 键撤除，`lo/hi`
/// 两键就是 `Rr3ServoCfg` 行程的填值来源，`center` 是档案中位锚的随路真值）。五键
/// 都来自档案（件二装载器真值），模块侧零判定载荷——只解析不判域（行程窗口自洽
/// lo<center<hi 在 rrd 装载器已判，这里不设第二裁决点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rr3ActAxisState {
    pub(crate) hard_stop: rr_hal_api::Rr3ActHardStop,
    pub(crate) calibrated: bool,
    /// 行程下限（度）——档案 lo_deg
    pub(crate) lo_deg: u16,
    /// 行程上限（度）——档案 hi_deg
    pub(crate) hi_deg: u16,
    /// 中位锚（度）——档案 center_deg（当前仅随路可查：jog/走位锚随 Rr3Actuator
    /// 进场接线，先保真值不丢）
    pub(crate) center_deg: u16,
}

impl Rr3ActAxisState {
    /// 保守缺省：未标定 + 挡块未知 + **器件全域行程**——判据按 §6 硬约束把 Auto
    /// 全拒，执行面只剩点动；行程给器件全域（0..=180）而非窄域，窄域会杀死点动面
    /// （点动是未标定轴唯一合法动作面，行程窄域等于把"活着"也收走）。center 取
    /// 全域中点 90。
    pub(crate) fn rr3_conservative() -> Self {
        Self {
            hard_stop: rr_hal_api::Rr3ActHardStop::Unknown,
            calibrated: false,
            lo_deg: 0,
            hi_deg: 180,
            center_deg: 90,
        }
    }

    /// 注入文本解析：`calibrated=<bool>,hard_stop=<词>,lo=<n>,hi=<n>,center=<n>`，
    /// 键序无关、五键必在、重复键/坏布尔/词表外词/越 u16/未知键逐条点名。解析失败
    /// 由调用方折保守缺省并打点（不 exit——保守态是安全向，退出反而进重启循环）。
    fn rr3_from_env(text: &str) -> Result<Self, String> {
        let mut calibrated = None;
        let mut hard_stop = None;
        let mut lo = None;
        let mut hi = None;
        let mut center = None;
        for kv in text.split(',') {
            let (k, v) = kv.split_once('=').ok_or_else(|| {
                format!(
                    "执行轴状态段 {kv:?} 缺 =（形状 calibrated=<bool>,hard_stop=<词>,lo=<n>,hi=<n>,center=<n>）"
                )
            })?;
            match k {
                "calibrated" => {
                    if calibrated.is_some() {
                        return Err("执行轴状态 calibrated 重复".into());
                    }
                    calibrated = Some(match v {
                        "true" => true,
                        "false" => false,
                        _ => return Err(format!("执行轴状态 calibrated={v:?} 非显式布尔")),
                    });
                }
                "hard_stop" => {
                    if hard_stop.is_some() {
                        return Err("执行轴状态 hard_stop 重复".into());
                    }
                    hard_stop = Some(
                        rr_hal_api::Rr3ActHardStop::rr3_hard_stop_parse(v).ok_or_else(|| {
                            format!("执行轴状态 hard_stop={v:?} 不在档案词表（unknown/yes/no）")
                        })?,
                    );
                }
                "lo" => {
                    if lo.is_some() {
                        return Err("执行轴状态 lo 重复".into());
                    }
                    lo =
                        Some(v.parse::<u16>().map_err(|_| {
                            format!("执行轴状态 lo={v:?} 非行程下限整数（u16 域）")
                        })?);
                }
                "hi" => {
                    if hi.is_some() {
                        return Err("执行轴状态 hi 重复".into());
                    }
                    hi =
                        Some(v.parse::<u16>().map_err(|_| {
                            format!("执行轴状态 hi={v:?} 非行程上限整数（u16 域）")
                        })?);
                }
                "center" => {
                    if center.is_some() {
                        return Err("执行轴状态 center 重复".into());
                    }
                    center =
                        Some(v.parse::<u16>().map_err(|_| {
                            format!("执行轴状态 center={v:?} 非中位锚整数（u16 域）")
                        })?);
                }
                other => return Err(format!("执行轴状态未知键 {other:?}")),
            }
        }
        Ok(Self {
            calibrated: calibrated.ok_or("执行轴状态缺 calibrated")?,
            hard_stop: hard_stop.ok_or("执行轴状态缺 hard_stop")?,
            lo_deg: lo.ok_or("执行轴状态缺 lo（档案行程下限）")?,
            hi_deg: hi.ok_or("执行轴状态缺 hi（档案行程上限）")?,
            center_deg: center.ok_or("执行轴状态缺 center（档案中位锚）")?,
        })
    }
}

/// 执行轴状态装载：env 读 + 坏形/缺席折保守缺省并点名（安全向，不退），回显注入
/// 的策略面（A-9：改数据即改行为，日志是第一证据）。自原 composite.rs 逐字迁入
/// （缺席/坏形/回显三文案逐字）。
pub(crate) fn rr3_axis_state_load() -> Rr3ActAxisState {
    let axis = match std::env::var(RR3_ACT_AXIS_STATE_ENV) {
        Ok(text) => Rr3ActAxisState::rr3_from_env(&text).unwrap_or_else(|e| {
            eprintln!("rr3-act: {e}，按保守缺省起跑（未标定，只受点动）");
            Rr3ActAxisState::rr3_conservative()
        }),
        Err(_) => {
            eprintln!("执行轴状态: 未注入（无档案/坏件/副本/轴错配），按保守缺省起跑");
            Rr3ActAxisState::rr3_conservative()
        }
    };
    eprintln!(
        "执行轴状态: calibrated={} hard_stop={} 行程=[{}, {}] 中位={}（注入策略面在启动自述可见，A-9）",
        axis.calibrated,
        axis.hard_stop.rr3_hard_stop_name(),
        axis.lo_deg,
        axis.hi_deg,
        axis.center_deg
    );
    axis
}

/// 执行面三句柄：舵机 + 灯 + 继电器
pub(crate) type Rr3ActDevs = (
    Box<dyn Rr3PwmServo>,
    Box<dyn Rr3GpioOut>,
    Box<dyn Rr3GpioOut>,
);

/// 桥被 sense 独占时的舵机面置换（A9.2）：host 档改走 `rr3-sense/servo/set`，
/// 换回 `(远端执行器, 回显后端名)`；rtos/bare 档无跨进程总线通路，维持
/// fail-closed 点名（不虚构一个不可达的远端面）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rr3_remote_bridge(
    bus: &BusHandle,
    backend: &'static str,
) -> Result<(Box<dyn Rr3PwmServo>, &'static str), String> {
    eprintln!("rr-hal: 舵机桥被 rr3-sense 独占（{backend} Busy），改走远端命令面（A9.2）");
    let dev = remote::Rr3RemoteServo::rr3_remote_new(bus)?;
    Ok((Box::new(dev), "remote(rr3-sense)"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn rr3_remote_bridge(
    _bus: &BusHandle,
    backend: &'static str,
) -> Result<(Box<dyn Rr3PwmServo>, &'static str), String> {
    Err(format!("舵机面探活失败({backend}): HAL 设备忙"))
}

/// HAL 执行面三面选择 + 开机探活。舵机面是执行器核心：Unsupported / 探活读失败
/// 即致命（显式回错不静默退化，不给"半个执行器"的假活）。LED / 继电器是**可缺
/// 面**：组合档装配单如实申报二者不在位（[enabled_slots] 未启用），rrd 据此注入
/// 占位后端——占位在命令面以 Unsupported 回错就是诚实语义，进程不该因此拒启，
/// 否则"只启用 act.servo 的合法装配"永远起不来（2026-11-09 真板实录）。其余读
/// 失败（Busy / 参数错）仍是真故障，fail-closed 上抛。探活全是读面 &self。
///
/// A9.2 桥冲突降级：组合档 `/dev/ttyAMA0` 由 rr3-sense 独占（音频 + 命令旁路），
/// 本进程再开同一设备必得 `Busy`。此时舵机面改走 sense 的 `servo/set` RPC（同一
/// 物理执行器、单一写者），并把 backend 名回显为 `remote(rr3-sense)`；其余
/// Unsupported/参数错仍 fail-closed 上抛，不把"开不了桥"当通用降级借口。
pub(crate) fn rr3_act_probe(bus: &BusHandle) -> Result<Rr3ActDevs, String> {
    let (servo_dev, servo_backend) = {
        let (dev, backend) = rr3_hal_servo();
        match dev.angle() {
            Ok(_) => (dev, backend),
            Err(Rr3HalError::Busy) if backend == RR3_BACKEND_ESP32 => {
                rr3_remote_bridge(bus, backend)?
            }
            Err(e) => return Err(format!("舵机面探活失败({backend}): {e}")),
        }
    };
    let (led_gpio, led_backend) = rr3_hal_gpio_out("led");
    let (relay_gpio, relay_backend) = rr3_hal_gpio_out("relay");
    rr3_opt_face("LED", led_backend, led_gpio.level())?;
    rr3_opt_face("继电器", relay_backend, relay_gpio.level())?;
    eprintln!("执行面就绪: servo={servo_backend} led={led_backend} relay={relay_backend}");
    Ok((servo_dev, led_gpio, relay_gpio))
}

/// 可缺面探活：占位后端（`Unsupported`）是装配单"该格未启用"的诚实投影——点名
/// 后继续，使用该面时仍由占位逐次回错；其余错误（Busy/参数错）是真故障，致命。
fn rr3_opt_face(face: &str, backend: &str, probe: Result<bool, Rr3HalError>) -> Result<(), String> {
    match probe {
        Ok(_) => Ok(()),
        Err(Rr3HalError::Unsupported(_)) => {
            eprintln!("{face} 面未启用（{backend}），该面命令按 Unsupported 回错，其余面继续");
            Ok(())
        }
        Err(e) => Err(format!("{face} 面探活失败({backend}): {e}")),
    }
}

/// 反射环宿主线程 spawn：进程内 1kHz 闭环的位子，永不进总线（ADR-0007 裁定 1）。
/// `bus.clone()` 进 move 闭包——该 BusHandle clone 只归反射环（0.0.8 方案 §4.6），
/// 反射环与 Dispatcher 零共享状态不变量保持；Builder 名与失败文案逐字随迁。
pub(crate) fn rr3_act_spawn_reflex(cfg: Rr3ReflexCfg, bus: &BusHandle) -> Result<(), String> {
    let reflex_bus = bus.clone();
    std::thread::Builder::new()
        .name("rr3-act-reflex".into())
        .spawn(move || rr3_reflex_loop(cfg, reflex_bus))
        .map_err(|e| format!("起反射环宿主线程失败: {e}"))?;
    Ok(())
}

/// 本模块 config 的汇总形状（led/intent 带注入集合 → 去 Copy）
#[derive(Debug, Clone)]
pub(crate) struct Rr3ActCfg {
    pub(crate) servo: Rr3ServoCfg,
    pub(crate) led: Rr3LedCfg,
    pub(crate) intent: Rr3IntentCfg,
    pub(crate) reflex: Rr3ReflexCfg,
}

/// config `[rr3-act.led]` 的形状（`effects` = 本形态灯效白名单子集，A-9）
#[derive(Debug, Clone)]
pub(crate) struct Rr3LedCfg {
    pub(crate) blink_period_ms: u64,
    pub(crate) effects: Vec<Rr3LedEffect>,
}

/// config `[rr3-act.intent]` 的形状（`open_topics` = 意图执行面开放集，A-9）。
/// 组合件读入后透传给 `intent.rs`——组合件层无 ops 校验机制（⑧-c 只扫
/// `atoms/*/assemble.toml`），故本策略落 config 键而非装配清单 ops 槽。
#[derive(Debug, Clone)]
pub(crate) struct Rr3IntentCfg {
    pub(crate) open_topics: Vec<String>,
}

/// 反射环节拍上界（Hz）：`1_000_000 / loop_hz` 微秒取整，超过 1kHz 的配置
/// 在生产里没有物理意义，却会让循环周期归零（满转自旋）。
const RR3_MAX_LOOP_HZ: u32 = 1_000;

/// TOML 文本 → Rr3ActCfg。手工下钻不引 serde derive：字段个位数，
/// 点名缺字段的文案比派生宏的默认报错可读。
pub(crate) fn rr3_act_config_parse(text: &str) -> Result<Rr3ActCfg, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("config TOML 解析失败: {e}"))?;
    let sect = |name: &str| -> Result<&toml::Value, String> {
        v.get("rr3-act")
            .and_then(|s| s.get(name))
            .ok_or_else(|| format!("config 缺 [rr3-act.{name}] 段"))
    };
    let u64_of = |sect: &toml::Value, key: &str, sect_name: &str| -> Result<u64, String> {
        sect.get(key)
            .and_then(toml::Value::as_integer)
            .filter(|n| *n >= 0)
            .map(|n| n as u64)
            .ok_or_else(|| format!("config [rr3-act.{sect_name}] 缺 {key}（非负整数）"))
    };
    let servo_sect = sect("servo")?.clone();
    let led_sect = sect("led")?.clone();
    let intent_sect = sect("intent")?.clone();
    let reflex_sect = sect("reflex")?.clone();
    // 行程键不在 config（批N3 件五门 g 收敛）：Rr3ServoCfg 的 angle_min/max_deg
    // 由执行轴状态注入填值（RR3_ACT_AXIS_STATE 的 lo/hi，档案真值），本解析面
    // 只管时限/限速。config 出现任何行程键 = 读者零的死键，装配期
    // （rrd assemble scan）与 CI 门 ⑨-e 双面执法，此处不再解析。
    // loop_hz 越界拒 boot（0.0.5 批I A-3）：`as u32` 裸截断会把 2^32+1 之类
    // 值偷偷收成 1，节拍面静默变形——try_from 点名拒绝。本轮审计补上界：
    // 1_000_000/loop_hz 在 loop_hz 极大时取整为 0µs，反射环会变成满转自旋。
    let loop_hz_raw = u64_of(&reflex_sect, "loop_hz", "reflex")?.max(1);
    let loop_hz = u32::try_from(loop_hz_raw).map_err(|_| {
        format!("config [rr3-act.reflex] loop_hz={loop_hz_raw} 超出 u32 范围，拒 boot")
    })?;
    if loop_hz > RR3_MAX_LOOP_HZ {
        return Err(format!(
            "config [rr3-act.reflex] loop_hz 须在 1..={RR3_MAX_LOOP_HZ}，收到 {loop_hz}（更大值会退化为满转自旋）"
        ));
    }
    // 灯效白名单子集（A-9）：数组逐元素必须是**在册契约值**，非在册值或非字符串
    // 逐条点名拒 boot（fail-closed）；键在而数组为空 = 本形态一条点名都不认（合法）
    let effects_arr = led_sect
        .get("effects")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "config [rr3-act.led] 缺 effects（灯效白名单数组）".to_string())?;
    let mut effects = Vec::with_capacity(effects_arr.len());
    for (i, e) in effects_arr.iter().enumerate() {
        let s = e
            .as_str()
            .ok_or_else(|| format!("config [rr3-act.led] effects[{i}] 必须是字符串，收到 {e}"))?;
        let eff = Rr3LedEffect::rr3_led_effect_parse(s).ok_or_else(|| {
            format!(
                "config [rr3-act.led] effects[{i}] 非在册灯效 {s:?}（在册值 {}）",
                RR3_LED_EFFECT_VALUES.join("/")
            )
        })?;
        effects.push(eff);
    }
    // 意图执行面开放集（A-9）：只认本模块**有执行臂**的在册意图主题
    // （relay/set、servo/command），越界值逐条点名拒 boot（fail-closed）；空集合法
    let open_arr = intent_sect
        .get("open_topics")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "config [rr3-act.intent] 缺 open_topics（意图开放主题数组）".to_string())?;
    let mut open_topics = Vec::with_capacity(open_arr.len());
    for (i, t) in open_arr.iter().enumerate() {
        let s = t.as_str().ok_or_else(|| {
            format!("config [rr3-act.intent] open_topics[{i}] 必须是字符串，收到 {t}")
        })?;
        if s != RR3_SERVO_COMMAND_TOPIC && s != RR3_RELAY_SET_TOPIC {
            return Err(format!(
                "config [rr3-act.intent] open_topics[{i}] 非本模块可执行的意图主题 {s:?}（只认 {}/{}）",
                RR3_RELAY_SET_TOPIC,
                RR3_SERVO_COMMAND_TOPIC
            ));
        }
        open_topics.push(s.to_string());
    }
    Ok(Rr3ActCfg {
        servo: Rr3ServoCfg {
            // 行程占位（批N3 件五）：真值由装配段 rr3_act_run 用执行轴状态注入
            // 覆填（cfg.servo.angle_min/max_deg = axis.lo/hi_deg），此处不解析
            angle_min_deg: 0,
            angle_max_deg: 180,
            move_timeout_ms: u64_of(&servo_sect, "move_timeout_ms", "servo")?,
            state_min_interval_ms: u64_of(&servo_sect, "state_min_interval_ms", "servo")?,
        },
        led: Rr3LedCfg {
            blink_period_ms: u64_of(&led_sect, "blink_period_ms", "led")?.max(1),
            effects,
        },
        intent: Rr3IntentCfg { open_topics },
        reflex: Rr3ReflexCfg {
            loop_hz,
            saturate_notice_ms: u64_of(&reflex_sect, "saturate_notice_ms", "reflex")?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 轴状态注入文本解析（批N3 件三建、件五扩五键）：正例键序无关；缺键/
    /// 坏布尔/词表外词/未知键/重复键/越 u16 逐条点名红——装配链注入的文本坏了
    /// 必须看得见，不许静默猜。
    #[test]
    fn axis_state_env_parses_strict() {
        let ok =
            Rr3ActAxisState::rr3_from_env("calibrated=true,hard_stop=yes,lo=60,hi=120,center=90")
                .unwrap();
        assert_eq!(
            ok,
            Rr3ActAxisState {
                calibrated: true,
                hard_stop: rr_hal_api::Rr3ActHardStop::Yes,
                lo_deg: 60,
                hi_deg: 120,
                center_deg: 90
            }
        );
        let ok = Rr3ActAxisState::rr3_from_env(
            "hard_stop=unknown,calibrated=false,lo=0,hi=180,center=90",
        )
        .unwrap();
        assert_eq!(
            ok,
            Rr3ActAxisState {
                calibrated: false,
                hard_stop: rr_hal_api::Rr3ActHardStop::Unknown,
                lo_deg: 0,
                hi_deg: 180,
                center_deg: 90
            },
            "键序无关"
        );
        for (bad, needle) in [
            (
                "calibrated=true,hard_stop=yes,lo=60,hi=120,center=90,x=1",
                "未知键",
            ),
            (
                "calibrated=true,hard_stop=yes,center=90",
                "lo（档案行程下限）",
            ),
            (
                "calibrated=true,hard_stop=yes,lo=60,hi=120",
                "center（档案中位锚）",
            ),
            (
                "calibrated=true,hard_stop=yes,lo=60,center=90",
                "hi（档案行程上限）",
            ),
            (
                "calibrated=true,hard_stop=yes,lo=60,hi=120,center=70000",
                "u16",
            ),
            (
                "calibrated=true,hard_stop=yes,lo=abc,hi=120,center=90",
                "u16",
            ),
        ] {
            let e = Rr3ActAxisState::rr3_from_env(bad).unwrap_err();
            assert!(e.contains(needle), "{bad:?} → {e}");
        }
    }

    /// 与仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红。
    /// 行程键不在 config（批N3 件五门 g 收敛），本夹具与仓内文件同形。
    #[test]
    fn config_parse_covers_repo_shape() {
        let text = r#"
[rr3-act.servo]
move_timeout_ms = 800
state_min_interval_ms = 50

[rr3-act.led]
blink_period_ms = 500
effects = ["on", "off", "blink"]

[rr3-act.intent]
open_topics = ["rr3-act/relay/set", "rr3-act/servo/command"]

[rr3-act.reflex]
loop_hz = 1000
saturate_notice_ms = 500
"#;
        let cfg = rr3_act_config_parse(text).unwrap();
        // 行程两键在 config 解析面恒为占位 0/180——真值由装配段 rr3_act_run 用
        // 执行轴状态覆填（门 g 收敛：档案是唯一真值点）
        assert_eq!(cfg.servo.angle_min_deg, 0);
        assert_eq!(cfg.servo.angle_max_deg, 180);
        assert_eq!(cfg.servo.move_timeout_ms, 800);
        assert_eq!(cfg.servo.state_min_interval_ms, 50);
        assert_eq!(cfg.led.blink_period_ms, 500);
        assert_eq!(cfg.led.effects, led_effects_all(), "灯效白名单子集注入");
        assert_eq!(
            cfg.intent.open_topics,
            vec![
                "rr3-act/relay/set".to_string(),
                "rr3-act/servo/command".to_string()
            ],
            "意图开放集注入"
        );
        assert_eq!(cfg.reflex.loop_hz, 1000);
        assert_eq!(cfg.reflex.saturate_notice_ms, 500);
    }

    /// 可缺面门：占位（装配单未启用该格）只点名不致命；Busy/参数错是真故障。
    #[test]
    fn opt_face_degrades_only_placeholder() {
        assert!(rr3_opt_face("LED", "placeholder", Err(Rr3HalError::Unsupported("x"))).is_ok());
        assert!(rr3_opt_face("LED", "linux", Err(Rr3HalError::Busy)).is_err());
        assert!(rr3_opt_face("继电器", "linux", Ok(true)).is_ok());
    }

    #[test]
    fn config_parse_rejects_missing_sections_and_fields() {
        let e = rr3_act_config_parse("").unwrap_err();
        assert!(e.contains("[rr3-act.servo]"), "缺段点名: {e}");
        // 行程键已撤（批N3 件五门 g），servo 节只剩时限/限速两必填键——
        // 全节齐备即应解析通过（同仓内 config.toml 形状）
        let ok = rr3_act_config_parse(
            "[rr3-act.servo]\nmove_timeout_ms = 800\nstate_min_interval_ms = 50\n[rr3-act.led]\nblink_period_ms = 500\neffects = [\"on\"]\n[rr3-act.intent]\nopen_topics = []\n[rr3-act.reflex]\nloop_hz = 1000\nsaturate_notice_ms = 500\n",
        );
        assert!(ok.is_ok(), "时限/限速两键齐备即完整: {:?}", ok.err());
        // 缺必填键照旧点名（换 move_timeout_ms 当靶）
        let e = rr3_act_config_parse(
            "[rr3-act.servo]\nstate_min_interval_ms = 50\n[rr3-act.led]\nblink_period_ms = 500\neffects = [\"on\"]\n[rr3-act.intent]\nopen_topics = []\n[rr3-act.reflex]\nloop_hz = 1000\nsaturate_notice_ms = 500\n",
        )
        .unwrap_err();
        assert!(e.contains("move_timeout_ms"), "缺字段点名: {e}");
        let e = rr3_act_config_parse("garbage =\n").unwrap_err();
        assert!(e.contains("TOML"), "坏文本点名: {e}");
    }

    /// loop_hz 越界拒 boot（0.0.5 批I A-3）：装不进 u32 的值点名拒绝，
    /// 不得被 `as u32` 裸截断成小值静默变形
    #[test]
    fn config_parse_rejects_loop_hz_overflow() {
        let text = r#"
[rr3-act.servo]
move_timeout_ms = 800
state_min_interval_ms = 50

[rr3-act.led]
blink_period_ms = 500
effects = ["on"]

[rr3-act.intent]
open_topics = []

[rr3-act.reflex]
loop_hz = 5000000000
saturate_notice_ms = 500
"#;
        let e = rr3_act_config_parse(text).unwrap_err();
        assert!(e.contains("loop_hz"), "越界点名: {e}");
        // 本轮审计：装得进 u32 但大到让 1_000_000/hz 归零的档位同样拒
        let text = text.replace("loop_hz = 5000000000", "loop_hz = 4000000000");
        let e = rr3_act_config_parse(&text).unwrap_err();
        assert!(
            e.contains("loop_hz") && e.contains("满转自旋"),
            "超大 loop_hz 点名: {e}"
        );
    }

    /// A-9 fail-closed：灯效白名单子集与意图开放集逐条点名——缺键/类型错/
    /// 越界值都拒 boot（空数组合法，另行覆盖）
    #[test]
    fn config_parse_rejects_bad_effects_and_open_topics() {
        // 基座（四段齐备），按需替换 led.effects / intent.open_topics 行
        let base = |led: &str, top: &str| {
            format!(
                "[rr3-act.servo]\nmove_timeout_ms = 800\nstate_min_interval_ms = 50\n[rr3-act.led]\nblink_period_ms = 500\n{led}\n[rr3-act.intent]\n{top}\n[rr3-act.reflex]\nloop_hz = 1000\nsaturate_notice_ms = 500\n"
            )
        };
        // 缺 effects 键
        let e = rr3_act_config_parse(&base("", "open_topics = []")).unwrap_err();
        assert!(e.contains("effects"), "缺键点名: {e}");
        // 非数组
        let e = rr3_act_config_parse(&base("effects = 1", "open_topics = []")).unwrap_err();
        assert!(e.contains("effects"), "非数组点名: {e}");
        // 元素非字符串
        let e = rr3_act_config_parse(&base("effects = [1]", "open_topics = []")).unwrap_err();
        assert!(e.contains("effects[0]"), "元素类型点名: {e}");
        // 越在册值
        let e =
            rr3_act_config_parse(&base("effects = [\"pulse\"]", "open_topics = []")).unwrap_err();
        assert!(e.contains("非在册灯效"), "越界值点名在册值: {e}");
        // 缺 intent 段
        let e = rr3_act_config_parse(
            "[rr3-act.servo]\nmove_timeout_ms = 800\nstate_min_interval_ms = 50\n[rr3-act.led]\nblink_period_ms = 500\neffects = [\"on\"]\n[rr3-act.reflex]\nloop_hz = 1000\nsaturate_notice_ms = 500\n",
        )
        .unwrap_err();
        assert!(e.contains("[rr3-act.intent]"), "缺段点名: {e}");
        // 缺 open_topics 键
        let e = rr3_act_config_parse(&base("effects = [\"on\"]", "")).unwrap_err();
        assert!(e.contains("open_topics"), "缺键点名: {e}");
        // 非数组
        let e = rr3_act_config_parse(&base("effects = [\"on\"]", "open_topics = 1")).unwrap_err();
        assert!(e.contains("open_topics"), "非数组点名: {e}");
        // 元素非字符串
        let e = rr3_act_config_parse(&base("effects = [\"on\"]", "open_topics = [1]")).unwrap_err();
        assert!(e.contains("open_topics[0]"), "元素类型点名: {e}");
        // 越界主题（不在本模块执行臂内）
        let e = rr3_act_config_parse(&base(
            "effects = [\"on\"]",
            "open_topics = [\"rr3-act/led/effect\"]",
        ))
        .unwrap_err();
        assert!(e.contains("非本模块可执行"), "越界主题点名: {e}");
    }

    /// 仓内默认灯效白名单全集（三值）——config 用例的统一断言基线
    fn led_effects_all() -> Vec<Rr3LedEffect> {
        vec![Rr3LedEffect::On, Rr3LedEffect::Off, Rr3LedEffect::Blink]
    }
}
