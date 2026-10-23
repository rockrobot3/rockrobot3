//! rr3-act 组合件实现（组合件五件套③，0.0.5 批C② 原子重铺）。
//!
//! 组合件 = servo ⊗ led ⊗ relay（composite.toml 的 ⊗ 链；⊗=装配期、
//! 执行单元内函数调用，ADR-0019 裁定 2）。本文件承载装配与运行机制：config
//! 解析（行程/时限等策略经 config 数据化注入，机制件零策略载荷）、HAL 三面
//! 探活、intent 订阅执行面的接入编排（intent.rs 两道归属门，归属面留组合件，
//! 0.0.5 方案 §12.1）、反射环宿主线程（reflex.rs，守护面归批D）、事务面三
//! exposes 的点名分发与主循环。
//!
//! 启动序列（自原入口 rr3_run 逐字迁入，语义零改动）：读 `RR3_MANIFEST`（rrd
//! spawn 注入的 manifest 绝对路径，supervisor.rs）→ [`Rr3BusClient::rr3_connect`]
//! → 读 config.toml → 选 HAL 执行面（servo / led-gpio / relay-gpio）并**开机
//! 探活**（Unsupported 即 exit(1)：能力缺失显式回错，不静默带病常驻）→ 起反射
//! 环宿主线程 → 主循环收帧。主循环只做两件事：
//! 1. 应答本模块 exposes 的四个点名（servo/command、led/effect、relay/set、ops/list——
//!    末者为批N4 件九二的 ops 槽快照读口）；
//! 2. 订阅面：`rr3-brain/intent/result` 到达 → `actions` 结构化动作走两道归属门
//!    （exposes 归属 + 意图面开放）逐条执行 relay/servo（0.0.3 T3，Q1 裁定
//!    "brain 产出意图、执行器广播认领"；servo 批F2 随判向-转向编排开放——判据 8），
//!    状态变化发对应 `relay/state` / `servo/state` 摘要；无 actions 字段即纯对话
//!    轮，记一行日志。
//!
//! servo/state 限速流：状态变化必发，两次至少隔 state_min_interval_ms（Rr3RateGate）。
//! 业务性失败（缺参数/越界/HAL 写失败）折 error 应答回话；传输性失败（发布/应答
//! 写不出去）`eprintln` + `exit(1)`——模块不装"活着"，死交给 supervisor 按
//! restart_policy=always 重启（rrd 的进程语义）。

#[path = "intent.rs"]
mod intent;
#[path = "../atoms/led/led.rs"]
mod led;
#[path = "reflex.rs"]
mod reflex;
#[path = "../atoms/relay/relay.rs"]
mod relay;
#[path = "../atoms/servo/servo.rs"]
mod servo;

use std::time::Duration;

use rr_bus_api::{Rr3BusClient, Rr3TimeBase};
use rr_core::control::Rr3LowGate;
use rr_hal::backend::{rr3_hal_gpio_out, rr3_hal_servo};
use rr_hal_api::{Rr3GpioOut, Rr3PwmServo};
use rr_ipc::{opcode, Rr3PayloadType};
use serde_json::{json, Value};

use led::{rr3_led_effects_join, Rr3Led, Rr3LedEffect, RR3_LED_EFFECT_VALUES};
use servo::Rr3ServoCfg;

/// 低电禁动拒因（MC-5，§20 ② ③）：点名是哪条执行令被闸门拒了、解禁口径、
/// 以及"不自动重放被拒的令"（拒因回显，由上层决定是否重发）。
fn rr3_low_power_reject(what: &str) -> String {
    format!("低电禁动：{what} 拒动（回阈 + 迟滞后解禁，不自动重放）")
}

/// rrd spawn 时注入的 manifest 绝对路径（supervisor.rs；rr-ipc 尚无常量，模块侧本地定义）
const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml）
const RR3_ACT_CONFIG_ENV: &str = "RR3_ACT_CONFIG";

/// rrd spawn 注入的执行轴状态（批N3 件三，supervisor 与 RR3_HAL_BINDINGS 同位注入）：
/// 装配链装载 `hardware/<形态>/axis/*.toml`、按 `RR3_HAL_PERIPH_AXIS`（缺省契约
/// 常量）消歧后，对 instance 相等的恰一份合法档案投影
/// `calibrated=<bool>,hard_stop=<档案词>,lo=<n>,hi=<n>,center=<n>`。未注入（无档案/
/// 坏件/副本/轴错配）与坏形文本一律折保守缺省——§6"未标定不得自动"是代码拒动，
/// 缺真相就按最窄面起跑（行程窄域除外：保守行程 = 器件全域，点动面必须活着）。
const RR3_ACT_AXIS_STATE_ENV: &str = "RR3_ACT_AXIS_STATE";

/// 执行轴状态（批N3 件三建，件五扩形）：拒动判据 [`rr_hal_api::rr3_act_motion_allowed`]
/// 的输入面＋**行程真值**（门 g 收敛：模块行程改读档案——config 键撤除，`lo/hi`
/// 两键就是 `Rr3ServoCfg` 行程的填值来源，`center` 是档案中位锚的随路真值）。五键
/// 都来自档案（件二装载器真值），模块侧零判定载荷——只解析不判域（行程窗口自洽
/// lo<center<hi 在 rrd 装载器已判，这里不设第二裁决点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rr3ActAxisState {
    hard_stop: rr_hal_api::Rr3ActHardStop,
    calibrated: bool,
    /// 行程下限（度）——档案 lo_deg
    lo_deg: u16,
    /// 行程上限（度）——档案 hi_deg
    hi_deg: u16,
    /// 中位锚（度）——档案 center_deg（当前仅随路可查：jog/走位锚随 Rr3Actuator
    /// 进场接线，先保真值不丢）
    center_deg: u16,
}

