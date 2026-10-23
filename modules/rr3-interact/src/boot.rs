//! rr3-interact 接入层（0.0.8 W-A 批4 拆分，ADR-0017 五层表接入层）。
//!
//! config schema 解析（自原 composite.rs 迁入：MC-10 I-11 起四段活值面——
//! player/face/panel/selftest；缺段/缺键回落默认、类型错/负数/越界/在册外值
//! 逐键点名拒 boot（fail-closed），文案是装配观测面逐字不变）＋ 播放面/显示面
//! 选择与开机探活编排（打开/探活失败 Err 上抛——fail-closed 显式回错，不静默
//! 带病常驻也不给"没有显示面还常驻"的假活）。config 路径解析
//! （RR3_INTERACT_CONFIG_ENV 覆盖 + manifest 同目录缺省）已随批0 并入 rr3mod
//! `load_config_text` 单点，本层不再持有路径段（批N28 单点收口同款）。
//!
//! rr3_hal 直依仅 backend 装配两函数（⑨-w 白名单 backend 装配类，sense
//! `rr3_sense_probe` 同款）；契约类型（Rr3AudioSink/Rr3Display）与采样规格
//! 常量走 rr_hal_api 直依（⑨-w 模块单元契约面纪律）。

use rr_hal::backend::{rr3_hal_audio_sink, rr3_hal_display};
use rr_hal_api::{Rr3AudioSink, Rr3Display, RR3_AUDIO_CFG_16K_MONO16};

use crate::face::{rr3_face_expression_lookup, RR3_FACE_EXPRESSIONS_JOINED};
use crate::panel::server::{
    Rr3SelftestExpect, RR3_SELFTEST_CONTRACTS_EXPECTS, RR3_SELFTEST_LEG_STATES,
    RR3_SELFTEST_WIRING_EXPECTS,
};

/// 播放面 + 显示面选择与开机探活编排：[`rr3_hal_audio_sink`] 选择＋ open 探活
/// （打开失败 Err 上抛）＋「播放面就绪」回显 → [`rr3_hal_display`] 选择＋ frame()
/// 探活（失败 Err，R11③ 显式回错不静默退化——真件档板显式 linux 时 MAX7219
/// 后端未实装，探活 exit(1) 是预期语义）＋「显示面就绪」回显。返回两元句柄，
/// backend 名回显后丢弃（回显已尽的观测面不随行）。
pub(crate) fn rr3_interact_probe() -> Result<(Box<dyn Rr3AudioSink>, Box<dyn Rr3Display>), String> {
    let (mut sink, backend) = rr3_hal_audio_sink();
    sink.open(RR3_AUDIO_CFG_16K_MONO16)
        .map_err(|e| format!("打开播放面({backend})失败: {e}"))?;
    eprintln!("播放面就绪: backend={backend} rate=16000 ch=1");

    // 显示面选择 + 开机探活：读回当前帧，Unsupported 即致命——sim 档走虚拟点阵
    // 内存记录，主循环出帧全链可见
    let (display, display_backend) = rr3_hal_display();
    display
        .frame()
        .map_err(|e| format!("显示面探活失败({display_backend}): {e}"))?;
    eprintln!("显示面就绪: backend={display_backend} 8x8");
    Ok((sink, display))
}

/// 本模块 config 的汇总形状（自原 composite.rs 迁入，逐字）
#[derive(Debug, Clone)]
pub struct Rr3InteractCfg {
    pub player: Rr3PlayerCfg,
    pub face: Rr3FaceCfg,
    pub panel: Rr3PanelCfg,
    pub selftest: Rr3SelftestCfg,
}

/// config `[rr3-interact.player]` 的形状（MC-2）：起播水位与播放增益。
/// 缺键/缺段回落默认（同 sense S-5 口径：代码默认是文件缺键的回落）；
/// 类型错/负数/越界显式拒（fail-closed，不静默吞）。
#[derive(Debug, Clone)]
pub struct Rr3PlayerCfg {
    pub prebuffer_ms: u64,
    pub volume_pct: u32,
}

impl Default for Rr3PlayerCfg {
    fn default() -> Self {
        // legacy 0cf029e 调优值（0.5s 预缓冲抗抖动、70% 音量）——一个事实
        // 一个真值点：默认值只在此处与 config.toml 注释对表
        Self {
            prebuffer_ms: 500,
            volume_pct: 70,
        }
    }
}

