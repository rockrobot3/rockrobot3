// SPDX-License-Identifier: Apache-2.0
//! conn 单测（文件头：本文件只放测试，生产面在 `conn.rs`）。
//!
//! 拆分动因：`conn.rs` 已 592 行，续轮二十五的写超时用例再内联会触 600 行门。

use super::*;

/// 写超时分类（续轮二十五）：对端不收 + 短写超时 → `WouldBlock`，计数必须 +1
/// （与"通道坏了"分开点名）。对端缓冲可能很大，故连续写若干帧直到失败，
/// 不用单帧大小赌缓冲上限。
///
/// 单帧仍须在**总线上界内**（批91 起写侧按 1MiB 帽先拒）：旧夹具用 4MiB 帧，
/// 那种帧在生产里根本发不出去，测的是"超帽被拒"而不是"对端不收"。这里改成
/// 64KiB 帧写满 socket 缓冲——真正走慢消费者那条路。
#[test]
fn write_counted() {
    let (peer, writer) = UnixStream::pair().expect("pair");
    writer
        .set_write_timeout(Some(Duration::from_millis(50)))
        .expect("set_write_timeout");
    let out = Arc::new(Mutex::new(writer));
    let frame = rr3_out_frame(Rr3PayloadType::Notification, 7, &vec![0u8; 64 << 10]);
    let before = rr3_wr_timeouts();
    let mut failed = false;
    for _ in 0..256 {
        if !rr3_write(&out, &frame) {
            failed = true;
            break;
        }
    }
    assert!(failed, "对端不收时连续写必须在写超时后失败");
    assert!(
        rr3_wr_timeouts() > before,
        "写超时必须单独入账（慢消费者点名的另一半）"
    );
    drop(peer);
}