impl Rr3ActAxisState {
    /// 保守缺省：未标定 + 挡块未知 + **器件全域行程**——判据按 §6 硬约束把 Auto
    /// 全拒，执行面只剩点动；行程给器件全域（0..=180）而非窄域，窄域会杀死点动面
    /// （点动是未标定轴唯一合法动作面，行程窄域等于把"活着"也收走）。center 取
    /// 全域中点 90。
    fn rr3_conservative() -> Self {
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
                        rr_hal_api::Rr3ActHardStop::rr3_act_hard_stop_parse(v).ok_or_else(
                            || format!("执行轴状态 hard_stop={v:?} 不在档案词表（unknown/yes/no）"),
                        )?,
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

/// 订阅面主题之一（module.toml [events].subscribes 同源）：意图面
const RR3_INTENT_TOPIC: &str = "rr3-brain/intent/result";

/// 订阅面主题之二（MCP-3 申报 2026-10-12）：rrd 的低电旗广播——低电禁动闸门的
/// **唯一触发源**（0.0.5 §20 ② ①：判定已在 power 侧、rrd 置旗已在，件内不再判定
/// 电压，避免第三真值点）。帧体 `{topic, reason:"battery-low", percent, voltage_mv}`。
const RR3_SYSTEM_POWER_TOPIC: &str = "rr3-runtime/system/power";

/// 收帧等待上界：无帧时空转一圈的代价，值不敏感（心跳在独立线程）
const RR3_POLL_WAIT: Duration = Duration::from_millis(250);

/// 本模块 exposes 的 ops 槽快照读口（批N4 件九二，module.toml [rpc].exposes 同源）：
/// 白名单组体应答，槽名与 assemble.toml 槽表申报同源（门 d covers 申报的对账面）
const RR3_OPS_LIST_TOPIC: &str = "rr3-act/ops/list";

// 限速门/眨眼节拍只看间隔不看绝对值：时基 = `rr_bus_api::Rr3TimeBase`（Instant
// 单调毫秒，0.0.5 批I A-2 的 std 供给）。旧本地 SystemTime 实现自称"单调"
// 是误称（墙钟可回拨），已删。

/// 组合件入口：装配原子件并跑主循环（原 main.rs `rr3_run` 逐字迁入，打印序列不变）。
pub fn rr3_act_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
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

    // 总线接入：manifest 路径是 rrd spawn 的契约环境变量，直跑（开发调试）须自行设置
    let manifest_path = std::env::var(RR3_MANIFEST_ENV).map_err(|_| {
        format!("缺环境变量 {RR3_MANIFEST_ENV}（rrd spawn 注入；直跑请指向 module.toml）")
    })?;
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读 manifest {manifest_path} 失败: {e}"))?;
    let bus = Rr3BusClient::rr3_connect(&manifest_text)?;
    eprintln!("总线就绪: name={} task={}", bus.rr3_name(), bus.rr3_task());
    // 单调时基（0.0.5 批I A-2）：led 节拍/servo 限速门/led 点名同锚点
    let tb = Rr3TimeBase::rr3_time_base_new();

    // 执行轴状态：装配链注入真值，坏形/缺席折保守缺省并点名（安全向，不退）。
    // 解析先于 config——行程真值随路（批N3 件五门 g 收敛）：Rr3ServoCfg 的行程
    // 填值从这里取，config 行程键已撤（档案是唯一真值点）。
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
        axis.hard_stop.rr3_act_hard_stop_name(),
        axis.lo_deg,
        axis.hi_deg,
        axis.center_deg
    );

    let cfg = rr3_config_load(&manifest_path)?;
    // 行程填值 = 档案注入（批N3 件五门 g）：config 只持时限/限速，行程键撤除后
    // 模块行程真值唯一来源 = RR3_ACT_AXIS_STATE 的 lo/hi（保守缺省时为器件全域）
    let mut cfg = cfg;
    cfg.servo.angle_min_deg = axis.lo_deg;
    cfg.servo.angle_max_deg = axis.hi_deg;
    // 注入的策略面在启动自述里可见（A-9：改数据即改行为，日志是第一证据）
    let intent_open = if cfg.intent.open_topics.is_empty() {
        "(空)".to_string()
    } else {
        cfg.intent.open_topics.join("/")
    };
    eprintln!(
        "config 就绪: servo [{}, {}]（行程=轴档案）/{}/ms state>={}ms blink={}ms led_effects={} intent_open={intent_open} reflex {}Hz",
        cfg.servo.angle_min_deg,
        cfg.servo.angle_max_deg,
        cfg.servo.move_timeout_ms,
        cfg.servo.state_min_interval_ms,
        cfg.led.blink_period_ms,
        led::rr3_led_effects_join(&cfg.led.effects),
        cfg.reflex.loop_hz
    );

    // HAL 执行面选择 + 开机探活：三面各读回一次，Unsupported 即致命——
    // 显式回错不静默退化（rr-hal 占位件的契约），也不给"半个执行器"的假活。
    // 探活全是读面 &self（批I F1 同步化后无需 block_on）
    let (mut servo_dev, servo_backend) = rr3_hal_servo();
    let (mut led_gpio, led_backend) = rr3_hal_gpio_out("led");
    let (mut relay_gpio, relay_backend) = rr3_hal_gpio_out("relay");
    servo_dev
        .angle()
        .map_err(|e| format!("舵机面探活失败({servo_backend}): {e}"))?;
    led_gpio
        .level()
        .map_err(|e| format!("LED 面探活失败({led_backend}): {e}"))?;
    relay_gpio
        .level()
        .map_err(|e| format!("继电器面探活失败({relay_backend}): {e}"))?;
    eprintln!("执行面就绪: servo={servo_backend} led={led_backend} relay={relay_backend}");

