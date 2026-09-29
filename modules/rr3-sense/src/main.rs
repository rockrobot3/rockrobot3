//! rr3-sense 骨架：感知（麦克风阵列 / 环境传感器 / IMU）。
//!
//! v0.0.1 只搭结构：总线接入随 M2（rrd + UDS），采集实现随 M3（rr-hal linux 后端）。

mod audio;
mod env;
mod imu;

fn main() {
    println!("rr3-sense 0.0.1 (骨架，未接入总线)");
    for line in audio::surface().chain(env::SURFACE.iter().copied()).chain(imu::SURFACE.iter().copied()) {
        println!("  {line}");
    }
}
