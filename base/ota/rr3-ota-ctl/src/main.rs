// SPDX-License-Identifier: Apache-2.0
//! rr3-ota-ctl —— OTA 槽位查看/切换、回滚链与制品签名 CLI。
//!
//! 子命令：
//!   slots                                打印在册槽位与对合关系（读侧，不碰文件）
//!   switch <rr_a|rr_b> --bootconf P      改写引导配置 root=LABEL 做持久 A/B 切换
//!   factory            --bootconf P      根定位切 rr_factory 做一次性出厂引导
//!   rollback arm --limit N --altcmd C --env P
//!                                        U-Boot 回滚链上膛（四变量）
//!   rollback ok        --env P           回滚链解除（两变量）
//!
//! 判定与改写逻辑全在 `rr-ota`（`bootconf` / `bootenv`，base/runtime workspace 内，
//! 随七步门 ③ 全量跑测试）；本 CLI 只做参数解析与落盘调用，且**不缺省碰任何
//! 路径**——四个写命令必须显式给 `--bootconf` / `--env`。退出码：0 成功 /
//! 1 业务拒或 IO 失败 / 2 用法错。
//!
//! `sign` / `verify` / `manifest` / `keypub` 的机制在 `rr-ota::sign`（ADR-0026），
//! 本文件只保留分派与 usage；参数面在 `args`，子命令面在 `bootconf_cmd` /
//! `sign_cmd`。

mod args;
mod bootconf_cmd;
mod sign_cmd;

use std::process::ExitCode;

use rr_ota::{Rr3Slot, RR3_OTA_STAGING_PARTITION};

const ALL_SLOTS: [Rr3Slot; 3] = [Rr3Slot::SlotA, Rr3Slot::SlotB, Rr3Slot::Factory];

/// slots：只读打印，不碰任何路径。
fn rr3_print_slots() {
    for slot in ALL_SLOTS {
        println!(
            "{}  peer={}",
            slot.rr3_slot_label(),
            slot.rr3_slot_peer().rr3_slot_label()
        );
    }
    println!("{RR3_OTA_STAGING_PARTITION}  (模块级暂存，无对侧)");
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match rr3_dispatch(&argv) {
        Ok(msg) => {
            if !msg.is_empty() {
                println!("{msg}");
            }
            ExitCode::SUCCESS
        }
        Err((msg, code)) => {
            eprintln!("{msg}");
            code
        }
    }
}

/// 两级分派：槽位/引导面进 bootconf_cmd，制品签名面进 sign_cmd；未知子命令一次点名。
fn rr3_dispatch(argv: &[String]) -> Result<String, (String, ExitCode)> {
    match argv.first().map(String::as_str) {
        Some("slots") => {
            rr3_print_slots();
            Ok(String::new())
        }
        Some(cmd @ ("switch" | "factory" | "rollback")) => {
            bootconf_cmd::rr3_dispatch(cmd, &argv[1..])
        }
        Some(cmd @ ("keypub" | "manifest" | "sign" | "verify" | "install")) => {
            sign_cmd::rr3_dispatch(cmd, &argv[1..])
        }
        Some(other) => Err(args::rr3_use(format!(
            "未知子命令 {other:?}（在册：slots / switch / factory / rollback / keypub / manifest / sign / verify / install）"
        ))),
        None => Err(args::rr3_use(
            "缺子命令（在册：slots / switch / factory / rollback / keypub / manifest / sign / verify / install）"
                .into(),
        )),
    }
}

#[cfg(test)]
mod tests;
