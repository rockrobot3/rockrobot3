//! 总线字节流窄缝（批J G1，ADR-0018 修订 1 ③ / 修订 3 ①，architecture §12.8 接缝二）。
//!
//! 机制核面的一条**两方法缝**：无错误类型、不引 `std::io`、不装箱——板侧要接总线
//! 从"实现两个 std trait"降成"写两个 `usize` 方法"。[`Rr3TransportConn`](crate::transport::Rr3TransportConn)
//! 的 blanket impl 在它上面换底（`transport/mod.rs`）；std 承载侧由同模块的单向
//! adapter（`impl<T: Read + Write> Rr3ByteStream for T`）接住，宿主三实现的行为与
//! 线上字节零改动（§15 缝清单："动 blanket impl 的下底、加一个 adapter"）。
//!
//! **错误折叠语义**（缝上没有错误通道的代价，两侧都如实写明）：
//! - 读：返回 0 = 对端关闭**或读失败**（std adapter 把 io 错误折叠为 0）——缝上
//!   只看得见"没了"，看不见"为什么没了"；
//! - 写：返回值 < `buf.len()` = 写失败（可能是部分写出）；blanket impl 据此报 `Io`。
//!
//! 本文件 `target_os = "none"` 可编译（core+alloc）；测试只在宿主跑（帧语义的
//! 形状证据在 `transport/mod.rs` 的板侧 conn 形状用例）。

use crate::protocol::wire::Rr3FrameError;

/// 总线字节流窄缝：阻塞读 + 整段写，两个 `usize` 方法。
///
/// - [`rr3_read_into`](Self::rr3_read_into)：阻塞至少读到 1 字节才返回 n≥1；
///   返回 0 = 对端关闭（或读失败折叠，见模块注记）。
/// - [`rr3_write_all_bytes`](Self::rr3_write_all_bytes)：把整段写出（实现内部
///   自己循环消化部分写），返回**实际写出字节数**；返回值 < `buf.len()` 即失败。
pub trait Rr3ByteStream {
    /// 阻塞读进 `buf`：读到 ≥1 字节返回 n；0 = 对端关闭。
    fn rr3_read_into(&mut self, buf: &mut [u8]) -> usize;
    /// 整段写出：实现内部循环消化部分写，返回实际写出字节数。
    fn rr3_write_all_bytes(&mut self, buf: &[u8]) -> usize;
}

/// 缝上的计数读满（帧缓冲 [`rr3_framebuf_read`](crate::transport::buf::rr3_framebuf_read)
/// 的唯一计数来源）：返回 `Ok(Some(已读字节数))`；`Ok(None)` = 第一个字节都没读到
/// （对端关闭）。[`crate::transport::frame`] 的 std::io 半边另有同形 `read_fill`
/// ——io 面与缝面**故意各一份**：io 面要保 os 错误文本上抛，缝面没有错误通道。
pub(crate) fn rr3_stream_read_fill<S: Rr3ByteStream>(
    s: &mut S,
    buf: &mut [u8],
) -> Result<Option<usize>, Rr3FrameError> {
    let mut done = 0usize;
    while done < buf.len() {
        let n = s.rr3_read_into(&mut buf[done..]);
        if n == 0 {
            return Ok(if done == 0 { None } else { Some(done) });
        }
        done += n;
    }
    Ok(Some(done))
}
