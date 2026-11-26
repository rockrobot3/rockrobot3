// SPDX-License-Identifier: Apache-2.0
//! rr-hal-api —— 能力面契约（no_std，ADR-0018 修订 1 ③）。
//!
//! 件 ↔ 硬件的唯一契约面：10 trait **同步签名**、去 `Send + Sync`、值类型去
//! `String`/`Vec`（core-only）。async 留模块侧；契约名与路径由 `rr-hal` re-export。
//!
//! # 定形要点
//!
//! - **同步签名**：方法即调用即返回，无 future。写方法 `&mut self`（单所有者），
//!   读方法 `&self`。
//! - **值类型去 String**：`Rr3LinkState.ssid` 用 [`Rr3SsidText`]（定长 + 长度，
//!   SSID 上限 32 octets）；错型 [`Rr3HalError::InvalidParam`] 用 [`Rr3ErrText`]
//!   （定长缓冲，文案语义不变）。
//! - **`Vec<i16>` 换借用形状**：[`Rr3AudioSource::read`] 写入调用方缓冲、返有效
//!   样本数；[`Rr3AudioSink::write`] 借用切片（采样配置在 `open` 时固定）。
//!
//! 占位后端 [`Rr3UnavailableSource`] 一并迁入：承载"能力缺失必须显式回错，不许
//! 静默退化"的裁定，no_std 可达面直接复用。

#![cfg_attr(not(test), no_std)]
// 契约错型 Rr3HalError 携带定长 128 octets 文案（Rr3ErrText）——Result 的体积是
// F1"值类型去 String"裁定的直接结果，不是失误：result_large_err 的默认阈值恰被
// 文案缓冲顶到。契约形状不让步，lint 在 crate 级显式豁免并留注（不留无解释 allow）。
#![allow(clippy::result_large_err)]

mod act;
mod audio;
mod cfg;
mod err;
mod link;
mod power;
mod profile;
mod unavailable;

#[cfg(test)]
mod tests;

pub use act::{
    rr3_motion_allowed, Rr3ActHardStop, Rr3ActMotion, Rr3Actuator, Rr3DigitalIn, Rr3Display,
    Rr3FaceFrame, Rr3GpioOut, Rr3Position, Rr3PositionSource, Rr3PwmServo, Rr3Sample,
    Rr3SampleKind, Rr3SampleQuality, Rr3Sensor,
};
pub use audio::{Rr3AudioSink, Rr3AudioSource};
pub use cfg::{Rr3AudioCfg, RR3_AUDIO_CFG_16K_MONO16};
pub use err::{Rr3ErrText, Rr3HalError};
pub use link::{Rr3Link, Rr3LinkState, Rr3SsidText};
pub use power::{
    rr3_rail_tier_fold, Rr3BatteryState, Rr3Power, Rr3Rail, Rr3RailState, Rr3RailTier,
};
pub use profile::{
    Rr3ActFieldKind, Rr3ActFieldReader, Rr3ActHold, Rr3ActProfile, Rr3ActProfileField,
    Rr3ActSource, Rr3Store, RR3_ACT_PROFILE_FIELDS, RR3_PERIPH_AXIS_DEFAULT, RR3_PERIPH_AXIS_ENV,
};
pub use unavailable::Rr3UnavailableSource;
