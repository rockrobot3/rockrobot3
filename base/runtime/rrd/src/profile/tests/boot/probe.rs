//! 探针注入与缺格点名：逐格读探测结果、不整单连坐。

use super::super::*;

/// MC-4 门禁用例①：注入非 linux 生产探针（有硬件面的格位恒缺）→ 这些格位进缺格
/// 清单，且**点名到件与键**（(模块, 格位) 成对，模块不错配到别的格）——这条钉的
/// 是"缺格 → Degraded 点名件与键"的检出真值（探测拿不到答案不当作在位的下游）。
/// 无硬件面可反证的格位（cognition.llm）不在缺格清单：那是"没有反证"不是"拿不到答案"。
#[test]
fn injected_absent_probe_names_missing_module_and_slot() {
    let root = Root::new("probe-absent");
    let p = root.write_profile("absent.toml", GOOD);
    let boot = boot_inputs_with_probe(&p, &crate::probe::Rr3AbsentProbe)
        .expect("缺格不是致命项，boot 输入必须成功返回");
    assert_eq!(
        boot.missing,
        vec![
            ("rr3-pa".to_string(), "act.servo".to_string()),
            ("rr3-pb".to_string(), "power.charger".to_string()),
        ],
        "有硬件面的格位必须点名件与键、升序去重（软件格 cognition.llm 不在内）: {:?}",
        boot.missing
    );
}

/// MC-4 门禁用例②：探测件只对一格报缺 → 只有那一格的点名模块进缺格，
/// 其余同在位——证明缺格判定确实**逐格读探测结果**，不是整单连坐。
#[test]
fn injected_selective_probe_marks_only_its_slot() {
    struct OnlyServoAbsent;
    impl crate::probe::Rr3Probe for OnlyServoAbsent {
        fn rr3_probe_slot(&self, slot: &str) -> bool {
            slot != "act.servo"
        }
    }
    let root = Root::new("probe-selective");
    let p = root.write_profile("selective.toml", GOOD);
    let boot = boot_inputs_with_probe(&p, &OnlyServoAbsent).expect("部分缺格不致命");
    assert_eq!(
        boot.missing,
        vec![("rr3-pa".to_string(), "act.servo".to_string())],
        "只有探到缺的那一格点名件与键: {:?}",
        boot.missing
    );
}