    let mut led_ctrl = Rr3Led::rr3_led_new(cfg.led.blink_period_ms);
    // 初始落灭态（原 worker 起手写 0 的语义上收组合件；写失败折日志不退——妆面件）
    led_gpio
        .set(false)
        .unwrap_or_else(|e| eprintln!("rr3-act-led: 初始写电平失败: {e}"));

    // 反射环宿主：进程内 1kHz 闭环的位子，永不进总线（ADR-0007 裁定 1）
    let reflex_bus = bus.clone();
    std::thread::Builder::new()
        .name("rr3-act-reflex".into())
        .spawn(move || reflex::rr3_reflex_loop(cfg.reflex, reflex_bus))
        .map_err(|e| format!("起反射环宿主线程失败: {e}"))?;

    // 主循环状态：限速门 + 流序 + 低电禁动闸门（MC-5）＋ 执行轴状态（前段已解析，
    // 批N3 件三建、件五扩形——行程真值随路）。
    // 闸门复位保守默认（§6 复位语义④）：构造即闭锁未武装。host 档现无轨压样本流
    // （rr3-runtime/system/power 自批N5 件二笔3 起两值 reason——battery-low/battery-ok，
    // 解除沿已进场、e2e_power_ok 点名；battery-ok 到本侧只点名不解禁，见订阅臂——
    // 解禁只认轨压样本），起手武装 + 解禁是**窄化的诚实开局**：假设轨压健康，把
    // "订阅 battery/state 判新鲜样本再武装"的喂入面挂账批N3 件七（D5 读数位）。
    // trip 随闭锁撤销武装，此后 host 无样本可再武装 → 拒动直至重启（battery-ok
    // 不构成样本，解除归喂入面）。
    let mut servo_seq: u64 = 0;
    let mut relay_seq: u64 = 0;
    let mut servo_gate = servo::Rr3RateGate::rr3_gate_new(cfg.servo.state_min_interval_ms);
    let mut low_gate = Rr3LowGate::rr3_low_gate_new();
    low_gate.rr3_low_gate_arm();
    low_gate.rr3_low_gate_release();