/// config `[rr3-interact.face]` 的形状（`non_pointable` = 本形态不可点名集，I-11）。
/// 值取 `&'static str`（经白名单对账），件面判定直接吃 `&[&str]`。批N4 件五
/// I-17/M-2/M-3：`face_tick_ms` = 表情节拍（组合件主循环节步频）与
/// `default_expr` = 兜底表情（mech 状态机各回落点）数据化——节拍与人格取向
/// 都是策略，缺键回落 legacy 值。
#[derive(Debug, Clone)]
pub struct Rr3FaceCfg {
    pub non_pointable: Vec<&'static str>,
    pub face_tick_ms: u64,
    pub default_expr: &'static str,
}

/// config `[rr3-interact.panel]` 的形状（批N4 件五 I-18②）：面板五限额/超时
/// 数据化——面板是"覆盖率门缺失"的活证（I-15 分子分母数不到它们），本批补齐。
/// 缺段/缺键回落默认（legacy 常量值，S-5 口径）；类型错/负数/越界点名拒 boot。
#[derive(Debug, Clone)]
pub struct Rr3PanelCfg {
    pub max_conns: u64,
    pub read_timeout_ms: u64,
    pub header_limit: u64,
    pub body_limit: u64,
    pub call_timeout_ms: u64,
}

impl Default for Rr3PanelCfg {
    fn default() -> Self {
        // legacy 常量值（server.rs 原 RR3_PANEL_* 常量）——一个事实一个真值点：
        // 默认值只在此处与 config.toml 注释对表
        Self {
            max_conns: 8,
            read_timeout_ms: 5000,
            header_limit: 8 * 1024,
            body_limit: 4 * 1024,
            call_timeout_ms: 5000,
        }
    }
}

/// config `[rr3-interact.selftest]` 的形状（批N4 件五 I-17/M-10）：自检期望
/// 表情是产线取向，可按形态改数据。值必须在册（白名单对账）。批N5 件五
/// F-31 扩预期表 `expect`：固定腿逐名期望三态 + wiring 组预期——verdict =
/// 结果集与预期集等值，判据数据化（值域闭集与机制面同住在 panel server）。
#[derive(Debug, Clone)]
pub struct Rr3SelftestCfg {
    pub face_expect: &'static str,
    pub expect: Rr3SelftestExpect,
}

impl Default for Rr3SelftestCfg {
    fn default() -> Self {
        Self {
            face_expect: "happy",
            expect: Rr3SelftestExpect::default(),
        }
    }
}

