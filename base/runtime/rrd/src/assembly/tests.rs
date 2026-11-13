//! assembly 的单测（自同名单测模块原样抽出，判据不变）。

use super::*;
use rr_hal_api::{Rr3ActHardStop, Rr3ActHold, Rr3ActProfile, Rr3ActSource};
use rr_registry::rr3_role_parse;

/// 最小快照：一 pin（有落点）+ 一 pin（零引用）+ 一域 + 一线组 + 一轴 + 一角色
fn snapshot() -> Rr3AssemblySnapshot {
    let profile = Rr3Profile {
        platform_tier: "host".into(),
        platform_chip: "rk3566".into(),
        supplies: crate::profile::Rr3Supplies {
            domains: vec![crate::profile::Rr3Domain {
                name: "rail5v".into(),
                nominal_mv: 5000,
                mild_mv: 4600,
                severe_mv: 4400,
                current_cap_ma: 2000,
                sense: "none".into(),
                current_sense: "none".into(),
                switch: "none".into(),
                switchable: false,
                armed: false,
                max_age_ms: 0,
                evidence: "none".into(),
            }],
            ..Default::default()
        },
        pins: vec![
            crate::profile::Rr3PinEntry {
                role: "act.axis.pwm".into(),
                instance: "pan".into(),
                controller: "pwm0".into(),
                channel: Some(0),
                physical: vec![32],
                direction: "out".into(),
                rail: "rail5v".into(),
                addr: "none".into(),
                device: String::new(),
                failsafe: "release".into(),
                mux_group: vec![],
            },
            crate::profile::Rr3PinEntry {
                role: "ui.face.spi".into(),
                instance: String::new(),
                controller: "spidev0".into(),
                channel: Some(0),
                physical: vec![],
                direction: "out".into(),
                rail: "none".into(),
                addr: "none".into(),
                device: String::new(),
                failsafe: "release".into(),
                mux_group: vec![],
            },
        ],
        ..Default::default()
    };
    let wiring = crate::wiring::Rr3Wiring {
        wires: vec![crate::wiring::Rr3Wire {
            connector: "J3".into(),
            hdr_pin: vec![2, 3, 4],
            signal: vec!["vcc".into(), "act.axis.pwm.pan".into(), "gnd".into()],
            net: "rail5v".into(),
            feeds: vec![],
            mate: "none".into(),
            keyed: true,
            rated_a: 1.0,
            gnd_common: "star-1".into(),
        }],
    };
    let axis = vec![Rr3AxisFile {
        pin: "act.axis.pwm.pan".into(),
        profile: Rr3ActProfile {
            center_deg: 90,
            lo_deg: 60,
            hi_deg: 120,
            dir_sign: 1,
            slew_deg_s: 100,
            settle_ms: 200,
            hold: Rr3ActHold::Release,
            current_ceil_ma: 750,
            warmup_ms: 0,
            hard_stop: Rr3ActHardStop::Yes,
            calibrated: true,
            source: Rr3ActSource::Manual,
        },
    }];
    let roles = rr3_role_parse(
            "[[role]]\nname = \"act.axis.pwm\"\nslot = \"act.servo\"\nrequires = [\"controller\", \"channel\"]\nderives = [\"pwm_chip\"]\n",
        );
    Rr3AssemblySnapshot {
        profile_name: "test形态".into(),
        profile,
        wiring,
        peak_by_mate: BTreeMap::from([("SG90-3P".to_string(), 700)]),
        axis,
        roles,
    }
}

#[test]
fn snapshot_set_is_first_wins_and_readable() {
    // OnceLock 进程级单点：首份胜出、二份被拒、只读口取首份
    let first = snapshot();
    let mut second = snapshot();
    second.profile_name = "另一形态".into();
    assert!(rr3_assembly_set(first));
    assert!(!rr3_assembly_set(second));
    assert_eq!(
        rr3_assembly_snap().expect("首份应可读").profile_name,
        "test形态"
    );
}