    loop {
        // led 节拍（批I A-1：线程上收组合件）：每圈先推状态机，眨眼到点即落
        // 电平（写失败折日志不退——妆面件）
        if let Some(level) = led::rr3_led_tick(tb.rr3_now_ms(), &mut led_ctrl) {
            led_gpio
                .set(level)
                .unwrap_or_else(|e| eprintln!("rr3-act-led: 翻转写电平失败: {e}"));
        }
        // 总线死亡探针：断开后 poll_wait 立即返回空 Vec，与"暂时没帧"不可区分——
        // 不探测，总线一死本循环就满转成僵尸。假了就走"不装活着"的退出语义。
        if !bus.rr3_connected() {
            return Err("总线已断开（reader EOF）：交 supervisor 按 restart_policy 裁定".into());
        }
        for frame in bus.rr3_poll_wait(RR3_POLL_WAIT) {
            if frame.header.opcode != opcode::SEND {
                continue;
            }
            // 事务面：rr3.call 转发帧（Request，trace_id=调用方 task，顺原路应答）
            if let Ok((Rr3PayloadType::Request, body)) = Rr3BusClient::rr3_frame_body(&frame) {
                if body.get("method").and_then(Value::as_str) != Some(rr_core::RR3_METHOD_CALL) {
                    continue;
                }
                let id = body.get("id").and_then(Value::as_u64).unwrap_or(0);
                let params = body.get("params").cloned().unwrap_or(Value::Null);
                let topic = params.get("topic").and_then(Value::as_str).unwrap_or("");
                let args = params.get("args").cloned().unwrap_or(Value::Null);

                // 业务性失败折 error 应答（请求-应答语义），不当致命错误。
                // 低电禁动闸门（MC-5 §20 ② ②）：只辖**执行面** servo/relay（点名与
                // 意图两路同判），**妆面 led 不禁**（禁动它只增加卡死面积）。
                let blocked = low_gate.rr3_low_gate_is_blocked();
                let outcome = match topic {
                    servo::RR3_SERVO_COMMAND_TOPIC => {
                        rr3_handle_servo(&mut *servo_dev, &cfg.servo, &args, blocked, &axis)
                    }
                    led::RR3_LED_EFFECT_TOPIC => rr3_handle_led(
                        &mut led_ctrl,
                        &mut *led_gpio,
                        &args,
                        tb.rr3_now_ms(),
                        &cfg.led.effects,
                    ),
                    relay::RR3_RELAY_SET_TOPIC => {
                        if blocked {
                            Err(rr3_low_power_reject(relay::RR3_RELAY_SET_TOPIC))
                        } else {
                            rr3_relay_parse_args(&args)
                                .and_then(|on| {
                                    relay::rr3_relay_apply(&mut *relay_gpio, on)
                                        .map_err(|e| rr3_relay_apply_error_text(&e))
                                })
                                .map(|(on, prev)| (rr3_relay_result(on), None, Some((on, prev))))
                        }
                    }
                    RR3_OPS_LIST_TOPIC => Ok((
                        json!({ "ops": rr3_act_ops_snapshot(&cfg) }),
                        None,
                        None,
                    )),
                    other => Err(format!(
                        "rr3-act 只 exposes {}/{}/{}/{}，收到 {other}",
                        servo::RR3_SERVO_COMMAND_TOPIC,
                        led::RR3_LED_EFFECT_TOPIC,
                        relay::RR3_RELAY_SET_TOPIC,
                        RR3_OPS_LIST_TOPIC
                    )),
                };
                let reply = match outcome {
                    Ok((reply, state_angle, relay_applied)) => {
                        // servo/state 限速流先于应答上路：被限速挡掉的发布不补发，
                        // 下一次命令自然携带最新角度（流是摘要不是逐拍镜像）
                        if let Some(angle) = state_angle {
                            if servo_gate.rr3_gate_allow(tb.rr3_now_ms()) {
                                servo_seq += 1;
                                bus.rr3_publish(
                                    servo::RR3_SERVO_STATE_TOPIC,
                                    rr3_servo_state_body(servo_seq, angle),
                                )?;
                            }
                        }
                        // relay/state 变化即发（0.0.5 批I A-7）：只在电平**真变**时续
                        // seq 并发布/计数——同态命令不产生状态帧（§9 不变量；
                        // counters 与发布同门，保持键值 = 实际翻转数）
                        if let Some((on, prev)) = relay_applied {
                            if relay::rr3_relay_state_due(prev, on) {
                                relay_seq += 1;
                                bus.rr3_counter_bump(relay::rr3_relay_counter_key(on), 1);
                                bus.rr3_publish(
                                    relay::RR3_RELAY_STATE_TOPIC,
                                    rr3_relay_state_body(relay_seq, on),
                                )?;
                            }
                        }
                        reply
                    }
                    Err(e) => json!({ "error": e }),
                };
                bus.rr3_reply_call(frame.header.trace_id, id, reply)?; // 传输性失败 → exit(1)
                continue;
            }
            // 订阅面：广播帧只认 `rr3-brain/intent/result`（Q1 裁定：brain 产出意图、
            // 执行器广播认领）。actions 结构化动作经 intent.rs 两道归属门逐条执行，
            // 生效即发 relay/state 摘要；拒执行/非法载荷点名折日志（业务性失败）；
            // 无 actions 字段即纯对话轮，记一行日志。
            if let Ok((_, body)) = Rr3BusClient::rr3_frame_body(&frame) {
                let topic = body.get("topic").and_then(Value::as_str).unwrap_or("");
                // 低电禁动闸门（MC-5 §20 ② ①）：触发源唯一 = rrd 低电旗广播
                // `rr3-runtime/system/power`（判定已在 power 侧、rrd 置旗已在，件内
                // 不再判定电压）。置位即闭锁执行面；trip 自带去抖（重复广播不翻转）。
                // 批N5 件二笔3 波及订正：该主题现两值 reason——battery-low 闭锁；
                // **battery-ok 是解除广播，不闭锁也不解禁**（解禁只认轨压样本，复位
                // 语义④；低电段中本件重启会被解除广播误闭锁至重启，缺 reason/未知值
                // 按 fail-closed 闭锁安全向）。
                if topic == RR3_SYSTEM_POWER_TOPIC {
                    let reason = body.get("reason").and_then(Value::as_str).unwrap_or("");
                    if reason == "battery-ok" {
                        eprintln!("低电禁动：battery-ok 解除广播不闭锁（本侧解禁只认轨压样本，喂入面挂账批N3 件七）");
                    } else if low_gate.rr3_low_gate_trip() {
                        eprintln!(
                            "低电禁动：执行面（servo/relay）闭锁，妆面不受辖（{RR3_SYSTEM_POWER_TOPIC} reason={reason}）"
                        );
                    }
                    continue;
                }
                if topic != RR3_INTENT_TOPIC {
                    continue;
                }
                let trace = body.get("trace").and_then(Value::as_u64).unwrap_or(0);
                match body.get("actions") {
                    None | Some(Value::Null) => {
                        eprintln!("intent: trace={trace}（纯对话轮，无动作）");
                    }
                    Some(actions) => {
                        for r in intent::rr3_intent_actions_handle(
                            actions,
                            &mut *relay_gpio,
                            &mut *servo_dev,
                            &cfg.servo,
                            low_gate.rr3_low_gate_is_blocked(),
                            &axis,
                            &cfg.intent.open_topics,
                        ) {
                            match r {
                                Ok(intent::Rr3IntentOutcome::Relay(on, prev)) => {
                                    // 变化即发（0.0.5 批I A-7）：同态命令折日志不发布
                                    if relay::rr3_relay_state_due(prev, on) {
                                        relay_seq += 1;
                                        bus.rr3_counter_bump(relay::rr3_relay_counter_key(on), 1);
                                        bus.rr3_publish(
                                            relay::RR3_RELAY_STATE_TOPIC,
                                            rr3_relay_state_body(relay_seq, on),
                                        )?;
                                        eprintln!(
                                            "intent: trace={trace} relay on={on}，state#{relay_seq} 已发"
                                        );
                                    } else {
                                        eprintln!(
                                            "intent: trace={trace} relay on={on}（同态不续发）"
                                        );
                                    }
                                }
                                Ok(intent::Rr3IntentOutcome::Servo(angle)) => {
                                    // servo/state 限速门同 RPC 路（两条驱动路共用一条状态流）；
                                    // 被限速挡掉的发布不补发，下一次动作自然携带最新角度
                                    if servo_gate.rr3_gate_allow(tb.rr3_now_ms()) {
                                        servo_seq += 1;
                                        bus.rr3_publish(
                                            servo::RR3_SERVO_STATE_TOPIC,
                                            rr3_servo_state_body(servo_seq, angle),
                                        )?;
                                        eprintln!(
                                            "intent: trace={trace} servo angle={angle}，state#{servo_seq} 已发"
                                        );
                                    } else {
                                        eprintln!(
                                            "intent: trace={trace} servo angle={angle}（state 被限速挡下）"
                                        );
                                    }
                                }
                                Err(e) => eprintln!("intent: trace={trace} 拒执行: {e}"),
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 点名处置三元组：(应答体, servo 待发新角（servo 面才有）, relay `(生效, 前一)`
/// （relay 面才有——A-7 同态抑制的组合件侧原料）)。Err = 业务性失败折 result.error。
type Rr3CallOutcome = Result<(Value, Option<u16>, Option<(bool, bool)>), String>;

/// servo/command 点名：低电闸门 → 拒动判据（批N3 件三）→ 参数裁决 → HAL 落角 →
/// 回 (应答体, 待发的 state 角度, relay 无)。`blocked` = 低电禁动闸门闭锁（MC-5：
/// 闸门先于一切裁决，闭锁期一律拒动，拒因明确点名低电）；`axis` = 执行轴状态——
/// 动作形态先裁（§6 硬约束：未标定轴 Auto 拒，判据 rr-hal-api 单真值点），
/// 参数值域后裁（形态都不许，参数值没有意义）。
fn rr3_handle_servo(
    servo: &mut dyn Rr3PwmServo,
    cfg: &Rr3ServoCfg,
    args: &Value,
    blocked: bool,
    axis: &Rr3ActAxisState,
) -> Rr3CallOutcome {
    if blocked {
        return Err(rr3_low_power_reject(servo::RR3_SERVO_COMMAND_TOPIC));
    }
    // 拒动硬约束（§6，批N3 件三）：词缺席 = auto（保守——不申报按自动）；
    // 判据拒收回 InvalidParam，折业务性失败点名言
    let motion = servo::rr3_servo_check_motion(args.get("motion").and_then(Value::as_str))?;
    rr_hal_api::rr3_act_motion_allowed(axis.hard_stop, axis.calibrated, motion)
        .map_err(|e| format!("动作形态 {} 被拒: {e}", motion.rr3_act_motion_name()))?;
    let (angle, move_ms) = rr3_servo_parse_args(args, cfg)?;
    // 安全核钳位最后包一道（批D₃ 裁定 5：安全核永远包住控制图）——上游裁决
    // 被绕过时输出也不越行程；输入合法时恒等，行为守恒
    let angle = rr_core::control::rr3_travel_clamp(angle, cfg.angle_min_deg, cfg.angle_max_deg);
    // HAL 写失败按业务性失败折 error（同 interact 的后端写失败口径）
    servo
        .set_angle(angle)
        .map_err(|e| format!("舵机写入失败: {e}"))?;
    eprintln!("servo: angle={angle} move_ms={move_ms}（sim 即时到位；实机按时限走反射环闭环）");
    Ok((
        json!({ "angle_deg": angle, "move_ms": move_ms }),
        Some(angle),
        None,
    ))
}

/// led/effect 点名：效果裁决 → 状态机切换 + 同步落电平（批I A-1：写失败折日志
/// 不退——妆面件；时基由组合件单调时钟注入，批I A-2）→ 回 (应答体含生效/前效果, servo/relay 无)。
/// `effects` = 本形态白名单子集（A-9：config 注入，非内联）。
fn rr3_handle_led(
    led: &mut Rr3Led,
    gpio: &mut dyn Rr3GpioOut,
    args: &Value,
    now_ms: u64,
    effects: &[Rr3LedEffect],
) -> Rr3CallOutcome {
    let effect = rr3_led_parse_args(args, effects)?;
    let (applied, previous, level) = led.rr3_led_apply(now_ms, effect);
    gpio.set(level)
        .unwrap_or_else(|e| eprintln!("rr3-act-led: 写电平失败: {e}"));
    eprintln!(
        "led: {}（前态 {}）",
        applied.rr3_led_effect_as_str(),
        previous.rr3_led_effect_as_str()
    );
    Ok((
        json!({
            "effect": applied.rr3_led_effect_as_str(),
            "previous": previous.rr3_led_effect_as_str(),
        }),
        None,
        None,
    ))
}

// ── 载荷组装与参数下钻面（0.0.5 批I A-8，ADR-0019 修订 4：json! 组装与 Value
// 下钻上移承载层，件面 serde_json 清零；错误文案逐字，每句落在唯一一处）──

/// relay/set 参数裁决：只认真布尔——`{"on": 1}`、`{"on": "true"}` 都拒，
/// 通断没有"近似真"。（自 relay 原子件上移）
pub fn rr3_relay_parse_args(args: &Value) -> Result<bool, String> {
    let Some(v) = args.get("on") else {
        return Err("缺 on（bool，true=吸合 false=断开）".into());
    };
    v.as_bool()
        .ok_or_else(|| format!("on 必须是布尔，收到 {v}"))
}

/// relay/set 应答体：回显生效后的通断
pub fn rr3_relay_result(on: bool) -> Value {
    json!({ "on": on })
}

/// relay 机制错误的人读文案（批N24 件三，L-3 形状清理）：机制层零堆枚举
/// （`Rr3RelayApplyError`，HAL 错原样随行），文案在编排层渲染——act 册 §9
/// 契约字面量逐字保持（面板与 legacy 依赖字面），每句落在唯一一处。
pub fn rr3_relay_apply_error_text(e: &relay::Rr3RelayApplyError) -> String {
    match e {
        relay::Rr3RelayApplyError::ReadBack(e) => format!("继电器读回失败: {e}"),
        relay::Rr3RelayApplyError::Write(e) => format!("继电器写入失败: {e}"),
    }
}

/// relay/state 摘要体：变化即发，seq 是本状态流自己的计数
/// （contracts/events/act-relay_state.schema.json 的形状事实源）
pub fn rr3_relay_state_body(seq: u64, on: bool) -> Value {
    json!({ "seq": seq, "on": on })
}

/// `rr3-act/ops/list` 应答体（批N4 件九二，面板 /api/act/ops 读口）：ops 槽快照，
/// **白名单组体**——槽名与 assemble.toml 槽表申报同源（led/servo 两组，门 d
/// covers 申报的对账面），config 其余键（intent/reflex 等非槽表键）一律不进应答；
/// 行程真值随轴档案（批N3 件五门 g），不在快照面。灯效子集按词表名成列。
fn rr3_act_ops_snapshot(cfg: &Rr3ActCfg) -> Value {
    json!({
        "led": {
            "blink_period_ms": cfg.led.blink_period_ms,
            "led_effects": cfg
                .led
                .effects
                .iter()
                .map(Rr3LedEffect::rr3_led_effect_as_str)
                .collect::<Vec<_>>(),
        },
        "servo": {
            "move_timeout_ms": cfg.servo.move_timeout_ms,
            "state_min_interval_ms": cfg.servo.state_min_interval_ms,
        },
    })
}

/// servo/command 参数下钻（Value→纯值）：缺参/类型错/越 u16 在这里点名；
/// 行程与时限的值域判定留件（`servo::rr3_servo_check_travel` /
/// `servo::rr3_servo_check_timeout`），点名顺序与上移前逐字同序。
pub fn rr3_servo_parse_args(args: &Value, cfg: &Rr3ServoCfg) -> Result<(u16, u64), String> {
    let angle = args
        .get("angle_deg")
        .ok_or_else(|| "缺 angle_deg（int，行程内角度）".to_string())?;
    let angle = angle
        .as_u64()
        .ok_or_else(|| format!("angle_deg 必须是非负整数，收到 {angle}"))?;
    let angle = u16::try_from(angle).map_err(|_| format!("angle_deg 超出 u16 范围: {angle}"))?;
    // 行程/时限的值域判定留件（零堆枚举，批N24 件三），文案在此渲染逐字
    // （act 册 §9 契约字面量，每句落在唯一一处）
    servo::rr3_servo_check_travel(angle, cfg).map_err(|e| match e {
        servo::Rr3ServoTravelError::OutOfRange {
            angle_deg,
            min_deg,
            max_deg,
        } => format!("angle_deg={angle_deg} 超出行程 [{min_deg}, {max_deg}]"),
    })?;
    let move_ms = match args.get("move_ms") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .ok_or_else(|| format!("move_ms 必须是非负整数，收到 {v}"))?,
        ),
    };
    let move_ms = servo::rr3_servo_check_timeout(move_ms, cfg).map_err(|e| match e {
        servo::Rr3ServoTimeoutError::ZeroMs => "move_ms 必须大于 0".to_string(),
        servo::Rr3ServoTimeoutError::BeyondLimit { ms, limit } => {
            format!("move_ms={ms} 超出时限上限 {limit}（不给堵转留机会）")
        }
    })?;
    Ok((angle, move_ms))
}

/// servo/state 体：seq 逐条 +1（流序），angle_deg 是当前角
pub fn rr3_servo_state_body(seq: u64, angle_deg: u16) -> Value {
    json!({ "seq": seq, "angle_deg": angle_deg })
}

/// led/effect 参数下钻：缺/非串在这里点名；契约值→枚举的映射留件
/// （`Rr3LedEffect::rr3_led_effect_parse`），本形态子集判定用注入的 `effects`
/// （A-9：白名单是数据，不再内联 on/off/blink）。
pub fn rr3_led_parse_args(args: &Value, effects: &[Rr3LedEffect]) -> Result<Rr3LedEffect, String> {
    let Some(e) = args.get("effect") else {
        return Err(format!("缺 effect（{}）", rr3_led_effects_join(effects)));
    };
    let Some(s) = e.as_str() else {
        return Err(format!("effect 必须是字符串，收到 {e}"));
    };
    Rr3LedEffect::rr3_led_effect_parse(s)
        .filter(|eff| effects.contains(eff))
        .ok_or_else(|| {
            format!(
                "不认的灯效 {s:?}（本形态白名单 {}）",
                rr3_led_effects_join(effects)
            )
        })
}

/// config.toml 解析：env 覆盖优先，缺省 manifest 同目录。四段各自点名缺字段。
fn rr3_config_load(manifest_path: &str) -> Result<Rr3ActCfg, String> {
    let path = std::env::var(RR3_ACT_CONFIG_ENV).unwrap_or_else(|_| {
        std::path::Path::new(manifest_path)
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("config.toml")
            .to_string_lossy()
            .into_owned()
    });
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读 config {path} 失败: {e}"))?;
    rr3_act_config_parse(&text)
}

/// 本模块 config 的汇总形状（led/intent 带注入集合 → 去 Copy）
#[derive(Debug, Clone)]
struct Rr3ActCfg {
    servo: Rr3ServoCfg,
    led: Rr3LedCfg,
    intent: Rr3IntentCfg,
    reflex: reflex::Rr3ReflexCfg,
}

/// config `[rr3-act.led]` 的形状（`effects` = 本形态灯效白名单子集，A-9）
#[derive(Debug, Clone)]
struct Rr3LedCfg {
    blink_period_ms: u64,
    effects: Vec<Rr3LedEffect>,
}

/// config `[rr3-act.intent]` 的形状（`open_topics` = 意图执行面开放集，A-9）。
/// 组合件读入后透传给 `intent.rs`——组合件层无 ops 校验机制（⑧-c 只扫
/// `atoms/*/assemble.toml`），故本策略落 config 键而非装配清单 ops 槽。
#[derive(Debug, Clone)]
struct Rr3IntentCfg {
    open_topics: Vec<String>,
}

/// TOML 文本 → Rr3ActCfg。手工下钻不引 serde derive：字段个位数，
/// 点名缺字段的文案比派生宏的默认报错可读。
fn rr3_act_config_parse(text: &str) -> Result<Rr3ActCfg, String> {
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
    // 值偷偷收成 1，节拍面静默变形——try_from 点名拒绝
    let loop_hz_raw = u64_of(&reflex_sect, "loop_hz", "reflex")?.max(1);
    let loop_hz = u32::try_from(loop_hz_raw).map_err(|_| {
        format!("config [rr3-act.reflex] loop_hz={loop_hz_raw} 超出 u32 范围，拒 boot")
    })?;
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
        if s != servo::RR3_SERVO_COMMAND_TOPIC && s != relay::RR3_RELAY_SET_TOPIC {
            return Err(format!(
                "config [rr3-act.intent] open_topics[{i}] 非本模块可执行的意图主题 {s:?}（只认 {}/{}）",
                relay::RR3_RELAY_SET_TOPIC,
                servo::RR3_SERVO_COMMAND_TOPIC
            ));
        }
        open_topics.push(s.to_string());
    }
    Ok(Rr3ActCfg {
        servo: Rr3ServoCfg {
            // 行程占位（批N3 件五）：真值由调用方 rr3_act_run 用执行轴状态注入
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
        reflex: reflex::Rr3ReflexCfg {
            loop_hz,
            saturate_notice_ms: u64_of(&reflex_sect, "saturate_notice_ms", "reflex")?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn servo_cfg() -> servo::Rr3ServoCfg {
        servo::Rr3ServoCfg {
            angle_min_deg: 0,
            angle_max_deg: 180,
            move_timeout_ms: 800,
            state_min_interval_ms: 50,
        }
    }

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

    /// 拒动硬约束·RPC 路（§6，批N3 件三）：未标定轴的 servo/command（无 motion
    /// 键 = auto 保守缺省）折 result.error 点名"未标定"；显式 jog 落角；已标定
    /// 轴 auto 照常。与 intent 路同判（判据 rr-hal-api 单真值点）。
    #[test]
    fn rpc_servo_refuses_uncalibrated_auto_but_jogs() {
        let conservative = Rr3ActAxisState::rr3_conservative();
        let calibrated = Rr3ActAxisState {
            hard_stop: rr_hal_api::Rr3ActHardStop::Yes,
            calibrated: true,
            lo_deg: 60,
            hi_deg: 120,
            center_deg: 90,
        };
        let run = |args: &Value, axis: &Rr3ActAxisState| {
            let mut servo = rr_hal::sim::Rr3SimServo::rr3_sim_servo_new(90);
            rr3_handle_servo(&mut servo, &servo_cfg(), args, false, axis)
        };
        // 未标定 + auto（缺省）：拒动，拒因点名动作形态与未标定
        let e = run(&json!({"angle_deg": 90}), &conservative).unwrap_err();
        assert!(e.contains("未标定"), "{e}");
        assert!(e.contains("auto"), "{e}");
        // 未标定 + 显式 jog：点动落角
        let v = run(&json!({"angle_deg": 90, "motion": "jog"}), &conservative).unwrap();
        assert_eq!(v.0["angle_deg"], 90);
        // 已标定 + auto：照常走位
        let v = run(&json!({"angle_deg": 90}), &calibrated).unwrap();
        assert_eq!(v.0["angle_deg"], 90);
        // 标定面词（calib-*）不进运行命令面（mech 白名单窄化）
        let e = run(
            &json!({"angle_deg": 90, "motion": "calib-openloop"}),
            &calibrated,
        )
        .unwrap_err();
        assert!(e.contains("只认 jog/auto"), "{e}");
    }

    /// 仓内默认灯效白名单全集（三值）——下钻用例的统一实参基线
    fn led_effects_all() -> Vec<Rr3LedEffect> {
        vec![Rr3LedEffect::On, Rr3LedEffect::Off, Rr3LedEffect::Blink]
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
        // 行程两键在 config 解析面恒为占位 0/180——真值由 rr3_act_run 用执行轴
        // 状态覆填（门 g 收敛：档案是唯一真值点）
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

    // ── 载荷组装与下钻用例（随 A-8 自件面迁入：件测"输入→结构体/判定"，
    // 承载层测"输入→字节"）──

    /// relay/set 参数裁决：只认真布尔（自 relay 原子件迁入，文案断言逐字）
    #[test]
    fn relay_parse_accepts_only_real_booleans() {
        assert!(rr3_relay_parse_args(&json!({"on": true})).unwrap());
        assert!(!rr3_relay_parse_args(&json!({"on": false})).unwrap());
        for bad in [
            json!({}),
            json!({"on": 1}),
            json!({"on": "true"}),
            json!({"on": null}),
        ] {
            assert!(rr3_relay_parse_args(&bad).is_err(), "非真布尔必须拒: {bad}");
        }
        let e = rr3_relay_parse_args(&json!({"on": 1})).unwrap_err();
        assert!(e.contains("布尔"), "类型不符要点名: {e}");
    }

    /// relay 应答/摘要体逐字对齐契约 schema（schema 钉死键数，多键即漂移）
    #[test]
    fn relay_body_matches_contract_shape() {
        assert_eq!(rr3_relay_result(true)["on"], true);
        let v = rr3_relay_state_body(3, true);
        assert_eq!(v["seq"], 3);
        assert_eq!(v["on"], true);
        assert_eq!(
            v.as_object().map(|m| m.len()),
            Some(2),
            "schema 钉死两键，多键即漂移: {v}"
        );
    }

    /// servo/command 下钻全链：合法值三条路径（缺 move_ms / 显式时限 / 行程边界）
    #[test]
    fn servo_parse_accepts_angle_with_optional_move_ms() {
        let (angle, ms) = rr3_servo_parse_args(&json!({"angle_deg": 90}), &servo_cfg()).unwrap();
        assert_eq!((angle, ms), (90, 800), "缺 move_ms 取 config 上限");
        let (angle, ms) =
            rr3_servo_parse_args(&json!({"angle_deg": 0, "move_ms": 200}), &servo_cfg()).unwrap();
        assert_eq!((angle, ms), (0, 200), "行程下界合法，时限显式生效");
        let (angle, _) =
            rr3_servo_parse_args(&json!({"angle_deg": 180, "move_ms": 800}), &servo_cfg()).unwrap();
        assert_eq!(angle, 180, "行程上界合法");
    }

    /// 下钻点名：缺参/类型错/越 u16 在承载层；行程内越界由件判定点名
    #[test]
    fn servo_parse_rejects_missing_non_integer_and_out_of_range() {
        let e = rr3_servo_parse_args(&json!({}), &servo_cfg()).unwrap_err();
        assert!(e.contains("angle_deg"), "缺参数要点名: {e}");
        for bad in [
            json!({"angle_deg": -5}),
            json!({"angle_deg": "90"}),
            json!({"angle_deg": 90.5}),
        ] {
            assert!(
                rr3_servo_parse_args(&bad, &servo_cfg()).is_err(),
                "非合法非负整数必须拒: {bad}"
            );
        }
        let e = rr3_servo_parse_args(&json!({"angle_deg": 70000}), &servo_cfg()).unwrap_err();
        assert!(e.contains("u16"), "越 u16 先于行程点名: {e}");
        let e = rr3_servo_parse_args(&json!({"angle_deg": 181}), &servo_cfg()).unwrap_err();
        assert!(e.contains("行程"), "行程内越界由件判定点名: {e}");
    }

    #[test]
    fn servo_parse_rejects_move_ms_beyond_timeout_and_zero() {
        let e = rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 801}), &servo_cfg())
            .unwrap_err();
        assert!(e.contains("时限上限"), "超上限拒收: {e}");
        let e = rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 0}), &servo_cfg())
            .unwrap_err();
        assert!(e.contains("大于 0"), "零时限拒收: {e}");
        let e = rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": "x"}), &servo_cfg())
            .unwrap_err();
        assert!(e.contains("move_ms"), "类型不符点名: {e}");
    }

    /// 错误文案逐字钉死（批N24 件三，L-3 形状清理的对账单测）：机制层改交
    /// 零堆枚举后，编排层渲染的 `String` 必须与改前逐字一致——act 册 §9
    /// 契约字面量（面板与 legacy 依赖字面），零行为改变的直接证据。
    #[test]
    fn servo_parse_error_texts_are_verbatim() {
        assert_eq!(
            rr3_servo_parse_args(&json!({"angle_deg": 181}), &servo_cfg()).unwrap_err(),
            "angle_deg=181 超出行程 [0, 180]"
        );
        assert_eq!(
            rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 801}), &servo_cfg())
                .unwrap_err(),
            "move_ms=801 超出时限上限 800（不给堵转留机会）"
        );
        assert_eq!(
            rr3_servo_parse_args(&json!({"angle_deg": 90, "move_ms": 0}), &servo_cfg())
                .unwrap_err(),
            "move_ms 必须大于 0"
        );
    }

    /// relay 错误文案逐字钉死（批N24 件三）：两腿渲染各一断言，`{e}` 走
    /// `Rr3HalError` Display（HAL 错文本随行原样）。
    #[test]
    fn relay_apply_error_text_is_verbatim() {
        use rr_hal_api::Rr3HalError;
        let e = relay::Rr3RelayApplyError::ReadBack(Rr3HalError::Busy);
        assert_eq!(rr3_relay_apply_error_text(&e), "继电器读回失败: HAL 设备忙");
        let e = relay::Rr3RelayApplyError::Write(Rr3HalError::Busy);
        assert_eq!(rr3_relay_apply_error_text(&e), "继电器写入失败: HAL 设备忙");
    }

    /// servo/state 体 seq/angle 两键钉死（自 servo 原子件迁入）
    #[test]
    fn servo_state_body_carries_seq_and_angle() {
        let v = rr3_servo_state_body(7, 135);
        assert_eq!(v["seq"], 7);
        assert_eq!(v["angle_deg"], 135);
    }

    /// led/effect 下钻：缺/非串/不认逐条点名（A-9：白名单子集由注入实参给）
    #[test]
    fn led_parse_args_rejects_missing_non_string_and_unknown() {
        let all = led_effects_all();
        let e = rr3_led_parse_args(&json!({}), &all).unwrap_err();
        assert!(e.contains("effect"), "缺参数要点名: {e}");
        let e = rr3_led_parse_args(&json!({"effect": 3}), &all).unwrap_err();
        assert!(e.contains("字符串"), "类型不符要点名: {e}");
        let e = rr3_led_parse_args(&json!({"effect": "strobe"}), &all).unwrap_err();
        assert!(e.contains("白名单"), "不认的值点名白名单: {e}");
    }

    /// A-9：注入子集收窄即行为改变——在册但不在子集内的值被拒，报本形态白名单
    #[test]
    fn led_parse_args_narrows_by_injected_subset() {
        let only_on = [Rr3LedEffect::On];
        assert_eq!(
            rr3_led_parse_args(&json!({"effect": "on"}), &only_on).unwrap(),
            Rr3LedEffect::On,
            "子集内合法"
        );
        let e = rr3_led_parse_args(&json!({"effect": "blink"}), &only_on).unwrap_err();
        assert!(e.contains("blink"), "被拒的值要点名: {e}");
        assert!(e.contains("on"), "拒因要点名本形态白名单: {e}");
        // 空子集：一条点名都不认（合法数据，运行期全拒）
        let e = rr3_led_parse_args(&json!({"effect": "on"}), &[]).unwrap_err();
        assert!(e.contains("(空)"), "空白名单点名为空: {e}");
    }
}
