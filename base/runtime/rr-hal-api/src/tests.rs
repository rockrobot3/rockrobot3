//! 契约面单元测试（原 lib.rs 内联测试整体搬出）。

use crate::*;

struct DummyGpio(bool);

impl Rr3GpioOut for DummyGpio {
    fn set(&mut self, level: bool) -> Result<(), Rr3HalError> {
        self.0 = level;
        Ok(())
    }
    fn level(&self) -> Result<bool, Rr3HalError> {
        Ok(self.0)
    }
}

/// dyn 兼容校验：trait 对象必须能装箱/借用（承载层要持有异构设备集合）。
/// 同步 trait 无 `Send + Sync` 约束——单所有者语义，承载层主循环独占。
#[test]
fn hal_traits_are_object_safe() {
    let mut boxed: Box<dyn Rr3GpioOut> = Box::new(DummyGpio(false));
    assert!(boxed.set(true).is_ok());
    assert!(boxed.level().unwrap());
    let _: Box<dyn Rr3AudioSource> = Box::new(Rr3UnavailableSource);
    let _: Box<dyn Rr3AudioSink> = Box::new(Rr3UnavailableSource);
    let _: Box<dyn Rr3Power> = Box::new(Rr3UnavailableSource);
    let _: Box<dyn Rr3Rail> = Box::new(Rr3UnavailableSource);
    let _: Box<dyn Rr3Link> = Box::new(Rr3UnavailableSource);
    let _: Box<dyn Rr3PwmServo> = Box::new(Rr3UnavailableSource);
    let _: Box<dyn Rr3Display> = Box::new(Rr3UnavailableSource);
}

#[test]
fn audio_default_cfg_matches_legacy_pcm() {
    let cfg = RR3_AUDIO_CFG_16K_MONO16;
    assert_eq!(
        (cfg.sample_rate_hz, cfg.channels, cfg.bit_depth),
        (16000, 1, 16)
    );
}

#[test]
fn hal_error_carries_backend_name() {
    let e = Rr3HalError::Unsupported("linux/iio");
    assert_eq!(e.to_string(), "HAL 后端不支持: linux/iio");
    let e = Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new("行程 [0, 180]"));
    assert_eq!(e.to_string(), "HAL 参数非法: 行程 [0, 180]");
}

/// 定长缓冲：容量内逐字保留；超长按 char 边界截断（多字节字符不劈半），
/// `as_str` 恒合法。
#[test]
fn err_text_truncates_on_char_boundary() {
    let t = Rr3ErrText::rr3_err_text_new("行程 [0, 180] 越域");
    assert_eq!(t.rr3_err_text_as_str(), "行程 [0, 180] 越域");
    let long = "汉".repeat(200);
    let t = Rr3ErrText::rr3_err_text_new(&long);
    // char 边界截断后字节数不必顶满容量（42×3=126 ≤ 128），只须不超且不劈字符
    assert!(t.rr3_err_text_as_str().len() <= Rr3ErrText::rr3_err_text_cap());
    assert_eq!(t.rr3_err_text_as_str().chars().count(), 42); // 42×3=126 ≤ 128
}

#[test]
fn ssid_text_roundtrips_and_truncates() {
    let t = Rr3SsidText::rr3_ssid_text_new("home-5g");
    assert_eq!(t.rr3_ssid_text_as_str(), "home-5g");
    let long = "s".repeat(40);
    let t = Rr3SsidText::rr3_ssid_text_new(&long);
    assert_eq!(
        t.rr3_ssid_text_as_str().len(),
        Rr3SsidText::rr3_ssid_text_cap()
    );
}

/// 占位后端回错（能力缺失显式回错，不静默退化）
#[test]
fn placeholder_returns_unsupported() {
    let mut p = Rr3UnavailableSource;
    assert!(matches!(
        p.set(true),
        Err(Rr3HalError::Unsupported("placeholder/gpio"))
    ));
    assert!(matches!(
        p.battery(),
        Err(Rr3HalError::Unsupported("placeholder/power"))
    ));
    assert!(matches!(
        p.rr3_read("rail5v"),
        Err(Rr3HalError::Unsupported("placeholder/rail"))
    ));
    assert!(matches!(
        p.rr3_current_ma("rail5v"),
        Err(Rr3HalError::Unsupported("placeholder/rail"))
    ));
    assert!(matches!(
        p.rr3_switch("rail5v", false),
        Err(Rr3HalError::Unsupported("placeholder/rail"))
    ));
}

