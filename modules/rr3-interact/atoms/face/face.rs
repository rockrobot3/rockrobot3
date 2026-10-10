//! face 原子件入口（五件套②，0.0.5 批C③ 原子重铺）：只声明公开面，机制在 mech.rs。
//!
//! 占格 `ui.face`（atom.toml）。SURFACE 是启动自述（组合件 rr3_interact_run 开头
//! 逐字打印，契约面零改动红线）。表情渲染：idle / blink / talk / think / happy /
//! listen / turn——七态白名单与 `talk` 自动挂起恢复是现在就要钉死的契约面；
//! 显示面经 rr-hal 显示 trait（批F5a③）由组合件注入，原子只钉"状态→帧"映射
//! 与推进语义。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

/// 启动自述（字符串逐字冻结——原 src/face.rs SURFACE 常量，契约面零改动）
pub const SURFACE: &[&str] = &["face: rpc rr3-interact/face/expression (request)"];

/// 本模块 exposes 的表情主题（module.toml [rpc].exposes 同源；原子层不重述 rpc 契约）
pub const RR3_FACE_TOPIC: &str = "rr3-interact/face/expression";

/// 合法表情白名单（contracts/topics.toml 的 note 同源；批F3 增 listen/turn——
/// 判据 9 点名的听取/转向两态）
pub const RR3_FACE_EXPRESSIONS: [&str; 7] =
    ["idle", "blink", "talk", "think", "happy", "listen", "turn"];

/// TTS 播放期间的临时表情（模块内自动切换，不接受点名）
pub const RR3_FACE_TALK: &str = "talk";

/// 8x8 点阵单帧：类型事实源在 rr-hal 的 `Rr3Display` trait 签名（批F5a③），
/// 此处 re-export 保模块内名。
pub use rr_hal::Rr3FaceFrame;

pub use mech::{rr3_face_frames, Rr3Face};
