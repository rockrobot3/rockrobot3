//! llm 原子件入口（五件套②，批N32 件四 Rust 拆分收敛，ADR-0016）：只声明
//! 公开面，机制在 mech.rs。占格 `cognition.llm`（atom.toml）。
//! brain 启动自述是组合件级逐字面（main.rs 自印 publish/subscribe/rpc——Python
//! composite.py 打印序列契约零改动），不走 atom SURFACE 链；装配 = src/main.rs
//! `#[path]` 原位挂载（rr3-sense 同形）。本件只装**纯机制**（对话历史窗口，⑧-b
//! 依赖闭包纪律：无 IO 无环境读取无 JSON）；模式档位类型 [`Rr3LlmMode`] 定义在
//! [`crate::boot`]（解析面），补全客户端与 messages 的 JSON 装配在组合件级
//! [`crate::cloud`]。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 公开面：对话历史窗口（system 恒守首位 + 最近 N 轮 + 失败回滚）。
/// 窗口轮数常量仅组合件测试面按名消费——bin 面导出会吃 unused_imports
/// （capture.rs 先例：测试面专用导出打 `#[cfg(test)]`）。
pub use mech::Rr3LlmHistory;
#[cfg(test)]
pub use mech::RR3_LLM_HISTORY_ROUNDS;
