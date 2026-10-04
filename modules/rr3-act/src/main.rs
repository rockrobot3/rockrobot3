//! rr3-act 真入口（0.0.2 T4a）：总线注册/心跳 + 舵机/LED/继电器三事务面 + 反射环宿主。
//!
//! 启动序列与 rr3-sense/rr3-interact 同构：读 `RR3_MANIFEST`（rrd spawn 注入的
//! manifest 绝对路径，supervisor.rs）→ [`Rr3BusClient::rr3_connect`] → 读 config.toml
//! → 选 HAL 执行面（servo / led-gpio / relay-gpio）并**开机探活**（Unsupported 即
//! exit(1)：能力缺失显式回错，不静默带病常驻）→ 起反射环宿主线程 → 主循环收帧。
//! 主循环只做两件事：
//! 1. 应答本模块 exposes 的三个点名（servo/command、led/effect、relay/set）；
//! 2. 排空订阅面：`rr3-brain/intent/result` 到达记一行日志（意图→动作的映射属
//!    策略层，随 brain 演进定义；本模块先保住"订阅的命令流存在"）。
//!
//! servo/state 限速流：状态变化必发，两次至少隔 state_min_interval_ms（Rr3RateGate）。
//! 业务性失败（缺参数/越界/HAL 写失败）折 error 应答回话；传输性失败（发布/应答
//! 写不出去）`eprintln` + `exit(1)`——模块不装"活着"，死交给 supervisor 按
//! restart_policy=always 重启（rrd 的进程语义）。

mod led;
mod reflex;
mod relay;
mod servo;

use std::time::{Duration, Instant};

use rr_bus::Rr3BusClient;
use rr_hal::{backend::rr3_hal_gpio_out, backend::rr3_hal_servo, rr3_block_on, Rr3PwmServo};
use rr_ipc::{opcode, Rr3PayloadType};
use serde_json::{json, Value};

use led::Rr3Led;
use servo::Rr3ServoCfg;

/// rrd spawn 时注入的 manifest 绝对路径（supervisor.rs；rr-ipc 尚无常量，模块侧本地定义）
const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml）
const RR3_ACT_CONFIG_ENV: &str = "RR3_ACT_CONFIG";

/// 订阅面唯一主题（module.toml [events].subscribes 同源）
const RR3_INTENT_TOPIC: &str = "rr3-brain/intent/result";

/// 收帧等待上界：无帧时空转一圈的代价，值不敏感（心跳在独立线程）
const RR3_POLL_WAIT: Duration = Duration::from_millis(250);

fn main() {
    if let Err(e) = rr3_run() {
        eprintln!("rr3-act 退出: {e}");
        std::process::exit(1);
    }
}

