//! boot 的单测（自同名单测模块原样抽出，判据不变）。

use super::*;

/// 仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红
#[test]
fn config_parse_covers_repo_shape() {
    let text = r#"
[rr3-interact.player]
prebuffer_ms = 500
volume_pct = 70

[rr3-interact.face]
non_pointable = ["talk"]
face_tick_ms = 250
default_expression = "idle"

[rr3-interact.panel]
max_conns = 8
read_timeout_ms = 5000
header_limit = 8192
body_limit = 4096
call_timeout_ms = 5000

[rr3-interact.selftest]
face_expect = "happy"
"#;
    let cfg = rr3_itact_cfg_parse(text).unwrap();
    assert_eq!(cfg.face.non_pointable, vec!["talk"], "不可点名集注入");
    assert_eq!(cfg.player.prebuffer_ms, 500, "起播水位注入（MC-2）");
    assert_eq!(cfg.player.volume_pct, 70, "播放增益注入（MC-2）");
    assert_eq!(cfg.face.face_tick_ms, 250, "表情节拍注入（I-17/M-2）");
    assert_eq!(cfg.face.default_expr, "idle", "兜底表情注入（I-17/M-3）");
    assert_eq!(cfg.panel.max_conns, 8, "面板限额注入（I-18②）");
    assert_eq!(
        cfg.selftest.face_expect, "happy",
        "自检期望注入（I-17/M-10）"
    );
}

/// face/panel/selftest 新键（批N4 件五 I-17/I-18②）：键/段缺省回落 legacy
/// 值；类型错、在册外值、0 值逐键点名拒（fail-closed）
#[test]
fn face_panel_selftest_keys_fall_back_and_reject_bad_values() {
    let minimal = "[rr3-interact.face]\nnon_pointable = []\n";
    // 全缺省：face 节拍/兜底回落 250/idle，panel 五值回落 legacy，
    // selftest 回落 happy
    let cfg = rr3_itact_cfg_parse(minimal).unwrap();
    assert_eq!(cfg.face.face_tick_ms, 250);
    assert_eq!(cfg.face.default_expr, "idle");
    assert_eq!(cfg.panel.max_conns, 8);
    assert_eq!(cfg.panel.read_timeout_ms, 5000);
    assert_eq!(cfg.panel.header_limit, 8 * 1024);
    assert_eq!(cfg.panel.body_limit, 4 * 1024);
    assert_eq!(cfg.panel.call_timeout_ms, 5000);
    assert_eq!(cfg.selftest.face_expect, "happy");
    // 显式值合法读入：非 legacy 取值同样放行（在册表情任选）
    let cfg = rr3_itact_cfg_parse(
            "[rr3-interact.face]\nnon_pointable = []\nface_tick_ms = 120\ndefault_expression = \"happy\"\n[rr3-interact.panel]\nmax_conns = 2\n[rr3-interact.selftest]\nface_expect = \"blink\"\n",
        )
        .unwrap();
    assert_eq!(cfg.face.face_tick_ms, 120);
    assert_eq!(cfg.face.default_expr, "happy");
    assert_eq!(cfg.panel.max_conns, 2);
    assert_eq!(cfg.selftest.face_expect, "blink");
    // 0 值自残拒放：节拍/限额折 1 下界——0 不当合法配置
    let cfg = rr3_itact_cfg_parse(
            "[rr3-interact.face]\nnon_pointable = []\nface_tick_ms = 0\n[rr3-interact.panel]\nmax_conns = 0\n",
        )
        .unwrap();
    assert_eq!(cfg.face.face_tick_ms, 1, "节拍 0 折 1");
    assert_eq!(cfg.panel.max_conns, 1, "并发 0 折 1");
    // 类型错拒：字符串进整数键、整数进字符串键
    let e =
        rr3_itact_cfg_parse("[rr3-interact.face]\nnon_pointable = []\nface_tick_ms = \"soon\"\n")
            .unwrap_err();
    assert!(e.contains("face_tick_ms"), "类型错点名: {e}");
    let e =
        rr3_itact_cfg_parse("[rr3-interact.face]\nnon_pointable = []\ndefault_expression = 3\n")
            .unwrap_err();
    assert!(e.contains("default_expression"), "非字符串点名: {e}");
    // 在册外值拒：表情名必须过白名单对账
    let e = rr3_itact_cfg_parse(
        "[rr3-interact.face]\nnon_pointable = []\ndefault_expression = \"wink\"\n",
    )
    .unwrap_err();
    assert!(e.contains("非在册表情"), "兜底表情在册外点名: {e}");
    let e = rr3_itact_cfg_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest]\nface_expect = \"wink\"\n",
        )
        .unwrap_err();
    assert!(e.contains("非在册表情"), "自检期望在册外点名: {e}");
}

