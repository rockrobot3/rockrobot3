//! cloud：云端对话客户端（ASR / LLM / TTS HTTP 流程）
//!
//! 迁移硬约束：本目录及子模块【不得出现 esp-idf 类型】——
//! HTTP 走抽象 trait（由 main/hal_esp 注入 EspHttpConnection 实现或将来的 libcurl 实现），
//! 对话数据结构（PCM/文本）用标准库类型。
//! Linux 迁移时本目录零改动平移（HTTP 实现换 libcurl）。

pub mod asr;
pub mod llm;
pub mod tts;
