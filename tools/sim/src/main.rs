// SPDX-License-Identifier: Apache-2.0
//! rr3-sim —— x86 全链路联调入口。
//!
//! 规划（architecture.md §3）：mock HAL + 假模块，让整条链路在无板设备上跑起来。
//! 本入口**脚本化**走一遍真实链路（全部真进程、真套接字、真契约）：
//!
//! 1. 起 rrd，`--manifest` 带 **rr3-interact**（boot spawn，resident 常驻喂狗）；
//! 2. 等 rr3-interact 到 `running`（轮询 `rr3-ctl status`，一次性 ctl 连接）；
//! 3. **外部**起第二个真模块 **rr3-sense**（sim_peer + `publish_once`），
//!    发布 `rr3-sense/audio/vad_start` 后退场——事件真的流过一次总线；
//! 4. `rr3-ctl` 四命令巡检：status（断言 `denied_publish = 0`）、health、logs、
//!    ota（恒拒随 M6，预期非 0 退出）；
//! 5. 报告 + 收尾（`--keep` 保留现场）。
//!
//! 另有三个互斥模式：`--brain`（全语音对话闭环，`brain.rs`）、`--profile`
//! （装配单驱动 boot + 全模块真进程巡检，`assemble.rs`）与 `--chain`（六真进程
//! 全链 mock 闭环 + 总线观察者取证，`chain.rs`）。
//!
//! sim 设备源（`devices.rs`，rr-hal 的 sim 后端消费方）保留：报告里带一段
//! 静音流的时长换算，证明"sim 与 linux 后端交出同一种流"这条不变量仍有落点。

#[cfg(not(unix))]
fn main() {
    eprintln!("rr3-sim: 全链路演练依赖 UDS，仅 Unix 可用");
    std::process::exit(2);
}

// 演练编排依赖 UDS 与真进程（Unix）；drill 是纯逻辑（参数/路径/解析），全平台参与编译，
// 它的单测因此 Windows 上也能跑到（④ 步的编译面与 ④ 之外的本地测试都受益）。
#[cfg(unix)]
mod assemble;
#[cfg(unix)]
mod brain;
#[cfg(unix)]
mod chain;
#[cfg(unix)]
mod devices;
mod drill;

#[cfg(unix)]
mod run;

#[cfg(unix)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run::rr3_sim_main(&args));
}

#[cfg(test)]
mod tests;
