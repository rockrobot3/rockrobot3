#![cfg(unix)]
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

/// 夹具注册（两处同款）：sima 以 always/100/500 上册并消费注册回执
fn register_sima(c: &mut RrdClient) {
    rr3_send(
        c,
        &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"),
    );
    rr3_read_rpc(c);
}
