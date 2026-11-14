//! HAL 执行面三面选择与开机探活编排（舵机 / LED / 继电器）。

use rr3mod::BusHandle;
use rr_hal::backend::{
    rr3_audio_backend, rr3_backend_resolve, rr3_hal_gpio_out, rr3_hal_servo, rr3_servo_backend,
    RR3_BACKEND_ESP32, RR3_BACKEND_PLACEHOLDER,
};
use rr_hal_api::{Rr3GpioOut, Rr3HalError, Rr3PwmServo};

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
    let dev = super::remote::Rr3RemoteServo::rr3_remote_new(bus)?;
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
pub(super) fn rr3_shared_rule(servo: &str, audio: &str) -> bool {
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
    // 面选择摘要（可定位一行）：全局 hal 档 + 执行三面后端 + 音频面（桥归属判据）
    let audio = rr3_audio_backend().unwrap_or(RR3_BACKEND_PLACEHOLDER);
    eprintln!(
        "面选择: hal={} servo={servo_backend} led={led_backend} relay={relay_backend} audio={audio} bridge={}",
        rr3_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER),
        if rr3_bridge_shared(servo_backend) { "remote" } else { "local" }
    );
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
    let Err(e) = dev.angle() else {
        return Ok((dev, backend));
    };
    if matches!(&e, Rr3HalError::Busy) && backend == RR3_BACKEND_ESP32 {
        return rr3_remote_bridge(bus, backend);
    }
    Err(format!("舵机面探活失败({backend}): {e}"))
}

/// 可缺面探活：占位后端（`Unsupported`）是装配单"该格未启用"的诚实投影——点名
/// 后继续，使用该面时仍由占位逐次回错；其余错误（Busy/参数错）是真故障，致命。
pub(super) fn rr3_opt_face(
    face: &str,
    backend: &str,
    probe: Result<bool, Rr3HalError>,
) -> Result<(), String> {
    match probe {
        Ok(_) => Ok(()),
        Err(Rr3HalError::Unsupported(_)) => {
            eprintln!("{face} 面未启用（{backend}），该面命令按 Unsupported 回错，其余面继续");
            Ok(())
        }
        Err(e) => Err(format!("{face} 面探活失败({backend}): {e}")),
    }
}
