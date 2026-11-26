// SPDX-License-Identifier: Apache-2.0
//! `--profile` 装配演练（D9）：装配单驱动 boot + 全模块真进程全链路巡检。
//!
//! 与 `--manifest`（手工清单）和 `--brain`（对话闭环）互补：本模式证**广度**——
//! 仓内装配单（缺省 profiles/default.toml，16 格满配六模块）驱动 rrd 装配链正门
//! （`--profile` + `--contracts`，参数与 rrd.service 的 ExecStart 同形），boot
//! spawn 全部 face 模块（六 Rust 模块），判据是「六模块全 running +
//! `denied_publish = 0`」+ rr3-ctl 四命令巡检。
//!
//! 演练 modules-root 布局 = **装机布局**（`<root>/profiles/<机>.toml` 上两级是
//! modules-root、`modules/<name>/{module.toml,config.toml,<entry>}`）：manifest 用
//! 仓源原文、只改 entry 一个值（rr3-pack `rr3_manifest_rewrite` 同语义同三道门）——
//! provides 真值必须是仓源那一册；entry 绝对化与 deb 打包、buildroot post-build 同面。
//! config 同目录就位用仓源；brain 云端由 rrd 进程 env `RR3_BRAIN_CLOUD=mock` 压成 mock 档。
//!
//! mock 注入走 rrd 进程 env（brain 继承）+ `RR3_HAL_BACKEND=sim`；HAL 面的 sim 落点：
//! [`rr3_asm_sim_text`] 变换装配单副本。supervisor 按装配单声明注入子进程（部署真值），
//! rrd env 只护自己；`--manifest` 调试路径再兜一层。仅 Unix 编译。

mod report;
mod spawn;
mod text;
mod verify;

pub(crate) use report::*;
pub(crate) use spawn::*;
pub(crate) use text::*;
pub(crate) use verify::*;

#[cfg(test)]
mod tests;
