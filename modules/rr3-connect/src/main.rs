//! rr3-connect 骨架：连接（WiFi 链路 / 云端会话 / 蓝牙配网）。

mod bt;
mod session;
mod wifi;

fn main() {
    println!("rr3-connect 0.0.1 (骨架，未接入总线)");
    for line in wifi::SURFACE.iter().chain(session::SURFACE.iter()).chain(bt::SURFACE.iter()) {
        println!("  {line}");
    }
}
