// SPDX-License-Identifier: Apache-2.0
//! `--chain` 演练：六真进程全链 mock 闭环（rrd spawn sense + 外部 brain/interact/act
//! + 演练自带总线观察者），无人工干预跑通"采集→意图→判向→转向→表情"一轮以上。
//!
//! 五环：采集 = sense 双麦 config + `RR3_HAL_SIM_WAVE=burst`（偶秒有声/奇秒静音），
//! pcm 下混 mono 入总线；判向 = `RR3_HAL_SIM_DOA_DEG=-60` 注入，onset 携
//! `doa{deg,bin}` 上 `rr3-sense/audio/vad_start`；意图 = brain 走 mock 云 +
//! mock ASR 文本"向左看"（`RR3_BRAIN_MOCK_ASR_TEXT`）；转向 = brain 词表把左档映射成
//! `rr3-act/servo/command`(70°)，act 的 sim 舵机落角并发 `rr3-act/servo/state`；
//! 表情 = TTS 期间 interact 自动挂起 talk（`tts:` 行留痕），sim 档无显示件。
//!
//! 观察者同 rrd/tests/support 的 `rr3_connect_observer` 机制（裸 UDS + `rr3.register`，
//! 订阅 vad_start 与 servo/state），但**必须在外部模块起跑前注册**——servo/state 是
//! 一次性广播，晚注册即漏收。
//!
//! 云端走 mock；报告按 R9 真/mock 矩阵逐项标注档位——本演练是采购前档，不充当真件
//! 收口证据。仅 Unix 编译（依赖 UDS 与真进程）。

mod mount;
mod plan;
mod report;
mod steady;
mod step;

pub(crate) use mount::*;
pub(crate) use plan::*;
pub(crate) use report::*;
pub(crate) use steady::*;
pub(crate) use step::*;

#[cfg(test)]
mod tests;
