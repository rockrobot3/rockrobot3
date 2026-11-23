// SPDX-License-Identifier: Apache-2.0
//! axis_write 单测（文件头：本文件只放测试，生产面在 `axis_write.rs`）。
//!
//! 拆分动因：`axis_write.rs` 509 行、内联测试自 283 行起占近半，先按仓内同形搬出。
//! 整册挂 unix 门（原子写序与目录布局用例只在 unix 成立），与父模块同门。
//!
//! 父模块的 `#[cfg(all(test, unix))] mod tests;` 就是本文件的门。

use super::*;
use std::path::Path;

/// 装配单最小夹具（schema_version = 5，与 e2e_act PROFILE 同形状缩减到
/// 一条舵机轴 pin）——写入面的输入只有它，接线档与模块装配都不需要。
/// 文本 = 数据夹具单一真值（src/testdata/）+ 本夹具独有的 slots/bindings 尾段。
const PROFILE: &str = concat!(
    include_str!("../testdata/profile_host_prefix.toml"),
    r#"
[slots]
present = ["act.servo"]

[bindings]
"act.servo" = "rr3-act"

[enabled_slots]
rr3-act = ["act.servo"]

"#,
    include_str!("../testdata/pin_servo_pan.toml"),
);

/// 合法候选（§4.4 示例档值面，与 axis.rs 单测 sample_text 同形状）：
/// 手工标定产物——calibrated=true + hard_stop=yes + source=manual。
fn candidate_text(center: u16) -> String {
    crate::test_fixtures::axis_sample_text(center, "yes", true)
}

fn tmpdir(tag: &str) -> PathBuf {
    crate::test_fixtures::tmpdir_ns("rrd-axis-write-t-", tag)
}

/// 装机布局夹具：profiles/default.toml（形态名 = 文件名 stem）。Drop 清树，
/// 断言 panic 后 /tmp 不留残迹。
struct Root {
    dir: PathBuf,
}

impl Root {
    fn new(tag: &str) -> Root {
        let dir = tmpdir(tag);
        std::fs::create_dir_all(dir.join("profiles")).unwrap();
        Root { dir }
    }

    fn profile(&self) -> PathBuf {
        let p = self.dir.join("profiles").join("default.toml");
        std::fs::write(&p, PROFILE).unwrap();
        p
    }

    fn write_candidate(&self, text: &str) -> PathBuf {
        let p = self.dir.join("candidate.toml");
        std::fs::write(&p, text).unwrap();
        p
    }

    /// 装配链的落位（写入面推导的同一位置）：hardware/default/axis/
    fn axis_dir(&self) -> PathBuf {
        self.dir.join("hardware").join("default").join("axis")
    }

