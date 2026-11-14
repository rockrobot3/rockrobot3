//! 集成测试助手：起**真 rrd 子进程** + 裸 UDS 客户端，断言线级行为。
//!
//! M2 Task 9 起它还管**第二个真进程**：`SimPeer`（跑 `rrd` 包的 `sim_peer` 二进制）与
//! `rr3_fixture_manifest`（读 `base/runtime/tests/fixtures/<dir>/module.toml`，把 entry 占位符
//! 换成构建产物的绝对路径后落到 pid 唯一的 /tmp，返回 `SimManifest` 守卫）。逐行收集与等待
//! 抽成 `rr3_spawn_line_reader` / `rr3_wait_line` 两个自由函数，rrd 与 sim-peer 共用一份口径。
//!
//! 只在 Unix 编译（每个测试文件顶层 `#![cfg(unix)]`），Windows 侧这个模块整体不参与编译，
//! 所以 `cargo check --all-targets` 在 Windows 上是绿的——那不是证据，是盲区。
//!
//! 顺序约定（F1 之后是**真的**与顺序无关，不再只是注释）：应答由 conn 线程写、广播由这条连接的
//! writer 线程写，同一 socket 上**只保证整帧原子，不保证相对顺序**（Task 7 的并发形状，brief 明令；
//! 入队次序 ≠ 上线次序，见 `rrd/src/conn.rs` 的同一处注释）。
//! 所以本夹具按连接维护一条 `VecDeque<Rr3Frame>` 待处理队列：`rr3_read_where` 把谓词不成立的帧
//! **存回队列**而不是从 socket 上丢掉，"先按类型读应答、再按字段读 state"这类两阶段读因此与到达
//! 次序无关。读不到帧时给的是"没等到 X；队列里已积压 N 帧"的诊断，不是一句裸 io panic。
//! 没有"读第 N 帧"的助手——想这么写说明测试在赌时序。

#![allow(dead_code)] // 各测试文件按需取用，没用到不算死码

mod client;
mod fixture;
mod frames;
mod inst;

pub use client::*;
pub use fixture::*;
pub use frames::*;
pub use inst::*;
