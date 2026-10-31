//! asr 原子件入口（五件套②，批N32 件四 Rust 拆分收敛，ADR-0016）：只声明
//! 公开面，机制在 mech.rs。占格 `cognition.asr`（atom.toml）。
//! brain 启动自述是组合件级逐字面（main.rs 自印 publish/subscribe/rpc——Python
//! composite.py 打印序列契约零改动），不走 atom SURFACE 链；装配 = src/main.rs
//! `#[path]` 原位挂载（rr3-sense 同形）。本件只装**纯机制**（WAV 头/multipart
//! 装配，⑧-b 依赖闭包纪律：无 IO 无环境读取无 JSON）；云档/密钥/HTTP/应答解析
//! 策略面与识别客户端在组合件级 [`crate::cloud`]。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 公开面：multipart 请求体装配（44B WAV 头由装配内部构成——mech 内 pub fn
/// 留给件级 parity 腿与装配自用，bin 面无件外读者不进入口 re-export）
pub use mech::{rr3_asr_boost_pcm, rr3_asr_multipart_body};
