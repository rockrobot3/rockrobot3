//! `--chain` 演练（0.0.6 批F4 sim 档，§5.2 判据 10）：六真进程全链 mock 闭环。
//!
//! rrd（boot spawn rr3-sense）+ 外部 rr3-brain + rr3-interact + rr3-act + 演练自带
//! 的**总线观察者**（rrd 连接五真进程 + 演练进程自己持一条 UDS 观察连接），无人工
//! 干预跑通"采集→意图→判向→转向→表情"一轮以上：
//!
//! - **采集**：sense 以双麦 config（channels=2，R14 双麦期）+ `RR3_HAL_SIM_WAVE=burst`
//!   起 sim 源（偶秒有声/奇秒静音，onset/offset 周期性成对），pcm 下混 mono 入总线；
//! - **判向**：`RR3_HAL_SIM_DOA_DEG=-60` 注入（与批F2 判向 e2e 同值），onset 携
//!   `doa{deg,bin}` 上 `rr3-sense/audio/vad_start`——观察者现场收帧取证；
//! - **意图**：brain 走 mock 云（`RR3_BRAIN_CLOUD=mock`）+ mock ASR 文本"向左看"
//!   （`RR3_BRAIN_MOCK_ASR_TEXT` 批F2 注入键），`对话轮完成` 行携 `actions=1`；
//! - **转向**：brain 词表把左档意图映射成 `rr3-act/servo/command`(70°)（策略层
//!   唯一归属，0.0.3 Q1），act 的 sim 舵机落角并发 `rr3-act/servo/state`——
//!   观察者同时取证落角；
//! - **表情**：TTS 播放期间 interact 表情机自动挂起 talk（批F3 语义，不依赖
//!   调用方自觉），`tts:` 日志行携「talk 挂起」留痕；sim 档无显示件。
//!
//! 观察者与 rrd/tests/support 的 `rr3_connect_observer` 同一机制（注册最小身份
//! 专收广播），但演练是脚本编排不依赖测试夹具——裸 UDS + rr3.register，订阅
//! vad_start/servo/state 两个取证主题。观察者在**外部模块起跑前**注册（先于
//! 意图动作），servo/state 是一次性广播，晚注册会漏收。
//!
//! 云端走 mock（R4 口径）；报告按 R9 真/mock 矩阵逐项标注交付档位——本演练是
//! **采购前档**，不充当真件收口证据。仅 Unix 参与编译（依赖 UDS 与真进程）。

mod mount;
mod plan;
mod report;
mod step;

pub(crate) use mount::*;
pub(crate) use plan::*;
pub(crate) use report::*;
pub(crate) use step::*;

#[cfg(test)]
mod tests;
