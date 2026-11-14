//! 装载输入面单测（批B15 自 `tests.rs` 按被试族切出）。
//!
//! 覆盖执行轴档案投影（单轴/双轴/坏件/副本/轴错配）、探针注入与 binding 归属、
//! `[hardware]` 标量串化与排序转义、SPI 词形推导，以及引脚/域/证据三道装载判据。

mod axis;
mod domains;
mod hardware;
mod missing;
mod pins;
mod probe;
