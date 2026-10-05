//! rr3-ota-ctl —— OTA 槽位查看/切换 CLI（0.0.3 批D 主机面实装）。
//!
//! 三个子命令：
//!   slots                              打印在册槽位与对合关系（读侧，不碰文件）
//!   switch <rr_a|rr_b> --bootconf P    改写引导配置 root=LABEL 做持久 A/B 切换
//!   factory           --bootconf P     根定位切 rr_factory 做一次性出厂引导
//!
//! 判定与改写逻辑全在 `rr-ota::bootconf`（base/runtime workspace 内，随七步门
//! ③ 全量跑测试）；本 CLI 只做参数解析与落盘调用，且**不缺省碰任何路径**——
//! switch/factory 必须显式给 --bootconf。退出码：0 成功 / 1 业务拒 / 2 用法错。
//! 板上件（U-Boot 环境变量写侧、块设备/挂载探测）本版本不 claim，
//! 见 base/ota/partition-layout.md 增补段。

use std::path::PathBuf;
use std::process::ExitCode;

use rr_ota::bootconf;
use rr_ota::{Rr3Slot, RR3_OTA_STAGING_PARTITION};

const ALL_SLOTS: [Rr3Slot; 3] = [Rr3Slot::SlotA, Rr3Slot::SlotB, Rr3Slot::Factory];

const RR3_USAGE: &str = "\
用法:
  rr3-ota-ctl slots
  rr3-ota-ctl switch <rr_a|rr_b> --bootconf <extlinux.conf 路径>
  rr3-ota-ctl factory --bootconf <extlinux.conf 路径>

  slots   打印在册槽位与对合关系（读侧）
  switch  改写引导配置 root=LABEL，持久 A/B 切换
  factory 根定位切 rr_factory 做一次性出厂引导（rr_factory 分区本体不写）";

fn usage() -> ExitCode {
    eprintln!("{RR3_USAGE}");
    ExitCode::from(2)
}

/// 目标标签解析：在册标签映射槽位；不在册标签属用法错（exit 2）
fn rr3_slot_arg(label: &str) -> Result<Rr3Slot, String> {
    Rr3Slot::rr3_slot_from_label(label)
        .ok_or_else(|| format!("未知槽位标签 {label:?}（在册：rr_a / rr_b / rr_factory）"))
}

/// 抓取 --bootconf <路径> 参数（分离式写法）。无缺省值——本 CLI 不缺省碰任何路径。
fn rr3_bootconf_arg(args: &[String]) -> Result<PathBuf, String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--bootconf" {
            return it
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--bootconf 缺路径参数".into());
        }
    }
    Err("switch/factory 必须显式传 --bootconf <路径>".into())
}

fn rr3_parse_switch(args: &[String]) -> Result<(Rr3Slot, PathBuf), ExitCode> {
    let Some(label) = args.first() else {
        eprintln!("switch 缺目标槽位参数");
        return Err(usage());
    };
    let target = match rr3_slot_arg(label) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return Err(usage());
        }
    };
    match rr3_bootconf_arg(&args[1..]) {
        Ok(path) => Ok((target, path)),
        Err(e) => {
            eprintln!("{e}");
            Err(usage())
        }
    }
}

/// switch：读 → 判定改写 → 原子写回。业务拒（同槽幂等拒 / rr_factory 目标 /
/// 形状漂移）一律 (文案, 1)——文案全部来自 rr-ota::bootconf，本层不重抄语义。
fn rr3_cmd_switch(target: Rr3Slot, path: &PathBuf) -> Result<String, (String, ExitCode)> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| (format!("读引导配置 {path:?} 失败: {e}"), ExitCode::from(1)))?;
    let from = bootconf::rr3_bootconf_current_slot(&text)
        .map_err(|e| (format!("拒绝切换：{e}"), ExitCode::from(1)))?;
    let out = bootconf::rr3_bootconf_switch(&text, target)
        .map_err(|e| (format!("拒绝切换：{e}"), ExitCode::from(1)))?;
    bootconf::rr3_bootconf_write_atomic(path, &out)
        .map_err(|e| (format!("原子落盘失败: {e}"), ExitCode::from(1)))?;
    Ok(format!(
        "引导配置已改写：root=LABEL={} → {}（持久切换，重启后生效）",
        from.rr3_slot_label(),
        target.rr3_slot_label()
    ))
}

