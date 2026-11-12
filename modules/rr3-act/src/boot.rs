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
use rr_hal::backend::{
    rr3_hal_gpio_out, rr3_hal_servo, rr3_servo_backend, RR3_BACKEND_ESP32, RR3_BACKEND_PLACEHOLDER,
};
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
        let mut raw = Rr3AxisRaw::default();
        Self::rr3_axis_pairs(text, &mut raw)?;
        Self::rr3_axis_finish(raw)
    }

    /// 键值对切分：形状错点名，逐键交 [`rr3_axis_kv`] 路由。
    fn rr3_axis_pairs(text: &str, raw: &mut Rr3AxisRaw) -> Result<(), String> {
        for kv in text.split(',') {
            let (k, v) = Self::rr3_axis_split(kv)?;
            rr3_axis_kv(raw, k, v)?;
        }
        Ok(())
    }

    /// 单个键值对切分（缺 `=` 点名形状）。
    fn rr3_axis_split(kv: &str) -> Result<(&str, &str), String> {
        kv.split_once('=').ok_or_else(|| {
            format!(
                "执行轴状态段 {kv:?} 缺 =（形状 calibrated=<bool>,hard_stop=<词>,lo=<n>,hi=<n>,center=<n>）"
            )
        })
    }

    /// 五键齐备检查 → 投影（缺键逐条点名）。
    fn rr3_axis_finish(raw: Rr3AxisRaw) -> Result<Self, String> {
        let (calibrated, hard_stop) = rr3_axis_flags(raw.calibrated, raw.hard_stop)?;
        let (lo_deg, hi_deg, center_deg) = rr3_axis_pose_of(raw.lo, raw.hi, raw.center)?;
        Ok(Self {
            calibrated,
            hard_stop,
            lo_deg,
            hi_deg,
            center_deg,
        })
    }
}

/// 布尔/词表两键（缺席逐条点名）。
fn rr3_axis_flags(
    calibrated: Option<bool>,
    hard_stop: Option<rr_hal_api::Rr3ActHardStop>,
) -> Result<(bool, rr_hal_api::Rr3ActHardStop), String> {
    Ok((
        calibrated.ok_or("执行轴状态缺 calibrated")?,
        hard_stop.ok_or("执行轴状态缺 hard_stop")?,
    ))
}

/// 三数值键（缺席逐条点名）。
fn rr3_axis_pose_of(
    lo: Option<u16>,
    hi: Option<u16>,
    center: Option<u16>,
) -> Result<(u16, u16, u16), String> {
    Ok((
        lo.ok_or("执行轴状态缺 lo（档案行程下限）")?,
        hi.ok_or("执行轴状态缺 hi（档案行程上限）")?,
        center.ok_or("执行轴状态缺 center（档案中位锚）")?,
    ))
}

/// 注入文本的原始槽位（五键各一，重复键由 [`rr3_axis_seen`] 拒）。
#[derive(Default)]
struct Rr3AxisRaw {
    calibrated: Option<bool>,
    hard_stop: Option<rr_hal_api::Rr3ActHardStop>,
    lo: Option<u16>,
    hi: Option<u16>,
    center: Option<u16>,
}

/// 单键解析（**只做路由**；值与词表/域校验各自成体，文案逐字保留）。
/// B9：重复键判定前移一次（[`rr3_axis_seen`]），三数值键归一臂
/// [`rr3_axis_pose`]——路由本身不再每臂各判一次。
fn rr3_axis_kv(raw: &mut Rr3AxisRaw, k: &str, v: &str) -> Result<(), String> {
    if rr3_axis_seen(raw, k)? {
        return Err(format!("执行轴状态 {k} 重复"));
    }
    rr3_axis_slot(raw, k, v)
}

/// 单键落位分派（重复键已由调用方判过；未知键点名）。
fn rr3_axis_slot(raw: &mut Rr3AxisRaw, k: &str, v: &str) -> Result<(), String> {
    match k {
        "calibrated" | "hard_stop" => rr3_axis_flag(raw, k, v),
        "lo" | "hi" | "center" => rr3_axis_pose(raw, k, v),
        other => Err(format!("执行轴状态未知键 {other:?}")),
    }
}

