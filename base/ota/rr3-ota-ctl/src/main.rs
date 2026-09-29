//! rr3-ota-ctl —— OTA 槽位查看/切换 CLI（骨架）。
//!
//! 读侧（`slots`）已经可用：它打印的是 `rr-ota` 里的常量，因此"文档说的标签"和
//! "代码用的标签"不可能漂移。写侧（切换/回滚）随 M6 与 genimage.cfg 一起落地，
//! **在此之前一律拒绝执行**——对块设备写引导标志属不可逆操作，不允许半成品上路。

use rr_ota::{Rr3Slot, RR3_OTA_STAGING_PARTITION};

const ALL_SLOTS: [Rr3Slot; 3] = [Rr3Slot::SlotA, Rr3Slot::SlotB, Rr3Slot::Factory];

fn usage() -> ! {
    eprintln!("用法: rr3-ota-ctl <slots | switch <rr_a|rr_b>>");
    eprintln!("slots  = 打印分区标签布局（读侧，可用）");
    eprintln!("switch = 切换活动槽（写侧，随 M6 落地，当前拒绝执行）");
    std::process::exit(2)
}

/// 活动槽的读写随 M6；在此之前任何切换请求都必须被拒
fn rr3_ota_ctl_refuse_switch(target: &str) -> String {
    format!("拒绝切换 {target}：槽位切换/回滚的写侧随 M6 与 genimage.cfg 落地（当前只读）")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("slots") => {
            for slot in ALL_SLOTS {
                println!(
                    "{}  peer={}",
                    slot.rr3_slot_label(),
                    slot.rr3_slot_peer().rr3_slot_label()
                );
            }
            println!("{RR3_OTA_STAGING_PARTITION}  (模块级暂存，无对侧)");
        }
        Some("switch") => {
            let Some(target) = args.get(1) else { usage() };
            eprintln!("{}", rr3_ota_ctl_refuse_switch(target));
            std::process::exit(1);
        }
        _ => usage(),
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

    #[test]
    fn switch_is_always_refused_until_m6() {
        for target in ["rr_a", "rr_b", "nonsense"] {
            let msg = rr3_ota_ctl_refuse_switch(target);
            assert!(msg.starts_with("拒绝切换"), "切换未被拒: {msg}");
            assert!(msg.contains("M6"), "拒绝文案没说明何时可用: {msg}");
        }
    }
}
