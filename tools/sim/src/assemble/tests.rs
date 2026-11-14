//! 装配演练单测：entry 改写、sim 变换与仓内装机布局前提。

use super::*;
use std::path::Path;

use rrd::manifest::rr3_manifest_parse;
use rrd::profile::rr3_profile_parse;

use crate::drill::rr3_bin_dir;

const GOOD: &str = "[manifest]\nschema_version = 1\nname = \"rr3-x\"\nlang = \"rust\"\n\
entry = \"rr3-x\" # 行尾注释随原值保留\n\n[provides]\ncapabilities = [\"a.b\"]\n\n\
[resources]\nrestart_policy = \"always\"\n\n[health]\nheartbeat_ms = 1000\n\
watchdog_timeout_ms = 5000\n";

#[test]
fn rewrite_changes_only_the_entry_value() {
    let out = rr3_entry_rewrite(
        GOOD,
        Path::new("/usr/lib/rr3/modules/rr3-x/rr3-x"),
        "rr3-x",
        "rr3-x",
    )
    .unwrap();
    assert!(
        out.contains("entry = \"/usr/lib/rr3/modules/rr3-x/rr3-x\" # 行尾注释随原值保留\n"),
        "{out}"
    );
    // 除 entry 引号内的值外逐字节一致
    let strip = |s: &str| {
        s.lines()
            .map(|l| {
                if l.trim_start().starts_with("entry") {
                    "<entry>"
                } else {
                    l
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(strip(GOOD), strip(&out));
    // 尾换行形状原样保留
    assert!(out.ends_with('\n'));
}

#[test]
fn rewrite_gates_reject_the_bad_shapes() {
    // 多条 entry 行
    let t = format!("{GOOD}entry = \"other\"\n");
    assert!(
        rr3_entry_rewrite(&t, Path::new("/x"), "rr3-x", "rr3-x")
            .unwrap_err()
            .contains("恰一条"),
        "多 entry 行须拒"
    );
    // 无 entry 行
    let t = GOOD.replace("entry = \"rr3-x\" # 行尾注释随原值保留\n", "");
    assert!(
        rr3_entry_rewrite(&t, Path::new("/x"), "rr3-x", "rr3-x")
            .unwrap_err()
            .contains("无 entry 行"),
        "无 entry 行须拒"
    );
    // 旧值与 facts 交叉核验
    assert!(rr3_entry_rewrite(GOOD, Path::new("/x"), "wrong", "rr3-x")
        .unwrap_err()
        .contains("不一致"));
    // 不可装机：相对 / .. 段
    for bad in ["rel/entry", "/abs/../esc"] {
        assert!(
            rr3_entry_rewrite(GOOD, Path::new(bad), "rr3-x", "rr3-x")
                .unwrap_err()
                .contains("不可装机"),
            "{bad} 须拒"
        );
    }
}

#[test]
fn sim_profile_transform_rewrites_hal_and_drops_face_overrides() {
    let t = "[runtime]\nlog_ring_cap = 512\nhal_backend = \"linux\"\n\
                 audio_backend = \"esp32\"\nservo_backend = \"esp32\"\n\n\
                 [platform]\ntier = \"host\"\nnote = \"hal_backend 字样在段外不受影响\"\n";
    let out = rr3_asm_sim_text(t).unwrap();
    assert!(out.contains("hal_backend = \"sim\"\n"), "{out}");
    assert!(!out.contains("audio_backend"), "{out}");
    assert!(!out.contains("servo_backend"), "{out}");
    assert!(out.contains("段外不受影响"), "{out}");
    // 缺 hal_backend 即红（P0-3 必填无落点）
    let no_hal = "[runtime]\nlog_ring_cap = 512\n\n[platform]\ntier = \"host\"\n";
    assert!(rr3_asm_sim_text(no_hal)
        .unwrap_err()
        .contains("缺 hal_backend"));
}

#[test]
fn layout_stages_install_shape_from_a_fixture_repo() {
    // 临时"仓"：profiles/default.toml + modules/rr3-pa（rust）
    let repo = std::env::temp_dir().join(format!("rr3-assemble-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&repo);
    let (m, lang, entry) = ("rr3-pa", "rust", "rr3-pa");
    let dir = repo.join("modules").join(m);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("module.toml"),
        format!(
            "[manifest]\nschema_version = 1\nname = \"{m}\"\nlang = \"{lang}\"\n\
                 entry = \"{entry}\"\n\n[provides]\ncapabilities = []\n\n\
                 [resources]\nrestart_policy = \"always\"\n\n[health]\n\
                 heartbeat_ms = 1000\nwatchdog_timeout_ms = 5000\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("config.toml"), "[x]\nv = 1\n").unwrap();
    // atoms/ 申报面：校验⑨ 从装机树读 demands，夹具必须与装机布局同形
    std::fs::create_dir_all(dir.join("atoms/servo")).unwrap();
    std::fs::write(dir.join("atoms/servo/assemble.toml"), "[assemble]\n").unwrap();
    std::fs::create_dir_all(repo.join("profiles")).unwrap();
    std::fs::write(
        repo.join("profiles/default.toml"),
        // schema_version=5 形状（批N5 [runtime] 进场，F-30 环容量落数据）：host 档
        // 合法不适用显式写；本夹具不声明 [pins]，"a.b" 是夹具自造格位，本函数
        // 只走形状解析（rr3_profile_parse），不查册。
        "schema_version = 5\n\n[runtime]\nlog_ring_cap = 512\nhal_backend = \"sim\"\n\n[platform]\ntier = \"host\"\nchip = \"rk3566\"\n\n\
             [supplies]\nwatchdog_present = false\nwatchdog_timeout_ms = 15000\n\
             watchdog_feeder = \"none\"\ntick_period_ms = 200\ntick_jitter_ms = 100\n\
             safety_core_interval_ms = 1000\n\n\
             [[supplies.domain]]\nname = \"rail5v\"\nnominal_mv = 5000\nmild_mv = 4600\n\
             severe_mv = 4400\ncurrent_cap_ma = 2000\nsense = \"none\"\n\
             current_sense = \"none\"\nswitch = \"none\"\nswitchable = false\n\
             armed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n\
             [supplies.budget]\nnot_applicable = true\n\n\
             [bindings]\n\"a.b\" = \"rr3-pa\"\n\n\
             [enabled_slots]\nrr3-pa = [\"a.b\"]\n\n\
             [slots]\npresent = [\"a.b\"]\n",
    )
    .unwrap();
    // 接线档仓源（校验⑦ 装机档必在）：vcc/gnd-only 单线组，对夹具不声明
    // [pins] 免疫
    std::fs::create_dir_all(repo.join("hardware/default")).unwrap();
    std::fs::write(
        repo.join("hardware/default/wiring.toml"),
        "[[wire]]\nconnector = \"J-T\"\nhdr_pin = [1, 2]\nsignal = [\"vcc\", \"gnd\"]\n\
             net = \"rail5v\"\nfeeds = []\nmate = \"none\"\nkeyed = false\nrated_a = 1.0\n\
             gnd_common = \"star-1\"\n",
    )
    .unwrap();

    let work = std::env::temp_dir().join(format!("rr3-assemble-w-{}", std::process::id()));
    let (profile_dst, entries) =
        rr3_assemble_layout(&repo, &work, &repo.join("profiles/default.toml")).expect("布局装配");
    // 装配单原文零改
    assert_eq!(
        std::fs::read_to_string(&profile_dst).unwrap(),
        std::fs::read_to_string(repo.join("profiles/default.toml")).unwrap()
    );
    // 接线档同批原文零改
    assert_eq!(
        std::fs::read_to_string(work.join("hardware/default/wiring.toml")).unwrap(),
        std::fs::read_to_string(repo.join("hardware/default/wiring.toml")).unwrap()
    );
    // 模块就位：entry 指与 rr3-sim 同 target-dir 的产物
    assert_eq!(entries.len(), 1);
    let want_entry = rr3_bin_dir(&repo).join("rr3-pa");
    assert_eq!(entries[0], ("rr3-pa".into(), want_entry.clone()));
    // manifest 已绝对化且原文仅 entry 一处改写
    let staged = std::fs::read_to_string(work.join("modules/rr3-pa/module.toml")).unwrap();
    assert!(
        staged.contains(&format!("entry = \"{}\"", want_entry.display())),
        "{staged}"
    );
    assert!(staged.contains("name = \"rr3-pa\""));
    // config 同目录就位
    assert!(work.join("modules/rr3-pa/config.toml").is_file());
    // atoms/ 树同批装机（漏拷会让 rrd 校验⑨ 误判"件未申报 demands"）
    assert_eq!(
        std::fs::read_to_string(work.join("modules/rr3-pa/atoms/servo/assemble.toml")).unwrap(),
        "[assemble]\n"
    );
    std::fs::remove_dir_all(&repo).ok();
    std::fs::remove_dir_all(&work).ok();
}

/// 装配演练的仓内前提：default 机型 face 的每个模块，仓源 manifest 可解析、
/// entry 可装机（仓相对）、config 在；接线档在（校验⑦ 装机档必在）——
/// 契约/目录漂移让单测红，不是演练跑一半红
#[test]
fn repo_default_enabled_slots_are_assembly_ready() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let profile_text = std::fs::read_to_string(repo.join("profiles/default.toml")).unwrap();
    let profile = rr3_profile_parse(&profile_text).expect("default 装配单可解析");
    assert_eq!(profile.enabled_slots.len(), 6, "default 机六模块满配");
    assert!(
        repo.join("hardware/default/wiring.toml").is_file(),
        "default 机缺仓源接线档（校验⑦ 要求 hardware/<形态>/wiring.toml 必在）"
    );
    for module in profile.enabled_slots.keys() {
        let dir = repo.join("modules").join(module);
        let text = std::fs::read_to_string(dir.join("module.toml"))
            .unwrap_or_else(|e| panic!("读 {module} manifest: {e}"));
        let facts =
            rr3_manifest_parse(&text).unwrap_or_else(|e| panic!("{module} manifest 解析: {e:?}"));
        assert_eq!(
            facts.name, *module,
            "manifest.name == 目录名（naming.md §3）"
        );
        assert!(
            !facts.entry.starts_with('/') && facts.entry.split('/').all(|s| s != ".."),
            "{module} entry 须仓相对且无 .. 段"
        );
        assert!(
            dir.join("config.toml").is_file(),
            "{module} 缺仓源 config.toml"
        );
    }
}
