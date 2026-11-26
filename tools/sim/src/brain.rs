// SPDX-License-Identifier: Apache-2.0
//! `--brain` 演练（M4，ADR-0014 裁定 4）：rrd + rr3-sense(sim) + rr3-brain + rr3-interact
//! 四真进程全语音对话闭环。
//!
//! 与既有两面演练（main.rs `run` 模块）同构：夹具（临时目录 + 套接字 + 三份 wire
//! manifest）→ rrd boot spawn rr3-sense（sim 正弦源，必过 VAD 起音阈值）→ 外部起
//! rr3-interact 与 rr3-brain（`target/debug` 同名二进制，与其余模块同形）→ 有界等待
//! brain 日志出现"对话轮完成"证据 → `rr3-ctl status` 断言三模块 running 且
//! `denied_publish = 0` → 报告 + 收尾。
//!
//! 云端缺省走 mock（`RR3_BRAIN_CLOUD=mock`）：brain 的 ASR/LLM/TTS 全部确定性应答——
//! 闭环里只有云端 socket 一跳是假的，其余（总线、rrd 路由、rr3.call、扇出）全真。
//! 真云档（`--brain-cloud real --brain-base-url <端点>` + `RR3_CLOUD_API_KEY`）把最后
//! 一跳也接真——端点 fail-closed 必给（演练 config 缺省是死地址，不落）。音频后端缺省
//! sim，`--audio-backend mac` 换 macOS 真麦/扬声器；`--sink-backend` 分列 interact 回放面
//! （`--audio-backend esp32` 必给——flock 排他下 interact 不得吃源面 UART）。仅 Unix 编译。

mod mock;
mod proto;
mod state;

pub(crate) use mock::*;
pub(crate) use proto::*;
pub(crate) use state::*;

#[cfg(test)]
mod tests;