/// factory：同上，但目标是 rr_factory 的一次性出厂引导（分区本体不写）。
fn rr3_cmd_factory(path: &PathBuf) -> Result<String, (String, ExitCode)> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| (format!("读引导配置 {path:?} 失败: {e}"), ExitCode::from(1)))?;
    let from = bootconf::rr3_bootconf_current_slot(&text)
        .map_err(|e| (format!("拒绝恢复出厂：{e}"), ExitCode::from(1)))?;
    let out = bootconf::rr3_bootconf_factory(&text)
        .map_err(|e| (format!("拒绝恢复出厂：{e}"), ExitCode::from(1)))?;
    bootconf::rr3_bootconf_write_atomic(path, &out)
        .map_err(|e| (format!("原子落盘失败: {e}"), ExitCode::from(1)))?;
    Ok(format!(
        "引导配置已改写：root=LABEL={} → rr_factory（一次性出厂引导，重启后生效；rr_factory 分区本体不写）",
        from.rr3_slot_label()
    ))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        Some("slots") => {
            for slot in ALL_SLOTS {
                println!(
                    "{}  peer={}",
                    slot.rr3_slot_label(),
                    slot.rr3_slot_peer().rr3_slot_label()
                );
            }
            println!("{RR3_OTA_STAGING_PARTITION}  (模块级暂存，无对侧)");
            return ExitCode::SUCCESS;
        }
        Some("switch") => match rr3_parse_switch(&args[1..]) {
            Ok((target, path)) => rr3_cmd_switch(target, &path),
            Err(code) => return code,
        },
        Some("factory") => match rr3_bootconf_arg(&args[1..]) {
            Ok(path) => rr3_cmd_factory(&path),
            Err(e) => {
                eprintln!("{e}");
                return usage();
            }
        },
        _ => return usage(),
    };
    match outcome {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err((msg, code)) => {
            eprintln!("{msg}");
            code
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 打印的标签必须与 rr-ota 常量逐一对应（否则文档与代码就分家了）
    #[test]
    fn slot_labels_come_from_rr_ota_constants() {
        assert_eq!(
            ALL_SLOTS.map(|s| s.rr3_slot_label()),
            ["rr_a", "rr_b", "rr_factory"]
        );
        assert_eq!(rr_ota::RR3_OTA_SLOT_NAMES, ["rr_a", "rr_b"]);
    }

    /// 写侧实装后，"恒拒"面由"无效目标拒"取代：不在册标签在用法层拒
    /// （exit 2）；业务规则（同槽 / factory 目标 / 形状漂移）归 rr-ota::bootconf
    #[test]
    fn slot_arg_maps_known_labels_and_rejects_unknown_ones() {
        assert_eq!(rr3_slot_arg("rr_a"), Ok(Rr3Slot::SlotA));
        assert_eq!(rr3_slot_arg("rr_b"), Ok(Rr3Slot::SlotB));
        assert_eq!(rr3_slot_arg("rr_factory"), Ok(Rr3Slot::Factory));
        let e = rr3_slot_arg("nonsense").unwrap_err();
        assert!(e.contains("未知槽位标签"), "不在册标签必须拒: {e}");
    }

    /// CLI 壳端到端：临时文件 读 → 改写 → 原子写回；业务拒（同槽）透传 rr-ota 文案
    #[test]
    fn switch_command_rewrites_file_and_rejects_same_slot() {
        let dir = std::env::temp_dir().join(format!("rr3-ota-ctl-{}-e2e", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("extlinux.conf");
        std::fs::write(
            &path,
            "label rockrobot3\n    append root=LABEL=rr_a rootwait\n",
        )
        .unwrap();

        let msg = rr3_cmd_switch(Rr3Slot::SlotB, &path).unwrap();
        assert!(msg.contains("rr_b"), "成功文案须点名新槽: {msg}");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("root=LABEL=rr_b"),
            "文件必须真的落了新标签"
        );

        // 同槽再切：业务拒（退出码 1），文件不被重写
        let err = rr3_cmd_switch(Rr3Slot::SlotB, &path).unwrap_err();
        assert_eq!(err.1, ExitCode::from(1), "业务拒退出码是 1");
        assert!(err.0.contains("拒绝切换"), "拒因须点名: {}", err.0);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("root=LABEL=rr_b"),
            "被拒的请求不得改动文件"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// --bootconf 是硬门槛：缺参必须在用法层拒，不得缺省碰任何路径
    #[test]
    fn bootconf_arg_is_mandatory_and_never_defaulted() {
        let args: Vec<String> = vec!["switch".into(), "rr_b".into()];
        assert!(rr3_bootconf_arg(&args).is_err(), "缺 --bootconf 必须拒");
        let args: Vec<String> = vec!["switch".into(), "rr_b".into(), "--bootconf".into()];
        assert!(
            rr3_bootconf_arg(&args).is_err(),
            "--bootconf 缺路径值必须拒"
        );
        let args: Vec<String> = vec![
            "switch".into(),
            "rr_b".into(),
            "--bootconf".into(),
            "/tmp/x.conf".into(),
        ];
        assert_eq!(
            rr3_bootconf_arg(&args).unwrap(),
            PathBuf::from("/tmp/x.conf")
        );
    }
}
