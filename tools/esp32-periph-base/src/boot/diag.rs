//! 板侧启动诊断：失败路径的上下文打印（引脚/频率/栈），
//! 供 `boot.rs` 的 `map_err` 单点复用；不决定控制流。

use super::{
    I2C_BAUD_HZ, LEDC_FREQ_HZ, LEDC_RES_BITS, RR3_BIND_LEDC_PAN, RR3_BIND_LEDC_TILT,
    RR3_BIND_UART2_RX, RR3_BIND_UART2_TX, THREAD_STACK, UART2_BAUD_HZ,
};

pub(super) fn uart2_fail<E: std::fmt::Debug>(e: E) -> E {
    eprintln!(
        "[periph] UART2 驱动安装失败 GPIO{RR3_BIND_UART2_TX}/GPIO{RR3_BIND_UART2_RX} @{UART2_BAUD_HZ}: {e:?}"
    );
    e
}

pub(super) fn i2c0_fail<E: std::fmt::Debug>(e: E) -> E {
    eprintln!("[periph] I2C0 驱动安装失败 @{I2C_BAUD_HZ}Hz: {e:?}（查 SDA/SCL 引脚与上拉）");
    e
}

pub(super) fn ledc_timer_fail<E: std::fmt::Debug>(e: E) -> E {
    eprintln!("[periph] LEDC timer0 配置失败 {LEDC_FREQ_HZ}Hz/{LEDC_RES_BITS:?}: {e:?}");
    e
}

pub(super) fn ledc_pan_fail<E: std::fmt::Debug>(e: E) -> E {
    eprintln!("[periph] LEDC channel0(PAN) 安装失败 GPIO{RR3_BIND_LEDC_PAN}: {e:?}");
    e
}

pub(super) fn ledc_tilt_fail<E: std::fmt::Debug>(e: E) -> E {
    eprintln!("[periph] LEDC channel1(TILT) 安装失败 GPIO{RR3_BIND_LEDC_TILT}: {e:?}");
    e
}

pub(super) fn rt_thread_fail<E: std::fmt::Debug>(e: E) -> E {
    eprintln!("[periph] 起运行面线程失败（stack={THREAD_STACK}）: {e:?}");
    e
}

pub(super) fn ctl_thread_fail<E: std::fmt::Debug>(e: E) -> E {
    eprintln!("[periph] 起控制面线程失败（stack={THREAD_STACK}）: {e:?}");
    e
}