#[test]
fn cross_checks_cover_four_families_with_three_states() {
    let s = snapshot();
    let checks = rr3_cross_checks(&s.profile, &s.wiring, &s.peak_by_mate);
    // signal 族 3 导体：vcc/gnd 记 not-applicable，pin 导体记 pass
    let sig = |item: &str| checks.iter().find(|c| c.item == item).expect("点名位必在");
    assert_eq!(sig("signal:J3[0]:vcc").pass, RR3_CHECK_NA);
    assert_eq!(sig("signal:J3[1]:act.axis.pwm.pan").pass, RR3_CHECK_PASS);
    assert_eq!(sig("signal:J3[2]:gnd").pass, RR3_CHECK_NA);
    // net 族：在册域 pass（none 形状归独立用例）
    assert_eq!(sig("net:J3:rail5v").pass, RR3_CHECK_PASS);
    // 额定加总族：J3 电力线组（mate=none 负载未注册按 0 计，feeds=[]）判 pass
    assert_eq!(sig("rated:J3").pass, RR3_CHECK_PASS);
    assert!(
        sig("rated:J3").reason.contains("⑦-c1"),
        "{:?}",
        sig("rated:J3").reason
    );
    // 落点族：pan 有 1 导体引用 pass；face.spi 零引用记 not-applicable（合法态）
    assert_eq!(sig("pin:act.axis.pwm.pan").pass, RR3_CHECK_PASS);
    assert_eq!(sig("pin:ui.face.spi").pass, RR3_CHECK_NA);
    assert!(sig("pin:ui.face.spi").reason.contains("零引用"));
    // fail 分支：signal 越出 pin_id 集（⑦-a 破）与 net 越出 domains（⑦-b 破）
    let broken = crate::wiring::Rr3Wiring {
        wires: vec![crate::wiring::Rr3Wire {
            connector: "JX".into(),
            hdr_pin: vec![1],
            signal: vec!["act.axis.pwm.tilt".into()],
            net: "rail24".into(),
            feeds: vec![],
            mate: "none".into(),
            keyed: false,
            rated_a: 0.5,
            gnd_common: "none".into(),
        }],
    };
    let bad = rr3_cross_checks(&s.profile, &broken, &s.peak_by_mate);
    assert_eq!(bad[0].pass, RR3_CHECK_FAIL);
    assert!(bad[0].reason.contains("⑦-a"));
    assert_eq!(bad[1].pass, RR3_CHECK_FAIL);
    assert!(bad[1].reason.contains("⑦-b"));
    // 额定加总族在 fail 场合照常收账（JX 电力线组照判，与 ⑦-a/⑦-b 的红同帧）
    assert_eq!(bad[2].pass, RR3_CHECK_PASS);
    assert!(bad[2].reason.contains("⑦-c1"), "{:?}", bad[2].reason);
}