/// calibrated/hard_stop 两键（显式布尔 / 档案词表）。
fn rr3_axis_flag(raw: &mut Rr3AxisRaw, k: &str, v: &str) -> Result<(), String> {
    match k {
        "calibrated" => raw.calibrated = Some(rr3_axis_bool(v)?),
        "hard_stop" => raw.hard_stop = Some(rr3_axis_stop(v)?),
        _ => return Err(format!("执行轴状态未知键 {k:?}")),
    }
    Ok(())
}

/// 键是否已出现（重复键判据的单点；未知键在此点名，与旧 [`rr3_axis_once`]
/// 同一文案域）。
fn rr3_axis_seen(raw: &Rr3AxisRaw, k: &str) -> Result<bool, String> {
    match k {
        "calibrated" => Ok(raw.calibrated.is_some()),
        "hard_stop" => Ok(raw.hard_stop.is_some()),
        "lo" => Ok(raw.lo.is_some()),
        "hi" => Ok(raw.hi.is_some()),
        "center" => Ok(raw.center.is_some()),
        other => Err(format!("执行轴状态未知键 {other:?}")),
    }
}

/// lo/hi/center 三数值键（同形 u16 域，文案按键取域说明）
fn rr3_axis_pose(raw: &mut Rr3AxisRaw, k: &str, v: &str) -> Result<(), String> {
    let n = rr3_axis_num(v, k, rr3_axis_what(k))?;
    match k {
        "lo" => raw.lo = Some(n),
        "hi" => raw.hi = Some(n),
        _ => raw.center = Some(n),
    }
    Ok(())
}

/// lo/hi/center 三数值键的文案域说明。
fn rr3_axis_what(k: &str) -> &'static str {
    match k {
        "lo" => "行程下限",
        "hi" => "行程上限",
        _ => "中位锚",
    }
}

/// 显式布尔（只认 `true`/`false`：注入链错值必须点名，不猜同义写法）。
fn rr3_axis_bool(v: &str) -> Result<bool, String> {
    match v {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("执行轴状态 calibrated={v:?} 非显式布尔")),
    }
}

/// 硬限位词表（档案三值 `unknown/yes/no`；词表外点名回错）。
fn rr3_axis_stop(v: &str) -> Result<rr_hal_api::Rr3ActHardStop, String> {
    rr_hal_api::Rr3ActHardStop::rr3_hard_stop_parse(v)
        .ok_or_else(|| format!("执行轴状态 hard_stop={v:?} 不在档案词表（unknown/yes/no）"))
}

/// u16 域整数键（lo/hi/center 三键同形；`what` 是文案里的域说明）。
fn rr3_axis_num(v: &str, k: &str, what: &str) -> Result<u16, String> {
    v.parse::<u16>()
        .map_err(|_| format!("执行轴状态 {k}={v:?} 非{what}整数（u16 域）"))
}

/// 执行轴状态装载：env 读 + 坏形/缺席折保守缺省并点名（安全向，不退），回显注入
/// 的策略面（A-9：改数据即改行为，日志是第一证据）。自原 composite.rs 逐字迁入
/// （缺席/坏形/回显三文案逐字）。
pub(crate) fn rr3_axis_state_load() -> Rr3ActAxisState {
    let axis = rr3_axis_env_axis();
    rr3_axis_echo(&axis);
    axis
}

/// env 读取 + 坏形/缺席折保守缺省并点名（安全向，不退）。
fn rr3_axis_env_axis() -> Rr3ActAxisState {
    match std::env::var(RR3_ACT_AXIS_STATE_ENV) {
        Ok(text) => Rr3ActAxisState::rr3_from_env(&text).unwrap_or_else(|e| rr3_axis_bad(&e)),
        Err(_) => {
            eprintln!("执行轴状态: 未注入（无档案/坏件/副本/轴错配），按保守缺省起跑");
            Rr3ActAxisState::rr3_conservative()
        }
    }
}

/// 注入文本坏形：点名后折保守缺省（安全向，不退）。
fn rr3_axis_bad(e: &str) -> Rr3ActAxisState {
    eprintln!("rr3-act: {e}，按保守缺省起跑（未标定，只受点动）");
    Rr3ActAxisState::rr3_conservative()
}

