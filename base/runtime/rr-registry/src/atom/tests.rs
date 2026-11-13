//! atom 的单测（自同名单测模块原样抽出，判据不变）。

use super::*;
use crate::slots::{rr3_slot_parse, Rr3SlotTable};

// include_str! 以本文件所在目录（src/）为基准：src → rr-registry → runtime → base → 仓库根
const CONTRACT: &str = include_str!("../../../../contracts/slots.toml");

fn slots() -> Rr3SlotTable {
    rr3_slot_parse(CONTRACT)
}

fn battery_text() -> String {
    r#"
# battery 原子件 manifest（批B 打样）
schema_version = 1

[atom]
name = "battery"
version = "0.0.1"

[provides]
slots = ["power.battery"]

[uses]
slots = []

[events]
publishes = ["rr3-power/battery/state", "rr3-power/battery/low"]
subscribes = []
"#
    .to_string()
}

/// 合法 atom 文本零报错；schema 字段集与 atom.schema.toml 同步（路径唯一）
#[test]
fn valid_battery_atom_passes() {
    let errs = rr3_atom_chk(&battery_text(), &slots());
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn atom_field_paths_are_unique() {
    let specs = rr3_atom_field_specs();
    let mut seen: Vec<&str> = specs.iter().map(|s| s.path.as_str()).collect();
    let before = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), before, "字段路径不得重复");
    assert_eq!(specs.len(), 7);
}

/// 查册门：未登记格位拒绝；空格位表 fail-closed
#[test]
fn unregistered_slot_is_rejected_and_empty_table_fail_closes() {
    let bad = battery_text().replace("power.battery", "power.solar");
    let errs = rr3_atom_chk(&bad, &slots());
    assert!(
        errs.iter().any(|e| e.contains("未登记格位 power.solar")),
        "{errs:?}"
    );

    let errs = rr3_atom_chk(&battery_text(), &Rr3SlotTable::default());
    assert_eq!(
        errs,
        vec!["格位表为空：拒绝校验任何 atom manifest（空注册表不是放行许可证）"]
    );
}

/// 每格一件：provides.slots 恰一项，多项/零项都点名裁定出处
#[test]
fn provides_slots_must_be_exactly_one() {
    let two = battery_text().replace(
        "slots = [\"power.battery\"]",
        "slots = [\"power.battery\", \"power.charger\"]",
    );
    let errs = rr3_atom_chk(&two, &slots());
    assert!(
        errs.iter()
            .any(|e| e.contains("恰有一项") && e.contains("2 项")),
        "{errs:?}"
    );

    let none = battery_text().replace("slots = [\"power.battery\"]", "slots = []");
    let errs = rr3_atom_chk(&none, &slots());
    assert!(
        errs.iter()
            .any(|e| e.contains("恰有一项") && e.contains("0 项")),
        "{errs:?}"
    );
}

/// 板无关断言：注释里塞厂商/引言语 token 也必须命中（注释是载荷面）
#[test]
fn board_free_assertion_hits_tokens_anywhere() {
    let tainted = battery_text() + "\n# 后端绑定 INA219 @ GPIO2/3\n";
    let errs = rr3_atom_chk(&tainted, &slots());
    assert!(errs.iter().any(|e| e.contains("ina219")), "{errs:?}");
    assert!(errs.iter().any(|e| e.contains("gpio")), "{errs:?}");

    // 断言独立可用（生成器与批E 门禁的复用入口）
    let errs = rr3_board_free_chk("clean manifest");
    assert!(errs.is_empty());
    assert_eq!(rr3_board_free_chk("MAX7219").len(), 1);
}

/// 必填字段与 topic 形状：缺字段点名、坏主题点名、uses/事件缺省放行
#[test]
fn required_fields_and_topic_shape_are_enforced() {
    let missing = battery_text().replace("version = \"0.0.1\"\n", "");
    let errs = rr3_atom_chk(&missing, &slots());
    assert!(
        errs.iter().any(|e| e.contains("缺少必填字段 atom.version")),
        "{errs:?}"
    );

    let bad_topic = battery_text().replace("rr3-power/battery/state", "rr3-power/batteryState");
    let errs = rr3_atom_chk(&bad_topic, &slots());
    assert!(errs.iter().any(|e| e.contains("topic 非法")), "{errs:?}");

    let minimal = r#"
schema_version = 1
[atom]
name = "vad"
version = "0.1.0"
[provides]
slots = ["audio.vad"]
"#;
    let errs = rr3_atom_chk(minimal, &slots());
    assert!(errs.is_empty(), "uses/events 缺省即合规: {errs:?}");
}

/// 批B③ 打样件钉死：rr3-power 的两份 atom.toml 必须过契约 + 查册门 + 板无关断言
/// （打样件漂移当场红；include_str! 相对 src/：src → rr-registry → runtime → base → 仓根）
#[test]
fn power_pilot_atoms_stay_compliant() {
    let battery = include_str!("../../../../modules/rr3-power/atoms/battery/atom.toml");
    let charger = include_str!("../../../../modules/rr3-power/atoms/charger/atom.toml");
    let errs = rr3_atom_chk(battery, &slots());
    assert!(errs.is_empty(), "battery 打样件漂移: {errs:?}");
    let errs = rr3_atom_chk(charger, &slots());
    assert!(errs.is_empty(), "charger 打样件漂移: {errs:?}");
}

