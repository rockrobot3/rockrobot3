//! `--profile` 装配演练（D9，0.0.2 方案）：装配单驱动 boot + 全模块真进程全链路巡检。
//!
//! 与既有两面演练（`--manifest` 手工清单）和 `--brain`（对话闭环）互补：本模式证
//! **广度**——仓内装配单（缺省 profiles/default.toml，16 格满配六模块）驱动 rrd
//! 装配链正门（`--profile` + `--contracts`，参数与 rrd.service 的 ExecStart 同形），
//! boot spawn 全部 face 模块（六 Rust 模块，批N32 件四起 brain 也是 Rust），判据是
//! 「六模块全 running + `denied_publish = 0`」+ rr3-ctl 四命令巡检。
//!
//! 演练 modules-root 布局 = **装机布局**（`<root>/profiles/<机>.toml` 上两级是
//! modules-root、`modules/<name>/{module.toml,config.toml,<entry>}`）：
//! - manifest 用仓源原文、只改 entry 一个值（rr3-pack `rr3_manifest_rewrite`
//!   同语义同三道门）——装配对账的 provides 真值必须是仓源那一册，自造 manifest
//!   等于第二份真值；entry 绝对化是「entry 在仓相对、装机绝对」的第三面
//!   （装机 = 演练 modules-root，前两面：deb 打包、buildroot post-build）；
//! - config 同目录就位：全部模块用仓源 config（纯运行参数）。brain 的云端由
//!   rrd 进程 env `RR3_BRAIN_CLOUD=mock` 压成 mock 档（确定性应答不发网络，
//!   仓源真云 base_url 不触达）。
//!
//! mock 注入走 rrd 进程 env（`RR3_BRAIN_CLOUD=mock` 由 brain 继承；其余模块无
//! 消费者）+ `RR3_HAL_BACKEND=sim`。HAL 面的 sim 落点有二：沙箱装配单副本经
//! [`rr3_asm_sim_text`] 变换（hal_backend → sim、可选真后端专面摘除）——
//! P0-3 起 supervisor 按装配单声明的 `RR3_HAL_BACKEND` 注入子进程、装配单是
//! 部署真值，rrd env 的 sim 只护 rrd 自己不护模块；rrd env 再兜一层
//! `--manifest` 调试路径。本模块依赖 UDS 与真进程，仅 Unix 参与编译。

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