/// selftest expect 预期表（批N5 件五 F-31）：段/键缺省回落固定腿全 pass +
/// wiring 组零 fail；在册三态任选；值域外、组预期词表外、未知腿名、非字符
/// 串值、非表段逐键点名拒 boot（fail-closed）
#[test]
fn selftest_expect_table_falls_back_and_rejects_unknown_legs_and_states() {
    let minimal = "[rr3-interact.face]\nnon_pointable = []\n";
    // 段/键缺省：预期表回落固定腿全 pass + 两组零 fail
    let cfg = rr3_itact_cfg_parse(minimal).unwrap();
    assert_eq!(cfg.selftest.expect.speaker, "pass");
    assert_eq!(cfg.selftest.expect.relay, "pass");
    assert_eq!(cfg.selftest.expect.face, "pass");
    assert_eq!(cfg.selftest.expect.wiring, "zero-fail");
    assert_eq!(cfg.selftest.expect.contracts, "zero-fail");
    // 显式值合法读入：在册三态任选（恒败形态可如实申报期望 fail/n-a）
    let cfg = rr3_itact_cfg_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest.expect]\nspeaker = \"fail\"\nrelay = \"pass\"\nface = \"n-a\"\nwiring = \"zero-fail\"\ncontracts = \"zero-fail\"\n",
        )
        .unwrap();
    assert_eq!(cfg.selftest.expect.speaker, "fail");
    assert_eq!(cfg.selftest.expect.relay, "pass");
    assert_eq!(cfg.selftest.expect.face, "n-a");
    assert_eq!(cfg.selftest.expect.wiring, "zero-fail");
    assert_eq!(cfg.selftest.expect.contracts, "zero-fail");
    // 三态值域外拒：非在册判值点名
    let e = rr3_itact_cfg_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest.expect]\nspeaker = \"ok\"\n",
        )
        .unwrap_err();
    assert!(
        e.contains("非在册三态") && e.contains("speaker"),
        "值域闭集点名: {e}"
    );
    // wiring 组预期词表外拒（单值词表也是闭集，拼错当场红）
    let e = rr3_itact_cfg_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest.expect]\nwiring = \"zero_fail\"\n",
        )
        .unwrap_err();
    assert!(e.contains("非在册组预期"), "组预期词表点名: {e}");
    // contracts 组预期词表外拒（批N5 件五笔4，同闭集）
    let e = rr3_itact_cfg_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest.expect]\ncontracts = \"zero_fail\"\n",
        )
        .unwrap_err();
    assert!(
        e.contains("非在册组预期") && e.contains("contracts"),
        "组预期词表点名: {e}"
    );
    // 未知腿名拒：腿册外预期不容静默（fail-closed）
    let e = rr3_itact_cfg_parse(
        "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest.expect]\narm = \"pass\"\n",
    )
    .unwrap_err();
    assert!(
        e.contains("未知腿名") && e.contains("arm"),
        "腿名册点名: {e}"
    );
    // 非字符串值拒
    let e = rr3_itact_cfg_parse(
        "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest.expect]\nspeaker = 3\n",
    )
    .unwrap_err();
    assert!(e.contains("必须是字符串"), "类型错点名: {e}");
    // expect 段非表拒
    let e = rr3_itact_cfg_parse(
        "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest]\nexpect = 3\n",
    )
    .unwrap_err();
    assert!(e.contains("expect 段必须是表"), "段形状点名: {e}");
}

