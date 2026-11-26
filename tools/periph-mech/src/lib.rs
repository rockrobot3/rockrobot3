// SPDX-License-Identifier: Apache-2.0
//! periph-mech —— 外设机制核（骨架轨与板侧后端的共同前置）。
//!
//! 为什么单独成 crate：帧面收发、OLED 编码、舵机缓速/轨压拒动、轨压双阈值判定都是平台
//! 无关机制——换平台只换五端口实现（[`rr_periph_proto::ports`]），这四件零改动。
//!
//! 边界纪律：零 esp-idf 依赖（宿主与板档双向引用本 crate，任何平台依赖都会污染另一侧
//! 构建面，ADR-0024）；不读墙上时钟、不睡眠、不持线程（时序与随机由适配层注入，机制只
//! 算）；零策略量（阈值/行程/亮度阶梯/步进/引脚一律按参传入，本 crate 不写板名）；
//! 不搬驱动构造、控制/运行线程、`httpd`、`mic`——它们归各平台适配层。
//!
//! 模块：[`frame`] 帧面收帧/应答；[`oled_codec`] OLED 纯编码；[`periph_core`] 舵机行程/
//! 缓速/轨压/亮度；[`rail_judge`] 分压换算 + 双阈值分类。

pub mod frame;
pub mod oled_codec;
pub mod periph_core;
pub mod rail_judge;