/// 注入策略面回显（A-9：改数据即改行为，日志是第一证据）。
fn rr3_axis_echo(axis: &Rr3ActAxisState) {
    eprintln!(
        "执行轴状态: calibrated={} hard_stop={} 行程=[{}, {}] 中位={}（注入策略面在启动自述可见，A-9）",
        axis.calibrated,
        axis.hard_stop.rr3_hard_stop_name(),
        axis.lo_deg,
        axis.hi_deg,
        axis.center_deg
    );
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

/// 桥共享判据（纯函数）：舵机面与音频采集面同选 esp32 ⇒ 同一外设桥，契约所有者
/// 是音频面 rr3-sense；act 不参与 flock 竞争，直接走远端命令面。
/// 只在宿主档有跨进程命令面（`rr3_bridge_shared` 的 linux/macos 臂）——rtos/bare
/// 档恒 false，函数随臂一起 cfg，避免板侧编出"从未被调用"的死码。
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rr3_shared_rule(servo: &str, audio: &str) -> bool {
    servo == RR3_BACKEND_ESP32 && audio == RR3_BACKEND_ESP32
}

/// 生效后端上裁决桥是否共享：音频面覆盖键优先于全局（与 rr-hal 同一选择函数）。
/// 非宿主档没有跨进程命令面，恒 false（保持既有 rtos 行为）。
fn rr3_bridge_shared(servo: &str) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        rr_hal::backend::rr3_audio_backend()
            .map(|audio| rr3_shared_rule(servo, audio))
            .unwrap_or(false)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = servo;
        false
    }
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
    let (servo_dev, servo_backend) = rr3_servo_pick(bus)?;
    let (led_gpio, led_backend) = rr3_hal_gpio_out("led");
    let (relay_gpio, relay_backend) = rr3_hal_gpio_out("relay");
    rr3_probe_quiet(
        (led_backend, led_gpio.as_ref()),
        (relay_backend, relay_gpio.as_ref()),
    )?;
    eprintln!("执行面就绪: servo={servo_backend} led={led_backend} relay={relay_backend}");
    Ok((servo_dev, led_gpio, relay_gpio))
}

/// 可缺面（LED / 继电器）逐面探活：占位只点名，其余错误致命。
fn rr3_probe_quiet(
    led: (&str, &dyn Rr3GpioOut),
    relay: (&str, &dyn Rr3GpioOut),
) -> Result<(), String> {
    rr3_opt_face("LED", led.0, led.1.level())?;
    rr3_opt_face("继电器", relay.0, relay.1.level())?;
    Ok(())
}

/// 舵机面选择：共享桥单所有者优先走远端命令面，否则本进程开桥探活。
fn rr3_servo_pick(bus: &BusHandle) -> Result<(Box<dyn Rr3PwmServo>, &'static str), String> {
    // 单所有者优先（A9 根因）：音频采集面同走 esp32 桥时，桥的契约所有者是
    // rr3-sense（音频流 + 命令旁路）。此处**不得先开桥再等 Busy**——两进程同时
    // 起跑时 act 先 open 会赢下 flock，sense 随后连报 HAL 设备忙（2026-11-09
    // 真板实录，两次部署复现）。改为直接走远端命令面；sense 未起时 act 显式
    // 失败交 supervisor 退避重试，绝不抢桥。
    let backend = rr3_servo_backend().unwrap_or(RR3_BACKEND_PLACEHOLDER);
    if rr3_bridge_shared(backend) {
        return rr3_remote_bridge(bus, backend);
    }
    rr3_servo_open(bus)
}

/// 本进程舵机面探活（Busy + esp32 降级远端，其余读失败 fail-closed 上抛）。
fn rr3_servo_open(bus: &BusHandle) -> Result<(Box<dyn Rr3PwmServo>, &'static str), String> {
    let (dev, backend) = rr3_hal_servo();
    match dev.angle() {
        Ok(_) => Ok((dev, backend)),
        Err(Rr3HalError::Busy) if backend == RR3_BACKEND_ESP32 => rr3_remote_bridge(bus, backend),
        Err(e) => Err(format!("舵机面探活失败({backend}): {e}")),
    }
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

/// 饱和通告窗上界（毫秒，本轮静态审计）：`rr3_reflex_plan` 以 250ms 窗折算
/// 窗数 `div_ceil(… ) as u32`——无上界的巨值会静默截断成任意窗数（饱和通告
/// 语义失真）；超 10min 的连续饱和通告窗在生产无意义，超界即配置错点名拒。
const RR3_MAX_NOTICE_MS: u64 = 600_000;

/// TOML 文本 → Rr3ActCfg。手工下钻不引 serde derive：字段个位数，
/// 点名缺字段的文案比派生宏的默认报错可读。
pub(crate) fn rr3_act_config_parse(text: &str) -> Result<Rr3ActCfg, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("config TOML 解析失败: {e}"))?;
    rr3_act_cfg_build(&v)
}

