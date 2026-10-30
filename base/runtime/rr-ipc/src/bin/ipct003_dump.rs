//! IPC-T-003 跨架构对拍夹具（x86 与 aarch64 字节布局一致，ADR-0006 后果项）。
//!
//! 打印七行 `<name> <hex 小写>`：
//! - `header`：与 [`contracts/golden/event-header-128.hex`](../../../contracts/golden/event-header-128.hex)
//!   同一条头的 128 字节（opcode=SEND / trace=0x12345678 / ts=100 / src=7 / dst=0 / payload_len=96）；
//! - `request`/`response`/`event`/`stream`/`notification`：五种 payload 类型各一**整帧**
//!   （128B 头 + 2 字节类型前缀 + 定长定值体），头字段同 golden 但 payload_len 取真实值；
//! - 末行 `version <contracts schema version>`：把 `RR3_CONTRACT_SCHEMA_VERSION` 带进对拍，
//!   防两架构各编了一个旧版契约还互相对上了。
//!
//! 全部字段是编译期字面量：无时钟、无随机、无环境依赖，同源码在任何架构上必须逐字节同值——
//! 这正是 IPC-T-003 要证明的命题。对拍入口是 `tools/ci/ipct003.sh`（不进七步门，
//! 登记在 conventions.md §2），它把宿主输出、linux/arm64 容器输出与本 golden 三方 diff。

use rr_ipc::{
    opcode, rr3_body_encode, rr3_header_bytes_with_crc, Rr3Header, Rr3PayloadType,
    RR3_CONTRACT_SCHEMA_VERSION,
};

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 整帧字节：128B 头（payload_len 按真实值 + 载荷 CRC32）+ 2 字节前缀 + 体
fn frame_bytes(ty: Rr3PayloadType, body: &[u8]) -> Vec<u8> {
    let payload = rr3_body_encode(ty, body);
    let hdr = Rr3Header {
        opcode: opcode::SEND,
        flags: 0,
        trace_id: 0x1234_5678,
        timestamp_ns: 100,
        src_task: 7,
        dst_task: 0,
        payload_len: payload.len() as u32,
        payload_crc32: 0, // 由 rr3_header_bytes_with_crc 按载荷算出覆盖
    };
    let mut out = rr3_header_bytes_with_crc(&hdr, &payload).to_vec();
    out.extend_from_slice(&payload);
    out
}

fn main() {
    // 与 golden 同一条头（payload_len=96，不带 payload）——header 行可直接对 golden 文件 diff
    let golden_hdr = Rr3Header {
        opcode: opcode::SEND,
        flags: 0,
        trace_id: 0x1234_5678,
        timestamp_ns: 100,
        src_task: 7,
        dst_task: 0,
        payload_len: 96,
        payload_crc32: 0, // golden 行是无载荷纯头（CRC 覆盖载荷，故为 0）
    };
    println!("header {}", hex(&golden_hdr.to_bytes()));

    // 五种 payload 类型各一帧，体为定长定值字节（字面量，无任何运行期来源）
    println!(
        "request {}",
        hex(&frame_bytes(
            Rr3PayloadType::Request,
            b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}"
        ))
    );
    println!(
        "response {}",
        hex(&frame_bytes(
            Rr3PayloadType::Response,
            b"{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":\"pong\"}"
        ))
    );
    println!(
        "event {}",
        hex(&frame_bytes(
            Rr3PayloadType::Event,
            b"{\"topic\":\"rr3-sim/ping\",\"n\":1}"
        ))
    );
    println!(
        "stream {}",
        hex(&frame_bytes(
            Rr3PayloadType::Stream,
            &[0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
        ))
    );
    println!(
        "notification {}",
        hex(&frame_bytes(
            Rr3PayloadType::Notification,
            b"{\"topic\":\"rr3-sim/beat\",\"seq\":1}"
        ))
    );

    println!("version {RR3_CONTRACT_SCHEMA_VERSION}");
}