    fn target(&self) -> PathBuf {
        self.axis_dir().join("pan.toml")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn args_of(profile: &Path, candidate: &Path) -> Rr3AxisWriteArgs {
    Rr3AxisWriteArgs {
        profile: profile.to_path_buf(),
        candidate: candidate.to_path_buf(),
    }
}

#[test]
fn candidate_passing_full_judgement_is_stored_atomically() {
    let root = Root::new("ok");
    let profile = root.profile();
    let candidate = root.write_candidate(&candidate_text(90));

    let out = rr3_axis_write_run(&args_of(&profile, &candidate)).expect("合法候选必须落盘");
    assert!(
        out.contains(&root.target().display().to_string()),
        "回显必须点名写入路径: {out}"
    );
    assert!(out.contains("calibrated = true"), "回显要点名标定位: {out}");

    // 落盘内容过装载闭环：写进去的档案能被 boot 链同一路径装载回来
    let pin_ids: BTreeSet<String> = ["act.axis.pwm.pan".to_string()].into_iter().collect();
    let loaded = rr3_act_profile_load(&root.target(), &pin_ids)
        .expect("写出的档案必须装载绿")
        .expect("档案必须在场");
    assert_eq!(loaded.pin, "act.axis.pwm.pan");
    assert_eq!(loaded.profile.center_deg, 90);
    assert!(loaded.profile.calibrated);
}

#[test]
fn candidate_failing_judgement_writes_nothing() {
    let root = Root::new("badcand");
    let profile = root.profile();
    // 缺 calibrated 键（F-6 全键必填）：判据红 → 一个字节都不许落
    let broken = candidate_text(90).replace("calibrated = true\n", "");
    let candidate = root.write_candidate(&broken);

    let e = rr3_axis_write_run(&args_of(&profile, &candidate)).expect_err("缺键必须拒");
    assert!(e.contains("calibrated"), "拒因要点名病灶键: {e}");
    assert!(!root.target().exists(), "判据红的候选不许落盘");
}

#[test]
fn candidate_pin_must_hit_assembly_pins() {
    let root = Root::new("pinfk");
    let profile = root.profile();
    // 装配单只有 pan：tilt 不在 pin_id 全集（F-4 外键对账的写侧半边）
    let text =
        candidate_text(90).replace("pin = \"act.axis.pwm.pan\"", "pin = \"act.axis.pwm.tilt\"");
    let candidate = root.write_candidate(&text);

    let e = rr3_axis_write_run(&args_of(&profile, &candidate)).expect_err("外键未命中必须拒");
    assert!(
        e.contains("act.axis.pwm.tilt") && e.contains("pin_id"),
        "拒因要点名 pin 与对账面: {e}"
    );
    assert!(!root.target().exists());
}

#[test]
fn broken_target_is_never_overwritten() {
    let root = Root::new("brokentgt");
    let profile = root.profile();
    let candidate = root.write_candidate(&candidate_text(90));

    // 预置一份装载红的坏档（缺键）在目标位
    let axis = root.axis_dir();
    std::fs::create_dir_all(&axis).unwrap();
    std::fs::write(root.target(), "schema = 1\npin = \"act.axis.pwm.pan\"\n").expect("预置坏档");

    let e = rr3_axis_write_run(&args_of(&profile, &candidate)).expect_err("坏目标必须拒覆盖");
    assert!(
        e.contains("不许覆盖") && e.contains("装载红"),
        "拒因要点名不许覆盖与病灶: {e}"
    );
    let after = std::fs::read_to_string(root.target()).unwrap();
    assert_eq!(
        after, "schema = 1\npin = \"act.axis.pwm.pan\"\n",
        "坏档必须原封不动"
    );
}

#[test]
fn same_pin_under_other_name_is_refused() {
    let root = Root::new("duppin");
    let profile = root.profile();
    let candidate = root.write_candidate(&candidate_text(90));

    // 预置一份合法同 pin 异名档（遗留副本的真实形貌——文件名不是身份键，
    // 装载链按 pin 对账；两份合法 = boot 拒注入）
    let axis = root.axis_dir();
    std::fs::create_dir_all(&axis).unwrap();
    std::fs::write(axis.join("pan-copy.toml"), candidate_text(90)).expect("预置副本");

    let e = rr3_axis_write_run(&args_of(&profile, &candidate)).expect_err("同 pin 副本必须拒");
    assert!(
        e.contains("pan-copy.toml") && e.contains("恰一份"),
        "拒因要点名副本文件与投影纪律: {e}"
    );
    assert!(!root.target().exists(), "副本在场时目标位不许再写");
}

#[test]
fn existing_profile_for_same_pin_is_replaced() {
    let root = Root::new("replace");
    let profile = root.profile();

    // 先写一版（center=90），再写修订版（center=95）：同一 pin 的版本化更新
    // 是写入面的正路（档案"版本化、可回滚"，§4.4）——原子序保证要么全新要么全旧。
    let v1 = root.write_candidate(&candidate_text(90));
    rr3_axis_write_run(&args_of(&profile, &v1)).expect("首版必须落盘");
    let v2 = root.write_candidate(&candidate_text(95));
    rr3_axis_write_run(&args_of(&profile, &v2)).expect("修订版必须落盘");

    let pin_ids: BTreeSet<String> = ["act.axis.pwm.pan".to_string()].into_iter().collect();
    let loaded = rr3_act_profile_load(&root.target(), &pin_ids)
        .expect("修订版必须装载绿")
        .expect("档案必须在场");
    assert_eq!(loaded.profile.center_deg, 95, "修订值必须生效");
}

#[test]
fn parse_requires_both_paths_and_rejects_unknown_flags() {
    let ok = rr3_axis_write_parse(&[
        "--profile".into(),
        "/p/default.toml".into(),
        "--candidate".into(),
        "/p/c.toml".into(),
    ])
    .expect("双参数必须收");
    assert_eq!(ok.profile, PathBuf::from("/p/default.toml"));
    assert_eq!(ok.candidate, PathBuf::from("/p/c.toml"));

    let e = rr3_axis_write_parse(&["--profile".into(), "/p/default.toml".into()])
        .expect_err("缺 --candidate 必须拒");
    assert!(e.contains("--candidate 缺失") && e.contains("用法"), "{e}");
    let e = rr3_axis_write_parse(&[]).expect_err("全缺必须拒");
    assert!(e.contains("--profile 缺失"), "{e}");
    let e = rr3_axis_write_parse(&["--bogus".into()]).expect_err("未知开关必须拒");
    assert!(e.contains("未知选项 --bogus"), "{e}");
    // 疑似开关不当值吞下（与 rrd cli 的 need_path_value 同一条口径）
    let e = rr3_axis_write_parse(&["--profile".into(), "--candidate".into(), "/p/c.toml".into()])
        .expect_err("开关名不当值必须拒");
    assert!(
        e.contains("--profile 缺参数") && e.contains("--candidate"),
        "{e}"
    );
}
