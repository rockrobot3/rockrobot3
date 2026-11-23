// SPDX-License-Identifier: Apache-2.0
//! runtime 单测（文件头：本文件只放测试，生产面在 `runtime.rs`）。
//!
//! 拆分动因：`runtime.rs` 489 行、内联测试自 278 行起占近半，先按仓内同形搬出。

use super::*;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

/// 构造即 Registering：无心跳、无拍点、net.online 为假
#[test]
fn new_starts_registering_with_no_heartbeat() {
    let rt = Rr3ModuleRuntime::rr3_runtime_new(50);
    assert_eq!(rt.rr3_link_state(), Rr3LinkState::Registering);
    assert!(!rt.rr3_link_online());
    assert!(!rt.rr3_heartbeat_due(1_000));
    assert_eq!(rt.rr3_beat_wait_ms(1_000), None);
    assert_eq!(rt.rr3_heartbeat_ms(), 50);
}

/// 心跳间隔保底 ≥1ms（std 驱动器 `hb_ms.max(1)` 同一条规则进机制核）
#[test]
fn heartbeat_interval_floors_at_one_ms() {
    assert_eq!(Rr3ModuleRuntime::rr3_runtime_new(0).rr3_heartbeat_ms(), 1);
    assert_eq!(Rr3ModuleRuntime::rr3_runtime_new(50).rr3_heartbeat_ms(), 50);
}

/// 注册转移：Online + 首拍 = 注册时刻 + 间隔（拍点序列与 std 心跳线程逐拍一致）
#[test]
fn registered_schedules_first_beat_one_interval_out() {
    let mut rt = Rr3ModuleRuntime::rr3_runtime_new(50);
    rt.rr3_mark_registered(7, 1_000);
    assert_eq!(rt.rr3_link_state(), Rr3LinkState::Online { task: 7 });
    assert!(rt.rr3_link_online());
    assert!(!rt.rr3_heartbeat_due(1_049), "到期前一毫秒不算到期");
    assert!(rt.rr3_heartbeat_due(1_050), "拍点整点到期");
    assert_eq!(rt.rr3_beat_wait_ms(1_000), Some(50));
    assert_eq!(rt.rr3_beat_wait_ms(1_030), Some(20));
    assert_eq!(rt.rr3_beat_wait_ms(1_050), Some(0));
}

/// 相对节拍：下一拍 = 本拍发出时刻 + 间隔（drift 语义，与 std 行为一致；
/// 迟到的一拍不追拍）
#[test]
fn heartbeat_sent_advances_relative_beat() {
    let mut rt = Rr3ModuleRuntime::rr3_runtime_new(50);
    rt.rr3_mark_registered(7, 1_000);
    rt.rr3_heartbeat_sent(1_060); // 迟到 10ms 才发出
    assert!(!rt.rr3_heartbeat_due(1_060));
    assert!(rt.rr3_heartbeat_due(1_110));
    rt.rr3_heartbeat_sent(1_110);
    assert!(rt.rr3_heartbeat_due(1_160));
}

/// 断开转移（幂等）：Down 后无节拍、net.online 翻假、sent 是无操作
#[test]
fn mark_down_is_terminal_and_idempotent() {
    let mut rt = Rr3ModuleRuntime::rr3_runtime_new(50);
    rt.rr3_mark_registered(7, 1_000);
    rt.rr3_mark_down();
    rt.rr3_mark_down();
    assert_eq!(rt.rr3_link_state(), Rr3LinkState::Down);
    assert!(!rt.rr3_link_online());
    assert!(!rt.rr3_heartbeat_due(9_999));
    assert_eq!(rt.rr3_beat_wait_ms(9_999), None);
    rt.rr3_heartbeat_sent(9_999); // 无操作：断开后不得再排拍
    assert_eq!(rt.rr3_beat_wait_ms(9_999), None);
}

/// 逐拍一致性：注入时钟推进，拍点序列 = 注册点 + k·间隔（std 心跳线程的
/// sleep 间隔序列的机制核镜像）
#[test]
fn beat_train_is_periodic_under_injected_clock() {
    let mut rt = Rr3ModuleRuntime::rr3_runtime_new(20);
    rt.rr3_mark_registered(7, 0);
    let mut fired = Vec::new();
    let mut now = 0u64;
    while now < 100 {
        if rt.rr3_heartbeat_due(now) {
            fired.push(now);
            rt.rr3_heartbeat_sent(now);
        }
        now += 1;
    }
    assert_eq!(fired, vec![20, 40, 60, 80], "拍点必须严格等距");
}

