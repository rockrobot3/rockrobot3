//! 进程内动作族：动作递主循环并等回执（beep/face 串行代执行）。

use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use serde_json::Value;

use super::super::server::{Rr3PanelAction, Rr3PanelShared};

/// 进程内动作执行：递主循环、等应答（动作在主循环串行处理，与 TTS 点名互斥）。
/// 主循环回的是业务 Result——Err（不认的表情等）原样上抛给 execute 折 502 语义。
pub(super) fn rr3_panel_act(
    shared: &Rr3PanelShared,
    action: Rr3PanelAction,
) -> Result<Value, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    shared
        .actions
        .send((action, tx))
        .map_err(|_| "主循环不在了（进程正在退出）".to_string())?;
    rr3_act_wait(rx, shared.limits.call_timeout)
}

/// 等主循环回执：超时/通道断各点名（业务性失败，不致命）。
fn rr3_act_wait(
    rx: std::sync::mpsc::Receiver<Result<Value, String>>,
    timeout: Duration,
) -> Result<Value, String> {
    match rx.recv_timeout(timeout) {
        Ok(v) => v,
        Err(RecvTimeoutError::Timeout) => Err("主循环处理动作超时".to_string()),
        Err(RecvTimeoutError::Disconnected) => Err("应答通道已断（主循环退出）".to_string()),
    }
}
