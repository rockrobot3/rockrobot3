//! rr3-act 入口（0.0.8 W-A 批5 拆分，ADR-0017 五层表装配层，≤80 行）。
//! 装配只做启动编排：SURFACE 自述 → [`rr3mod::bootstrap`]（收敛序列前四段）→ 轴状态
//! 装载＋行程填值＋config 就绪行 → 三面探活 → reflex spawn（[`boot`]）→ Dispatcher →
//! [`rr3mod::serve`]。机制在 [`dispatch`]（11 字段＋on_rpc 四臂＋on_event 两臂＋on_tick
//! led 节拍），协议面在 [`rpc`]；intent/reflex 宿主留本模块（0.0.5 方案 §12.1），原子件
//! 在 `atoms/` 原位不动。

#[path = "../atoms/servo/servo.rs"]
mod servo;
#[path = "../atoms/led/led.rs"]
mod led;
#[path = "../atoms/relay/relay.rs"]
mod relay;
mod boot;
mod dispatch;
mod intent;
mod reflex;
mod rpc;

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
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    eprintln!("rr3-act 0.0.1 (0.0.2 T4a 实装，0.0.3 T3 意图执行)");
    let lines = servo::SURFACE.iter().copied()
        .chain(led::SURFACE.iter().copied())
        .chain(relay::SURFACE.iter().copied())
        .chain(reflex::SURFACE.iter().copied());
    for line in lines {
        eprintln!("  {line}");
    }

    // 总线接入 + config 读取解析（收敛序列前四段在 rr3mod bootstrap，文案逐字）
    let (bus, mut cfg) = bootstrap(Rr3ModConfig {
        config_env: Some(RR3_ACT_CONFIG_ENV),
        parse_config: boot::rr3_act_config_parse,
    })?;

    // 执行轴状态（坏形/缺席折保守缺省并点名，安全向不退；先于 config 就绪行解析——行程真值随路，批N3 件五门 g 档案唯一真值点）
    let axis = boot::rr3_act_axis_state_load();
    // 行程填值 = 档案注入（门 g）：行程真值唯一来源 = RR3_ACT_AXIS_STATE 的 lo/hi
    cfg.servo.angle_min_deg = axis.lo_deg;
    cfg.servo.angle_max_deg = axis.hi_deg;
    // 注入的策略面在启动自述里可见（A-9：改数据即改行为，日志是第一证据）
    let open = &cfg.intent.open_topics;
    let intent_open = if open.is_empty() { "(空)".to_string() } else { open.join("/") };
    eprintln!(
        "config 就绪: servo [{}, {}]（行程=轴档案）/{}/ms state>={}ms blink={}ms led_effects={} intent_open={intent_open} reflex {}Hz",
        cfg.servo.angle_min_deg, cfg.servo.angle_max_deg, cfg.servo.move_timeout_ms,
        cfg.servo.state_min_interval_ms, cfg.led.blink_period_ms, led::rr3_led_effects_join(&cfg.led.effects),
        cfg.reflex.loop_hz
    );

    // HAL 执行面选择 + 开机探活：三面各读回一次，Unsupported 即致命——fail-closed
    // 显式回错不静默退化，也不给"半个执行器"的假活（探活全是读面 &self）
    let (servo_dev, led_gpio, relay_gpio) = boot::rr3_act_probe()?;

    // 反射环宿主：进程内 1kHz 闭环的位子，永不进总线（ADR-0007 裁定 1）
    boot::rr3_act_spawn_reflex(cfg.reflex, &bus)?;

    // 机制层注册面构造 + 主循环骨架（rr3mod serve：死亡探针/分帧/RPC/订阅/节拍）
    let mut dispatcher = dispatch::Rr3ActDispatcher::new(servo_dev, led_gpio, relay_gpio, cfg, axis);
    serve(bus, &mut dispatcher, RR3_POLL_WAIT)
}
