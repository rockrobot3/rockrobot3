#![cfg(unix)]
// SPDX-License-Identifier: Apache-2.0
//! 注册面与**发布面**的线级用例（spec §9 的注册/重复/保留名/坏 manifest/src_task 各条，
//! 加 Task 8 的发布前缀强制与内建命名空间冒充拒绝）。
//! 发布面共 7 条：合法转发、越权前缀、冒充 rr3-runtime/*、在册未登记主题、
//! 叠加层不是生产放行口、Event 体缺 topic 不牵连控制面、转发帧按内容而非到达序读。

mod support;

#[path = "register_publish/handshake.rs"]
mod handshake;
#[path = "register_publish/publish.rs"]
mod publish;
#[path = "register_publish/register.rs"]
mod register;

use support::*;

/// 夹具注册（两处同款）：sima 以旁观窗口上册并消费注册回执。
/// 窗口走 `rr3_register_passive` 的长值——测试端不喂狗，服务端静默上界 = 2×watchdog，
/// 短窗口会让稍慢的用例在读帧前丢掉整条连接。
fn register_sima(c: &mut RrdClient) {
    let _ = rr3_register_passive(c, "rr3-sima");
}