/// P-15（T-6）：档位词表四态在册——`Unknown` 与三电平档在接口上可区分，
/// 档名串与板侧守护行同源（三档同 `RailTier::name`，第四态补 `unknown`）。
#[test]
fn rail_tier_has_four_states_with_pinned_names() {
    assert_eq!(Rr3RailTier::Ok.rr3_rail_tier_name(), "ok");
    assert_eq!(Rr3RailTier::Mild.rr3_rail_tier_name(), "mild");
    assert_eq!(Rr3RailTier::Severe.rr3_rail_tier_name(), "severe");
    assert_eq!(Rr3RailTier::Unknown.rr3_rail_tier_name(), "unknown");
    // 读数形状：mv/tier/age_ms 同帧（T-6 断头清单的闭合形状——龄期与档位
    // 不可拆着给，消费者判龄判的是同一帧）
    let s = Rr3RailState {
        mv: 4400,
        tier: Rr3RailTier::Unknown,
        age_ms: 9_999,
    };
    assert_eq!(s.tier, Rr3RailTier::Unknown);
    assert_ne!(
        Rr3RailState {
            mv: 4400,
            tier: Rr3RailTier::Severe,
            age_ms: 0
        },
        s,
        "陈旧的 Severe 与新鲜的 Severe 在接口上必须可区分（age_ms 入等值面）"
    );
}

/// ⑥-c2 判据红例组（批N2 件三，全案 §6）：超龄与无基准折 Unknown、新鲜原样、
/// 边界不折、陈旧 Severe 不许冒充新鲜（T-6 立项理由——没有 Unknown 第四态，
/// 面板/brain 拿着不敢信的数据照样判定）。
#[test]
fn rail_tier_effective_folds_stale_or_unclocked_readings_into_unknown() {
    // 新鲜读数原样（三电平档都不折）
    assert_eq!(
        rr3_rail_tier_fold(Rr3RailTier::Ok, Some(100), 1_000),
        Rr3RailTier::Ok
    );
    assert_eq!(
        rr3_rail_tier_fold(Rr3RailTier::Severe, Some(1_000), 1_000),
        Rr3RailTier::Severe,
        "边界 age == max_age_ms 不折（判据是严格 >，新鲜度上界闭区间内）"
    );
    // 超龄 Ok ⇒ Unknown（陈旧的 Ok 不许冒充新鲜）
    assert_eq!(
        rr3_rail_tier_fold(Rr3RailTier::Ok, Some(1_001), 1_000),
        Rr3RailTier::Unknown
    );
    // 超龄 Severe ⇒ Unknown（陈旧的 Severe 与"新鲜的 Severe"在执行面等处置，
    // 但在可信度上必须可区分——冒充新鲜即 T-6 红例）
    assert_eq!(
        rr3_rail_tier_fold(Rr3RailTier::Severe, Some(u32::MAX), 1_000),
        Rr3RailTier::Unknown
    );
    // 时基取不到基准（None）⇒ Unknown（Y-4：编造龄期不许冒充新鲜）
    assert_eq!(
        rr3_rail_tier_fold(Rr3RailTier::Ok, None, 1_000),
        Rr3RailTier::Unknown
    );
}

/// 拒动判据红例组（批N3，全案 §6 硬约束——"未标定不得自动"与 hard_stop
/// 护栏都是代码拒动）：四形态 × 三挡块事实 × 标定位的全组合关键臂。
#[test]
fn act_motion_allowed_refuses_uncalibrated_auto_and_unknown_hardstop() {
    use Rr3ActHardStop as Hs;
    use Rr3ActMotion as Mo;
    // 点动永远放行（未标定轴的唯一合法动作面）
    for hs in [Hs::Unknown, Hs::Yes, Hs::No] {
        for cal in [false, true] {
            assert!(rr3_motion_allowed(hs, cal, Mo::Jog).is_ok());
        }
    }
    // 硬约束一：calibrated=false ⇒ Auto 一律拒（判据文案点名判据名）
    let e = rr3_motion_allowed(Hs::Yes, false, Mo::Auto).unwrap_err();
    assert!(e.to_string().contains("未标定轴不得自动"), "{e}");
    // 标定过 ⇒ Auto 放行
    assert!(rr3_motion_allowed(Hs::Yes, true, Mo::Auto).is_ok());
    // 硬约束二：hard_stop=unknown ⇒ C 自动段 / B 找挡块都拒
    let e = rr3_motion_allowed(Hs::Unknown, true, Mo::CalibOpenLoop).unwrap_err();
    assert!(e.to_string().contains("hard_stop=unknown"), "{e}");
    assert!(rr3_motion_allowed(Hs::Unknown, false, Mo::CalibStallSearch).is_err());
    // hard_stop=yes ⇒ C 自动段 / B 找挡块放行（标定动作，与 calibrated 无关）
    assert!(rr3_motion_allowed(Hs::Yes, false, Mo::CalibOpenLoop).is_ok());
    assert!(rr3_motion_allowed(Hs::Yes, false, Mo::CalibStallSearch).is_ok());
    // hard_stop=no ⇒ B 拒（无硬挡块可找）、C 自动段放行（软挡块路线不依赖该事实）
    assert!(rr3_motion_allowed(Hs::No, true, Mo::CalibStallSearch).is_err());
    assert!(rr3_motion_allowed(Hs::No, true, Mo::CalibOpenLoop).is_ok());
}