/// 批C① 重铺件钉死：rr3-sense 的四份 atom.toml 必须过契约 + 查册门 + 板无关断言
/// （重铺件漂移当场红；每格一件由校验器跨字段规则把守）
#[test]
fn sense_relaid_atoms_stay_compliant() {
    let capture = include_str!("../../../../modules/rr3-sense/atoms/capture/atom.toml");
    let vad = include_str!("../../../../modules/rr3-sense/atoms/vad/atom.toml");
    let env = include_str!("../../../../modules/rr3-sense/atoms/env/atom.toml");
    let imu = include_str!("../../../../modules/rr3-sense/atoms/imu/atom.toml");
    for (name, text) in [
        ("capture", capture),
        ("vad", vad),
        ("env", env),
        ("imu", imu),
    ] {
        let errs = rr3_atom_chk(text, &slots());
        assert!(errs.is_empty(), "{name} 重铺件漂移: {errs:?}");
    }
}

/// 批C② 重铺件钉死：rr3-act 的三份 atom.toml 必须过契约 + 查册门 + 板无关断言
/// （重铺件漂移当场红；每格一件由校验器跨字段规则把守）
#[test]
fn act_relaid_atoms_stay_compliant() {
    let servo = include_str!("../../../../modules/rr3-act/atoms/servo/atom.toml");
    let led = include_str!("../../../../modules/rr3-act/atoms/led/atom.toml");
    let relay = include_str!("../../../../modules/rr3-act/atoms/relay/atom.toml");
    for (name, text) in [("servo", servo), ("led", led), ("relay", relay)] {
        let errs = rr3_atom_chk(text, &slots());
        assert!(errs.is_empty(), "{name} 重铺件漂移: {errs:?}");
    }
}

/// 批C③ 重铺件钉死：rr3-interact 的两份 atom.toml 必须过契约 + 查册门 + 板无关断言
/// （重铺件漂移当场红；每格一件由校验器跨字段规则把守）
#[test]
fn interact_relaid_atoms_stay_compliant() {
    let face = include_str!("../../../../modules/rr3-interact/atoms/face/atom.toml");
    let player = include_str!("../../../../modules/rr3-interact/atoms/player/atom.toml");
    for (name, text) in [("face", face), ("player", player)] {
        let errs = rr3_atom_chk(text, &slots());
        assert!(errs.is_empty(), "{name} 重铺件漂移: {errs:?}");
    }
}

/// 批C④ 重铺件钉死：rr3-connect 的两份 atom.toml 必须过契约 + 查册门 + 板无关断言
/// （重铺件漂移当场红；每格一件由校验器跨字段规则把守）
#[test]
fn connect_relaid_atoms_stay_compliant() {
    let wifi = include_str!("../../../../modules/rr3-connect/atoms/wifi/atom.toml");
    let bt = include_str!("../../../../modules/rr3-connect/atoms/bt/atom.toml");
    for (name, text) in [("wifi", wifi), ("bt", bt)] {
        let errs = rr3_atom_chk(text, &slots());
        assert!(errs.is_empty(), "{name} 重铺件漂移: {errs:?}");
    }
}

/// 批C⑤ 重铺件钉死：rr3-brain（Python 面）的四份 atom.toml 必须过契约、查册门
/// 与板无关断言（机制本体 .py 原位——Python 单文件即机制+入口合一，Rust 五件套
/// 拆分随 ADR-0016 迁移收敛；重铺件漂移当场红）
#[test]
fn brain_relaid_atoms_stay_compliant() {
    let dialog = include_str!("../../../../modules/rr3-brain/atoms/dialog/atom.toml");
    let asr = include_str!("../../../../modules/rr3-brain/atoms/asr/atom.toml");
    let llm = include_str!("../../../../modules/rr3-brain/atoms/llm/atom.toml");
    let tts = include_str!("../../../../modules/rr3-brain/atoms/tts/atom.toml");
    for (name, text) in [("dialog", dialog), ("asr", asr), ("llm", llm), ("tts", tts)] {
        let errs = rr3_atom_chk(text, &slots());
        assert!(errs.is_empty(), "{name} 重铺件漂移: {errs:?}");
    }
}

/// 收口批 E'：未知字段反向断言（rr3_no_unknown_keys）——
/// typo 键（uses.slost）与未登记段双路红，合法 manifest 零命中（fail-closed 自证）
#[test]
fn unknown_fields_assert_flags_typo_keys() {
    let specs = rr3_atom_field_specs();
    let bad: Value = toml::from_str(
            "[atom]\nname = \"x\"\nversion = \"0.1.0\"\n[provides]\nslots = [\"ui.face\"]\n[uses]\nslost = [\"audio.playback\"]\n[bogus]\ny = 2\n",
        )
        .expect("样例可解析");
    let errs = crate::validate::rr3_no_unknown_keys(&bad, &specs, "atom manifest");
    assert!(
        errs.iter().any(|e| e.contains("uses.slost")),
        "typo 键必须点名: {errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.contains("[bogus]")),
        "未登记段必须点名: {errs:?}"
    );
    let ok: Value = toml::from_str(
        "[atom]\nname = \"x\"\nversion = \"0.1.0\"\n[provides]\nslots = [\"ui.face\"]\n",
    )
    .expect("合法样例可解析");
    assert!(crate::validate::rr3_no_unknown_keys(&ok, &specs, "atom manifest").is_empty());
}