/// 分发分类表：Response → 等待者路由；其余四类 → inbox
#[test]
fn dispatch_class_routes_only_response_to_pending() {
    assert_eq!(
        rr3_dispatch_class(Rr3PayloadType::Response),
        Rr3Dispatch::Pending
    );
    assert_eq!(
        rr3_dispatch_class(Rr3PayloadType::Request),
        Rr3Dispatch::Inbox
    );
    assert_eq!(
        rr3_dispatch_class(Rr3PayloadType::Event),
        Rr3Dispatch::Inbox
    );
    assert_eq!(
        rr3_dispatch_class(Rr3PayloadType::Stream),
        Rr3Dispatch::Inbox
    );
    assert_eq!(
        rr3_dispatch_class(Rr3PayloadType::Notification),
        Rr3Dispatch::Inbox
    );
}

// -- 守护行对拍（0.0.5 批K H6：起动态 + 喂拍 + 安全核触发/拒动，§4.0.3 两腿）--

/// 腿一（确定性）：同一事件序列两次格式化，行序列逐字节相同——
/// 同一输入在 Linux 实时环与裸机定时器上产出同一输出序列（§12.5 逐拍一致）。
#[test]
fn guard_line_train_is_deterministic() {
    let events = [
        Rr3GuardEvent::Boot(Rr3LinkState::Registering),
        Rr3GuardEvent::Boot(Rr3LinkState::Online { task: 7 }),
        Rr3GuardEvent::Feed { seq: 1 },
        Rr3GuardEvent::Safety {
            trip: true,
            reason: Rr3GuardReason::TravelClamp,
        },
        Rr3GuardEvent::Boot(Rr3LinkState::Down),
    ];
    let mut a = Rr3GuardBuf::rr3_guard_buf_new();
    let mut b = Rr3GuardBuf::rr3_guard_buf_new();
    let mut train_a = alloc::vec::Vec::new();
    let mut train_b = alloc::vec::Vec::new();
    for ev in &events {
        train_a.push(a.rr3_guard_format(ev).to_string());
        train_b.push(b.rr3_guard_format(ev).to_string());
    }
    assert_eq!(train_a, train_b, "同一事件序列必须产出同一行序列");
    // 覆盖式写入：缓冲复用不残留上一行
    assert_eq!(
        a.rr3_guard_format(&Rr3GuardEvent::Boot(Rr3LinkState::Down)),
        "RR3G boot down"
    );
}

/// 腿二（契约）：每类事件的行形状与 [`Rr3GuardEvent`] 文法逐字节一致
/// （期望值来自注释钉死的文法，不来自实现）。
#[test]
fn guard_line_matches_contract_grammar() {
    let mut g = Rr3GuardBuf::rr3_guard_buf_new();
    assert_eq!(
        g.rr3_guard_format(&Rr3GuardEvent::Boot(Rr3LinkState::Registering)),
        "RR3G boot registering"
    );
    assert_eq!(
        g.rr3_guard_format(&Rr3GuardEvent::Boot(Rr3LinkState::Online { task: 7 })),
        "RR3G boot online task=7"
    );
    assert_eq!(
        g.rr3_guard_format(&Rr3GuardEvent::Boot(Rr3LinkState::Down)),
        "RR3G boot down"
    );
    assert_eq!(
        g.rr3_guard_format(&Rr3GuardEvent::Feed { seq: 12 }),
        "RR3G feed seq=12"
    );
    // 安全核触发（trip）× 封闭原因码全表
    assert_eq!(
        g.rr3_guard_format(&Rr3GuardEvent::Safety {
            trip: true,
            reason: Rr3GuardReason::TravelClamp
        }),
        "RR3G safe trip travel-clamp"
    );
    assert_eq!(
        g.rr3_guard_format(&Rr3GuardEvent::Safety {
            trip: true,
            reason: Rr3GuardReason::LowEdge
        }),
        "RR3G safe trip low-edge"
    );
    assert_eq!(
        g.rr3_guard_format(&Rr3GuardEvent::Safety {
            trip: true,
            reason: Rr3GuardReason::Saturation
        }),
        "RR3G safe trip saturation"
    );
    // 拒动（deny）：限速流拒发
    assert_eq!(
        g.rr3_guard_format(&Rr3GuardEvent::Safety {
            trip: false,
            reason: Rr3GuardReason::RateDenied
        }),
        "RR3G safe deny rate-denied"
    );
}

/// 容量上界：u64::MAX 的最长行（42 字节）完整落行不截断；空缓冲读出空串
/// （RR3_GUARD_LINE_CAP = 48 的定容依据，改文法先改此数与注释）。
#[test]
fn guard_line_fits_worst_case_task_number() {
    let mut g = Rr3GuardBuf::rr3_guard_buf_new();
    assert_eq!(g.rr3_guard_as_str(), "");
    let worst = g.rr3_guard_format(&Rr3GuardEvent::Boot(Rr3LinkState::Online {
        task: u64::MAX,
    }));
    assert_eq!(
        worst, "RR3G boot online task=18446744073709551615",
        "最长行必须完整落行（22 前缀 + 20 位数字 = 42 ≤ 48）"
    );
    assert_eq!(worst.len(), 42);
    assert_eq!(RR3_GUARD_LINE_CAP, 48);
}