/// TOML 文本 → Rr3InteractCfg（bootstrap 的 parse_config 入口）。手工下钻不引
/// serde derive：字段个位数，点名缺字段的文案比派生宏的默认报错可读。
pub(crate) fn rr3_interact_config_parse(text: &str) -> Result<Rr3InteractCfg, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("config TOML 解析失败: {e}"))?;
    // player 段（MC-2）：段/键缺省回落默认（S-5 口径）；类型错或越界逐键点名
    // 拒 boot（fail-closed）。volume_pct 值域 0..=1000（10 倍封顶防呆，>100
    // 为放大、样点饱和由机制面夹断）。
    let mut player = Rr3PlayerCfg::default();
    if let Some(p) = v.get("rr3-interact").and_then(|s| s.get("player")) {
        let mut t = player.prebuffer_ms;
        rr3_cfg_u64(p, "prebuffer_ms", "player", &mut t)?;
        player.prebuffer_ms = t;
        let mut t = player.volume_pct as u64;
        rr3_cfg_u64(p, "volume_pct", "player", &mut t)?;
        player.volume_pct = u32::try_from(t)
            .ok()
            .filter(|pct| *pct <= 1000)
            .ok_or_else(|| {
                format!("[rr3-interact.player] volume_pct 越界（值域 0..=1000）: {t}")
            })?;
    }
    // panel 段（批N4 件五 I-18②）：段/键缺省回落 legacy 常量值（S-5 口径）；
    // 类型错/负数点名拒。四值下界：max_conns ≥1、read_timeout_ms ≥1、
    // header/body_limit ≥1（0 = 面板自残，不当合法配置放行）。
    let mut panel = Rr3PanelCfg::default();
    if let Some(p) = v.get("rr3-interact").and_then(|s| s.get("panel")) {
        let mut t = panel.max_conns;
        rr3_cfg_u64(p, "max_conns", "panel", &mut t)?;
        panel.max_conns = t.max(1);
        let mut t = panel.read_timeout_ms;
        rr3_cfg_u64(p, "read_timeout_ms", "panel", &mut t)?;
        panel.read_timeout_ms = t.max(1);
        let mut t = panel.header_limit;
        rr3_cfg_u64(p, "header_limit", "panel", &mut t)?;
        panel.header_limit = t.max(1);
        let mut t = panel.body_limit;
        rr3_cfg_u64(p, "body_limit", "panel", &mut t)?;
        panel.body_limit = t.max(1);
        let mut t = panel.call_timeout_ms;
        rr3_cfg_u64(p, "call_timeout_ms", "panel", &mut t)?;
        panel.call_timeout_ms = t.max(1);
    }
    // selftest 段（批N4 件五 I-17/M-10）：段缺省回落 legacy 期望（"happy"）；
    // 值必须在册（同 non_pointable 白名单对账口径），在册外点名拒 boot。
    // 批N5 件五 F-31 扩 expect 预期表：段/键缺省回落默认（固定腿全 pass +
    // wiring 组零 fail）；三态值域闭集与腿名册逐键对账，未知腿名/在册外值
    // 点名拒 boot（fail-closed——预期表错一个字就改判红绿，不容静默）。
    let mut selftest = Rr3SelftestCfg::default();
    if let Some(s) = v.get("rr3-interact").and_then(|x| x.get("selftest")) {
        if let Some(name) = s.get("face_expect").and_then(toml::Value::as_str) {
            selftest.face_expect = rr3_face_expression_lookup(name).ok_or_else(|| {
                format!(
                    "config [rr3-interact.selftest] face_expect 非在册表情 {name:?}（在册值 {}）",
                    RR3_FACE_EXPRESSIONS_JOINED
                )
            })?;
        }
        if let Some(e) = s.get("expect") {
            let table = e
                .as_table()
                .ok_or_else(|| format!("config [rr3-interact.selftest] expect 段必须是表，收到 {e}"))?;
            for (k, val) in table {
                let sval = val.as_str().ok_or_else(|| {
                    format!("config [rr3-interact.selftest.expect] {k} 必须是字符串，收到 {val}")
                })?;
                match k.as_str() {
                    "wiring" => {
                        selftest.expect.wiring = Rr3SelftestExpect::rr3_wiring_lookup(sval)
                            .ok_or_else(|| {
                                format!(
                                    "config [rr3-interact.selftest.expect] wiring 非在册组预期 {sval:?}（在册值 {}）",
                                    RR3_SELFTEST_WIRING_EXPECTS.join("/")
                                )
                            })?;
                    }
                    // contracts 组（批N5 件五笔4，Z-4）：同 wiring 组的闭集对账
                    "contracts" => {
                        selftest.expect.contracts = Rr3SelftestExpect::rr3_contracts_lookup(sval)
                            .ok_or_else(|| {
                                format!(
                                    "config [rr3-interact.selftest.expect] contracts 非在册组预期 {sval:?}（在册值 {}）",
                                    RR3_SELFTEST_CONTRACTS_EXPECTS.join("/")
                                )
                            })?;
                    }
                    leg @ ("speaker" | "relay" | "face") => {
                        let st = Rr3SelftestExpect::rr3_state_lookup(sval).ok_or_else(|| {
                            format!(
                                "config [rr3-interact.selftest.expect] {leg} 非在册三态 {sval:?}（在册值 {}）",
                                RR3_SELFTEST_LEG_STATES.join("/")
                            )
                        })?;
                        match leg {
                            "speaker" => selftest.expect.speaker = st,
                            "relay" => selftest.expect.relay = st,
                            _ => selftest.expect.face = st,
                        }
                    }
                    other => {
                        return Err(format!(
                            "config [rr3-interact.selftest.expect] 未知腿名 {other:?}（在册腿 speaker/relay/face/wiring/contracts）"
                        ))
                    }
                }
            }
        }
    }
    let face_sect = v
        .get("rr3-interact")
        .and_then(|s| s.get("face"))
        .ok_or_else(|| "config 缺 [rr3-interact.face] 段".to_string())?;
    // 不可点名集（I-11）：数组逐元素必须是**在册契约值**，非在册名或非字符串
    // 逐条点名拒 boot（fail-closed）；键在而数组为空 = 本形态不排除任何表情点名（合法）
    let arr = face_sect
        .get("non_pointable")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| {
            "config [rr3-interact.face] 缺 non_pointable（不可点名表情数组）".to_string()
        })?;
    let mut non_pointable = Vec::with_capacity(arr.len());
    for (i, e) in arr.iter().enumerate() {
        let s = e.as_str().ok_or_else(|| {
            format!("config [rr3-interact.face] non_pointable[{i}] 必须是字符串，收到 {e}")
        })?;
        let expr = rr3_face_expression_lookup(s).ok_or_else(|| {
            format!(
                "config [rr3-interact.face] non_pointable[{i}] 非在册表情 {s:?}（在册值 {}）",
                RR3_FACE_EXPRESSIONS_JOINED
            )
        })?;
        non_pointable.push(expr);
    }
    // 表情节拍与兜底表情（批N4 件五 I-17/M-2/M-3）：缺键回落 legacy 值；
    // default_expression 必须在册（同 non_pointable 白名单对账）
    let mut face_tick_ms = 250u64;
    rr3_cfg_u64(face_sect, "face_tick_ms", "face", &mut face_tick_ms)?;
    let face_tick_ms = face_tick_ms.max(1);
    let default_expr = match face_sect.get("default_expression") {
        Some(name) => {
            let s = name.as_str().ok_or_else(|| {
                format!("config [rr3-interact.face] default_expression 必须是字符串，收到 {name}")
            })?;
            rr3_face_expression_lookup(s).ok_or_else(|| {
                format!(
                    "config [rr3-interact.face] default_expression 非在册表情 {s:?}（在册值 {}）",
                    RR3_FACE_EXPRESSIONS_JOINED
                )
            })?
        }
        None => rr3_face_expression_lookup("idle")
            .expect("在册白名单恒含 idle，lookup 必中"),
    };
    Ok(Rr3InteractCfg {
        player,
        face: Rr3FaceCfg {
            non_pointable,
            face_tick_ms,
            default_expr,
        },
        panel,
        selftest,
    })
}

