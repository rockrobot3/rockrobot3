// SPDX-License-Identifier: Apache-2.0
//! 板侧启动失败诊断：失败上下文（引脚/频率/栈）的单点构造，返回可打印的
//! 错误串；不决定控制流。统一收口在 `boot.rs` 的最外层——先打印，再显式停机。

use super::{
    I2C_BAUD_HZ, LEDC_FREQ_HZ, LEDC_RES_BITS, RR3_BIND_LEDC_PAN, RR3_BIND_LEDC_TILT,
    RR3_BIND_UART2_RX, RR3_BIND_UART2_TX, THREAD_STACK, UART2_BAUD_HZ,
};

pub(super) fn uart2_fail<E: std::fmt::Debug>(e: E) -> String {
    format!(
        "UART2 驱动安装失败 GPIO{RR3_BIND_UART2_TX}/GPIO{RR3_BIND_UART2_RX} @{UART2_BAUD_HZ}: {e:?}"
    )
}

pub(super) fn i2c0_fail<E: std::fmt::Debug>(e: E) -> String {
    format!("I2C0 驱动安装失败 @{I2C_BAUD_HZ}Hz: {e:?}（查 SDA/SCL 引脚与上拉）")
}

pub(super) fn ledc_timer_fail<E: std::fmt::Debug>(e: E) -> String {
    format!("LEDC timer0 配置失败 {LEDC_FREQ_HZ}Hz/{LEDC_RES_BITS:?}: {e:?}")
}

pub(super) fn ledc_pan_fail<E: std::fmt::Debug>(e: E) -> String {
    format!("LEDC channel0(PAN) 安装失败 GPIO{RR3_BIND_LEDC_PAN}: {e:?}")
}

pub(super) fn ledc_tilt_fail<E: std::fmt::Debug>(e: E) -> String {
    format!("LEDC channel1(TILT) 安装失败 GPIO{RR3_BIND_LEDC_TILT}: {e:?}")
}

pub(super) fn rt_thread_fail<E: std::fmt::Debug>(e: E) -> String {
    format!("起运行面线程失败（stack={THREAD_STACK}）: {e:?}")
}

pub(super) fn ctl_thread_fail<E: std::fmt::Debug>(e: E) -> String {
    format!("起控制面线程失败（stack={THREAD_STACK}）: {e:?}")
}