/// 四个段址齐取后逐段解析（取址次序与解析次序分开，便于各自定位）。
fn rr3_act_cfg_build(v: &toml::Value) -> Result<Rr3ActCfg, String> {
    let servo_sect = rr3_act_cfg_sect(v, "servo")?;
    let led_sect = rr3_act_cfg_sect(v, "led")?;
    let intent_sect = rr3_act_cfg_sect(v, "intent")?;
    let reflex_sect = rr3_act_cfg_sect(v, "reflex")?;
    rr3_act_cfg_parts(servo_sect, led_sect, intent_sect, reflex_sect)
}

/// 段 → 汇总形状（字段按声明序求值）。
fn rr3_act_cfg_parts(
    servo_sect: &toml::Value,
    led_sect: &toml::Value,
    intent_sect: &toml::Value,
    reflex_sect: &toml::Value,
) -> Result<Rr3ActCfg, String> {
    Ok(Rr3ActCfg {
        servo: rr3_act_servo(servo_sect)?,
        led: rr3_act_led(led_sect)?,
        intent: Rr3IntentCfg {
            open_topics: rr3_act_cfg_topics(intent_sect)?,
        },
        reflex: rr3_act_reflex(reflex_sect)?,
    })
}

/// `[rr3-act.<名>]` 段取址：缺段点名段名（解析面唯一入口）。
fn rr3_act_cfg_sect<'a>(v: &'a toml::Value, name: &str) -> Result<&'a toml::Value, String> {
    v.get("rr3-act")
        .and_then(|s| s.get(name))
        .ok_or_else(|| format!("config 缺 [rr3-act.{name}] 段"))
}

/// 段内非负整数字段：缺键/类型错/负数一律点名 `[rr3-act.<段>] 缺 <键>`。
fn rr3_act_cfg_u64(sect: &toml::Value, key: &str, name: &str) -> Result<u64, String> {
    sect.get(key)
        .and_then(toml::Value::as_integer)
        .filter(|n| *n >= 0)
        .map(|n| n as u64)
        .ok_or_else(|| format!("config [rr3-act.{name}] 缺 {key}（非负整数）"))
}

/// servo 段：只管时限/限速。行程键不在 config（批N3 件五门 g 收敛）：Rr3ServoCfg
/// 的 angle_min/max_deg 由执行轴状态注入填值（RR3_ACT_AXIS_STATE 的 lo/hi，
/// 档案真值），config 出现任何行程键 = 读者零的死键，装配期（rrd assemble scan）
/// 与 CI 门 ⑨-e 双面执法，此处不再解析。
fn rr3_act_servo(sect: &toml::Value) -> Result<Rr3ServoCfg, String> {
    Ok(Rr3ServoCfg {
        // 行程占位（批N3 件五）：真值由装配段 rr3_act_run 用执行轴状态注入
        // 覆填（cfg.servo.angle_min/max_deg = axis.lo/hi_deg），此处不解析
        angle_min_deg: 0,
        angle_max_deg: 180,
        move_timeout_ms: rr3_act_cfg_u64(sect, "move_timeout_ms", "servo")?,
        state_min_interval_ms: rr3_act_cfg_u64(sect, "state_min_interval_ms", "servo")?,
    })
}

/// led 段：眨眼周期下限 1ms（0 为无意义自旋），灯效白名单子集见 `rr3_act_cfg_effects`。
fn rr3_act_led(sect: &toml::Value) -> Result<Rr3LedCfg, String> {
    Ok(Rr3LedCfg {
        blink_period_ms: rr3_act_cfg_u64(sect, "blink_period_ms", "led")?.max(1),
        effects: rr3_act_cfg_effects(sect)?,
    })
}