/// config 键的 u64 下钻（S-5 同款口径）：键缺省不动 `out`（回落调用方给的
/// 默认）；类型错（非整数/负数）显式点名拒 boot，不静默吞。段名进文案便于
/// 逐段定位（player/face/panel 各段共用）。
fn rr3_cfg_u64(
    sect: &toml::Value,
    key: &str,
    sect_name: &str,
    out: &mut u64,
) -> Result<(), String> {
    let Some(v) = sect.get(key) else {
        return Ok(());
    };
    let t = v.as_integer().ok_or_else(|| {
        format!("[rr3-interact.{sect_name}] {key} 必须是整数，收到 {v}")
    })?;
    *out = u64::try_from(t)
        .map_err(|_| format!("[rr3-interact.{sect_name}] {key} 不得为负: {t}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
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
        let cfg = rr3_interact_config_parse(text).unwrap();
        assert_eq!(cfg.face.non_pointable, vec!["talk"], "不可点名集注入");
        assert_eq!(cfg.player.prebuffer_ms, 500, "起播水位注入（MC-2）");
        assert_eq!(cfg.player.volume_pct, 70, "播放增益注入（MC-2）");
        assert_eq!(cfg.face.face_tick_ms, 250, "表情节拍注入（I-17/M-2）");
        assert_eq!(cfg.face.default_expr, "idle", "兜底表情注入（I-17/M-3）");
        assert_eq!(cfg.panel.max_conns, 8, "面板限额注入（I-18②）");
        assert_eq!(cfg.selftest.face_expect, "happy", "自检期望注入（I-17/M-10）");
    }

    /// face/panel/selftest 新键（批N4 件五 I-17/I-18②）：键/段缺省回落 legacy
    /// 值；类型错、在册外值、0 值逐键点名拒（fail-closed）
    #[test]
    fn face_panel_selftest_keys_fall_back_and_reject_bad_values() {
        let minimal = "[rr3-interact.face]\nnon_pointable = []\n";
        // 全缺省：face 节拍/兜底回落 250/idle，panel 五值回落 legacy，
        // selftest 回落 happy
        let cfg = rr3_interact_config_parse(minimal).unwrap();
        assert_eq!(cfg.face.face_tick_ms, 250);
        assert_eq!(cfg.face.default_expr, "idle");
        assert_eq!(cfg.panel.max_conns, 8);
        assert_eq!(cfg.panel.read_timeout_ms, 5000);
        assert_eq!(cfg.panel.header_limit, 8 * 1024);
        assert_eq!(cfg.panel.body_limit, 4 * 1024);
        assert_eq!(cfg.panel.call_timeout_ms, 5000);
        assert_eq!(cfg.selftest.face_expect, "happy");
        // 显式值合法读入：非 legacy 取值同样放行（在册表情任选）
        let cfg = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\nface_tick_ms = 120\ndefault_expression = \"happy\"\n[rr3-interact.panel]\nmax_conns = 2\n[rr3-interact.selftest]\nface_expect = \"blink\"\n",
        )
        .unwrap();
        assert_eq!(cfg.face.face_tick_ms, 120);
        assert_eq!(cfg.face.default_expr, "happy");
        assert_eq!(cfg.panel.max_conns, 2);
        assert_eq!(cfg.selftest.face_expect, "blink");
        // 0 值自残拒放：节拍/限额折 1 下界——0 不当合法配置
        let cfg = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\nface_tick_ms = 0\n[rr3-interact.panel]\nmax_conns = 0\n",
        )
        .unwrap();
        assert_eq!(cfg.face.face_tick_ms, 1, "节拍 0 折 1");
        assert_eq!(cfg.panel.max_conns, 1, "并发 0 折 1");
        // 类型错拒：字符串进整数键、整数进字符串键
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\nface_tick_ms = \"soon\"\n",
        )
        .unwrap_err();
        assert!(e.contains("face_tick_ms"), "类型错点名: {e}");
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\ndefault_expression = 3\n",
        )
        .unwrap_err();
        assert!(e.contains("default_expression"), "非字符串点名: {e}");
        // 在册外值拒：表情名必须过白名单对账
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\ndefault_expression = \"wink\"\n",
        )
        .unwrap_err();
        assert!(e.contains("非在册表情"), "兜底表情在册外点名: {e}");
        let e = rr3_interact_config_parse(
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
        let cfg = rr3_interact_config_parse(minimal).unwrap();
        assert_eq!(cfg.selftest.expect.speaker, "pass");
        assert_eq!(cfg.selftest.expect.relay, "pass");
        assert_eq!(cfg.selftest.expect.face, "pass");
        assert_eq!(cfg.selftest.expect.wiring, "zero-fail");
        assert_eq!(cfg.selftest.expect.contracts, "zero-fail");
        // 显式值合法读入：在册三态任选（恒败形态可如实申报期望 fail/n-a）
        let cfg = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest.expect]\nspeaker = \"fail\"\nrelay = \"pass\"\nface = \"n-a\"\nwiring = \"zero-fail\"\ncontracts = \"zero-fail\"\n",
        )
        .unwrap();
        assert_eq!(cfg.selftest.expect.speaker, "fail");
        assert_eq!(cfg.selftest.expect.relay, "pass");
        assert_eq!(cfg.selftest.expect.face, "n-a");
        assert_eq!(cfg.selftest.expect.wiring, "zero-fail");
        assert_eq!(cfg.selftest.expect.contracts, "zero-fail");
        // 三态值域外拒：非在册判值点名
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest.expect]\nspeaker = \"ok\"\n",
        )
        .unwrap_err();
        assert!(
            e.contains("非在册三态") && e.contains("speaker"),
            "值域闭集点名: {e}"
        );
        // wiring 组预期词表外拒（单值词表也是闭集，拼错当场红）
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest.expect]\nwiring = \"zero_fail\"\n",
        )
        .unwrap_err();
        assert!(e.contains("非在册组预期"), "组预期词表点名: {e}");
        // contracts 组预期词表外拒（批N5 件五笔4，同闭集）
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest.expect]\ncontracts = \"zero_fail\"\n",
        )
        .unwrap_err();
        assert!(
            e.contains("非在册组预期") && e.contains("contracts"),
            "组预期词表点名: {e}"
        );
        // 未知腿名拒：腿册外预期不容静默（fail-closed）
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest.expect]\narm = \"pass\"\n",
        )
        .unwrap_err();
        assert!(e.contains("未知腿名") && e.contains("arm"), "腿名册点名: {e}");
        // 非字符串值拒
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest.expect]\nspeaker = 3\n",
        )
        .unwrap_err();
        assert!(e.contains("必须是字符串"), "类型错点名: {e}");
        // expect 段非表拒
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest]\nexpect = 3\n",
        )
        .unwrap_err();
        assert!(e.contains("expect 段必须是表"), "段形状点名: {e}");
    }

    /// player 两键（MC-2）：键/段缺省回落默认；类型错、负数、越界逐键点名拒
    #[test]
    fn player_keys_fall_back_to_defaults_and_reject_bad_values() {
        // 整段缺省（I-11 最小形状）→ 回落 legacy 默认 500/70
        let cfg = rr3_interact_config_parse("[rr3-interact.face]\nnon_pointable = []\n").unwrap();
        assert_eq!(cfg.player.prebuffer_ms, 500, "缺段回落默认水位");
        assert_eq!(cfg.player.volume_pct, 70, "缺段回落默认增益");
        // 段在键缺 → 同样回落
        let cfg = rr3_interact_config_parse(
            "[rr3-interact.player]\n[rr3-interact.face]\nnon_pointable = []\n",
        )
        .unwrap();
        assert_eq!(cfg.player.prebuffer_ms, 500);
        assert_eq!(cfg.player.volume_pct, 70);
        // 越界策略值合法读入：水位 0（无预缓冲）与增益 1000（封顶）
        let cfg = rr3_interact_config_parse(
            "[rr3-interact.player]\nprebuffer_ms = 0\nvolume_pct = 1000\n[rr3-interact.face]\nnon_pointable = []\n",
        )
        .unwrap();
        assert_eq!(cfg.player.prebuffer_ms, 0);
        assert_eq!(cfg.player.volume_pct, 1000);
        // 类型错拒：字符串进整数键
        let e = rr3_interact_config_parse(
            "[rr3-interact.player]\nprebuffer_ms = \"soon\"\n[rr3-interact.face]\nnon_pointable = []\n",
        )
        .unwrap_err();
        assert!(e.contains("prebuffer_ms"), "类型错点名: {e}");
        // 负数拒：水位不得为负
        let e = rr3_interact_config_parse(
            "[rr3-interact.player]\nprebuffer_ms = -1\n[rr3-interact.face]\nnon_pointable = []\n",
        )
        .unwrap_err();
        assert!(e.contains("不得为负"), "负数点名: {e}");
        // 增益越界拒：值域 0..=1000
        let e = rr3_interact_config_parse(
            "[rr3-interact.player]\nvolume_pct = 1001\n[rr3-interact.face]\nnon_pointable = []\n",
        )
        .unwrap_err();
        assert!(e.contains("volume_pct 越界"), "增益越界点名: {e}");
    }

    #[test]
    fn config_parse_rejects_missing_sections_and_fields() {
        let e = rr3_interact_config_parse("").unwrap_err();
        assert!(e.contains("[rr3-interact.face]"), "缺段点名: {e}");
        let e = rr3_interact_config_parse("[rr3-interact.face]\n").unwrap_err();
        assert!(e.contains("non_pointable"), "缺键点名: {e}");
        let e = rr3_interact_config_parse("[rr3-interact.face]\nnon_pointable = 3\n").unwrap_err();
        assert!(e.contains("non_pointable"), "非数组点名: {e}");
        let e = rr3_interact_config_parse("[rr3-interact.face]\nnon_pointable = [\"wink\"]\n")
            .unwrap_err();
        assert!(e.contains("非在册"), "在册外值点名: {e}");
        // 空数组合法：本形态不排除任何表情点名（fail-closed 的键必填 + 空值合法）
        let cfg = rr3_interact_config_parse("[rr3-interact.face]\nnon_pointable = []\n").unwrap();
        assert!(cfg.face.non_pointable.is_empty());
    }
}
