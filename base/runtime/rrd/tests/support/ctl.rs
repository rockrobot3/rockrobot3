// SPDX-License-Identifier: Apache-2.0
//! ctl 面集成助手：一问一答、夹具注册、应答后即断断言。
//!
//! 两个消费面（`e2e_ctl.rs` 的查询面与 `e2e_counters.rs` 的计数闭环）共用同一
//! 口径：ctl 连接不注册、src 按契约填 0、一问一答后连接即被收掉。

use super::*;

/// 发一帧 ctl 请求（src 按契约填 0，与注册帧同口径），收回第一条 Response 体
pub fn rr3_ask_ctl(c: &mut RrdClient, method: &str, id: u64) -> serde_json::Value {
    rr3_send(c, &rr3_rpc_frame(method, id, serde_json::json!({}), 0));
    let f = rr3_read_where(c, &format!("{method} 的应答"), |f| {
        rr3_body_split(&f.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    rr3_json_of(&f)
}

/// 夹具注册：sima 以旁观窗口上册，带回注册分到的 src_task。
/// 长窗口与 `rr3_register_passive` 同口径——本文件不喂狗，短窗口会在 1s 后回收连接。
pub fn register_sima_task(t: &RrdTest) -> (RrdClient, u64) {
    let mut a = t.rr3_connect();
    let reg = rr3_register_passive(&mut a, "rr3-sima");
    let task = reg["result"]["src_task"]
        .as_u64()
        .expect("注册应答带 src_task");
    (a, task)
}

/// ctl 一问一答即断：应答之后这条连接读不到更多帧（EOF / 读超时收尾）
pub fn rr3_assert_ctl_closed(c: &mut RrdClient) {
    let rest = rr3_drain_until_closed(c);
    assert!(
        rest.iter().all(|f| rr3_body_split(&f.payload)
            .map(|(ty, _)| ty)
            .unwrap_or(Rr3PayloadType::Event)
            != Rr3PayloadType::Response),
        "应答后不该再有 Response 帧，实际: {rest:?}"
    );
}
