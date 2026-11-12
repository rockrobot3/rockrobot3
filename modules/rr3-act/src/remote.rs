//! 远端舵机（A9.2）：桥单所有者纪律下的 act 侧客户端。
//!
//! 组合档里 `/dev/ttyAMA0` 由 `rr3-sense` 独占（音频流 + 命令旁路）。本件把
//! `Rr3PwmServo` 语义映射到 `rr3-sense/servo/set` RPC：`set_angle` = 一次
//! `rr3.call`（sense 入有界旁路队列，板侧执行前回执），`angle` = 本进程最后
//! **已送达**的命令值（远端执行器是开环命令面，不回读硬件角；板侧真实姿位经
//! sense 的 ops/state 观测面查看——不虚构硬件读回）。
//!
//! 构造期探测 `rr3-sense/ops/list`（有界 2s 重试）：sense 未起或不应答即拒，
//! 不做"看起来就绪"的假执行器。失败文案带最后一次 RPC 错误，便于定位是总线、
//! rrd 转发还是 sense 侧拒绝。

use std::cell::Cell;
use std::time::Duration;

use rr3mod::BusHandle;
use rr_hal_api::{Rr3HalError, Rr3PwmServo, RR3_PERIPH_AXIS_DEFAULT};
use serde_json::{json, Map, Value};

/// sense 的舵机命令面（contracts/topics.toml 在册，A9.1 落地）
const RR3_SENSE_SERVO_TOPIC: &str = "rr3-sense/servo/set";
/// sense 的只读探活面（在册 RPC，构造期确认桥主在位）
const RR3_SENSE_OPS_TOPIC: &str = "rr3-sense/ops/list";
/// 单次 RPC 超时：命令面是反射环低频路径，500ms 足够覆盖 rrd 转发 + sense 入队
const RR3_REMOTE_TIMEOUT: Duration = Duration::from_millis(500);
/// 构造探测重试：模块启动序不保证 sense 先起，5s 窗口吸收正常启动抖动
/// （真板实测 sense 开桥到应答案接近 2s，20×100ms 只剩最后一次余量——
/// 这里放大到 50×100ms，避免宿主机负载高时 act 白白重启一轮）。
const RR3_REMOTE_TRIES: u32 = 50;
const RR3_REMOTE_BACKOFF_MS: u64 = 100;

/// 远端舵机：BusHandle clone 共享同一连接（客户端 reader 线程按 id 路由应答，
/// 回调内同步 call 不会自锁）。
pub(crate) struct Rr3RemoteServo {
    bus: BusHandle,
    axis: &'static str,
    last: Cell<u16>,
}

impl Rr3RemoteServo {
    /// 构造 + 探活。轴词取 `RR3_HAL_PERIPH_AXIS`（与 rr-hal 桥同键同词域）。
    pub(crate) fn rr3_remote_new(bus: &BusHandle) -> Result<Self, String> {
        let raw = std::env::var(rr_hal_api::RR3_PERIPH_AXIS_ENV).ok();
        let axis = rr3_remote_axis(raw.as_deref())?;
        rr3_remote_probe(bus, axis)?;
        Ok(Self {
            bus: bus.clone(),
            axis,
            last: Cell::new(90),
        })
    }
}

/// 构造期探活段：有界重试 sense 的 `ops/list`，成功即回。
fn rr3_remote_probe(bus: &BusHandle, axis: &str) -> Result<(), String> {
    match rr3_probe_retry(bus, axis) {
        None => Ok(()),
        Some(last_err) => Err(format!(
            "远端舵机面不可达（rr3-sense {RR3_REMOTE_TRIES} 次探测全败）: {last_err}"
        )),
    }
}

/// 有界重试段：成功回 None，全败回最后一次错误（文案归调用方组装）。
fn rr3_probe_retry(bus: &BusHandle, axis: &str) -> Option<String> {
    let mut last_err = String::new();
    for try_no in 1..=RR3_REMOTE_TRIES {
        match rr3_probe_once(bus, axis, try_no) {
            Ok(()) => return None,
            Err(e) => {
                last_err = e;
                std::thread::sleep(Duration::from_millis(RR3_REMOTE_BACKOFF_MS));
            }
        }
    }
    Some(last_err)
}

/// 单次探测：就绪即点名，失败原样回错（重试编排在调用方）。
fn rr3_probe_once(bus: &BusHandle, axis: &str, try_no: u32) -> Result<(), String> {
    bus.call(RR3_SENSE_OPS_TOPIC, &json!({}), RR3_REMOTE_TIMEOUT)
        .map(|_| {
            eprintln!("rr3-act: 远端舵机面就绪（axis={axis}，rr3-sense 持桥，第 {try_no} 次探测）");
        })
}

/// 轴词域：缺省 pan（契约常量），tilt 在册，其余 fail-closed（与 rr-hal 同域）。
fn rr3_remote_axis(v: Option<&str>) -> Result<&'static str, String> {
    match v {
        None | Some("") | Some("pan") => Ok(RR3_PERIPH_AXIS_DEFAULT),
        Some("tilt") => Ok("tilt"),
        Some(other) => Err(format!("远端舵机轴非法: {other}（只认 pan/tilt）")),
    }
}

impl Rr3PwmServo for Rr3RemoteServo {
    fn set_angle(&mut self, deg: u16) -> Result<(), Rr3HalError> {
        let mut args = Map::new();
        args.insert(self.axis.to_string(), json!(deg));
        match self.bus.call(
            RR3_SENSE_SERVO_TOPIC,
            &Value::Object(args),
            RR3_REMOTE_TIMEOUT,
        ) {
            Ok(_) => {
                self.last.set(deg);
                Ok(())
            }
            Err(e) => {
                eprintln!(
                    "rr3-act: 远端舵机命令失败（axis={} deg={deg}）: {e}",
                    self.axis
                );
                Err(Rr3HalError::Unsupported(
                    "rr3-sense 舵机命令未送达（详见 stderr）",
                ))
            }
        }
    }

    fn angle(&self) -> Result<u16, Rr3HalError> {
        Ok(self.last.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 轴词域：缺省/空 = pan，tilt 在册，域外 fail-closed。
    #[test]
    fn axis_vocab_gate() {
        assert_eq!(rr3_remote_axis(None).unwrap(), "pan");
        assert_eq!(rr3_remote_axis(Some("")).unwrap(), "pan");
        assert_eq!(rr3_remote_axis(Some("pan")).unwrap(), "pan");
        assert_eq!(rr3_remote_axis(Some("tilt")).unwrap(), "tilt");
        assert!(rr3_remote_axis(Some("yaw")).is_err());
    }
}
