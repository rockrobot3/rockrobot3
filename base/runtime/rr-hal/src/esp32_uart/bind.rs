// SPDX-License-Identifier: Apache-2.0
//! 板侧绑定输入面（0.0.5 批C 桥后端 + 批N4 件七(b) 基-24）：轴/波特率
//! env 取值裁决与描述符 → 板侧载荷的纯函数组。与帧面/连接策略分属两簇——
//! 本文件只做"文本 → 判决/载荷"，不碰 UART、不发帧。
//!
//! 真值单点纪律：轴词表在 rr-hal-api（`RR3_PERIPH_AXIS_ENV` 档），波特率档位在
//! rr-ipc（`rr3_uart_baud_speed`），板侧键名在 rr-ipc 的 `RR3_BOARD_BINDING_*`；
//! 本文件不抄第二份白名单。
//!
//! 拆分动因：`esp32_uart.rs` 487 行，绑定输入面 ~80 行可独立读判。

use rr_ipc::{
    rr3_uart_baud_speed, RR3_BOARD_BINDING_ADC_RAIL_PIN, RR3_BOARD_BINDING_CRED_STORE,
    RR3_BOARD_BINDING_I2C_SCL, RR3_BOARD_BINDING_I2C_SDA, RR3_BOARD_BINDING_I2S_BCLK,
    RR3_BOARD_BINDING_I2S_SD, RR3_BOARD_BINDING_I2S_WS, RR3_BOARD_BINDING_LEDC_PAN,
    RR3_BOARD_BINDING_LEDC_TILT, RR3_BOARD_BINDING_UART2_RX, RR3_BOARD_BINDING_UART2_TX,
};
use serde_json::{json, Map, Value};

use crate::descriptor::{rr3_bindings_parse, Rr3HalBindings, RR3_HAL_BINDINGS_ENV};

/// 轴 env 取值裁决：只认 `pan` / `tilt`（trim 后）。独立纯函数：裁决可单测。
pub fn rr3_axis_parse(s: &str) -> Option<&'static str> {
    ["pan", "tilt"].iter().copied().find(|a| *a == s.trim())
}

/// 波特率 env 取值裁决（批D②）：十进制解析后过 rr-ipc 档位表
/// （[`rr3_uart_baud_speed`]）——档位真值单点在 rr-ipc，本侧不抄第二份白名单。
pub fn rr3_baud_parse(s: &str) -> Option<u32> {
    let baud = s.trim().parse::<u32>().ok()?;
    rr3_uart_baud_speed(baud).ok().map(|_| baud)
}

/// 板侧绑定载荷（纯函数，批N4 件七(b) 基-24 桥上配置腿宿主半边）：描述符板侧
/// 键 `Some` 者逐键入载荷，值保持字符串（描述符同形纪律：注入文本值全字符串，JSON
/// 侧不换形）；全缺 = `None`（无板侧配置可注）。独立纯函数：键位/换形可单测。
pub(super) fn rr3_board_payload(b: &Rr3HalBindings) -> Option<Value> {
    let mut m = Map::new();
    rr3_board_pins(&mut m, b);
    rr3_cred_store_put(&mut m, b);
    if m.is_empty() {
        return None;
    }
    Some(Value::Object(m))
}

/// 凭证库键（描述符有值才注入，值保持字符串）。
pub(super) fn rr3_cred_store_put(m: &mut Map<String, Value>, b: &Rr3HalBindings) {
    if let Some(v) = &b.cred_store {
        m.insert(
            RR3_BOARD_BINDING_CRED_STORE.to_string(),
            json!(v.to_string()),
        );
    }
}

/// 十个板侧引脚键逐键入载荷：`Some` 者按十进制字符串写（描述符值全字符串同形）。
pub(super) fn rr3_board_pins(m: &mut Map<String, Value>, b: &Rr3HalBindings) {
    let pins: [(&str, Option<u8>); 10] = [
        (RR3_BOARD_BINDING_I2C_SDA, b.i2c_sda),
        (RR3_BOARD_BINDING_I2C_SCL, b.i2c_scl),
        (RR3_BOARD_BINDING_ADC_RAIL_PIN, b.adc_rail_pin),
        (RR3_BOARD_BINDING_UART2_TX, b.uart2_tx),
        (RR3_BOARD_BINDING_UART2_RX, b.uart2_rx),
        (RR3_BOARD_BINDING_LEDC_PAN, b.ledc_pan),
        (RR3_BOARD_BINDING_LEDC_TILT, b.ledc_tilt),
        (RR3_BOARD_BINDING_I2S_BCLK, b.i2s_bclk),
        (RR3_BOARD_BINDING_I2S_SD, b.i2s_sd),
        (RR3_BOARD_BINDING_I2S_WS, b.i2s_ws),
    ];
    for (key, v) in pins {
        let Some(v) = v else {
            continue;
        };
        m.insert(key.to_string(), json!(v.to_string()));
    }
}

/// 板侧绑定载荷取环境（[`RR3_HAL_BINDINGS_ENV`] → 同形解析 → 载荷）。返回 `None`
/// 的三种形态都**不构成错误**——绑定只服务板侧自愈路径，执法点在板侧消费面
/// fail-closed（未注入即拒并点名），桥上腿的职责是"尽力送达、失败可见"：
/// - env 未设 = 无描述符形态（纯 sim/`--manifest` 调试后门）；
/// - 文本解析失败 = 配置坏，eprintln 点名后跳过（fail-visible；后端选择侧已
///   fail-closed，此处兜底点名不静默放行坏值）；
/// - 三键全缺 = 无板侧配置可注（host 档描述符本就无板侧键）。
pub(super) fn rr3_env_bindings() -> Option<Value> {
    let text = std::env::var(RR3_HAL_BINDINGS_ENV).ok()?;
    let b = rr3_bindings_of(&text)?;
    rr3_board_payload(&b)
}

/// 描述符文本解析：失败点名后跳过（fail-visible，不静默放行坏值）。
pub(super) fn rr3_bindings_of(text: &str) -> Option<Rr3HalBindings> {
    rr3_bindings_parse(text)
        .map_err(|errs| {
            eprintln!(
                "rr-hal-esp32: {} 解析失败，跳过板侧绑定推送: {}",
                RR3_HAL_BINDINGS_ENV,
                errs.join("; ")
            );
        })
        .ok()
}