/// reflex 段：节拍走越界裁决（`rr3_act_cfg_loop`），饱和提示阈值同域裁决。
fn rr3_act_reflex(sect: &toml::Value) -> Result<Rr3ReflexCfg, String> {
    let notice = rr3_act_notice(sect)?;
    Ok(Rr3ReflexCfg {
        loop_hz: rr3_act_cfg_loop(sect)?,
        saturate_notice_ms: notice,
    })
}

/// 饱和通告窗阈：读数 + 上界裁决。
fn rr3_act_notice(sect: &toml::Value) -> Result<u64, String> {
    let notice = rr3_act_cfg_u64(sect, "saturate_notice_ms", "reflex")?;
    if notice > RR3_MAX_NOTICE_MS {
        return Err(format!(
            "config [rr3-act.reflex] saturate_notice_ms 须在 0..={RR3_MAX_NOTICE_MS}，收到 {notice}（窗数以 u32 折算，超界会静默截断）"
        ));
    }
    Ok(notice)
}

/// reflex 环节拍：越界拒 boot（0.0.5 批I A-3）——`as u32` 裸截断会把 2^32+1
/// 之类值偷偷收成 1，节拍面静默变形；上界补 1kHz（`1_000_000/loop_hz` 在极值
/// 时取整为 0µs，反射环会退化为满转自旋）。
fn rr3_act_cfg_loop(sect: &toml::Value) -> Result<u32, String> {
    let raw = rr3_act_loop_raw(sect)?;
    let hz = u32::try_from(raw)
        .map_err(|_| format!("config [rr3-act.reflex] loop_hz={raw} 超出 u32 范围，拒 boot"))?;
    rr3_loop_cap(hz)?;
    Ok(hz)
}

/// 环节拍上界（1..=上限）。
fn rr3_loop_cap(hz: u32) -> Result<(), String> {
    if hz > RR3_MAX_LOOP_HZ {
        return Err(format!(
            "config [rr3-act.reflex] loop_hz 须在 1..={RR3_MAX_LOOP_HZ}，收到 {hz}（更大值会退化为满转自旋）"
        ));
    }
    Ok(())
}

/// loop_hz 原始值（非负整数，下限 1）。
fn rr3_act_loop_raw(sect: &toml::Value) -> Result<u64, String> {
    Ok(sect
        .get("loop_hz")
        .and_then(toml::Value::as_integer)
        .filter(|n| *n >= 0)
        .map(|n| n as u64)
        .ok_or_else(|| "config [rr3-act.reflex] 缺 loop_hz（非负整数）".to_string())?
        .max(1))
}

/// 灯效白名单子集（A-9）：数组逐元素必须是**在册契约值**，非在册值或非字符串
/// 逐条点名拒 boot（fail-closed）；键在而数组为空 = 本形态一条点名都不认（合法）。
fn rr3_act_cfg_effects(sect: &toml::Value) -> Result<Vec<Rr3LedEffect>, String> {
    let arr = sect
        .get("effects")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "config [rr3-act.led] 缺 effects（灯效白名单数组）".to_string())?;
    rr3_effects_of(arr)
}

/// 灯效数组逐项落位（顺序与下标文案随项）。
fn rr3_effects_of(arr: &[toml::Value]) -> Result<Vec<Rr3LedEffect>, String> {
    let mut effects = Vec::with_capacity(arr.len());
    for (i, e) in arr.iter().enumerate() {
        effects.push(rr3_act_effect_at(i, e)?);
    }
    Ok(effects)
}

/// 单项灯效：必为在册契约值的字符串（越界值点名在册值）。
fn rr3_act_effect_at(i: usize, e: &toml::Value) -> Result<Rr3LedEffect, String> {
    let s = rr3_act_effect_str(i, e)?;
    Rr3LedEffect::rr3_led_effect_parse(s).ok_or_else(|| {
        format!(
            "config [rr3-act.led] effects[{i}] 非在册灯效 {s:?}（在册值 {}）",
            RR3_LED_EFFECT_VALUES.join("/")
        )
    })
}

/// 单项灯效必须是字符串（非字符串逐条点名）。
fn rr3_act_effect_str(i: usize, e: &toml::Value) -> Result<&str, String> {
    e.as_str()
        .ok_or_else(|| format!("config [rr3-act.led] effects[{i}] 必须是字符串，收到 {e}"))
}