/// 额定加总族（校验⑦-c1·自检时点判）四态：F-10 红例形状（主供线额定不足）、
/// 菱形分叉只收账一次、feeds 一般环 fail（装载期闭合面只拒自环，环即漂移）、
/// 自反口径钉门（本线对端负载计入"该线承载之和"）。
#[test]
fn rated_sum_family_judges_f10_topology_sum() {
    let s = snapshot(); // peak_by_mate：SG90-3P → 700mA
    let mk = |connector: &str, feeds: Vec<&str>, rated: f64| crate::wiring::Rr3Wire {
        connector: connector.into(),
        hdr_pin: vec![1, 2],
        signal: vec!["vcc".into(), "gnd".into()],
        net: "rail5v".into(),
        feeds: feeds.into_iter().map(str::to_string).collect(),
        mate: "SG90-3P".into(),
        keyed: true,
        rated_a: rated,
        gnd_common: "star-1".into(),
    };
    fn c<'a>(checks: &'a [Rr3AssemblyCheck], item: &str) -> &'a Rr3AssemblyCheck {
        checks
            .iter()
            .find(|x| x.item == item)
            .unwrap_or_else(|| panic!("点名位 {item} 必在: {checks:?}"))
    }
    // F-10 红例形状：主供线 1.0A 自挂 + 下馈两条 1.0A 舵机线 ⇒ 700×3 = 2100 > 1000 红
    let wiring = crate::wiring::Rr3Wiring {
        wires: vec![
            mk("J1", vec!["J2", "J3"], 1.0),
            mk("J2", vec![], 1.0),
            mk("J3", vec![], 1.0),
            crate::wiring::Rr3Wire {
                connector: "J9".into(),
                hdr_pin: vec![1],
                signal: vec!["gnd".into()],
                net: "none".into(),
                feeds: vec![],
                mate: "none".into(),
                keyed: false,
                rated_a: 0.5,
                gnd_common: "none".into(),
            },
        ],
    };
    let checks = rr3_cross_checks(&s.profile, &wiring, &s.peak_by_mate);
    assert_eq!(c(&checks, "rated:J1").pass, RR3_CHECK_FAIL);
    assert!(
        c(&checks, "rated:J1").reason.contains("2100"),
        "{:?}",
        c(&checks, "rated:J1").reason
    );
    assert_eq!(c(&checks, "rated:J2").pass, RR3_CHECK_PASS);
    assert_eq!(c(&checks, "rated:J3").pass, RR3_CHECK_PASS);
    // net = none 纯信号/地线组：⑦-c1 无判据面，not-applicable
    assert_eq!(c(&checks, "rated:J9").pass, RR3_CHECK_NA);

    // 自反口径：单线 0.5A 自挂 700mA 舵机 ⇒ 本线承载之和含自身对端，红
    let solo = crate::wiring::Rr3Wiring {
        wires: vec![mk("J7", vec![], 0.5)],
    };
    let checks = rr3_cross_checks(&s.profile, &solo, &s.peak_by_mate);
    assert_eq!(c(&checks, "rated:J7").pass, RR3_CHECK_FAIL);
    assert!(
        c(&checks, "rated:J7").reason.contains("700"),
        "{:?}",
        c(&checks, "rated:J7").reason
    );

    // 菱形分叉不是环：J1→{J2,J3}、J2→J4、J3→J4 ⇒ J4 只收账一次，Σ = 700×4 = 2800
    let diamond = crate::wiring::Rr3Wiring {
        wires: vec![
            mk("J1", vec!["J2", "J3"], 4.0),
            mk("J2", vec!["J4"], 1.0),
            mk("J3", vec!["J4"], 1.0),
            mk("J4", vec![], 1.0),
        ],
    };
    let checks = rr3_cross_checks(&s.profile, &diamond, &s.peak_by_mate);
    assert_eq!(c(&checks, "rated:J1").pass, RR3_CHECK_PASS);
    assert!(
        c(&checks, "rated:J1").reason.contains("2800"),
        "{:?}",
        c(&checks, "rated:J1").reason
    );

    // feeds 一般环：J5→J6→J5（装载期闭合面只拒自环，环漏进快照即漂移）——双红
    let cycle = crate::wiring::Rr3Wiring {
        wires: vec![mk("J5", vec!["J6"], 3.0), mk("J6", vec!["J5"], 1.0)],
    };
    let checks = rr3_cross_checks(&s.profile, &cycle, &s.peak_by_mate);
    assert_eq!(c(&checks, "rated:J5").pass, RR3_CHECK_FAIL);
    assert!(
        c(&checks, "rated:J5").reason.contains("环"),
        "{:?}",
        c(&checks, "rated:J5").reason
    );
    assert_eq!(c(&checks, "rated:J6").pass, RR3_CHECK_FAIL);
}

#[test]
fn wiring_json_renders_every_denominator_field() {
    // 门 d 分母四路的字段级全集必须逐键出现在应答（covers 申报"真实渲染"
    // 的兑现判据）——词表自门 d 同源常量导出，不抄第二份清单。
    // 测纯函数 + 局部快照：OnceLock 全局态与 cargo test 并行线程有竞态，
    // 纯函数口才可测（全局壳只做 available 位与 None 分支，语义见函数注）。
    let s = snapshot();
    let v = rr3_assembly_render(&s);
    assert_eq!(v["profile"], json!("test形态"));
    let pin = &v["pins"][0];
    for k in crate::profile::RR3_PIN_KEYS {
        assert!(
            pin.get(k).is_some(),
            "pin 应答缺分母字段 {k}（covers 申报 pin/{k} 的渲染面）"
        );
    }
    let dom = &v["domains"][0];
    for k in crate::profile::RR3_DOMAIN_KEYS {
        assert!(
            dom.get(k).is_some(),
            "domain 应答缺分母字段 {k}（covers 申报 domain/{k} 的渲染面）"
        );
    }
    let axis = &v["axis"][0];
    for f in rr_hal_api::RR3_ACT_PROFILE_FIELDS {
        assert!(
            axis.get(f.key).is_some(),
            "axis 应答缺分母字段 {}（covers 申报 axis/{} 的渲染面）",
            f.key,
            f.key
        );
    }
    assert_eq!(axis["hold"], json!("release"));
    assert_eq!(axis["hard_stop"], json!("yes"));
    assert_eq!(axis["source"], json!("manual"));
    // role 族：requires 逐项在应答（门 d role/<req> 的渲染面）
    let role = &v["roles"][0];
    assert_eq!(role["name"], json!("act.axis.pwm"));
    assert_eq!(role["requires"], json!(["controller", "channel"]));
    // checks 逐条进应答
    assert!(v["checks"].as_array().expect("checks 应为数组").len() >= 5);
}
