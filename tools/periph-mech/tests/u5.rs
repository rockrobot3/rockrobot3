//! U5 共享机制 digest 的跨平台稳定性用例（FNV-1a 64）。
//!
//! 像素编码/占空比换算/轨道判定三组机制在宿主与板侧共用同一份实现，本用例
//! 钉其输出字节与摘要不变——改机制即改 digest，跨平台漂移当场红。

use periph_mech::oled_codec::{rr3_frame_pages, rr3_page_cmd};
use periph_mech::periph_core::rr3_deg_to_duty;
use periph_mech::rail_judge::RailJudge;

/// U5 共享机制 digest 的稳定哈希（FNV-1a 64；不引额外依赖，跨平台逐位相同）。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// U5 行为等价（共享机制面）：舵机 angle→duty 全表、OLED 单页字节流、轨压分档表。
/// Pi/classic/S3 同用本 crate，本测试锁定三平台共同字节，任何一侧单改都会失败。
#[test]
fn u5_shared_mechanism_digest() {
    let mut lines = Vec::new();
    for a in 60..=120u32 {
        lines.push(format!("pan:{a}:{}", rr3_deg_to_duty(a, 410, 2048)));
    }
    for a in 80..=100u32 {
        lines.push(format!("tilt:{a}:{}", rr3_deg_to_duty(a, 410, 2048)));
    }

    let frame = [0xAAu8; 8];
    let pages = rr3_frame_pages(&frame);
    let page0 = pages[0]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("");
    lines.push(format!(
        "oled0:{}{}",
        rr3_page_cmd(0)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        page0
    ));

    let mut judge = RailJudge::new(4_600, 4_400);
    let mut mv = 6_000u16;
    loop {
        let (tier, edge) = judge.feed(mv);
        lines.push(format!("rail:{mv}:{}:{edge}", tier.name()));
        if mv == 0 {
            break;
        }
        mv -= 100;
    }

    let digest = fnv1a64(lines.join("\n").as_bytes());
    println!("U5_DIGEST {digest:016x}");
    assert_eq!(format!("{digest:016x}"), "7a8b983a76eefc38");
}
