// SPDX-License-Identifier: Apache-2.0
//! 唤醒词门控：机制核已抽到 [`rr_intent_mech::wake`]（Pi/S3 同源，W6 收编）；
//! 本件只做 crate 内 re-export，`dispatch::*` 与 `turn.rs` 的调用面不变。

pub(crate) use rr_intent_mech::wake::{rr3_wake_decide, Rr3WakeAction};
