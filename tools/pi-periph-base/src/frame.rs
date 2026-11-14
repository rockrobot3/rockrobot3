//! 帧面请求分发与绑定注入对账。

use rr_ipc::{rr3_board_bind_entry, Rr3BoardBindings, RR3_METHOD_SET_BINDINGS};
use serde_json::{json, Value};

use crate::app::Board;
use crate::board::*;
use crate::topics_gen::{RR3_FACE_TOPIC, RR3_SERVO_TOPIC};

impl Board {
    pub(crate) fn dispatch(&mut self, req: &periph_mech::frame::FrameReq) -> Result<Value, String> {
        if req.method == RR3_METHOD_SET_BINDINGS {
            return self.handle_set_bindings(&req.args);
        }
        if req.method != "rr3.call" {
            return Err(format!("method 非法: \"{}\"（恒 rr3.call）", req.method));
        }
        match req.topic.as_str() {
            RR3_SERVO_TOPIC => self.handle_servo(&req.args),
            RR3_FACE_TOPIC => self.handle_face(&req.args),
            other => Err(format!("未知主题: \"{other}\"")),
        }
    }

    /// 绑定注入（控制面）：键域/值域走 rr-ipc 单点；板侧逐键对账只计数不拒收。
    fn handle_set_bindings(&mut self, args: &Value) -> Result<Value, String> {
        let obj = args
            .as_object()
            .ok_or_else(|| format!("args 非法: {args}（须对象）"))?;
        let mut nb = Rr3BoardBindings::default();
        for (k, v) in obj {
            let s = v
                .as_str()
                .ok_or_else(|| format!("{k} 值非法: {v}（须字符串）"))?;
            rr3_board_bind_entry(&mut nb, k, s)?;
        }
        println!(
            "[periph] 绑定注入 ✓ i2c_sda={} i2c_scl={} uart2_tx={} uart2_rx={}",
            nb.i2c_sda.map_or("-".into(), |v| v.to_string()),
            nb.i2c_scl.map_or("-".into(), |v| v.to_string()),
            nb.uart2_tx.map_or("-".into(), |v| v.to_string()),
            nb.uart2_rx.map_or("-".into(), |v| v.to_string()),
        );
        let bad = reconcile_bindings(&nb);
        self.state.binding_mismatch = self.state.binding_mismatch.saturating_add(bad);
        Ok(json!({ "applied": true, "mismatch": bad }))
    }
}

/// 绑定对账：Pi 板只消费四个总线/桥键（其余键在 Pi 上不适用，不判等）。
pub(crate) fn reconcile_bindings(b: &Rr3BoardBindings) -> u32 {
    let mut bad = 0u32;
    for (key, got, want) in [
        ("i2c_sda", b.i2c_sda, BIND_I2C_SDA),
        ("i2c_scl", b.i2c_scl, BIND_I2C_SCL),
        ("uart2_tx", b.uart2_tx, BIND_UART2_TX),
        ("uart2_rx", b.uart2_rx, BIND_UART2_RX),
    ] {
        if let Some(v) = got {
            if v != want {
                eprintln!("[periph] 绑定对账 ✗ {key}: 注入 {v} ≠ 装配 {want}");
                bad = bad.saturating_add(1);
            }
        }
    }
    bad
}
