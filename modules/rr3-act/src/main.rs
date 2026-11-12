//! rr3-act 入口（0.0.8 W-A 批5 拆分，ADR-0017 五层表装配层，≤80 行）。
//! 装配只做启动编排：SURFACE 自述 → [`rr3mod::bootstrap`]（收敛序列前四段）→ 轴状态
//! 装载＋行程填值＋config 就绪行 → 三面探活 → reflex spawn（[`boot`]）→ Dispatcher →
//! [`rr3mod::serve`]。机制在 [`dispatch`]（11 字段＋on_rpc 四臂＋on_event 两臂＋on_tick
//! led 节拍），协议面在 [`rpc`]；intent/reflex 宿主与 `atoms/` 原子件原位不动（§12.1）。

mod boot;
mod dispatch;
mod intent;
#[path = "../atoms/led/led.rs"]
mod led;
mod reflex;
#[path = "../atoms/relay/relay.rs"]
mod relay;
mod rpc;
#[path = "../atoms/servo/servo.rs"]
mod servo;

use std::time::Duration;

use rr3mod::{bootstrap, serve, Rr3ModConfig};

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml；契约环境变量名不变）
const RR3_ACT_CONFIG_ENV: &str = "RR3_ACT_CONFIG";

/// 收帧等待上界：无帧时空转一圈的代价，值不敏感（心跳在独立线程）
const RR3_POLL_WAIT: Duration = Duration::from_millis(250);

fn main() {
    if let Err(e) = rr3_act_run() {
        eprintln!("rr3-act 退出: {e}");
        std::process::exit(1);
    }
}

/// 装配序列（自原 composite.rs `rr3_act_run` 迁入，打印序列逐字不变）。
fn rr3_act_run() -> Result<(), String> {
    boot::rr3_act_banner();

    let (bus, mut cfg) = bootstrap(Rr3ModConfig {
        config_env: Some(RR3_ACT_CONFIG_ENV),
        parse_config: boot::rr3_act_config_parse,
    })?;

    // 轴状态（坏形/缺席折保守缺省；先于 config 就绪行——行程真值随路，批N3 件五门 g）
    let axis = boot::rr3_axis_state_load();
    cfg.servo.angle_min_deg = axis.lo_deg;
    cfg.servo.angle_max_deg = axis.hi_deg;
    let intent_open = boot::rr3_intent_open(&cfg.intent.open_topics);
    eprintln!(
        "config 就绪: servo [{}, {}]（行程=轴档案）/{}/ms state>={}ms blink={}ms led_effects={} intent_open={intent_open} reflex {}Hz",
        cfg.servo.angle_min_deg, cfg.servo.angle_max_deg, cfg.servo.move_timeout_ms,
        cfg.servo.state_min_interval_ms, cfg.led.blink_period_ms, led::rr3_led_effects_join(&cfg.led.effects),
        cfg.reflex.loop_hz
    );

    // HAL 三面探活：各读回一次，Unsupported 即致命——fail-closed。
    // 桥被音频源（rr3-sense）独占时，舵机面自动改走其命令 RPC（A9.2）。
    let (servo_dev, led_gpio, relay_gpio) = boot::rr3_act_probe(&bus)?;
    // 反射环宿主：进程内 1kHz 闭环的位子，永不进总线（ADR-0007 裁定 1）
    boot::rr3_act_spawn_reflex(cfg.reflex, &bus)?;

    let mut dispatcher =
        dispatch::Rr3ActDispatcher::new(servo_dev, led_gpio, relay_gpio, cfg, axis);
    serve(bus, &mut dispatcher, RR3_POLL_WAIT)
}