/// 意图执行面开放集（A-9）：只认本模块**有执行臂**的在册意图主题
/// （relay/set、servo/command），越界值逐条点名拒 boot（fail-closed）；空集合法。
fn rr3_act_cfg_topics(sect: &toml::Value) -> Result<Vec<String>, String> {
    let arr = sect
        .get("open_topics")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "config [rr3-act.intent] 缺 open_topics（意图开放主题数组）".to_string())?;
    rr3_topics_of(arr)
}

/// 主题数组逐项落位（顺序与下标文案随项）。
fn rr3_topics_of(arr: &[toml::Value]) -> Result<Vec<String>, String> {
    let mut topics = Vec::with_capacity(arr.len());
    for (i, t) in arr.iter().enumerate() {
        topics.push(rr3_act_topic_at(i, t)?);
    }
    Ok(topics)
}

/// 单项主题：字符串且在本模块执行臂内（只认 relay/set 与 servo/command）。
fn rr3_act_topic_at(i: usize, t: &toml::Value) -> Result<String, String> {
    let s = rr3_act_topic_str(i, t)?;
    if !rr3_topic_known(s) {
        return Err(format!(
            "config [rr3-act.intent] open_topics[{i}] 非本模块可执行的意图主题 {s:?}（只认 {}/{}）",
            RR3_RELAY_SET_TOPIC, RR3_SERVO_COMMAND_TOPIC
        ));
    }
    Ok(s.to_string())
}

/// 本模块有执行臂的意图主题（relay/set 与 servo/command）。
fn rr3_topic_known(s: &str) -> bool {
    s == RR3_SERVO_COMMAND_TOPIC || s == RR3_RELAY_SET_TOPIC
}

/// 单项必须是字符串（非字符串逐条点名）。
fn rr3_act_topic_str(i: usize, t: &toml::Value) -> Result<&str, String> {
    t.as_str()
        .ok_or_else(|| format!("config [rr3-act.intent] open_topics[{i}] 必须是字符串，收到 {t}"))
}

/// 启动自述：模块名/版本行 + 四个原子件的 SURFACE 行（装配层调用，文案逐字不变）。
pub(crate) fn rr3_act_banner() {
    use crate::{led, reflex, relay, servo};
    eprintln!("rr3-act 0.0.1 (0.0.2 T4a 实装，0.0.3 T3 意图执行)");
    for line in servo::SURFACE
        .iter()
        .copied()
        .chain(led::SURFACE.iter().copied())
        .chain(relay::SURFACE.iter().copied())
        .chain(reflex::SURFACE.iter().copied())
    {
        eprintln!("  {line}");
    }
}

/// intent 开放集展示串（空集 = `(空)`）。
pub(crate) fn rr3_intent_open(open: &[String]) -> String {
    if open.is_empty() {
        "(空)".into()
    } else {
        open.join("/")
    }
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

    /// 桥共享判据：只有舵机面与音频面同选 esp32 才归 sense 独占。
    #[test]
    fn shared_rule_only_when_both_esp32() {
        assert!(rr3_shared_rule("esp32", "esp32"));
        assert!(!rr3_shared_rule("esp32", "linux"));
        assert!(!rr3_shared_rule("linux", "esp32"));
        assert!(!rr3_shared_rule("sim", "sim"));
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

    /// 饱和通告窗越界拒 boot（本轮静态审计）：`rr3_reflex_plan` 的窗数折算
    /// `div_ceil(…) as u32` 会把无上界巨值静默截断，语义失真且不可定位。
    #[test]
    fn config_parse_rejects_notice_overflow() {
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
loop_hz = 1000
saturate_notice_ms = 18446744073709551615
"#;
        let e = rr3_act_config_parse(text).unwrap_err();
        assert!(e.contains("saturate_notice_ms"), "越界点名: {e}");
        let ok = text.replace(
            "saturate_notice_ms = 18446744073709551615",
            "saturate_notice_ms = 600000",
        );
        assert_eq!(
            rr3_act_config_parse(&ok).unwrap().reflex.saturate_notice_ms,
            600_000,
            "上界本身合法"
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