/// player 两键（MC-2）：键/段缺省回落默认；类型错、负数、越界逐键点名拒
#[test]
fn player_keys_fall_back_to_defaults_and_reject_bad_values() {
    // 整段缺省（I-11 最小形状）→ 回落 legacy 默认 500/70
    let cfg = rr3_itact_cfg_parse("[rr3-interact.face]\nnon_pointable = []\n").unwrap();
    assert_eq!(cfg.player.prebuffer_ms, 500, "缺段回落默认水位");
    assert_eq!(cfg.player.volume_pct, 70, "缺段回落默认增益");
    // 段在键缺 → 同样回落
    let cfg =
        rr3_itact_cfg_parse("[rr3-interact.player]\n[rr3-interact.face]\nnon_pointable = []\n")
            .unwrap();
    assert_eq!(cfg.player.prebuffer_ms, 500);
    assert_eq!(cfg.player.volume_pct, 70);
    // 越界策略值合法读入：水位 0（无预缓冲）与增益 1000（封顶）
    let cfg = rr3_itact_cfg_parse(
            "[rr3-interact.player]\nprebuffer_ms = 0\nvolume_pct = 1000\n[rr3-interact.face]\nnon_pointable = []\n",
        )
        .unwrap();
    assert_eq!(cfg.player.prebuffer_ms, 0);
    assert_eq!(cfg.player.volume_pct, 1000);
    // 类型错拒：字符串进整数键
    let e = rr3_itact_cfg_parse(
        "[rr3-interact.player]\nprebuffer_ms = \"soon\"\n[rr3-interact.face]\nnon_pointable = []\n",
    )
    .unwrap_err();
    assert!(e.contains("prebuffer_ms"), "类型错点名: {e}");
    // 负数拒：水位不得为负
    let e = rr3_itact_cfg_parse(
        "[rr3-interact.player]\nprebuffer_ms = -1\n[rr3-interact.face]\nnon_pointable = []\n",
    )
    .unwrap_err();
    assert!(e.contains("不得为负"), "负数点名: {e}");
    // 增益越界拒：值域 0..=1000
    let e = rr3_itact_cfg_parse(
        "[rr3-interact.player]\nvolume_pct = 1001\n[rr3-interact.face]\nnon_pointable = []\n",
    )
    .unwrap_err();
    assert!(e.contains("volume_pct 越界"), "增益越界点名: {e}");
}

#[test]
fn config_parse_rejects_missing_sections_and_fields() {
    let e = rr3_itact_cfg_parse("").unwrap_err();
    assert!(e.contains("[rr3-interact.face]"), "缺段点名: {e}");
    let e = rr3_itact_cfg_parse("[rr3-interact.face]\n").unwrap_err();
    assert!(e.contains("non_pointable"), "缺键点名: {e}");
    let e = rr3_itact_cfg_parse("[rr3-interact.face]\nnon_pointable = 3\n").unwrap_err();
    assert!(e.contains("non_pointable"), "非数组点名: {e}");
    let e = rr3_itact_cfg_parse("[rr3-interact.face]\nnon_pointable = [\"wink\"]\n").unwrap_err();
    assert!(e.contains("非在册"), "在册外值点名: {e}");
    // 空数组合法：本形态不排除任何表情点名（fail-closed 的键必填 + 空值合法）
    let cfg = rr3_itact_cfg_parse("[rr3-interact.face]\nnon_pointable = []\n").unwrap();
    assert!(cfg.face.non_pointable.is_empty());
}
