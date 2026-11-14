//! 全链演练单测：manifest/config 形状、取证判定与参数互斥。

use super::*;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::drill::rr3_sim_args_parse;

#[test]
fn chain_manifests_have_absolute_entries_and_contract_shapes() {
    let ms = rr3_chain_manifests(
        Path::new("/abs/rr3-sense"),
        Path::new("/abs/rr3-interact"),
        Path::new("/abs/rr3-act"),
        Path::new("/abs/rr3-brain"),
    );
    assert_eq!(ms.len(), 4);
    let (names, texts): (Vec<&str>, Vec<&str>) =
        ms.iter().map(|(n, t)| (n.as_str(), t.as_str())).unzip();
    assert_eq!(names, ["rr3-sense", "rr3-interact", "rr3-brain", "rr3-act"]);
    // entry 全部绝对（注册门禁）；boot 的 sense 是 resident 常驻
    for t in &texts {
        assert!(t.contains("entry = \"/abs/"), "{t}");
    }
    assert!(
        texts[0].contains("restart_policy = \"always\""),
        "{}",
        texts[0]
    );
    assert!(texts[3].contains("type = \"module\""), "{}", texts[3]);
    assert!(texts[2].contains("type = \"brain\""), "{}", texts[2]);
    assert!(texts[2].contains("lang = \"rust\""), "{}", texts[2]);
    // act 的三面契约面与仓内 module.toml 同源
    assert!(
        texts[3].contains(
            "publishes = [\"rr3-act/servo/state\", \"rr3-act/relay/state\", \"rr3-act/motor/reflex\"]"
        ),
        "{}",
        texts[3]
    );
    assert!(
        texts[3].contains(
            "exposes = [\"rr3-act/servo/command\", \"rr3-act/led/effect\", \"rr3-act/relay/set\"]"
        ),
        "{}",
        texts[3]
    );
}

#[test]
fn obs_manifest_subscribes_both_evidence_topics() {
    let t = rr3_chain_obs(Path::new("/abs/rr3-act"));
    assert!(t.contains("name = \"rr3-obs\""), "{t}");
    assert!(
        t.contains(&format!(
            "subscribes = [\"{RR3_SENSE_TOPIC_VAD}\", \"{RR3_ACT_TOPIC_SERVO_STATE}\"]"
        )),
        "{t}"
    );
    assert!(t.contains("publishes = []"), "{t}");
}

#[test]
fn configs_carry_stereo_sense_and_act_servo_shape() {
    let sense = rr3_chain_sense_cfg();
    assert!(sense.contains("channels = 2"), "{sense}");
    assert!(sense.contains("rms_start = 1500"), "{sense}");
    assert!(sense.contains("bin_threshold_deg = 30"), "{sense}");
    let act = rr3_chain_act_cfg();
    // 行程键已撤（批N3 件五门 g），断言改锚存活键——servo 节时限仍在
    assert!(act.contains("move_timeout_ms = 800"), "{act}");
    assert!(act.contains("state_min_interval_ms = 50"), "{act}");
    assert!(!act.contains("limit_deg"), "行程键不得回流 config: {act}");
    let interact = rr3_chain_itact_cfg();
    assert!(
        interact.contains("non_pointable = [\"talk\"]"),
        "{interact}"
    );
}

#[test]
fn obs_feed_takes_doa_on_vad_and_angle_on_servo_only() {
    let mut obs = Rr3ChainObs::default();
    // offset 不带 doa：喂了也取不到（批F2 负载纪律）
    rr3_obs_feed(
        &mut obs,
        &json!({"topic": RR3_SENSE_TOPIC_VAD, "edge": "offset"}),
    );
    assert_eq!(obs, Rr3ChainObs::default());
    // onset 携 doa：判向档入袋
    rr3_obs_feed(
        &mut obs,
        &json!({"topic": RR3_SENSE_TOPIC_VAD, "edge": "onset",
                    "doa": {"deg": -60, "bin": "left"}}),
    );
    assert_eq!(obs.doa_deg, Some(-60));
    assert_eq!(obs.doa_bin.as_deref(), Some("left"));
    assert!(!obs.rr3_obs_ready(), "还缺转向落角");
    // servo/state：落角入袋；其余主题无视
    rr3_obs_feed(&mut obs, &json!({"topic": "rr3-sense/audio/pcm"}));
    rr3_obs_feed(
        &mut obs,
        &json!({"topic": RR3_ACT_TOPIC_SERVO_STATE, "seq": 2, "angle_deg": 70}),
    );
    assert_eq!(obs.servo_angle, Some(70));
    assert!(obs.rr3_obs_ready());
}

#[test]
fn chain_judge_accepts_left_turn_and_rejects_drift() {
    let ok = Rr3ChainObs {
        doa_deg: Some(-59),
        doa_bin: Some("left".into()),
        servo_angle: Some(70),
    };
    assert!(rr3_chain_judge(&ok).is_ok());
    // 反演出容差
    let drift = Rr3ChainObs {
        doa_deg: Some(-70),
        ..ok.clone()
    };
    let e = rr3_chain_judge(&drift).unwrap_err();
    assert!(e.contains("±2° 容差"), "{e}");
    // 分档错
    let bin = Rr3ChainObs {
        doa_bin: Some("right".into()),
        ..ok.clone()
    };
    assert!(rr3_chain_judge(&bin).unwrap_err().contains("左档"));
    // 落角不是词表左档
    let angle = Rr3ChainObs {
        servo_angle: Some(90),
        ..ok.clone()
    };
    assert!(rr3_chain_judge(&angle).unwrap_err().contains("70°"));
    // 证据缺一面
    let half = Rr3ChainObs {
        doa_deg: None,
        ..ok
    };
    assert!(rr3_chain_judge(&half).unwrap_err().contains("判向档缺失"));
}

#[test]
fn chain_args_parse_and_mutual_exclusion() {
    let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let a = rr3_sim_args_parse(&v(&["--chain"])).unwrap();
    assert!(a.chain && !a.brain && a.profile.is_none());
    let a = rr3_sim_args_parse(&v(&["--chain", "--act", "/abs/rr3-act"])).unwrap();
    assert_eq!(a.act.unwrap(), PathBuf::from("/abs/rr3-act"));
    // 三面互斥：chain 与 brain/profile 同给即拒
    for mixed in [
        &["--chain", "--brain"][..],
        &["--chain", "--profile", "/p"][..],
    ] {
        let e = rr3_sim_args_parse(&v(mixed)).unwrap_err();
        assert!(e.contains("互斥"), "{e}");
    }
    let e = rr3_sim_args_parse(&v(&["--act", "--chain"])).unwrap_err();
    assert!(e.contains("疑似开关"), "{e}");
}

#[test]
fn chain_identities_exist_in_the_real_contract() {
    // 钉住"演练用的名字/主题必须真实在契约里"——契约改名时这里红，不是演练跑到一半红
    let contract = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/topics.toml");
    let text = std::fs::read_to_string(&contract)
        .unwrap_or_else(|e| panic!("读 {}: {e}", contract.display()));
    for needle in [
        RR3_SIM_MODULE_ACT,
        RR3_SENSE_TOPIC_VAD,
        RR3_ACT_TOPIC_SERVO_STATE,
        "rr3-act/servo/command",
        "rr3-brain/intent/result",
        RR3_BRAIN_TOPIC_TTS_PCM,
        "rr3-interact/face/expression",
    ] {
        assert!(text.contains(needle), "契约缺 {needle}");
    }
}
