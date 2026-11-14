//! 树莓派 4B 板侧五端口后端（W4/P0-C，批N43）——**平台适配层**：
//!
//! 本文件只做"开设备、收帧/收行、分发、调机制核、回包"的装配与循环；
//! 帧面机制在 `periph-mech::frame`、OLED 编码在 `periph-mech::oled_codec`、
//! 舵机缓速/行程/轨压判定在 `periph-mech::periph_core`+`rail_judge`，
//! 五端口实现在 [`ports`]，策略量在 [`board`]。换板只改 `board`/`ports`，
//! 机制与协议字节零改动（U1/U2/U3 判据的板侧半边）。
//!
//! 运行形态：单线程控制面（UART 独占；I2C/PWM 慢动作在同一线程串行执行——
//! T3 无语音/无 WANDER/无并行音频，串行即可，且避免共享总线锁）。
//! 板侧双轴舵机的"机制件"按计划 §8.2 复用 `atoms/servo/mech.rs`（含生成行程表），
//! 面目几何复用 `atoms/face/mech.rs`——**同件复用，不抄第二份**。

mod app;
mod args;
mod board;
#[allow(dead_code)]
#[path = "../../esp32-periph-base/atoms/face/mech.rs"]
mod face;
mod face_cmd;
mod frame;
mod line;
mod motion;
mod oled;
mod ports;
#[allow(dead_code)]
#[path = "../../esp32-periph-base/atoms/servo/mech.rs"]
mod servo;
#[allow(dead_code)]
#[path = "../../esp32-periph-base/src/topics_gen.rs"]
mod topics_gen;
mod uart;
mod view;

use crate::app::Board;

fn main() {
    let uart = match ports::open_default_uart() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[periph] 启动失败: {e}");
            std::process::exit(1);
        }
    };
    let i2c = ports::open_default_i2c();
    let mut board = Board::new(uart, i2c);
    println!("READY periph-base(pi) v0.1.0 五端口（UART/I2C/PWM/ADS1115/CLOCK_MONOTONIC）");
    board.boot_surfaces();
    board.run();
}

#[cfg(test)]
mod tests {
    use crate::app::State;
    use crate::args::{parse_axis, parse_deg};
    use crate::board::{BIND_I2C_SCL, BIND_I2C_SDA, BIND_UART2_RX, BIND_UART2_TX};
    use crate::frame::reconcile_bindings;
    use crate::view::info_of;
    use rr_ipc::Rr3BoardBindings;
    use serde_json::{json, Value};

    #[test]
    fn parse_axis_and_degree_are_fail_closed() {
        assert!(parse_axis(None).is_ok());
        assert!(parse_axis(Some(&json!("pan"))).is_ok());
        assert!(parse_axis(Some(&Value::Null)).is_ok());
        match parse_axis(Some(&json!("yaw"))) {
            Err(e) => assert_eq!(e, "axis 非法: \"yaw\"（pan|tilt）"),
            Ok(_) => panic!("未知轴必须拒"),
        }
        assert_eq!(parse_deg(Some("90")), Ok(90));
        assert!(parse_deg(None).is_err());
        assert!(parse_deg(Some("-1")).is_err());
        assert!(parse_deg(Some("x")).is_err());
    }

    /// 绑定对账：四键失配逐条计数，未注入不判、其余键不参与（Pi 不适用）。
    #[test]
    fn reconcile_counts_only_pi_bus_keys() {
        let mut b = Rr3BoardBindings::default();
        assert_eq!(reconcile_bindings(&b), 0, "未注入不判");
        b.i2c_sda = Some(BIND_I2C_SDA);
        b.i2c_scl = Some(BIND_I2C_SCL);
        b.uart2_tx = Some(BIND_UART2_TX);
        b.uart2_rx = Some(BIND_UART2_RX);
        assert_eq!(reconcile_bindings(&b), 0, "四键齐且值对");
        b.i2c_sda = Some(9);
        b.uart2_rx = Some(20);
        assert_eq!(reconcile_bindings(&b), 2, "两键失配逐条计数");
    }

    /// INFO 对无 ADS 的板子必须显式 rail=absent，不得把默认 0mV/Ok 冒领成有效采样。
    #[test]
    fn info_marks_absent_rail_explicitly() {
        let absent = info_of(&State::boot());
        assert!(absent.contains("rail=absent"), "{absent}");
        assert!(!absent.contains("rail=0mV"), "{absent}");

        let mut ok = State::boot();
        ok.rail_valid = true;
        ok.rail_mv = 5_240;
        ok.rail_raw = 20_960;
        let line = info_of(&ok);
        assert!(
            line.contains("rail=5240mV raw=20960 rail_tier=ok"),
            "{line}"
        );
    }
}
