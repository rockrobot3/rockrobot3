//! Dispatcher trait（模块机制层注册面，0.0.8 方案 §2.3）：`serve` 骨架回调，
//! 模块机制层实现本 trait——trait 在骨架（机制），实现在各模块机制层
//! （机制/策略分离的注册面落点）。

use std::time::Instant;

use serde_json::Value;

use crate::BusHandle;

/// 模块机制层注册面。三入口对应三类输入/节拍，错误语义两档：**业务性失败折
/// error 应答**（[`Dispatcher::on_rpc`] 返 `Err`），**传输性/设备级失败上抛致命**
/// （`on_event` / `on_tick` 返 `Err` → `serve` 上抛 → main `exit(1)`）——与现行
/// 五模块逐字同款，本 trait 不发明新语义。
pub trait Dispatcher {
    /// rr3.call 点名应答（协议面）。Ok → result 应答体；Err → {"error": …} 折返。
    /// 需要发布状态流的处理（如 rr3-act servo 命令的限速 state 发布）在返回前
    /// 经 bus 完成——发布 → 应答的现行顺序不变。
    fn on_rpc(&mut self, topic: &str, args: &Value, bus: &BusHandle) -> Result<Value, String>;

    /// 广播事件（订阅面）。topic 本地过滤（不匹配直接返回 Ok）；发布副作用
    /// （如 rr3-act intent 执行后的 relay/state 摘要）经 bus 完成。
    fn on_event(&mut self, topic: &str, body: &Value, bus: &BusHandle) -> Result<(), String>;

    /// 帧无关节拍（每圈一次，poll_wait 分帧之后）：rr3-connect 链路拍 /
    /// rr3-power 双节拍 / rr3-sense 自 pacing 采集 / rr3-interact 面板动作排空。
    /// Err = 传输性/设备级（HAL 读数失败、发布写失败）→ 上抛致命。
    fn on_tick(&mut self, now: Instant, bus: &BusHandle) -> Result<(), String>;
}