fn rr3_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    println!("rr3-act 0.0.1 (0.0.2 T4a 实装)");
    for line in servo::SURFACE
        .iter()
        .copied()
        .chain(led::SURFACE.iter().copied())
        .chain(relay::SURFACE.iter().copied())
        .chain(reflex::SURFACE.iter().copied())
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
    println!(
        "config 就绪: servo [{}, {}]/{}ms state>={}ms blink={}ms reflex {}Hz",
        cfg.servo.angle_min_deg,
        cfg.servo.angle_max_deg,
        cfg.servo.move_timeout_ms,
        cfg.servo.state_min_interval_ms,
        cfg.led.blink_period_ms,
        cfg.reflex.loop_hz
    );

    // HAL 执行面选择 + 开机探活：三面各读回一次，Unsupported 即致命——
    // 显式回错不静默退化（rr-hal 占位件的契约），也不给"半个执行器"的假活
    let (servo_dev, servo_backend) = rr3_hal_servo();
    let (led_gpio, led_backend) = rr3_hal_gpio_out();
    let (relay_gpio, relay_backend) = rr3_hal_gpio_out();
    rr3_block_on(servo_dev.angle())
        .map_err(|e| format!("舵机面探活失败({servo_backend}): {e}"))?;
    rr3_block_on(led_gpio.as_ref().level())
        .map_err(|e| format!("LED 面探活失败({led_backend}): {e}"))?;
    rr3_block_on(relay_gpio.as_ref().level())
        .map_err(|e| format!("继电器面探活失败({relay_backend}): {e}"))?;
    println!(
        "执行面就绪: servo={servo_backend} led={led_backend} relay={relay_backend}"
    );

    let mut led_ctrl = Rr3Led::rr3_led_new(led_gpio.clone(), cfg.led.blink_period_ms);

    // 反射环宿主：进程内 1kHz 闭环的位子，永不进总线（ADR-0007 裁定 1）
    let reflex_bus = bus.clone();
    std::thread::Builder::new()
        .name("rr3-act-reflex".into())
        .spawn(move || reflex::rr3_reflex_loop(cfg.reflex, reflex_bus))
        .map_err(|e| format!("起反射环宿主线程失败: {e}"))?;

    // 主循环状态：限速门 + 流序
    let mut servo_seq: u64 = 0;
    let mut servo_gate = servo::Rr3RateGate::rr3_gate_new(cfg.servo.state_min_interval_ms);

    loop {
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
                if body.get("method").and_then(Value::as_str) != Some(rr_bus::RR3_METHOD_CALL) {
                    continue;
                }
                let id = body.get("id").and_then(Value::as_u64).unwrap_or(0);
                let params = body.get("params").cloned().unwrap_or(Value::Null);
                let topic = params.get("topic").and_then(Value::as_str).unwrap_or("");
                let args = params.get("args").cloned().unwrap_or(Value::Null);

                // 业务性失败折 error 应答（请求-应答语义），不当致命错误
                let outcome = match topic {
                    servo::RR3_SERVO_COMMAND_TOPIC => {
                        rr3_handle_servo(servo_dev.as_ref(), &cfg.servo, &args)
                    }
                    led::RR3_LED_EFFECT_TOPIC => rr3_handle_led(&mut led_ctrl, &args),
                    relay::RR3_RELAY_SET_TOPIC => relay::rr3_relay_parse_args(&args)
                        .and_then(|on| relay::rr3_relay_apply(relay_gpio.as_ref(), on))
                        .map(|on| (relay::rr3_relay_result(on), None)),
                    other => Err(format!(
                        "rr3-act 只 exposes {}/{}/{}，收到 {other}",
                        servo::RR3_SERVO_COMMAND_TOPIC,
                        led::RR3_LED_EFFECT_TOPIC,
                        relay::RR3_RELAY_SET_TOPIC
                    )),
                };
                let reply = match outcome {
                    Ok((reply, state_angle)) => {
                        // servo/state 限速流先于应答上路：被限速挡掉的发布不补发，
                        // 下一次命令自然携带最新角度（流是摘要不是逐拍镜像）
                        if let Some(angle) = state_angle {
                            if servo_gate.rr3_gate_allow(Instant::now()) {
                                servo_seq += 1;
                                bus.rr3_publish(
                                    servo::RR3_SERVO_STATE_TOPIC,
                                    servo::rr3_servo_state_body(servo_seq, angle),
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
            // 订阅面：广播（Event/Notification/Stream）只认 intent/result 记一行日志；
            // 意图→动作映射属策略层，随 brain 演进定义
            if let Ok((_, body)) = Rr3BusClient::rr3_frame_body(&frame) {
                if body.get("topic").and_then(Value::as_str) == Some(RR3_INTENT_TOPIC) {
                    println!(
                        "intent: trace={}（订阅面到达；映射属策略层，本模块不动作）",
                        body.get("trace").and_then(Value::as_u64).unwrap_or(0)
                    );
                }
            }
        }
    }
}

/// servo/command 点名：参数裁决 → HAL 落角 → 回 (应答体, 待发的 state 角度)
fn rr3_handle_servo(
    servo: &dyn Rr3PwmServo,
    cfg: &Rr3ServoCfg,
    args: &Value,
) -> Result<(Value, Option<u16>), String> {
    let (angle, move_ms) = servo::rr3_servo_parse_args(args, cfg)?;
    // HAL 写失败按业务性失败折 error（同 interact 的后端写失败口径）
    rr3_block_on(servo.set_angle(angle)).map_err(|e| format!("舵机写入失败: {e}"))?;
    println!("servo: angle={angle} move_ms={move_ms}（sim 即时到位；实机按时限走反射环闭环）");
    Ok((json!({ "angle_deg": angle, "move_ms": move_ms }), Some(angle)))
}

/// led/effect 点名：效果裁决 → worker 落电平 → 回 (生效, 前一效果)
fn rr3_handle_led(led: &mut Rr3Led, args: &Value) -> Result<(Value, Option<u16>), String> {
    let effect = led::rr3_led_parse_args(args)?;
    let (applied, previous) = led.rr3_led_apply(effect)?;
    println!("led: {}（前态 {}）", applied.rr3_led_effect_as_str(), previous.rr3_led_effect_as_str());
    Ok((
        json!({
            "effect": applied.rr3_led_effect_as_str(),
            "previous": previous.rr3_led_effect_as_str(),
        }),
        None,
    ))
}

/// config.toml 解析：env 覆盖优先，缺省 manifest 同目录。三段各自点名缺字段。
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

/// 本模块 config 的汇总形状
#[derive(Debug, Clone, Copy)]
struct Rr3ActCfg {
    servo: Rr3ServoCfg,
    led: Rr3LedCfg,
    reflex: reflex::Rr3ReflexCfg,
}

/// config `[rr3-act.led]` 的形状
#[derive(Debug, Clone, Copy)]
struct Rr3LedCfg {
    blink_period_ms: u64,
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
    let reflex_sect = sect("reflex")?.clone();
    let range = servo_sect
        .get("angle_limit_deg")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "config [rr3-act.servo] 缺 angle_limit_deg（[min, max]）".to_string())?;
    let bad_range = || "config [rr3-act.servo] angle_limit_deg 必须是 [min, max] 两个非负整数".to_string();
    if range.len() != 2 {
        return Err(bad_range());
    }
    let angle_min_deg = range[0].as_integer().filter(|n| *n >= 0).ok_or_else(bad_range)? as u64;
    let angle_max_deg = range[1].as_integer().filter(|n| *n >= 0).ok_or_else(bad_range)? as u64;
    Ok(Rr3ActCfg {
        servo: Rr3ServoCfg {
            angle_min_deg: u16::try_from(angle_min_deg).map_err(|_| bad_range())?,
            angle_max_deg: u16::try_from(angle_max_deg).map_err(|_| bad_range())?,
            move_timeout_ms: u64_of(&servo_sect, "move_timeout_ms", "servo")?,
            state_min_interval_ms: u64_of(&servo_sect, "state_min_interval_ms", "servo")?,
        },
        led: Rr3LedCfg {
            blink_period_ms: u64_of(&led_sect, "blink_period_ms", "led")?.max(1),
        },
        reflex: reflex::Rr3ReflexCfg {
            loop_hz: u64_of(&reflex_sect, "loop_hz", "reflex")?.max(1) as u32,
            saturate_notice_ms: u64_of(&reflex_sect, "saturate_notice_ms", "reflex")?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红
    #[test]
    fn config_parse_covers_repo_shape() {
        let text = r#"
[rr3-act]
log_level = "info"

[rr3-act.servo]
angle_limit_deg = [0, 180]
move_timeout_ms = 800
state_min_interval_ms = 50

[rr3-act.led]
blink_period_ms = 500

[rr3-act.reflex]
loop_hz = 1000
saturate_notice_ms = 500
"#;
        let cfg = rr3_act_config_parse(text).unwrap();
        assert_eq!(cfg.servo.angle_min_deg, 0);
        assert_eq!(cfg.servo.angle_max_deg, 180);
        assert_eq!(cfg.servo.move_timeout_ms, 800);
        assert_eq!(cfg.servo.state_min_interval_ms, 50);
        assert_eq!(cfg.led.blink_period_ms, 500);
        assert_eq!(cfg.reflex.loop_hz, 1000);
        assert_eq!(cfg.reflex.saturate_notice_ms, 500);
    }

    #[test]
    fn config_parse_rejects_missing_sections_and_fields() {
        let e = rr3_act_config_parse("[rr3-act]\nlog_level = \"info\"\n").unwrap_err();
        assert!(e.contains("[rr3-act.servo]"), "缺段点名: {e}");
        let e = rr3_act_config_parse(
            "[rr3-act.servo]\nmove_timeout_ms = 800\nstate_min_interval_ms = 50\n[rr3-act.led]\nblink_period_ms = 500\n[rr3-act.reflex]\nloop_hz = 1000\nsaturate_notice_ms = 500\n",
        )
        .unwrap_err();
        assert!(e.contains("angle_limit_deg"), "缺字段点名: {e}");
        let e = rr3_act_config_parse("garbage =\n").unwrap_err();
        assert!(e.contains("TOML"), "坏文本点名: {e}");
    }
}