/// 批E 门禁接线（architecture §13"每格一件·不留空格"反向断言，查册真值处执法）：
/// 仓扫描全部 `modules/*/atoms/*/atom.toml`——①每份过契约+查册门+板无关
/// （新增件**不钉即红**，堵住上面 include_str! 逐份钉法"漏钉不红"的缺口）；
/// ②provides 全集对 slots 表反查：除基座内建豁免集外**每格恰一件**（两件抢格 /
/// 新格无件当场红），内建格不立件。
#[test]
fn repo_atoms_compliant_and_every_slot_uniquely_covered() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("rr-registry 位于 base/runtime/ 下三级")
        .to_path_buf();
    // 基座内建豁免集（0.0.5 方案 §12.1 终裁：boot 即在位、无独立原子目录——
    // rr-bus/rrd 承载或机制核切片承载；power.battery 虽 base 层但由 battery
    // 原子承担，不在豁免集。此常量漂移 = 覆盖账不平断言当场红）
    // power.rail/power.budget（批N2 件二，ADR-0021 裁定 2）同为 base 层，承载是
    // rr3-power 进程内机制切片（RailMonitor/RailJudge，件五归属搬家）与板侧守护，
    // 无独立原子目录——立无源码的空原子件即 D15 所防的空壳件
    const BUILTIN_NO_ATOM: &[&str] = &[
        "bus.publish",
        "bus.subscribe",
        "bus.rpc",
        "net.online",
        "power.rail",
        "power.budget",
    ];
    let slots = slots();

    // ① 仓扫描：逐份全量校验 + 收格主（每格一件的唯一性在此建立）
    let mut owner: std::collections::BTreeMap<String, String> = Default::default();
    let mut scanned = 0usize;
    for m in std::fs::read_dir(root.join("modules")).expect("modules/ 必须存在") {
        let atoms = m.expect("modules 项可读").path().join("atoms");
        if !atoms.is_dir() {
            continue;
        }
        for a in std::fs::read_dir(&atoms).expect("atoms/ 必须可读") {
            let manifest = a.expect("atom 项可读").path().join("atom.toml");
            if !manifest.is_file() {
                continue;
            }
            scanned += 1;
            let text = std::fs::read_to_string(&manifest).expect("atom.toml 可读");
            let errs = rr3_atom_chk(&text, &slots);
            assert!(errs.is_empty(), "{} 漂移: {errs:?}", manifest.display());
            let v: Value = toml::from_str(&text).expect("atom.toml 已过 TOML 解析");
            // 批E'：目录名 = atom.name 唯一对应——⑦-b/⑦-c 门按目录名执法、
            // 契约按 atom.name，两名漂移即"合同与户口本不符"（件名两个真源）
            let dir_name = manifest
                .parent()
                .and_then(|p| p.file_name())
                .expect("atom.toml 必有父目录名")
                .to_string_lossy();
            let atom_name = v["atom"]["name"].as_str().expect("atom.name 已过必填校验");
            assert_eq!(
                atom_name, dir_name,
                "{dir_name}：atom.name 与目录名不一致（件名两个真源）"
            );
            let slot = v["provides"]["slots"][0]
                .as_str()
                .expect("provides.slots 恰一项且为字符串（validate 已断）")
                .to_string();
            if let Some(prev) = owner.insert(slot.clone(), manifest.display().to_string()) {
                panic!(
                    "格 {slot} 被两件抢占（每格一件，ADR-0019 裁定 1）: {prev} 与 {}",
                    manifest.display()
                );
            }
        }
    }
    assert!(
        scanned >= 17,
        "仓扫描仅见 {scanned} 份 atom.toml（打空/缺件属门禁失效，不放行）"
    );

    // ② 不留空格 + 内建格不立件：23 格逐格反查，末尾覆盖账必须轧平
    let names = slots.rr3_slot_names();
    for slot in &names {
        let provided = owner.get(slot);
        if BUILTIN_NO_ATOM.contains(&slot.as_str()) {
            assert!(
                provided.is_none(),
                "内建格 {slot} 不立件（基座内建/机制核切片承载），却被 {} 提供",
                provided.unwrap()
            );
        } else {
            assert!(
                provided.is_some(),
                "格 {slot} 无 atom 承担（不留空格）——立件或经契约级评审改格"
            );
        }
    }
    assert_eq!(
        owner.len() + BUILTIN_NO_ATOM.len(),
        names.len(),
        "覆盖账不平：atom 承担格数 + 内建豁免数 ≠ slots 表全数"
    );
}