/// 档案字段词表形状 pin（批N3，§4.4 档案十四键）：键名唯一、两处"只回显"
/// 如实标注（F-25 读者三态——settle_ms/warmup_ms 是 §4.4 点名的已知候选）。
#[test]
fn act_profile_fields_pinned_shape_and_reader_annotations() {
    assert_eq!(RR3_ACT_PROFILE_FIELDS.len(), 14);
    for (i, a) in RR3_ACT_PROFILE_FIELDS.iter().enumerate() {
        for b in RR3_ACT_PROFILE_FIELDS.iter().skip(i + 1) {
            assert_ne!(a.key, b.key, "档案键名重复：{}", a.key);
        }
    }
    let echo_keys: Vec<_> = RR3_ACT_PROFILE_FIELDS
        .iter()
        .filter(|f| f.reader == Rr3ActFieldReader::Echo)
        .map(|f| f.key)
        .collect();
    assert_eq!(echo_keys, ["settle_ms", "warmup_ms", "source"]);
    // 装载域键与判据输入键都属裁决侧（读者三态标注与实际读者集合一致）
    for k in [
        "schema",
        "pin",
        "lo_deg",
        "hi_deg",
        "hard_stop",
        "calibrated",
    ] {
        let f = RR3_ACT_PROFILE_FIELDS.iter().find(|f| f.key == k).unwrap();
        assert_eq!(f.reader, Rr3ActFieldReader::Adjudicate, "{k}");
    }
    // 挡块事实档名与档案取值串同源
    assert_eq!(Rr3ActHardStop::Unknown.rr3_hard_stop_name(), "unknown");
    assert_eq!(Rr3ActHardStop::Yes.rr3_hard_stop_name(), "yes");
    assert_eq!(Rr3ActHardStop::No.rr3_hard_stop_name(), "no");
}

/// 三档案词枚举（hold / hard_stop / source）name ↔ parse 同源 round-trip：
/// 词形改动只许落各 enum 的 impl 一处，档案文本串漂移即红。
#[test]
fn act_word_enums_name_parse_round_trip() {
    for (h, w) in [(Rr3ActHold::Hold, "hold"), (Rr3ActHold::Release, "release")] {
        assert_eq!(h.rr3_act_hold_name(), w);
        assert_eq!(Rr3ActHold::rr3_act_hold_parse(w), Some(h));
    }
    assert_eq!(Rr3ActHold::rr3_act_hold_parse("auto"), None);
    for (hs, w) in [
        (Rr3ActHardStop::Unknown, "unknown"),
        (Rr3ActHardStop::Yes, "yes"),
        (Rr3ActHardStop::No, "no"),
    ] {
        assert_eq!(Rr3ActHardStop::rr3_hard_stop_parse(w), Some(hs));
    }
    assert_eq!(Rr3ActHardStop::rr3_hard_stop_parse("maybe"), None);
    for (s, w) in [
        (Rr3ActSource::Manual, "manual"),
        (Rr3ActSource::Fixture, "fixture"),
        (Rr3ActSource::OpenLoopRecover, "openloop-recover"),
        (Rr3ActSource::StallSearch, "stall-search"),
    ] {
        assert_eq!(s.rr3_act_source_name(), w);
        assert_eq!(Rr3ActSource::rr3_act_source_parse(w), Some(s));
    }
    assert_eq!(Rr3ActSource::rr3_act_source_parse("unknown"), None);
    for (m, w) in [
        (Rr3ActMotion::Jog, "jog"),
        (Rr3ActMotion::Auto, "auto"),
        (Rr3ActMotion::CalibOpenLoop, "calib-openloop"),
        (Rr3ActMotion::CalibStallSearch, "calib-stall-search"),
    ] {
        assert_eq!(m.rr3_act_motion_name(), w);
        assert_eq!(Rr3ActMotion::rr3_act_motion_parse(w), Some(m));
    }
    assert_eq!(Rr3ActMotion::rr3_act_motion_parse("walk"), None);
}

/// 位置读数来源在册（坑 11：开环回显与闭环测量在接口上可区分——来源缺席的
/// 裸角度不许冒充测量）；占位后端对执行件/载体缝回错（能力缺失显式回错）。
#[test]
fn position_and_placeholder_actuator_store_pinned() {
    let p = Rr3Position {
        deg: 90,
        source: Rr3PositionSource::Commanded,
    };
    assert_eq!(p.source, Rr3PositionSource::Commanded);
    let _ = Rr3PositionSource::Encoder; // 预留格位（路线 D）在册不撤名
    let mut boxed: Box<dyn Rr3Actuator> = Box::new(Rr3UnavailableSource);
    assert!(matches!(
        boxed.rr3_jog(90, 800),
        Err(Rr3HalError::Unsupported("placeholder/actuator"))
    ));
    assert!(matches!(
        boxed.rr3_goto(90, 800),
        Err(Rr3HalError::Unsupported("placeholder/actuator"))
    ));
    let mut store: Box<dyn Rr3Store> = Box::new(Rr3UnavailableSource);
    assert!(matches!(
        store.rr3_store_write("calibrated", b"true"),
        Err(Rr3HalError::Unsupported("placeholder/store"))
    ));
}
